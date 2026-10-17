//! Session-only icons. Disk access and Win32 extraction never run on the UI or
//! collector threads. Only an open panel requests work; failure keeps initials.
use crate::{MainWindow, panel_is_presented};
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{SyncSender, sync_channel};

const CACHE_LIMIT: usize = 128;
const QUEUE_LIMIT: usize = 32;
const ICON_SIZE: u32 = 96;

#[derive(Default)]
struct IconCache {
    images: HashMap<String, slint::Image>,
    order: VecDeque<String>,
    pending: HashSet<String>,
    sender: Option<SyncSender<(String, slint::Weak<MainWindow>)>>,
    deferred: bool,
}

thread_local! {
    static CACHE: RefCell<IconCache> = RefCell::default();
}

pub fn cached(path: &str) -> slint::Image {
    CACHE.with_borrow(|cache| cache.images.get(path).cloned().unwrap_or_default())
}

#[cfg(test)]
pub fn pending_count() -> usize {
    CACHE.with_borrow(|cache| cache.pending.len())
}

impl IconCache {
    fn insert(&mut self, path: String, image: slint::Image) {
        self.pending.remove(&path);
        self.order.retain(|entry| entry != &path);
        self.order.push_back(path.clone());
        self.images.insert(path, image);
        while self.order.len() > CACHE_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.images.remove(&oldest);
            }
        }
    }
}

pub fn populate(ui: &MainWindow) {
    if !panel_is_presented(ui) {
        return;
    }
    CACHE.with_borrow_mut(|cache| {
        // Prioritize the selected app and the filtered ranking. A stable bounded
        // working set prevents cache eviction from starting an endless reload.
        let rows = ui.get_app_rows();
        let paths = ui
            .get_detail_loaded()
            .then(|| ui.get_detail_path().to_string())
            .into_iter()
            .chain(
                rows.iter()
                    .take(CACHE_LIMIT - 1)
                    .map(|row| row.path.to_string()),
            );
        for path in paths {
            if !local_executable_path(&path)
                || cache.images.contains_key(&path)
                || cache.pending.contains(&path)
                || cache.pending.len() >= QUEUE_LIMIT
            {
                continue;
            }
            if cache.sender.is_none() {
                cache.sender = start_worker();
            }
            if let Some(sender) = &cache.sender
                && sender.try_send((path.clone(), ui.as_weak())).is_ok()
            {
                cache.pending.insert(path);
            }
        }
    });
}

// Completed requests may enter the cache while hidden/minimized. Apply their
// results on return even if no database batch has changed in the meantime.
pub fn refresh_deferred(ui: &MainWindow) {
    if !panel_is_presented(ui) {
        return;
    }
    let images = CACHE.with_borrow_mut(|cache| {
        if !cache.deferred {
            return None;
        }
        cache.deferred = false;
        Some(cache.images.clone())
    });
    let Some(images) = images else {
        return;
    };
    let model = ui.get_app_rows();
    for index in 0..model.row_count() {
        if let Some(mut row) = model.row_data(index)
            && let Some(icon) = images.get(row.path.as_str())
        {
            row.has_icon = icon.size().width > 0;
            row.icon = icon.clone();
            model.set_row_data(index, row);
        }
    }
    if let Some(icon) = images.get(ui.get_detail_path().as_str()) {
        ui.set_detail_has_icon(icon.size().width > 0);
        ui.set_detail_icon(icon.clone());
    }
    populate(ui);
}

