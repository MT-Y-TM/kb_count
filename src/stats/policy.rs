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
        let lower = path.to_ascii_lowercase();
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
            .get(path)
            .copied()
            .unwrap_or_else(|| Self::for_application(path))
    }
}
