#[cfg(windows)]
#[allow(dead_code)]
pub mod windows {
    use std::path::Path;
    use windows::core::w;
    use windows::Win32::System::Registry::{RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ};
    const RUN_KEY: &windows::core::PCWSTR = &w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const VALUE_NAME: &windows::core::PCWSTR = &w!("kb_count");
    pub fn set_enabled(exe:&Path, enabled:bool)->windows::core::Result<()> {
        unsafe {
            let mut key=HKEY::default();
            let status=RegOpenKeyExW(HKEY_CURRENT_USER, *RUN_KEY, 0, KEY_SET_VALUE, &mut key);
            status.ok()?;
            let result=if enabled { let mut text=exe.as_os_str().to_string_lossy().replace('"', "").encode_utf16().collect::<Vec<_>>(); text.push(0); RegSetValueExW(key, *VALUE_NAME, 0, REG_SZ, Some(bytemuck_slice(&text))) } else { RegDeleteValueW(key, *VALUE_NAME) };
            let _=RegCloseKey(key); result.ok()
        }
    }
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn bytemuck_slice(values:&[u16])->&[u8] { std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len()*2) }
    #[allow(dead_code)] pub fn is_configured()->bool { false }
}
#[cfg(not(windows))]
#[allow(dead_code)]
pub mod windows { use std::path::Path; pub fn set_enabled(_: &Path, _: bool)->Result<(), std::io::Error>{ Ok(()) } }

