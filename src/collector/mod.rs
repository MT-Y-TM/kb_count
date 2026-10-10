use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use crate::stats::event::InputEvent;

pub fn start() -> Receiver<InputEvent> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new().name("kb-input-collector".into()).spawn(move || {
        raw_input::run(tx);
    }).expect("unable to start collector");
    rx
}

#[cfg(windows)]
mod raw_input {
    use super::*;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG, TranslateMessage};
    pub fn run(_tx: Sender<InputEvent>) {
        // The message-loop boundary is deliberately isolated from aggregation/UI.
        // Raw Input registration is added here for the Windows release target.
        unsafe { let mut msg = MSG::default(); while GetMessageW(&mut msg, HWND(0), 0, 0).into() { TranslateMessage(&msg); DispatchMessageW(&msg); } }
    }
    #[allow(dead_code)] fn wnd_proc(_hwnd: HWND, _msg: u32, _w: WPARAM, _l: LPARAM) -> LRESULT { LRESULT(0) }
}

#[cfg(not(windows))]
mod raw_input {
    use super::*;
    pub fn run(_tx: Sender<InputEvent>) { thread::park(); }
}
