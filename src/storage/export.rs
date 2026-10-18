use crate::storage::database::Database;
use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags, params};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed(u64),
    Cancelled,
}

struct TemporaryExport(PathBuf, bool);
impl Drop for TemporaryExport {
    fn drop(&mut self) {
        if !self.1 {
            let _ = fs::remove_file(&self.0);
        }
    }
}

fn temporary_file(parent: &Path) -> Result<(TemporaryExport, File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..32 {
        let path = parent.join(format!(
            ".kb_count_export_{}_{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((TemporaryExport(path, false), file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    bail!("无法创建导出临时文件，请重试")
}

fn resolved(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    Ok(path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?
        .join(path.file_name().context("缺少导出文件名")?))
}

fn protect_data(source: &Path, target: &Path) -> Result<()> {
    let source = resolved(source)?;
    let target = resolved(target)?.to_string_lossy().to_lowercase();
    let mut protected = vec![
        source.clone(),
        source
            .parent()
            .context("数据库目录无效")?
            .join("config.toml"),
    ];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut path = source.as_os_str().to_owned();
        path.push(suffix);
        protected.push(path.into());
    }
    if let Ok(exe) = std::env::current_exe() {
        protected.push(resolved(&exe)?);
    }
    if protected
        .iter()
        .any(|p| p.to_string_lossy().to_lowercase() == target)
    {
        bail!("导出不能覆盖 kb_count 的数据库、配置或程序文件，请选择其他文件名")
    }
    Ok(())
}

pub fn export_snapshot(
    database: &Path,
    from: &str,
    to: &str,
    target: &Path,
    cancelled: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<Outcome> {
    protect_data(database, target)?;
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_millis(250))?;
    export_connection(&connection, from, to, target, cancelled, progress)
}
#[allow(dead_code)]
pub fn export_date(db: &Database, date: &str, path: &Path) -> Result<()> {
    export_range(db, date, date, path)
}

pub fn export_range(db: &Database, from: &str, to: &str, path: &Path) -> Result<()> {
    if let Some(source) = db
        .conn
        .path()
        .filter(|path| !path.is_empty() && *path != ":memory:")
    {
        protect_data(Path::new(source), path)?;
    }
    export_connection(&db.conn, from, to, path, &AtomicBool::new(false), |_| {})?;
    Ok(())
}

fn export_connection(
    connection: &Connection,
    from: &str,
    to: &str,
    path: &Path,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<Outcome> {
    let start = NaiveDate::parse_from_str(from, "%Y-%m-%d")?;
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d")?;
    if start > end {
        bail!("导出开始日期不能晚于结束日期");
    }
    if cancelled.load(Ordering::Acquire) {
        return Ok(Outcome::Cancelled);
    }
    let mut statement = connection.prepare(
        "SELECT t.local_date,a.executable_path,t.keyboard_count,t.mouse_click_count,t.wheel_units
         FROM daily_app_totals t JOIN applications a ON a.id=t.app_id
         WHERE t.local_date BETWEEN ?1 AND ?2 ORDER BY t.local_date,t.app_id",
    )?;
    let mut rows = statement.query(params![start.to_string(), end.to_string()])?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (mut temporary, file) = temporary_file(parent)?;
    let mut file = BufWriter::with_capacity(64 * 1024, file);
    writeln!(
        file,
        "local_date,application_path,keyboard_count,mouse_click_count,wheel_units"
    )?;
    let mut count = 0;
    progress(0);
    while let Some(row) = rows.next()? {
        if cancelled.load(Ordering::Acquire) {
            return Ok(Outcome::Cancelled);
        }
        writeln!(
            file,
            "{},{},{},{},{}",
            row.get::<_, String>(0)?,
            csv_escape(&row.get::<_, String>(1)?),
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?
        )?;
        count += 1;
        if count == 1 || count % 128 == 0 {
            progress(count);
        }
    }
    if cancelled.load(Ordering::Acquire) {
        return Ok(Outcome::Cancelled);
    }
    file.flush()?;
    file.get_ref().sync_all()?;
    drop(file);
    if cancelled.load(Ordering::Acquire) {
        return Ok(Outcome::Cancelled);
    }
    fs::rename(&temporary.0, path).context("无法保存导出文件，原目标文件保持不变")?;
    temporary.1 = true;
    Ok(Outcome::Completed(count))
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

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "kb_count_csv_test_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn no_temporary_files(&self) -> bool {
            fs::read_dir(&self.0).unwrap().all(|p| {
                !p.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".kb_count_export_")
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only files inside this create-new, test-owned directory. No
            // recursive removal and no access to application/user statistics.
            if let Ok(entries) = fs::read_dir(&self.0) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn readonly_snapshot_allows_writes_and_streams_sparse_dates_consistently() {
        use crate::storage::history::sample;
        let files = Fixture::new();
        let source = files.file("stats.db");
        let target = files.file("result.csv");
        let mut writer = Database::open(&source).unwrap();
        writer
            .record_batch(&[
                sample("2026-10-10", None, "C:/a,b.exe", "key", 2),
                sample("2026-10-11", None, "C:/a,b.exe", "key", 3),
            ])
            .unwrap();
        fs::write(&target, "previous export").unwrap();
        let outcome = export_snapshot(
            &source,
            "0001-01-01",
            "9999-12-31",
            &target,
            &AtomicBool::new(false),
            |rows| {
                if rows == 1 {
                    writer
                        .record_batch(&[
                            sample("2026-10-11", None, "C:/a,b.exe", "key", 7),
                            sample("2026-10-12", None, "C:/a,b.exe", "key", 10),
                        ])
                        .unwrap();
                }
            },
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Completed(2));
        let csv = fs::read_to_string(&target).unwrap();
        assert!(csv.contains("2026-10-10,\"c:\\a,b.exe\",2,0,0"));
        assert!(csv.contains("2026-10-11,\"c:\\a,b.exe\",3,0,0"));
        assert!(!csv.contains("2026-10-12"));
        assert_eq!(writer.totals("2026-10-11").unwrap().0, 10);
        assert!(files.no_temporary_files());
    }

    #[test]
    fn cancellation_and_errors_preserve_existing_target_and_remove_partial_files() {
        use crate::storage::history::sample;
        let files = Fixture::new();
        let source = files.file("stats.db");
        let target = files.file("result.csv");
        let mut writer = Database::open(&source).unwrap();
        writer
            .record_batch(&[sample("2026-10-10", None, "Unknown", "key", 2)])
            .unwrap();
        fs::write(&target, "keep this file").unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(
            export_snapshot(
                &source,
                "2026-10-10",
                "2026-10-11",
                &target,
                &cancel,
                |rows| {
                    if rows == 1 {
                        cancel.store(true, Ordering::Release);
                    }
                }
            )
            .unwrap(),
            Outcome::Cancelled
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep this file");
        assert!(files.no_temporary_files());
        assert!(
            export_snapshot(
                &source,
                "2026-10-12",
                "2026-10-10",
                &target,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        let directory_target = files.file("directory.csv");
        fs::create_dir(&directory_target).unwrap();
        assert!(
            export_snapshot(
                &source,
                "2026-10-10",
                "2026-10-11",
                &directory_target,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        assert!(directory_target.is_dir());
        fs::remove_dir(directory_target).unwrap();
        assert!(files.no_temporary_files());
    }

    #[test]
    fn export_refuses_own_data_and_missing_source_is_not_created() {
        let files = Fixture::new();
        let source = files.file("stats.db");
        let _writer = Database::open(&source).unwrap();
        let config = files.file("config.toml");
        fs::write(&config, "capture_enabled = true").unwrap();
        assert!(protect_data(&source, &std::env::current_exe().unwrap()).is_err());
        for target in [
            &source,
            &config,
            &files.file("stats.db-wal"),
            &files.file("stats.db-shm"),
        ] {
            assert!(
                export_snapshot(
                    &source,
                    "2026-10-10",
                    "2026-10-11",
                    target,
                    &AtomicBool::new(false),
                    |_| {}
                )
                .is_err()
            );
        }
        assert_eq!(
            fs::read_to_string(config).unwrap(),
            "capture_enabled = true"
        );
        let missing = files.file("missing.db");
        assert!(
            export_snapshot(
                &missing,
                "2026-10-10",
                "2026-10-11",
                &files.file("out.csv"),
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        assert!(!missing.exists());
        assert!(files.no_temporary_files());
    }

    #[test]
    fn range_export_contains_each_date() {
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        db.record_batch(&[
            InputEvent {
                local_date: "2026-10-10".into(),
                local_hour: None,
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
                local_hour: None,
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
