use crate::stats::event::InputEvent;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

pub fn start(enabled: Arc<AtomicBool>) -> Receiver<InputEvent> {
    let (tx, rx) = mpsc::sync_channel(8192);
    thread::Builder::new()
        .name("kb-input-collector".into())
        .spawn(move || raw_input::run(tx, enabled))
        .expect("unable to start collector");
    rx
}

#[cfg(windows)]
mod raw_input {
    use super::*;
    use chrono::Local;
    use std::collections::{HashMap, HashSet};
    use std::sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::Input::{
        GetRawInputData, GetRegisteredRawInputDevices, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
        RAWKEYBOARD, RAWMOUSE, RID_INPUT, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD,
        RIM_TYPEMOUSE, RegisterRawInputDevices,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW,
        GA_ROOT, GetAncestor, GetCursorPos, GetForegroundWindow, GetMessageW,
        GetWindowThreadProcessId, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP, MSG,
        RegisterClassW, SetTimer, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        WH_KEYBOARD_LL, WM_INPUT, WM_KEYDOWN, WM_POWERBROADCAST, WM_SYSKEYDOWN, WM_TIMER,
        WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP, WindowFromPoint,
    };
    use windows::core::{PWSTR, w};
    type ProcessCacheEntry = (String, String, Instant);
    static SENDER: OnceLock<SyncSender<InputEvent>> = OnceLock::new();
    static CAPTURE_ENABLED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    static PROCESS_CACHE: OnceLock<Mutex<HashMap<u32, ProcessCacheEntry>>> = OnceLock::new();
    static PRESSED_KEYS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    static WHEEL_REMAINDER: OnceLock<Mutex<HashMap<String, (i32, i32)>>> = OnceLock::new();
    static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
    static FIRST_KEYBOARD_LOGGED: AtomicBool = AtomicBool::new(false);
    static FIRST_MOUSE_LOGGED: AtomicBool = AtomicBool::new(false);
    static USE_RAW_KEYBOARD: AtomicBool = AtomicBool::new(false);
    static RAW_INPUT_MESSAGES: AtomicU64 = AtomicU64::new(0);
    static RAW_INPUT_DATA_ERRORS: AtomicU64 = AtomicU64::new(0);
    static RAW_MOUSE_PACKETS: AtomicU64 = AtomicU64::new(0);
    static RAW_MOUSE_BUTTON_PACKETS: AtomicU64 = AtomicU64::new(0);
    static RAW_MOUSE_WHEEL_PACKETS: AtomicU64 = AtomicU64::new(0);
    static MOUSE_EVENTS_ENQUEUED: AtomicU64 = AtomicU64::new(0);
    static MOUSE_EVENTS_DROPPED: AtomicU64 = AtomicU64::new(0);
    static MOUSE_UNKNOWN_ATTRIBUTION: AtomicU64 = AtomicU64::new(0);
    const RI_MOUSE_WHEEL: u16 = 0x0400;
    const RI_MOUSE_HWHEEL: u16 = 0x0800;
    const RI_MOUSE_BUTTON_4_DOWN: u16 = 0x0040;
    const RI_MOUSE_BUTTON_5_DOWN: u16 = 0x0100;
    const MOUSE_COUNTED_FLAGS: u16 = 0x0001
        | 0x0004
        | 0x0010
        | RI_MOUSE_BUTTON_4_DOWN
        | RI_MOUSE_BUTTON_5_DOWN
        | RI_MOUSE_WHEEL
        | RI_MOUSE_HWHEEL;
    const RI_KEY_BREAK: u16 = 0x01;
    const RI_KEY_E0: u16 = 0x02;
    const RI_KEY_E1: u16 = 0x04;

