#[cfg(windows)]
#[allow(dead_code)]
pub mod windows {
    use std::path::Path;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE,
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows::core::w;
    const RUN_KEY: &windows::core::PCWSTR =
        &w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const VALUE_NAME: &windows::core::PCWSTR = &w!("kb_count");
    pub fn set_enabled(exe: &Path, enabled: bool) -> windows::core::Result<()> {
        unsafe {
            let mut key = HKEY::default();
            let status = RegOpenKeyExW(HKEY_CURRENT_USER, *RUN_KEY, 0, KEY_SET_VALUE, &mut key);
            status.ok()?;
            let result = if enabled {
                let path = exe.as_os_str().to_string_lossy().replace('"', "");
                let mut text = format!("\"{path}\"").encode_utf16().collect::<Vec<_>>();
                text.push(0);
                RegSetValueExW(key, *VALUE_NAME, 0, REG_SZ, Some(bytemuck_slice(&text)))
            } else {
                RegDeleteValueW(key, *VALUE_NAME)
            };
            let _ = RegCloseKey(key);
            result.ok()
        }
    }
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn bytemuck_slice(values: &[u16]) -> &[u8] {
        std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len() * 2)
    }
    #[allow(dead_code)]
    pub fn is_configured(exe: &Path) -> windows::core::Result<bool> {
        unsafe {
            let mut key = HKEY::default();
            let status = RegOpenKeyExW(HKEY_CURRENT_USER, *RUN_KEY, 0, KEY_QUERY_VALUE, &mut key);
            if status.is_err() {
                return Ok(false);
            }
            let mut value_type = REG_VALUE_TYPE(0);
            let mut bytes = vec![0u8; 32768];
            let mut size = bytes.len() as u32;
            let status = RegQueryValueExW(
                key,
                *VALUE_NAME,
                None,
                Some(&mut value_type as *mut _),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            );
            let _ = RegCloseKey(key);
            if status.is_err() || value_type != REG_SZ {
                return Ok(false);
            }
            let value = String::from_utf16_lossy(std::slice::from_raw_parts(
                bytes.as_ptr() as *const u16,
                (size as usize / 2).saturating_sub(1),
            ))
            .trim_matches('"')
            .to_string();
            Ok(value.eq_ignore_ascii_case(&exe.to_string_lossy()))
        }
    }
}
#[cfg(not(windows))]
#[allow(dead_code)]
pub mod windows {
    use std::path::Path;
    pub fn set_enabled(_: &Path, _: bool) -> Result<(), std::io::Error> {
        Ok(())
    }
    pub fn is_configured(_: &Path) -> Result<bool, std::io::Error> {
        Ok(false)
    }
}