fn start_worker() -> Option<SyncSender<(String, slint::Weak<MainWindow>)>> {
    let (sender, receiver) = sync_channel::<(String, slint::Weak<MainWindow>)>(QUEUE_LIMIT);
    std::thread::Builder::new()
        .name("kb-app-icons".into())
        .spawn(move || {
            while let Ok((path, ui)) = receiver.recv() {
                let pixels = extract(&path);
                if slint::invoke_from_event_loop(move || {
                    let icon = pixels
                        .map(|pixels| {
                            let mut buffer = slint::SharedPixelBuffer::new(ICON_SIZE, ICON_SIZE);
                            buffer.make_mut_slice().copy_from_slice(&pixels);
                            slint::Image::from_rgba8(buffer)
                        })
                        .unwrap_or_default();
                    CACHE.with_borrow_mut(|cache| cache.insert(path.clone(), icon.clone()));
                    if let Some(ui) = ui.upgrade() {
                        if panel_is_presented(&ui) {
                            let model = ui.get_app_rows();
                            for index in 0..model.row_count() {
                                if let Some(mut row) = model.row_data(index)
                                    && row.path.as_str() == path
                                {
                                    row.has_icon = icon.size().width > 0;
                                    row.icon = icon.clone();
                                    model.set_row_data(index, row);
                                }
                            }
                            if ui.get_detail_path().as_str() == path {
                                ui.set_detail_has_icon(icon.size().width > 0);
                                ui.set_detail_icon(icon);
                            }
                        } else {
                            CACHE.with_borrow_mut(|cache| cache.deferred = true);
                        }
                        // Continue a bounded batch, without timers or per-input work.
                        populate(&ui);
                    }
                })
                .is_err()
                {
                    break;
                }
            }
        })
        .ok()?;
    Some(sender)
}

fn local_executable_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 7
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && !bytes.contains(&0)
        && path.to_ascii_lowercase().ends_with(".exe")
}

#[cfg(not(windows))]
fn extract(_: &str) -> Option<Vec<slint::Rgba8Pixel>> {
    None
}

#[cfg(windows)]
fn extract(path: &str) -> Option<Vec<slint::Rgba8Pixel>> {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
    use windows::core::PCWSTR;
    if !local_executable_path(path) {
        return None;
    }
    let wide = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let drive = [wide[0], wide[1], b'\\' as u16, 0];
    // Skip mapped network drives as well as UNC paths. Do not ask the Shell to
    // resolve associations or use an online icon service.
    if !matches!(unsafe { GetDriveTypeW(PCWSTR(drive.as_ptr())) }, 2 | 3 | 6) {
        return None;
    }
    struct OwnedIcon(HICON);
    impl Drop for OwnedIcon {
        fn drop(&mut self) {
            let _ = unsafe { DestroyIcon(self.0) };
        }
    }
    let mut handle = HICON::default();
    let result = unsafe {
        SHDefExtractIconW(
            PCWSTR(wide.as_ptr()),
            0,
            0,
            Some(&mut handle),
            None,
            ICON_SIZE,
        )
    };
    if handle.is_invalid() {
        return None;
    }
    let icon = OwnedIcon(handle);
    if result.is_err() {
        return None;
    }
    let black = render_icon(icon.0, 0)?;
    let white = render_icon(icon.0, 255)?;
    let pixels = recover_alpha(&black, &white);
    pixels.iter().any(|pixel| pixel.a > 0).then_some(pixels)
}

#[cfg(windows)]
fn render_icon(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    background: u8,
) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx};
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ICON_SIZE as i32,
                biHeight: -(ICON_SIZE as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                let _ = DeleteDC(dc);
                return None;
            }
        };
        if bits.is_null() {
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap);
        if previous.is_invalid() {
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(dc);
            return None;
        }
        let length = (ICON_SIZE * ICON_SIZE * 4) as usize;
        std::ptr::write_bytes(bits.cast::<u8>(), background, length);
        let drawn = DrawIconEx(
            dc,
            0,
            0,
            icon,
            ICON_SIZE as i32,
            ICON_SIZE as i32,
            0,
            None,
            DI_NORMAL,
        )
        .is_ok();
        let flushed = GdiFlush().as_bool();
        let output = (drawn && flushed)
            .then(|| std::slice::from_raw_parts(bits.cast::<u8>(), length).to_vec());
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(dc);
        output
    }
}