    pub fn run(tx: SyncSender<InputEvent>, enabled: Arc<AtomicBool>) {
        let _ = SENDER.set(tx);
        let _ = CAPTURE_ENABLED.set(enabled);
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
                w!("kb_count_raw_input_window"),
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
            let mouse_device = RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: hwnd,
            };
            let keyboard_device = RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: hwnd,
            };
            let mouse_raw_input_registered = match RegisterRawInputDevices(
                &[mouse_device],
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            ) {
                Ok(()) => true,
                Err(error) => {
                    tracing::error!(%error, "RegisterRawInputDevices mouse registration failed");
                    false
                }
            };
            let keyboard_raw_input_registered = match RegisterRawInputDevices(
                &[keyboard_device],
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            ) {
                Ok(()) => true,
                Err(error) => {
                    tracing::error!(%error, "RegisterRawInputDevices keyboard registration failed");
                    false
                }
            };
            let raw_input_registered = mouse_raw_input_registered && keyboard_raw_input_registered;
            let keyboard_hook: Option<HHOOK> = match SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook),
                None,
                0,
            ) {
                Ok(hook) => Some(hook),
                Err(error) => {
                    USE_RAW_KEYBOARD.store(true, Ordering::Release);
                    tracing::error!(
                        %error,
                        "SetWindowsHookExW keyboard hook failed; using Raw Input keyboard fallback"
                    );
                    None
                }
            };
            tracing::info!(
                raw_input_registered,
                mouse_raw_input_registered,
                keyboard_raw_input_registered,
                low_level_keyboard_hook = keyboard_hook.is_some(),
                "Raw Input mouse and keyboard collector started"
            );
            if SetTimer(hwnd, 1, 10_000, None) == 0 {
                tracing::warn!("unable to start mouse input diagnostic timer");
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if let Some(keyboard_hook) = keyboard_hook {
                let _ = UnhookWindowsHookEx(keyboard_hook);
            }
            log_diagnostics(hwnd);
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
                if !should_count_keydown(&mut pressed, key_id, physically_down) {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
                drop(pressed);
                if !capture_enabled() {
                    return CallNextHookEx(None, code, wparam, lparam);
                }
                let (app_name, app_path) = foreground_process();
                emit_event(
                    Local::now().format("%Y-%m-%d").to_string(),
                    app_name,
                    app_path,
                    "keyboard",
                    format!("scan:{:02x}:ext:{}", data.scanCode, data.flags.0 & 1 != 0),
                    1,
                    1,
                    0,
                    0,
                );
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
        } else if msg == WM_TIMER {
            log_diagnostics(hwnd);
        } else if msg == WM_POWERBROADCAST {
            reset_input_state();
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn foreground_process() -> (String, String) {
        process_for_window(GetForegroundWindow())
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn point_process() -> (String, String) {
        let mut point = windows::Win32::Foundation::POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return ("Unknown".into(), "Unknown".into());
        }
        let hwnd = WindowFromPoint(point);
        let root = GetAncestor(hwnd, GA_ROOT);
        let root = if root.0.is_null() { hwnd } else { root };
        process_for_window(root)
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn process_for_window(hwnd: HWND) -> (String, String) {
        let mut pid = 0u32;
        if hwnd.0.is_null() || GetWindowThreadProcessId(hwnd, Some(&mut pid)) == 0 || pid == 0 {
            return ("Unknown".into(), "Unknown".into());
        }
        let cache = PROCESS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some((name, path, checked_at)) = cache
            .lock()
            .expect("process cache poisoned")
            .get(&pid)
            .cloned()
            && checked_at.elapsed() < Duration::from_secs(60)
        {
            return (name, path);
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
        let result = (name, path, Instant::now());
        let mut cache = cache.lock().expect("process cache poisoned");
        cache.retain(|_, (_, _, checked_at)| checked_at.elapsed() < Duration::from_secs(600));
        cache.insert(pid, result.clone());
        (result.0, result.1)
    }

    fn reset_input_state() {
        if let Some(pressed) = PRESSED_KEYS.get() {
            pressed.lock().expect("keyboard state poisoned").clear();
        }
        reset_wheel_state();
    }

    fn reset_wheel_state() {
        if let Some(remainder) = WHEEL_REMAINDER.get() {
            remainder.lock().expect("wheel state poisoned").clear();
        }
    }

    fn capture_enabled() -> bool {
        CAPTURE_ENABLED
            .get()
            .map(|enabled| enabled.load(Ordering::Relaxed))
            .unwrap_or(true)
    }

    fn mouse_registration_target_matches(hwnd: HWND) -> Option<bool> {
        let mut devices: [RAWINPUTDEVICE; 16] = std::array::from_fn(|_| RAWINPUTDEVICE::default());
        let mut capacity = devices.len() as u32;
        let count = unsafe {
            GetRegisteredRawInputDevices(
                Some(devices.as_mut_ptr()),
                &mut capacity,
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            )
        };
        if count == u32::MAX {
            return None;
        }
        Some(devices[..count as usize].iter().any(|device| {
            device.usUsagePage == 0x01 && device.usUsage == 0x02 && device.hwndTarget == hwnd
        }))
    }

    fn log_diagnostics(hwnd: HWND) {
        tracing::info!(
            mouse_registration_target_matches = ?mouse_registration_target_matches(hwnd),
            raw_input_messages = RAW_INPUT_MESSAGES.load(Ordering::Relaxed),
            raw_input_data_errors = RAW_INPUT_DATA_ERRORS.load(Ordering::Relaxed),
            raw_mouse_packets = RAW_MOUSE_PACKETS.load(Ordering::Relaxed),
            mouse_button_packets = RAW_MOUSE_BUTTON_PACKETS.load(Ordering::Relaxed),
            mouse_wheel_packets = RAW_MOUSE_WHEEL_PACKETS.load(Ordering::Relaxed),
            mouse_events_enqueued = MOUSE_EVENTS_ENQUEUED.load(Ordering::Relaxed),
            mouse_events_dropped = MOUSE_EVENTS_DROPPED.load(Ordering::Relaxed),
            mouse_unknown_attribution = MOUSE_UNKNOWN_ATTRIBUTION.load(Ordering::Relaxed),
            "mouse input diagnostic summary"
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn emit_event(
        date: String,
        app_name: String,
        app_path: String,
        input_type: &str,
        code: String,
        count: i64,
        keyboard_count: i64,
        mouse_count: i64,
        wheel_units: i64,
    ) {
        if !capture_enabled() {
            return;
        }
        if let Some(sender) = SENDER.get() {
            let first_logged = match input_type {
                "keyboard" => &FIRST_KEYBOARD_LOGGED,
                "mouse" => &FIRST_MOUSE_LOGGED,
                _ => &FIRST_KEYBOARD_LOGGED,
            };
            if !first_logged.swap(true, Ordering::AcqRel) {
                tracing::info!(input_type, "first input event received");
            }
            let result = sender.try_send(InputEvent {
                local_date: date,
                app_name,
                app_path,
                input_type: input_type.into(),
                code,
                count,
                keyboard_count,
                mouse_count,
                wheel_units,
                detailed: true,
            });
            if result.is_ok() && input_type == "mouse" {
                MOUSE_EVENTS_ENQUEUED.fetch_add(1, Ordering::Relaxed);
            }
            if result.is_err() {
                let dropped = DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
                if input_type == "mouse" {
                    MOUSE_EVENTS_DROPPED.fetch_add(1, Ordering::Relaxed);
                }
                if dropped.is_power_of_two() {
                    tracing::warn!(
                        dropped,
                        "input queue is full; aggregate events were dropped"
                    );
                }
            }
        }
    }

    fn normalize_wheel(app_path: &str, horizontal: bool, delta: i32) -> i64 {
        let remainder = WHEEL_REMAINDER.get_or_init(|| Mutex::new(HashMap::new()));
        let mut remainders = remainder.lock().expect("wheel state poisoned");
        if !remainders.contains_key(app_path)
            && remainders.len() >= 256
            && let Some(oldest) = remainders.keys().next().cloned()
        {
            remainders.remove(&oldest);
        }
        let value = remainders.entry(app_path.to_string()).or_default();
        let units = if horizontal {
            normalize_delta(&mut value.1, delta)
        } else {
            normalize_delta(&mut value.0, delta)
        };
        if value.0 == 0 && value.1 == 0 {
            remainders.remove(app_path);
        }
        units
    }

    fn normalize_delta(remainder: &mut i32, delta: i32) -> i64 {
        *remainder += delta;
        let units = *remainder / 120;
        *remainder %= 120;
        i64::from(units)
    }

    fn mouse_button_codes(flags: u16) -> impl Iterator<Item = &'static str> {
        [
            (0x0001, "left"),
            (0x0004, "right"),
            (0x0010, "middle"),
            (RI_MOUSE_BUTTON_4_DOWN, "x1"),
            (RI_MOUSE_BUTTON_5_DOWN, "x2"),
        ]
        .into_iter()
        .filter_map(move |(mask, code)| (flags & mask != 0).then_some(code))
    }

    fn signed_wheel_delta(button_data: u16) -> i32 {
        i32::from(i16::from_ne_bytes(button_data.to_ne_bytes()))
    }

    fn raw_payload_has_minimum(read_size: u32, payload_size: usize) -> bool {
        read_size
            >= (std::mem::size_of::<RAWINPUTHEADER>() as u32).saturating_add(payload_size as u32)
    }

    fn should_count_keydown(
        pressed: &mut HashSet<u32>,
        key_id: u32,
        physically_down: bool,
    ) -> bool {
        if pressed.contains(&key_id) && physically_down {
            return false;
        }
        pressed.remove(&key_id);
        pressed.insert(key_id)
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn handle_input(_hwnd: HWND, lparam: LPARAM) {
        RAW_INPUT_MESSAGES.fetch_add(1, Ordering::Relaxed);
        if lparam.0 == 0 {
            RAW_INPUT_DATA_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // A keyboard RAWINPUT payload can be smaller than the union's full
        // size, so keep a complete zeroed RAWINPUT object as the destination
        // instead of dereferencing a short byte buffer as a larger struct.
        let capacity = std::mem::size_of::<RAWINPUT>() as u32;
        let mut size = capacity;
        let mut raw = RAWINPUT::default();
        let read_size = GetRawInputData(
            windows::Win32::UI::Input::HRAWINPUT(lparam.0 as _),
            RID_INPUT,
            Some((&mut raw as *mut RAWINPUT).cast()),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if read_size == u32::MAX
            || read_size == 0
            || read_size > capacity
            || read_size < std::mem::size_of::<RAWINPUTHEADER>() as u32
            || raw.header.dwSize != read_size
        {
            RAW_INPUT_DATA_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
            if !raw_payload_has_minimum(read_size, std::mem::size_of::<RAWKEYBOARD>()) {
                RAW_INPUT_DATA_ERRORS.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if !USE_RAW_KEYBOARD.load(Ordering::Acquire) {
                return;
            }
            let date = Local::now().format("%Y-%m-%d").to_string();
            let k = raw.data.keyboard;
            let key_id =
                (u32::from(k.MakeCode) << 1) | u32::from(k.Flags & (RI_KEY_E0 | RI_KEY_E1) != 0);
            if k.Flags & RI_KEY_BREAK == 0 {
                let pressed = PRESSED_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
                let mut pressed = pressed.lock().expect("keyboard state poisoned");
                let physically_down = GetAsyncKeyState(k.VKey as i32) < 0;
                if !should_count_keydown(&mut pressed, key_id, physically_down) {
                    return;
                }
                drop(pressed);
                let (app_name, app_path) = foreground_process();
                emit_event(
                    date,
                    app_name,
                    app_path,
                    "keyboard",
                    format!(
                        "scan:{:02x}:ext:{}",
                        k.MakeCode,
                        k.Flags & (RI_KEY_E0 | RI_KEY_E1) != 0
                    ),
                    1,
                    1,
                    0,
                    0,
                );
            } else if let Some(pressed) = PRESSED_KEYS.get() {
                pressed
                    .lock()
                    .expect("keyboard state poisoned")
                    .remove(&key_id);
            }
            return;
        }
        if raw.header.dwType != RIM_TYPEMOUSE.0 {
            return;
        }
        if !raw_payload_has_minimum(read_size, std::mem::size_of::<RAWMOUSE>()) {
            RAW_INPUT_DATA_ERRORS.fetch_add(1, Ordering::Relaxed);
            return;
        }
        RAW_MOUSE_PACKETS.fetch_add(1, Ordering::Relaxed);
        if !capture_enabled() {
            reset_wheel_state();
            return;
        }
        let flags = raw.data.mouse.Anonymous.Anonymous.usButtonFlags;
        if flags & MOUSE_COUNTED_FLAGS == 0 {
            return;
        }
        let date = Local::now().format("%Y-%m-%d").to_string();
        let (app_name, app_path) = point_process();
        if app_path == "Unknown" {
            MOUSE_UNKNOWN_ATTRIBUTION.fetch_add(1, Ordering::Relaxed);
        }
        if mouse_button_codes(flags).next().is_some() {
            RAW_MOUSE_BUTTON_PACKETS.fetch_add(1, Ordering::Relaxed);
        }
        for code in mouse_button_codes(flags) {
            emit_event(
                date.clone(),
                app_name.clone(),
                app_path.clone(),
                "mouse",
                code.into(),
                1,
                0,
                1,
                0,
            );
        }
        let delta = signed_wheel_delta(raw.data.mouse.Anonymous.Anonymous.usButtonData);
        for (flag, horizontal, code) in [
            (RI_MOUSE_WHEEL, false, "wheel"),
            (RI_MOUSE_HWHEEL, true, "hwheel"),
        ] {
            if flags & flag != 0 {
                RAW_MOUSE_WHEEL_PACKETS.fetch_add(1, Ordering::Relaxed);
                let units = normalize_wheel(&app_path, horizontal, delta);
                if units != 0 {
                    emit_event(
                        date.clone(),
                        app_name.clone(),
                        app_path.clone(),
                        "mouse",
                        format!("{}_{}", code, if units > 0 { "up" } else { "down" }),
                        units.abs(),
                        0,
                        0,
                        units.abs(),
                    );
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::mouse_button_codes;
        use super::normalize_delta;
        use super::normalize_wheel;
        use super::raw_payload_has_minimum;
        use super::reset_input_state;
        use super::should_count_keydown;
        use super::signed_wheel_delta;
        use super::{
            MOUSE_COUNTED_FLAGS, RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_HWHEEL,
            RI_MOUSE_WHEEL,
        };
        use std::collections::HashSet;

        #[test]
        fn raw_input_payload_length_rejects_truncated_packets() {
            let mouse_payload = std::mem::size_of::<super::RAWMOUSE>();
            let header = std::mem::size_of::<super::RAWINPUTHEADER>() as u32;
            assert!(!raw_payload_has_minimum(
                header + mouse_payload as u32 - 1,
                mouse_payload
            ));
            assert!(raw_payload_has_minimum(
                header + mouse_payload as u32,
                mouse_payload
            ));
        }

        #[test]
        fn mouse_movement_and_button_release_skip_attribution() {
            for ignored in [0, 0x0002, 0x0008, 0x0020, 0x0080, 0x0200] {
                assert_eq!(ignored & MOUSE_COUNTED_FLAGS, 0);
            }
            for counted in [0x0001, 0x0004, 0x0010, 0x0040, 0x0100, 0x0400, 0x0800] {
                assert_ne!(counted & MOUSE_COUNTED_FLAGS, 0);
            }
        }

        #[test]
        fn high_precision_wheel_accumulates_until_one_detent() {
            let mut remainder = 0;
            assert_eq!(normalize_delta(&mut remainder, 30), 0);
            assert_eq!(normalize_delta(&mut remainder, 30), 0);
            assert_eq!(normalize_delta(&mut remainder, 30), 0);
            assert_eq!(normalize_delta(&mut remainder, 30), 1);
            assert_eq!(remainder, 0);
            assert_eq!(normalize_delta(&mut remainder, -240), -2);
        }

        #[test]
        fn wheel_remainders_are_isolated_by_application() {
            reset_input_state();
            assert_eq!(normalize_wheel("a.exe", false, 30), 0);
            assert_eq!(normalize_wheel("b.exe", false, 90), 0);
            assert_eq!(normalize_wheel("a.exe", false, 90), 1);
            assert_eq!(normalize_wheel("a.exe", true, 120), 1);
            reset_input_state();
        }

        #[test]
        fn repeated_keydown_is_filtered_until_keyup() {
            let mut pressed = HashSet::new();
            assert!(should_count_keydown(&mut pressed, 42, true));
            assert!(!should_count_keydown(&mut pressed, 42, true));
            pressed.remove(&42);
            assert!(should_count_keydown(&mut pressed, 42, true));
        }

        #[test]
        fn raw_mouse_wheel_flags_match_win32_values() {
            assert_eq!(RI_MOUSE_WHEEL, 0x0400);
            assert_eq!(RI_MOUSE_HWHEEL, 0x0800);
        }

        #[test]
        fn raw_mouse_xbutton_down_flags_match_win32_values() {
            assert_eq!(RI_MOUSE_BUTTON_4_DOWN, 0x0040);
            assert_eq!(RI_MOUSE_BUTTON_5_DOWN, 0x0100);
        }

        #[test]
        fn raw_mouse_button_flags_map_all_supported_buttons() {
            let flags = 0x0001 | 0x0004 | 0x0010 | RI_MOUSE_BUTTON_4_DOWN | RI_MOUSE_BUTTON_5_DOWN;
            assert_eq!(
                mouse_button_codes(flags).collect::<Vec<_>>(),
                vec!["left", "right", "middle", "x1", "x2"]
            );
        }

        #[test]
        fn raw_mouse_button_data_is_signed() {
            assert_eq!(signed_wheel_delta(120), 120);
            assert_eq!(signed_wheel_delta((-120i16) as u16), -120);
        }
    }
}

#[cfg(not(windows))]
mod raw_input {
    use super::*;
    pub fn run(_tx: SyncSender<InputEvent>, _enabled: Arc<AtomicBool>) {
        thread::park();
    }
}
