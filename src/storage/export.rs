use crate::storage::database::Database;
use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::path::Path;
pub fn export_date(db: &Database, date: &str, path: &Path) -> Result<()> {
    let mut file = File::create(path)?;
    writeln!(
        file,
        "local_date,application_path,keyboard_count,mouse_click_count,wheel_units"
    )?;
    for row in db.app_totals(date)? {
        writeln!(
            file,
            "{},{},{},{},{}",
            date,
            csv_escape(&row.app_path),
            row.keyboard,
            row.mouse,
            row.wheel
        )?;
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
