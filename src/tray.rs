use std::thread;
#[cfg(windows)]
pub fn start() {
    thread::Builder::new()
        .name("kb-tray".into())
        .spawn(|| unsafe { run_windows() })
        .expect("unable to start tray");
}
#[cfg(not(windows))]
pub fn start() {
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
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DispatchMessageW, GetMessageW, IDI_APPLICATION,
        LoadIconW, MSG, RegisterClassW, TranslateMessage, WM_APP, WNDCLASSW, WS_EX_TOOLWINDOW,
        WS_POPUP,
    };
    use windows::core::w;
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
        w!("kb_count"),
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
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_APP + 1;
    data.hIcon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
    let tip = "kb_count".encode_utf16().collect::<Vec<_>>();
    for (dst, src) in data.szTip.iter_mut().zip(tip.into_iter()) {
        *dst = src;
    }
    let _ = Shell_NotifyIconW(NIM_ADD, &data);
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).into() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
}
#[cfg(windows)]
extern "system" fn proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    w: windows::Win32::Foundation::WPARAM,
    l: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, w, l) }
}

