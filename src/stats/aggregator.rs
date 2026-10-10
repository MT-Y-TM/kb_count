use crate::stats::event::InputEvent;
use crate::stats::policy::{CaptureMode, normalize_app_path};
use std::collections::HashMap;
#[derive(Default)]
#[allow(dead_code)]
pub struct Aggregator {
    counts: HashMap<(String, String, String, String), (i64, bool)>,
}
#[allow(dead_code)]
impl Aggregator {
    pub fn record(&mut self, mut event: InputEvent, mode: CaptureMode) {
        if mode == CaptureMode::Excluded {
            return;
        }
        if mode == CaptureMode::TotalsOnly {
            event.code = "total".into();
            event.detailed = false;
        }
        event.app_path = normalize_app_path(&event.app_path);
        let key = (
            event.local_date.clone(),
            event.app_path.clone(),
            event.input_type.clone(),
            event.code.clone(),
        );
        let entry = self.counts.entry(key).or_insert((0, event.detailed));
        entry.0 += event.count;
        entry.1 &= event.detailed;
    }
    pub fn drain(&mut self) -> Vec<InputEvent> {
        self.counts
            .drain()
            .map(|((date, path, kind, code), (count, detailed))| {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|x| x.to_str())
                    .unwrap_or("Unknown")
                    .to_string();
                InputEvent {
                    local_date: date,
                    app_name: name,
                    app_path: path,
                    input_type: kind.clone(),
                    code,
                    count,
                    keyboard_count: if kind == "keyboard" { count } else { 0 },
                    mouse_count: if kind == "mouse" { count } else { 0 },
                    wheel_units: if kind == "wheel" { count } else { 0 },
                    detailed,
                }
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excluded_is_not_persisted() {
        let mut aggregator = Aggregator::default();
        aggregator.record(
            InputEvent {
                local_date: "2026-10-11".into(),
                app_name: "x".into(),
                app_path: "x.exe".into(),
                input_type: "keyboard".into(),
                code: "scan:1".into(),
                count: 1,
                keyboard_count: 1,
                mouse_count: 0,
                wheel_units: 0,
                detailed: true,
            },
            CaptureMode::Excluded,
        );
        assert!(aggregator.drain().is_empty());
    }
    #[test]
    fn totals_only_collapses_code() {
        let mut aggregator = Aggregator::default();
        aggregator.record(
            InputEvent {
                local_date: "2026-10-11".into(),
                app_name: "x".into(),
                app_path: "x.exe".into(),
                input_type: "keyboard".into(),
                code: "scan:1".into(),
                count: 2,
                keyboard_count: 2,
                mouse_count: 0,
                wheel_units: 0,
                detailed: true,
            },
            CaptureMode::TotalsOnly,
        );
        let values = aggregator.drain();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].code, "total");
    }
}
