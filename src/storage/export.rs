use crate::storage::database::Database;
use anyhow::Result;
use chrono::{Duration, NaiveDate};
use std::fs::File;
use std::io::Write;
use std::path::Path;
#[allow(dead_code)]
pub fn export_date(db: &Database, date: &str, path: &Path) -> Result<()> {
    export_range(db, date, date, path)
}

pub fn export_range(db: &Database, from: &str, to: &str, path: &Path) -> Result<()> {
    let start = NaiveDate::parse_from_str(from, "%Y-%m-%d")?;
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d")?;
    if start > end {
        anyhow::bail!("导出开始日期不能晚于结束日期");
    }
    let mut file = File::create(path)?;
    writeln!(
        file,
        "local_date,application_path,keyboard_count,mouse_click_count,wheel_units"
    )?;
    let mut date = start;
    while date <= end {
        let date_text = date.format("%Y-%m-%d").to_string();
        for row in db.app_totals(&date_text)? {
            writeln!(
                file,
                "{},{},{},{},{}",
                date_text,
                csv_escape(&row.app_path),
                row.keyboard,
                row.mouse,
                row.wheel
            )?;
        }
        date += Duration::days(1);
    }
    Ok(())
}
fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::event::InputEvent;
    use std::path::Path;

    #[test]
    fn range_export_contains_each_date() {
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        db.record_batch(&[
            InputEvent {
                local_date: "2026-10-10".into(),
                app_name: "a.exe".into(),
                app_path: "C:/a.exe".into(),
                input_type: "keyboard".into(),
                code: "scan:1".into(),
                count: 2,
                keyboard_count: 2,
                mouse_count: 0,
                wheel_units: 0,
                detailed: true,
            },
            InputEvent {
                local_date: "2026-10-11".into(),
                app_name: "a.exe".into(),
                app_path: "C:/a.exe".into(),
                input_type: "mouse".into(),
                code: "left".into(),
                count: 1,
                keyboard_count: 0,
                mouse_count: 1,
                wheel_units: 0,
                detailed: true,
            },
        ])
        .expect("record");
        let path = std::env::temp_dir().join(format!("kb_count_export_{}.csv", std::process::id()));
        export_range(&db, "2026-10-10", "2026-10-11", &path).expect("export");
        let text = std::fs::read_to_string(&path).expect("read export");
        let _ = std::fs::remove_file(path);
        assert!(text.contains(r"2026-10-10,c:\a.exe,2,0,0"));
        assert!(text.contains(r"2026-10-11,c:\a.exe,0,1,0"));
    }
}
