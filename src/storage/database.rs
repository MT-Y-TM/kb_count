use crate::stats::event::InputEvent;
use crate::stats::policy::CaptureMode;
use anyhow::Result;
use rusqlite::{Connection, params};
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
impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS applications(id INTEGER PRIMARY KEY, executable_name TEXT NOT NULL, executable_path TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, first_seen_at TEXT NOT NULL, last_seen_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS daily_input_counts(local_date TEXT NOT NULL, app_id INTEGER NOT NULL, input_type TEXT NOT NULL, input_code TEXT NOT NULL, count INTEGER NOT NULL, PRIMARY KEY(local_date,app_id,input_type,input_code)); CREATE TABLE IF NOT EXISTS daily_app_totals(local_date TEXT NOT NULL, app_id INTEGER NOT NULL, keyboard_count INTEGER NOT NULL DEFAULT 0, mouse_click_count INTEGER NOT NULL DEFAULT 0, wheel_units INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(local_date,app_id)); CREATE TABLE IF NOT EXISTS app_policies(app_id INTEGER PRIMARY KEY, capture_mode TEXT NOT NULL DEFAULT 'detailed'); CREATE INDEX IF NOT EXISTS idx_daily_input_date ON daily_input_counts(local_date); CREATE INDEX IF NOT EXISTS idx_daily_totals_date ON daily_app_totals(local_date);"
        )?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            conn.pragma_update(None, "user_version", 1)?;
        }
        Ok(Self { conn })
    }
    #[allow(dead_code)]
    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
    pub fn record(&mut self, event: &InputEvent) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO applications(executable_name,executable_path,display_name,first_seen_at,last_seen_at) VALUES (?1,?2,?3,datetime('now'),datetime('now')) ON CONFLICT(executable_path) DO UPDATE SET last_seen_at=datetime('now')", params![event.app_name,event.app_path,event.app_name])?;
        let id: i64 = tx.query_row(
            "SELECT id FROM applications WHERE executable_path=?1",
            [&event.app_path],
            |r| r.get(0),
        )?;
        if event.detailed {
            tx.execute("INSERT INTO daily_input_counts VALUES (?1,?2,?3,?4,?5) ON CONFLICT(local_date,app_id,input_type,input_code) DO UPDATE SET count=count+excluded.count",params![event.local_date,id,event.input_type,event.code,event.count])?;
        }
        tx.execute("INSERT INTO daily_app_totals(local_date,app_id,keyboard_count,mouse_click_count,wheel_units) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(local_date,app_id) DO UPDATE SET keyboard_count=keyboard_count+excluded.keyboard_count,mouse_click_count=mouse_click_count+excluded.mouse_click_count,wheel_units=wheel_units+excluded.wheel_units",params![event.local_date,id,event.keyboard_count,event.mouse_count,event.wheel_units])?;
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
        if let Some(path) = app_path {
            let mut stmt=self.conn.prepare("SELECT c.input_code,SUM(c.count) FROM daily_input_counts c JOIN applications a ON a.id=c.app_id WHERE c.local_date=?1 AND c.input_type=?3 AND a.executable_path=?2 GROUP BY c.input_code ORDER BY SUM(c.count) DESC")?;
            let rows = stmt.query_map(params![date, path, input_type], |r| {
                Ok(KeyTotal {
                    code: r.get(0)?,
                    count: r.get(1)?,
                })
            })?;
            return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        let mut stmt=self.conn.prepare("SELECT c.input_code,SUM(c.count) FROM daily_input_counts c WHERE c.local_date=?1 AND c.input_type=?2 GROUP BY c.input_code ORDER BY SUM(c.count) DESC")?;
        let rows = stmt.query_map(params![date, input_type], |r| {
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
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Unknown");
        self.conn.execute("INSERT INTO applications(executable_name,executable_path,display_name,first_seen_at,last_seen_at) VALUES (?1,?2,?1,datetime('now'),datetime('now')) ON CONFLICT(executable_path) DO NOTHING", params![name, path])?;
        self.conn.execute("INSERT INTO app_policies(app_id,capture_mode) SELECT id,?2 FROM applications WHERE executable_path=?1 ON CONFLICT(app_id) DO UPDATE SET capture_mode=excluded.capture_mode",params![path,mode])?;
        Ok(())
    }

    pub fn policies(&self) -> Result<HashMap<String, CaptureMode>> {
        let mut stmt = self.conn.prepare("SELECT a.executable_path,p.capture_mode FROM app_policies p JOIN applications a ON a.id=p.app_id")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                CaptureMode::parse(&row.get::<_, String>(1)?),
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    pub fn app_detail(&self, date: &str, path: &str) -> Result<Option<AppDetail>> {
        let row = self.conn.query_row(
            "SELECT a.executable_name,a.executable_path,COALESCE(t.keyboard_count,0),COALESCE(t.mouse_click_count,0),COALESCE(t.wheel_units,0),COALESCE(p.capture_mode,'') FROM applications a LEFT JOIN daily_app_totals t ON t.app_id=a.id AND t.local_date=?1 LEFT JOIN app_policies p ON p.app_id=a.id WHERE a.executable_path=?2",
            params![date, path],
            |row| {
                let mode: String = row.get(5)?;
                Ok(AppDetail {
                    app_name: row.get(0)?,
                    app_path: row.get(1)?,
                    keyboard: row.get(2)?,
                    mouse: row.get(3)?,
                    wheel: row.get(4)?,
                    policy: if mode.is_empty() { CaptureMode::for_application(path) } else { CaptureMode::parse(&mode) },
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
        tx.execute_batch("DELETE FROM daily_input_counts; DELETE FROM daily_app_totals; DELETE FROM app_policies; DELETE FROM applications;")?;
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
            policies.get("C:/Apps/editor.exe"),
            Some(&CaptureMode::Excluded)
        );
    }

    #[test]
    fn clear_all_removes_statistics_and_applications() {
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
        db.clear_all().expect("clear");
        assert_eq!(db.totals("2026-10-11").unwrap(), (0, 0, 0, 0));
        assert!(
            db.app_detail("2026-10-11", "C:/Apps/editor.exe")
                .unwrap()
                .is_none()
        );
    }
}
