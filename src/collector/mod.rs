use crate::stats::event::InputEvent;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

pub fn start() -> Receiver<InputEvent> {
    let (tx, rx) = mpsc::sync_channel(8192);
    thread::Builder::new()
        .name("kb-input-collector".into())
        .spawn(move || raw_input::run(tx))
        .expect("unable to start collector");
    rx
}

#[cfg(windows)]
mod raw_input {
    use super::*;
    use chrono::Local;
    use std::collections::{HashMap, HashSet};
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::Input::{
        GetRawInputData, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_DEVNOTIFY,
        RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE, RegisterRawInputDevices,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW,
        GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, KBDLLHOOKSTRUCT,
        LLKHF_INJECTED, LLKHF_UP, MSG, RegisterClassW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_INPUT, WM_KEYDOWN, WM_SYSKEYDOWN, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{PWSTR, w};
    static SENDER: OnceLock<SyncSender<InputEvent>> = OnceLock::new();
    static PROCESS_CACHE: OnceLock<Mutex<HashMap<u32, (String, String)>>> = OnceLock::new();
    static PRESSED_KEYS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();

    pub fn run(tx: SyncSender<InputEvent>) {
        let _ = SENDER.set(tx);
        unsafe {
            let instance = GetModuleHandleW(None).expect("module handle");
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("kb_count_raw_input"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                tracing::error!(error = ?windows::Win32::Foundation::GetLastError(), "RegisterClassW failed");
                return;
            }
            let hwnd = match CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("kb_count_raw_input"),
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
                Ok(value) => value,
                Err(error) => {
                    tracing::error!(%error, "CreateWindowExW failed");
                    return;
                }
            };
            if hwnd.0.is_null() {
                return;
            }
            let devices = [RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: hwnd,
            }];
            if let Err(error) =
                RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            {
                tracing::error!(%error, "RegisterRawInputDevices failed");
                return;
            }
            let keyboard_hook =
                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) {
                    Ok(hook) => hook,
                    Err(error) => {
                        tracing::error!(%error, "SetWindowsHookExW keyboard hook failed");
                        return;
                    }
                };
            tracing::info!("Raw Input mouse and low-level keyboard collector started");
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWindowsHookEx(keyboard_hook);
        }
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
            let data = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if data.flags.0 & LLKHF_INJECTED.0 == 0 {
                let key_id = (data.scanCode << 1) | u32::from(data.flags.0 & 1 != 0);
                let pressed = PRESSED_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
                let mut pressed = pressed.lock().expect("keyboard state poisoned");
                let physically_down = GetAsyncKeyState(data.vkCode as i32) < 0;
                if pressed.contains(&key_id) && physically_down {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
                pressed.remove(&key_id);
                if !pressed.insert(key_id) {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
                drop(pressed);
                let (app_name, app_path) = foreground_process();
                if let Some(sender) = SENDER.get() {
                    let _ = sender.try_send(InputEvent {
                        local_date: Local::now().format("%Y-%m-%d").to_string(),
                        app_name,
                        app_path,
                        input_type: "keyboard".into(),
                        code: format!("scan:{:02x}:ext:{}", data.scanCode, data.flags.0 & 1 != 0),
                        count: 1,
                        keyboard_count: 1,
                        mouse_count: 0,
                        wheel_units: 0,
                        detailed: true,
                    });
                }
            } else if data.flags.0 & LLKHF_UP.0 != 0 {
                let key_id = (data.scanCode << 1) | u32::from(data.flags.0 & 1 != 0);
                if let Some(pressed) = PRESSED_KEYS.get() {
                    pressed
                        .lock()
                        .expect("keyboard state poisoned")
                        .remove(&key_id);
                }
            }
        } else if code >= 0 && (data_is_key_up(wparam) || data_is_injected(lparam)) {
            clear_pressed_key(lparam);
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn data_is_key_up(wparam: WPARAM) -> bool {
        wparam.0 as u32 == 0x0101 || wparam.0 as u32 == 0x0105
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn data_is_injected(lparam: LPARAM) -> bool {
        let data = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        data.flags.0 & LLKHF_INJECTED.0 != 0
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn clear_pressed_key(lparam: LPARAM) {
        let data = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let key_id = (data.scanCode << 1) | u32::from(data.flags.0 & 1 != 0);
        if let Some(pressed) = PRESSED_KEYS.get() {
            pressed
                .lock()
                .expect("keyboard state poisoned")
                .remove(&key_id);
        }
    }

    extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_INPUT {
            unsafe {
                handle_input(hwnd, lparam);
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn foreground_process() -> (String, String) {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        if hwnd.0.is_null() || GetWindowThreadProcessId(hwnd, Some(&mut pid)) == 0 || pid == 0 {
            return ("Unknown".into(), "Unknown".into());
        }
        let cache = PROCESS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(value) = cache
            .lock()
            .expect("process cache poisoned")
            .get(&pid)
            .cloned()
        {
            return value;
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return ("Unknown".into(), "Unknown".into());
        };
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        let path = if QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
        {
            String::from_utf16_lossy(&buffer[..len as usize])
        } else {
            "Unknown".into()
        };
        let _ = CloseHandle(process);
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("Unknown")
            .to_string();
        let result = (name, path);
        cache
            .lock()
            .expect("process cache poisoned")
            .insert(pid, result.clone());
        result
    }
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn handle_input(_hwnd: HWND, lparam: LPARAM) {
        let mut size = 0u32;
        if GetRawInputData(
            windows::Win32::UI::Input::HRAWINPUT(lparam.0 as _),
            RID_INPUT,
            None,
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        ) == u32::MAX
            || size == 0
        {
            return;
        }
        let mut aligned = vec![0u64; (size as usize).div_ceil(std::mem::size_of::<u64>())];
        if GetRawInputData(
            windows::Win32::UI::Input::HRAWINPUT(lparam.0 as _),
            RID_INPUT,
            Some(aligned.as_mut_ptr() as *mut c_void),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        ) == u32::MAX
        {
            return;
        }
        let raw = &*(aligned.as_ptr() as *const RAWINPUT);
        let date = Local::now().format("%Y-%m-%d").to_string();
        let (app_name, app_path) = foreground_process();
        let (kind, code, count, key, mouse, wheel) = match raw.header.dwType {
            x if x == RIM_TYPEKEYBOARD.0 => {
                let k = raw.data.keyboard;
                if k.Message != 0x0100 {
                    return;
                }
                (
                    "keyboard",
                    format!("scan:{:02x}:ext:{}", k.MakeCode, (k.Flags & 1) != 0),
                    1,
                    1,
                    0,
                    0,
                )
            }
            x if x == RIM_TYPEMOUSE.0 => {
                let m = raw.data.mouse;
                let button = match m.Anonymous.Anonymous.usButtonFlags {
                    0x0001 => "left",
                    0x0002 => "left_up",
                    0x0004 => "right",
                    0x0008 => "right_up",
                    0x0010 => "middle",
                    0x0020 => "middle_up",
                    0x0040 => "x1",
                    0x0080 => "x2",
                    0x0100 => "wheel",
                    0x0200 => "hwheel",
                    _ => return,
                };
                let is_wheel = button == "wheel" || button == "hwheel";
                let wheel_units = if is_wheel {
                    let delta =
                        i16::from_ne_bytes(m.Anonymous.Anonymous.usButtonData.to_ne_bytes()) as i64;
                    if delta == 0 {
                        0
                    } else {
                        let units = delta / 120;
                        if units == 0 { delta.signum() } else { units }
                    }
                } else {
                    0
                };
                (
                    if is_wheel { "wheel" } else { "mouse" },
                    button.to_string(),
                    1,
                    0,
                    if is_wheel { 0 } else { 1 },
                    wheel_units,
                )
            }
            _ => return,
        };
        if let Some(sender) = SENDER.get() {
            let _ = sender.try_send(InputEvent {
                local_date: date,
                app_name,
                app_path,
                input_type: kind.into(),
                code,
                count,
                keyboard_count: key,
                mouse_count: mouse,
                wheel_units: wheel,
                detailed: true,
            });
        }
    }
}

#[cfg(not(windows))]
mod raw_input {
    use super::*;
    pub fn run(_tx: SyncSender<InputEvent>) {
        thread::park();
    }
}
