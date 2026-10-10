use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use crate::stats::event::InputEvent;

pub fn start() -> Receiver<InputEvent> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new().name("kb-input-collector".into()).spawn(move || raw_input::run(tx)).expect("unable to start collector");
    rx
}

#[cfg(windows)]
mod raw_input {
    use super::*;
    use chrono::Local;
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::{GetRawInputData, RegisterRawInputDevices, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE, RIDEV_DEVNOTIFY};
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP, WM_INPUT};
    static SENDER: OnceLock<Sender<InputEvent>> = OnceLock::new();

    pub fn run(tx: Sender<InputEvent>) {
        let _ = SENDER.set(tx);
        unsafe {
            let instance = GetModuleHandleW(None).expect("module handle");
            let class = WNDCLASSW { style: CS_HREDRAW | CS_VREDRAW, lpfnWndProc: Some(window_proc), hInstance: instance.into(), lpszClassName: w!("kb_count_raw_input"), ..Default::default() };
            if RegisterClassW(&class) == 0 { return; }
            let hwnd = match CreateWindowExW(WS_EX_TOOLWINDOW, w!("kb_count_raw_input"), w!("kb_count"), WS_POPUP, 0, 0, 0, 0, None, None, instance, None) { Ok(value) => value, Err(_) => return };
            if hwnd.0.is_null() { return; }
            let devices = [
                RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY, hwndTarget: hwnd },
                RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY, hwndTarget: hwnd },
            ];
            if RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_err() { return; }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() { let _ = TranslateMessage(&msg); DispatchMessageW(&msg); }
        }
    }

    extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_INPUT { unsafe { handle_input(hwnd, lparam); } }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn handle_input(_hwnd: HWND, lparam: LPARAM) {
        let mut size = 0u32;
        if GetRawInputData(windows::Win32::UI::Input::HRAWINPUT(lparam.0 as _), RID_INPUT, None, &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32) == u32::MAX || size == 0 { return; }
        let mut bytes = vec![0u8; size as usize];
        if GetRawInputData(windows::Win32::UI::Input::HRAWINPUT(lparam.0 as _), RID_INPUT, Some(bytes.as_mut_ptr() as *mut c_void), &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32) == u32::MAX { return; }
        let raw = &*(bytes.as_ptr() as *const RAWINPUT);
        let date = Local::now().format("%Y-%m-%d").to_string();
        let (kind, code, count, key, mouse, wheel) = match raw.header.dwType {
            x if x == RIM_TYPEKEYBOARD.0 => { let k = raw.data.keyboard; if k.Message != 0x0100 { return; } ("keyboard", format!("scan:{:02x}:ext:{}", k.MakeCode, (k.Flags & 1) != 0), 1, 1, 0, 0) }
            x if x == RIM_TYPEMOUSE.0 => { let m = raw.data.mouse; let button = match m.Anonymous.Anonymous.usButtonFlags { 0x0001=>"left",0x0002=>"left_up",0x0004=>"right",0x0008=>"right_up",0x0010=>"middle",0x0020=>"middle_up",0x0040=>"x1",0x0080=>"x2",0x0100=>"wheel",0x0200=>"hwheel",_=>return }; let is_wheel=button=="wheel"||button=="hwheel"; (if is_wheel{"wheel"}else{"mouse"}, button.to_string(), 1, 0, if is_wheel{0}else{1}, if is_wheel{1}else{0}) }
            _ => return,
        };
        if let Some(sender)=SENDER.get() { let _=sender.send(InputEvent{local_date:date,app_name:"Unknown".into(),app_path:"Unknown".into(),input_type:kind.into(),code,count,keyboard_count:key,mouse_count:mouse,wheel_units:wheel}); }
    }
}

#[cfg(not(windows))]
mod raw_input { use super::*; pub fn run(_tx: Sender<InputEvent>) { thread::park(); } }



