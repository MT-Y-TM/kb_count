#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Detailed,
    TotalsOnly,
    Excluded,
}
#[allow(dead_code)]
impl CaptureMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "totals_only" => Self::TotalsOnly,
            "excluded" => Self::Excluded,
            _ => Self::Detailed,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detailed => "detailed",
            Self::TotalsOnly => "totals_only",
            Self::Excluded => "excluded",
        }
    }
    pub fn for_application(path: &str) -> Self {
        let lower = normalize_app_path(path);
        if [
            "chrome.exe",
            "msedge.exe",
            "firefox.exe",
            "brave.exe",
            "opera.exe",
        ]
        .iter()
        .any(|name| lower.ends_with(name))
        {
            Self::TotalsOnly
        } else if [
            "keepass.exe",
            "keepassxc.exe",
            "1password.exe",
            "bitwarden.exe",
        ]
        .iter()
        .any(|name| lower.ends_with(name))
        {
            Self::Excluded
        } else {
            Self::Detailed
        }
    }

    pub fn from_overrides(path: &str, overrides: &std::collections::HashMap<String, Self>) -> Self {
        overrides
            .get(&normalize_app_path(path))
            .copied()
            .unwrap_or_else(|| Self::for_application(path))
    }
}

/// Normalize the executable identity used for policy and statistics joins.
/// Windows paths are case-insensitive, so keeping the identity lower-case
/// prevents separate rows when the same process is reported with different
/// casing. The original input text is never persisted as an event sequence.
pub fn normalize_app_path(path: &str) -> String {
    let value = path.trim().trim_matches('"');
    if value.is_empty() || value.eq_ignore_ascii_case("unknown") {
        return "Unknown".into();
    }
    value.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{CaptureMode, normalize_app_path};

    #[test]
    fn normalizes_identity_without_merging_unknown_with_real_paths() {
        assert_eq!(
            normalize_app_path(" C:/Apps/Editor.EXE "),
            "c:/apps/editor.exe"
        );
        assert_eq!(normalize_app_path("unknown"), "Unknown");
        assert_eq!(
            CaptureMode::for_application("C:/Apps/Chrome.EXE"),
            CaptureMode::TotalsOnly
        );
    }
}