// Recover straight alpha from black/white compositing. This also handles older
// mask-based icons whose DIB alpha channel is zero, without a black rectangle.
fn recover_alpha(black: &[u8], white: &[u8]) -> Vec<slint::Rgba8Pixel> {
    black
        .chunks_exact(4)
        .zip(white.chunks_exact(4))
        .map(|(black, white)| {
            let alpha = 255
                - white[0]
                    .saturating_sub(black[0])
                    .max(white[1].saturating_sub(black[1]))
                    .max(white[2].saturating_sub(black[2]));
            let channel = |value: u8| {
                if alpha == 0 {
                    0
                } else {
                    (u32::from(value) * 255 / u32::from(alpha)).min(255) as u8
                }
            };
            slint::Rgba8Pixel {
                r: channel(black[2]),
                g: channel(black[1]),
                b: channel(black[0]),
                a: alpha,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_stays_bounded_and_replaces_duplicate_entries() {
        let mut cache = IconCache::default();
        for index in 0..CACHE_LIMIT * 4 {
            cache.insert(format!("app-{index}"), slint::Image::default());
        }
        assert_eq!(cache.images.len(), CACHE_LIMIT);
        assert_eq!(cache.order.len(), CACHE_LIMIT);
        assert!(!cache.images.contains_key("app-0"));
        let last = cache.order.back().unwrap().clone();
        cache.insert(last, slint::Image::default());
        assert_eq!(cache.images.len(), CACHE_LIMIT);
        assert_eq!(cache.order.len(), CACHE_LIMIT);
    }
    #[test]
    fn alpha_recovery_handles_transparent_opaque_and_translucent_pixels() {
        let black = [0, 0, 0, 0, 40, 80, 200, 0, 0, 0, 128, 0];
        let white = [255, 255, 255, 0, 40, 80, 200, 0, 127, 127, 255, 0];
        let pixels = recover_alpha(&black, &white);
        assert_eq!(
            pixels[0],
            slint::Rgba8Pixel {
                r: 0,
                g: 0,
                b: 0,
                a: 0
            }
        );
        assert_eq!(
            pixels[1],
            slint::Rgba8Pixel {
                r: 200,
                g: 80,
                b: 40,
                a: 255
            }
        );
        assert_eq!(
            pixels[2],
            slint::Rgba8Pixel {
                r: 255,
                g: 0,
                b: 0,
                a: 128
            }
        );
    }
    #[test]
    fn unknown_relative_and_remote_paths_keep_the_fallback() {
        for path in [
            "Unknown",
            "app.exe",
            r"\\server\share\app.exe",
            r"\\?\UNC\server\app.exe",
            "C:\\bad\0.exe",
        ] {
            assert!(!local_executable_path(path));
        }
        assert!(local_executable_path(r"C:\Program Files\应用\app.exe"));
        assert!(extract(r"C:\kb_count_nonexistent_directory\missing.exe").is_none());
    }
    #[cfg(windows)]
    #[test]
    fn windows_executable_icon_contains_visible_and_transparent_pixels() {
        let system = std::env::var("SystemRoot").expect("Windows directory");
        let path = format!(r"{system}\System32\WindowsPowerShell\v1.0\powershell.exe");
        let pixels = extract(&path).expect("extract PowerShell's embedded icon");
        assert_eq!(pixels.len(), (ICON_SIZE * ICON_SIZE) as usize);
        assert!(pixels.iter().any(|pixel| pixel.a > 0));
        assert!(pixels.iter().any(|pixel| pixel.a == 0));
    }

    #[cfg(windows)]
    #[test]
    fn embedded_brand_resource_loads_at_common_windows_icon_sizes() {
        use windows::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            DestroyIcon, GetIconInfo, HICON, ICONINFO, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
        };
        use windows::core::PCWSTR;
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            for size in [16, 20, 24, 28, 32, 40, 48, 56, 64, 96, 128, 256] {
                let handle = LoadImageW(
                    instance,
                    PCWSTR(std::ptr::without_provenance::<u16>(1)),
                    IMAGE_ICON,
                    size,
                    size,
                    LR_DEFAULTCOLOR,
                )
                .expect("embedded brand icon");
                let icon = HICON(handle.0);
                let mut info = ICONINFO::default();
                let read = GetIconInfo(icon, &mut info);
                let mut bitmap = BITMAP::default();
                let bitmap_read = GetObjectW(
                    info.hbmColor,
                    std::mem::size_of::<BITMAP>() as i32,
                    Some((&mut bitmap as *mut BITMAP).cast()),
                );
                let _ = DeleteObject(info.hbmColor);
                let _ = DeleteObject(info.hbmMask);
                let _ = DestroyIcon(icon);
                assert!(read.is_ok());
                assert_ne!(bitmap_read, 0);
                assert_eq!((bitmap.bmWidth, bitmap.bmHeight), (size, size));
            }
        }
    }
}
