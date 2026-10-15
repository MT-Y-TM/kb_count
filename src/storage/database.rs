use crate::stats::event::InputEvent;
use crate::stats::policy::{CaptureMode, normalize_app_path};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::Path;
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AppTotal {
    pub app_name: String,
    pub app_path: String,
    pub keyboard: i64,
    pub mouse: i64,
    pub wheel: i64,
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct KeyTotal {
    pub code: String,
    pub count: i64,
}
#[derive(Debug, Clone)]
pub struct AppDetail {
    pub app_name: String,
    pub app_path: String,
    pub keyboard: i64,
    pub mouse: i64,
    pub wheel: i64,
    pub policy: CaptureMode,
}
pub struct Database {
    conn: Connection,
}
const SCHEMA_VERSION: i64 = 1;

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS applications(id INTEGER PRIMARY KEY, executable_name TEXT NOT NULL, executable_path TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, first_seen_at TEXT NOT NULL, last_seen_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS daily_input_counts(local_date TEXT NOT NULL, app_id INTEGER NOT NULL, input_type TEXT NOT NULL, input_code TEXT NOT NULL, count INTEGER NOT NULL, PRIMARY KEY(local_date,app_id,input_type,input_code)); CREATE TABLE IF NOT EXISTS daily_app_totals(local_date TEXT NOT NULL, app_id INTEGER NOT NULL, keyboard_count INTEGER NOT NULL DEFAULT 0, mouse_click_count INTEGER NOT NULL DEFAULT 0, wheel_units INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(local_date,app_id)); CREATE TABLE IF NOT EXISTS app_policies(app_id INTEGER PRIMARY KEY, capture_mode TEXT NOT NULL DEFAULT 'detailed'); CREATE INDEX IF NOT EXISTS idx_daily_input_date ON daily_input_counts(local_date); CREATE INDEX IF NOT EXISTS idx_daily_input_lookup ON daily_input_counts(local_date,input_type,app_id); CREATE INDEX IF NOT EXISTS idx_daily_totals_date ON daily_app_totals(local_date);",
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        bail!(
            "stats.db schema version {version} is newer than this kb_count version {SCHEMA_VERSION}"
        );
    }
    if version == 0 {
        create_schema(conn)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    } else {
        // Keep indexes present when opening a database created by an older 1.x build.
        create_schema(conn)?;
    }
    // Older development builds used a separate `wheel` input type. Merge
    // those aggregate rows into the schema's mouse category while preserving
    // any rows already written by the current version.
    conn.execute_batch(
        "INSERT INTO daily_input_counts(local_date,app_id,input_type,input_code,count) SELECT local_date,app_id,'mouse',input_code,count FROM daily_input_counts WHERE input_type='wheel' ON CONFLICT(local_date,app_id,input_type,input_code) DO UPDATE SET count=count+excluded.count; DELETE FROM daily_input_counts WHERE input_type='wheel';",
    )?;
    Ok(())
}
impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&conn)?;
        Ok(Self { conn })
    }
    #[allow(dead_code)]
    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
    #[allow(dead_code)]
    pub fn record(&mut self, event: &InputEvent) -> Result<()> {
        self.record_batch(std::slice::from_ref(event))
    }

    pub fn record_batch(&mut self, events: &[InputEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for event in events {
            let app_path = normalize_app_path(&event.app_path);
            let app_name = if app_path == "Unknown" {
                "Unknown".to_string()
            } else {
                std::path::Path::new(&app_path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&event.app_name)
                    .to_string()
            };
            let id = tx
                .query_row(
                    "SELECT id FROM applications WHERE executable_path=?1 COLLATE NOCASE",
                    [&app_path],
                    |r| r.get(0),
                )
                .optional()?;
            let id = if let Some(id) = id {
                tx.execute(
                    "UPDATE applications SET last_seen_at=datetime('now') WHERE id=?1",
                    [id],
                )?;
                id
            } else {
                tx.execute("INSERT INTO applications(executable_name,executable_path,display_name,first_seen_at,last_seen_at) VALUES (?1,?2,?3,datetime('now'),datetime('now'))", params![app_name,app_path,app_name])?;
                tx.last_insert_rowid()
            };
            if event.detailed {
                tx.execute("INSERT INTO daily_input_counts VALUES (?1,?2,?3,?4,?5) ON CONFLICT(local_date,app_id,input_type,input_code) DO UPDATE SET count=count+excluded.count",params![event.local_date,id,event.input_type,event.code,event.count])?;
            }
            tx.execute("INSERT INTO daily_app_totals(local_date,app_id,keyboard_count,mouse_click_count,wheel_units) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(local_date,app_id) DO UPDATE SET keyboard_count=keyboard_count+excluded.keyboard_count,mouse_click_count=mouse_click_count+excluded.mouse_click_count,wheel_units=wheel_units+excluded.wheel_units",params![event.local_date,id,event.keyboard_count,event.mouse_count,event.wheel_units])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn totals(&self, date: &str) -> Result<(i64, i64, i64, i64)> {
        self.conn.query_row("SELECT COALESCE(SUM(keyboard_count),0),COALESCE(SUM(mouse_click_count),0),COALESCE(SUM(wheel_units),0),COUNT(DISTINCT app_id) FROM daily_app_totals WHERE local_date=?1",[date],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(Into::into)
    }
    #[allow(dead_code)]
    pub fn app_totals(&self, date: &str) -> Result<Vec<AppTotal>> {
        self.app_totals_sorted(date, "total")
    }
    pub fn app_totals_sorted(&self, date: &str, sort: &str) -> Result<Vec<AppTotal>> {
        let order = match sort {
            "keyboard" => "t.keyboard_count DESC, t.mouse_click_count DESC",
            "mouse" => "t.mouse_click_count DESC, t.keyboard_count DESC",
            _ => "(t.keyboard_count+t.mouse_click_count+ABS(t.wheel_units)) DESC",
        };
        let sql = format!(
            "SELECT a.executable_name,a.executable_path,t.keyboard_count,t.mouse_click_count,t.wheel_units FROM daily_app_totals t JOIN applications a ON a.id=t.app_id WHERE t.local_date=?1 ORDER BY {order}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([date], |r| {
            Ok(AppTotal {
                app_name: r.get(0)?,
                app_path: r.get(1)?,
                keyboard: r.get(2)?,
                mouse: r.get(3)?,
                wheel: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    #[allow(dead_code)]
    pub fn key_ranking(&self, date: &str, app_path: Option<&str>) -> Result<Vec<KeyTotal>> {
        self.input_ranking(date, app_path, "keyboard")
    }

    pub fn input_ranking(
        &self,
        date: &str,
        app_path: Option<&str>,
        input_type: &str,
    ) -> Result<Vec<KeyTotal>> {
        let (type_filter, uses_type_parameter) = match input_type {
            "keyboard" => ("c.input_type='keyboard'", false),
            // Wheel rows use the mouse input type but have a wheel_* code.
            "mouse" => (
                "c.input_type='mouse' AND c.input_code NOT LIKE 'wheel_%' AND c.input_code NOT LIKE 'hwheel_%'",
                false,
            ),
            "wheel" => (
                "c.input_type='mouse' AND (c.input_code LIKE 'wheel_%' OR c.input_code LIKE 'hwheel_%')",
                false,
            ),
            _ => ("c.input_type=?3", true),
        };
        if let Some(path) = app_path {
            let path = normalize_app_path(path);
            let sql = format!(
                "SELECT c.input_code,SUM(c.count) FROM daily_input_counts c JOIN applications a ON a.id=c.app_id WHERE c.local_date=?1 AND {type_filter} AND a.executable_path=?2 COLLATE NOCASE GROUP BY c.input_code ORDER BY SUM(c.count) DESC"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            if uses_type_parameter {
                let rows = stmt.query_map(params![date, path, input_type], |r| {
                    Ok(KeyTotal {
                        code: r.get(0)?,
                        count: r.get(1)?,
                    })
                })?;
                return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            let rows = stmt.query_map(params![date, path], |r| {
                Ok(KeyTotal {
                    code: r.get(0)?,
                    count: r.get(1)?,
                })
            })?;
            return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        let sql = format!(
            "SELECT c.input_code,SUM(c.count) FROM daily_input_counts c WHERE c.local_date=?1 AND {type_filter} GROUP BY c.input_code ORDER BY SUM(c.count) DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        if uses_type_parameter {
            let rows = stmt.query_map(params![date, input_type], |r| {
                Ok(KeyTotal {
                    code: r.get(0)?,
                    count: r.get(1)?,
                })
            })?;
            return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        let rows = stmt.query_map([date], |r| {
            Ok(KeyTotal {
                code: r.get(0)?,
                count: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    #[allow(dead_code)]
    pub fn set_policy(&self, path: &str, mode: &str) -> Result<()> {
        let mode = CaptureMode::parse(mode).as_str();
        let path = normalize_app_path(path);
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Unknown");
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM applications WHERE executable_path=?1 COLLATE NOCASE",
                [&path],
                |row| row.get(0),
            )
            .optional()?;
        let id = if let Some(id) = id {
            id
        } else {
            self.conn.execute("INSERT INTO applications(executable_name,executable_path,display_name,first_seen_at,last_seen_at) VALUES (?1,?2,?1,datetime('now'),datetime('now'))", params![name, path])?;
            self.conn.last_insert_rowid()
        };
        self.conn.execute("INSERT INTO app_policies(app_id,capture_mode) VALUES (?1,?2) ON CONFLICT(app_id) DO UPDATE SET capture_mode=excluded.capture_mode", params![id, mode])?;
        Ok(())
    }

    pub fn policies(&self) -> Result<HashMap<String, CaptureMode>> {
        let mut stmt = self.conn.prepare("SELECT a.executable_path,p.capture_mode FROM app_policies p JOIN applications a ON a.id=p.app_id")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                normalize_app_path(&row.get::<_, String>(0)?),
                CaptureMode::parse(&row.get::<_, String>(1)?),
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    pub fn policy_rows(&self) -> Result<Vec<(String, CaptureMode)>> {
        let mut rows = self.policies()?.into_iter().collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(rows)
    }

    pub fn app_detail(&self, date: &str, path: &str) -> Result<Option<AppDetail>> {
        let path = normalize_app_path(path);
        let row = self.conn.query_row(
            "SELECT a.executable_name,a.executable_path,COALESCE(t.keyboard_count,0),COALESCE(t.mouse_click_count,0),COALESCE(t.wheel_units,0),COALESCE(p.capture_mode,'') FROM applications a LEFT JOIN daily_app_totals t ON t.app_id=a.id AND t.local_date=?1 LEFT JOIN app_policies p ON p.app_id=a.id WHERE a.executable_path=?2 COLLATE NOCASE",
            params![date, path],
            |row| {
                let mode: String = row.get(5)?;
                Ok(AppDetail {
                    app_name: row.get(0)?,
                    app_path: row.get(1)?,
                    keyboard: row.get(2)?,
                    mouse: row.get(3)?,
                    wheel: row.get(4)?,
                    policy: if mode.is_empty() { CaptureMode::for_application(&path) } else { CaptureMode::parse(&mode) },
                })
            },
        );
        match row {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn clear_all(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        // Clearing statistics must not discard application identities or the
        // privacy policies the user configured for them.
        tx.execute_batch("DELETE FROM daily_input_counts; DELETE FROM daily_app_totals;")?;
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::aggregator::Aggregator;

    #[test]
    fn schema_migration_initializes_version_one() {
        let conn = Connection::open_in_memory().expect("connection");
        migrate(&conn).expect("migration");
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='daily_input_counts'",
                [],
                |row| row.get(0),
            )
            .expect("schema table");
        assert_eq!(table_count, 1);
    }

    #[test]
    fn schema_migration_rejects_newer_database() {
        let conn = Connection::open_in_memory().expect("connection");
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("version");
        assert!(migrate(&conn).is_err());
    }

    #[test]
    fn schema_migration_merges_legacy_wheel_rows() {
        let conn = Connection::open_in_memory().expect("connection");
        create_schema(&conn).expect("schema");
        conn.execute(
            "INSERT INTO daily_input_counts(local_date,app_id,input_type,input_code,count) VALUES ('2026-10-11',1,'wheel','wheel_up',2)",
            [],
        )
        .expect("legacy wheel row");
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .expect("version");
        migrate(&conn).expect("migration");
        let row: (String, i64) = conn
            .query_row(
                "SELECT input_type,count FROM daily_input_counts WHERE local_date='2026-10-11' AND app_id=1 AND input_code='wheel_up'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("migrated row");
        assert_eq!(row, ("mouse".into(), 2));
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM daily_input_counts WHERE input_type='wheel'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .expect("legacy rows removed"),
            0
        );
    }

    #[test]
    fn totals_only_event_updates_totals_without_key_rows() {
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        db.record(&InputEvent {
            local_date: "2026-10-11".into(),
            app_name: "browser.exe".into(),
            app_path: "browser.exe".into(),
            input_type: "keyboard".into(),
            code: "total".into(),
            count: 3,
            keyboard_count: 3,
            mouse_count: 0,
            wheel_units: 0,
            detailed: false,
        })
        .expect("record");
        assert_eq!(db.totals("2026-10-11").unwrap().0, 3);
        assert!(db.key_ranking("2026-10-11", None).unwrap().is_empty());
    }

    #[test]
    fn policy_override_is_loaded_for_aggregator() {
        let db = Database::open(Path::new(":memory:")).expect("database");
        db.set_policy("C:/Apps/editor.exe", "excluded")
            .expect("policy");
        let policies = db.policies().expect("policies");
        assert_eq!(
            policies.get(r"c:\apps\editor.exe"),
            Some(&CaptureMode::Excluded)
        );
    }

    #[test]
    fn clear_all_removes_statistics_but_preserves_policy_identity() {
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        db.record(&InputEvent {
            local_date: "2026-10-11".into(),
            app_name: "editor.exe".into(),
            app_path: "C:/Apps/editor.exe".into(),
            input_type: "keyboard".into(),
            code: "scan:1".into(),
            count: 1,
            keyboard_count: 1,
            mouse_count: 0,
            wheel_units: 0,
            detailed: true,
        })
        .expect("record");
        db.set_policy("C:/Apps/editor.exe", "excluded")
            .expect("policy");
        db.clear_all().expect("clear");
        assert_eq!(db.totals("2026-10-11").unwrap(), (0, 0, 0, 0));
        let detail = db
            .app_detail("2026-10-11", "C:/Apps/editor.exe")
            .unwrap()
            .expect("application identity should remain");
        assert_eq!(detail.policy, CaptureMode::Excluded);
        assert_eq!(detail.keyboard, 0);
    }

    #[test]
    fn record_batch_accumulates_multiple_events() {
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        let events = [
            InputEvent {
                local_date: "2026-10-11".into(),
                app_name: "editor.exe".into(),
                app_path: "C:/Apps/editor.exe".into(),
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
                app_name: "editor.exe".into(),
                app_path: "C:/Apps/editor.exe".into(),
                input_type: "mouse".into(),
                code: "left".into(),
                count: 3,
                keyboard_count: 0,
                mouse_count: 3,
                wheel_units: 0,
                detailed: true,
            },
            InputEvent {
                local_date: "2026-10-11".into(),
                app_name: "editor.exe".into(),
                app_path: "C:/Apps/editor.exe".into(),
                input_type: "mouse".into(),
                code: "wheel_up".into(),
                count: 2,
                keyboard_count: 0,
                mouse_count: 0,
                wheel_units: 2,
                detailed: true,
            },
        ];
        db.record_batch(&events).expect("record batch");
        assert_eq!(db.totals("2026-10-11").unwrap(), (2, 3, 2, 1));
        assert_eq!(
            db.input_ranking("2026-10-11", None, "wheel").unwrap()[0].code,
            "wheel_up"
        );
        assert!(
            db.input_ranking("2026-10-11", None, "mouse")
                .unwrap()
                .iter()
                .all(|row| row.code != "wheel_up")
        );
    }

    #[test]
    fn mouse_categories_reach_sqlite_and_queries_with_unknown_attribution() {
        let mut aggregator = Aggregator::default();
        for code in ["left", "right", "middle", "x1", "x2"] {
            aggregator.record(
                InputEvent {
                    local_date: "2026-10-15".into(),
                    app_name: "Unknown".into(),
                    app_path: "Unknown".into(),
                    input_type: "mouse".into(),
                    code: code.into(),
                    count: 1,
                    keyboard_count: 0,
                    mouse_count: 1,
                    wheel_units: 0,
                    detailed: true,
                },
                CaptureMode::Detailed,
            );
        }
        for code in ["wheel_down", "hwheel_up"] {
            aggregator.record(
                InputEvent {
                    local_date: "2026-10-15".into(),
                    app_name: "Unknown".into(),
                    app_path: "Unknown".into(),
                    input_type: "mouse".into(),
                    code: code.into(),
                    count: 1,
                    keyboard_count: 0,
                    mouse_count: 0,
                    wheel_units: 1,
                    detailed: true,
                },
                CaptureMode::Detailed,
            );
        }
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        db.record_batch(&aggregator.drain()).expect("record batch");
        assert_eq!(db.totals("2026-10-15").unwrap(), (0, 5, 2, 1));
        let buttons = db
            .input_ranking("2026-10-15", Some("Unknown"), "mouse")
            .unwrap();
        let wheels = db
            .input_ranking("2026-10-15", Some("Unknown"), "wheel")
            .unwrap();
        assert_eq!(buttons.len(), 5);
        assert_eq!(wheels.len(), 2);
        assert!(buttons.iter().all(|row| row.count == 1));
        assert!(wheels.iter().all(|row| row.count == 1));
    }

    #[test]
    fn legacy_path_casing_reuses_existing_application_identity() {
        let mut db = Database::open(Path::new(":memory:")).expect("database");
        db.conn
            .execute(
                "INSERT INTO applications(executable_name,executable_path,display_name,first_seen_at,last_seen_at) VALUES ('Editor.EXE','C:/Apps/Editor.EXE','Editor.EXE',datetime('now'),datetime('now'))",
                [],
            )
            .expect("legacy app");
        db.record(&InputEvent {
            local_date: "2026-10-11".into(),
            app_name: "editor.exe".into(),
            app_path: "c:/apps/editor.exe".into(),
            input_type: "keyboard".into(),
            code: "scan:1e:ext:false".into(),
            count: 2,
            keyboard_count: 2,
            mouse_count: 0,
            wheel_units: 0,
            detailed: true,
        })
        .expect("record");
        assert_eq!(db.totals("2026-10-11").unwrap(), (2, 0, 0, 1));
        assert!(
            db.app_detail("2026-10-11", "C:/APPS/EDITOR.EXE")
                .unwrap()
                .is_some()
        );
    }
}
