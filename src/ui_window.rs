//! Keep the panel within the Windows work area when reopening it from the tray.
#[cfg(windows)]
pub fn fit_to_work_area(window: &slint::Window) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetClientRect, GetWindowRect, GetWindowThreadProcessId,
    };
    use windows::core::w;

    if window.is_maximized() || window.is_minimized() {
        return;
    }
    // Slint owns the HWND; only use Win32 to inspect its monitor and frame.
    // All sizing/position changes still go through Slint's event loop.
    unsafe {
        let Ok(hwnd) = FindWindowW(None, w!("kb_count")) else {
            return;
        };
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != std::process::id() {
            return;
        }
        let mut outer = RECT::default();
        let mut client = RECT::default();
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetWindowRect(hwnd, &mut outer).is_err()
            || GetClientRect(hwnd, &mut client).is_err()
            || !GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
            .as_bool()
        {
            return;
        }
        let work = monitor.rcWork;
        let frame_width = (outer.right - outer.left - client.right).max(0);
        let frame_height = (outer.bottom - outer.top - client.bottom).max(0);
        let width = client.right.min(work.right - work.left - frame_width);
        let height = client.bottom.min(work.bottom - work.top - frame_height);
        if width <= 0 || height <= 0 {
            return;
        }
        window.set_size(slint::PhysicalSize::new(width as u32, height as u32));
        // Read back the effective size because Slint enforces its logical minimum.
        let size = window.size();
        let x = outer.left.clamp(
            work.left,
            (work.right - size.width as i32 - frame_width).max(work.left),
        );
        let y = outer.top.clamp(
            work.top,
            (work.bottom - size.height as i32 - frame_height).max(work.top),
        );
        window.set_position(slint::PhysicalPosition::new(x, y));
    }
}
