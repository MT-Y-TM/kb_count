use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
static REQUEST: AtomicU8 = AtomicU8::new(0);
static CAPTURE_ENABLED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
#[cfg(windows)]
static TRAY_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
pub fn remove_icon() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::HWND,
            UI::Shell::{NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW},
        };
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(TRAY_WINDOW.load(Ordering::Acquire) as *mut _),
            uID: 1,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}
pub fn request_exit() {
    REQUEST.store(3, Ordering::Release);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Open,
    TogglePause,
    Exit,
    Settings,
}
pub fn take_request() -> Option<Command> {
    match REQUEST.swap(0, Ordering::AcqRel) {
        1 => Some(Command::Open),
        2 => Some(Command::TogglePause),
        3 => Some(Command::Exit),
        4 => Some(Command::Settings),
        _ => None,
    }
}
#[cfg(windows)]
pub fn activate_existing() -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};
    use windows::core::w;
    unsafe {
        FindWindowW(w!("kb_count_tray"), w!("kb_count_tray_window"))
            .and_then(|window| PostMessageW(window, WM_APP + 2, WPARAM(0), LPARAM(0)))
            .is_ok()
    }
}
#[cfg(windows)]
pub fn start(enabled: Arc<AtomicBool>) {
    let _ = CAPTURE_ENABLED.set(enabled);
    thread::Builder::new()
        .name("kb-tray".into())
        .spawn(|| unsafe { run_windows() })
        .expect("unable to start tray");
}
#[cfg(not(windows))]
pub fn start(enabled: Arc<AtomicBool>) {
    let _ = CAPTURE_ENABLED.set(enabled);
    let _ = thread::Builder::new()
        .name("kb-tray".into())
        .spawn(|| thread::park());
}
#[cfg(windows)]
#[allow(unsafe_op_in_unsafe_fn, clippy::field_reassign_with_default)]
unsafe fn run_windows() {
    use windows::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DestroyIcon, DispatchMessageW, GetMessageW,
        GetSystemMetrics, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LoadIconW,
        LoadImageW, MSG, RegisterClassW, SM_CXSMICON, TranslateMessage, WM_APP, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{PCWSTR, w};
    let instance =
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None).expect("module handle");
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(proc),
        hInstance: instance.into(),
        lpszClassName: w!("kb_count_tray"),
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        return;
    }
    let hwnd = match CreateWindowExW(
        WS_EX_TOOLWINDOW,
        w!("kb_count_tray"),
        w!("kb_count_tray_window"),
        WS_POPUP,
        0,
        0,
        0,
        0,
        None,
        None,
        instance,
        None,
    ) {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut data = NOTIFYICONDATAW::default();
    TRAY_WINDOW.store(hwnd.0 as isize, Ordering::Release);
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_APP + 1;
    let icon_size = GetSystemMetrics(SM_CXSMICON).max(16);
    struct TrayIcon(Option<HICON>);
    impl Drop for TrayIcon {
        fn drop(&mut self) {
            if let Some(icon) = self.0 {
                let _ = unsafe { DestroyIcon(icon) };
            }
        }
    }
    // Own the requested resolution; LR_SHARED may reuse a differently sized
    // icon. Keep the handle until after removing it from the notification area.
    let tray_icon = TrayIcon(
        LoadImageW(
            instance,
            PCWSTR(std::ptr::without_provenance::<u16>(1)),
            IMAGE_ICON,
            icon_size,
            icon_size,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .map(|handle| HICON(handle.0)),
    );
    data.hIcon = tray_icon
        .0
        .unwrap_or_else(|| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
    for (dst, src) in data.szTip.iter_mut().zip("kb_count".encode_utf16()) {
        *dst = src;
    }
    if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
        tracing::error!("无法创建系统托盘图标");
        return;
    }
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).into() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
}
#[cfg(windows)]
#[allow(unsafe_op_in_unsafe_fn)]
extern "system" fn proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    w: windows::Win32::Foundation::WPARAM,
    l: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe {
        if msg == windows::Win32::UI::WindowsAndMessaging::WM_APP + 2 {
            REQUEST.store(1, Ordering::Release);
        } else if msg == windows::Win32::UI::WindowsAndMessaging::WM_APP + 1 {
            match l.0 as u32 {
                windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDBLCLK => {
                    REQUEST.store(1, Ordering::Release)
                }
                windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
        }
        windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, w, l)
    }
}
#[cfg(windows)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn show_menu(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, GetCursorPos, MF_STRING, SetForegroundWindow,
        TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu,
    };
    use windows::core::w;
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let _ = AppendMenuW(menu, MF_STRING, 1, w!("打开统计面板"));
    let pause_label = if CAPTURE_ENABLED
        .get()
        .map(|enabled| enabled.load(Ordering::Relaxed))
        .unwrap_or(true)
    {
        w!("暂停统计")
    } else {
        w!("恢复统计")
    };
    let _ = AppendMenuW(menu, MF_STRING, 2, pause_label);
    let _ = AppendMenuW(menu, MF_STRING, 4, w!("设置"));
    let _ = AppendMenuW(menu, MF_STRING, 3, w!("退出"));
    let mut point = windows::Win32::Foundation::POINT::default();
    if GetCursorPos(&mut point).is_err() {
        return;
    }
    let _ = SetForegroundWindow(hwnd);
    let selected = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD,
        point.x,
        point.y,
        0,
        hwnd,
        None,
    );
    if selected.0 != 0 {
        REQUEST.store(selected.0 as u8, Ordering::Release);
    }
}
