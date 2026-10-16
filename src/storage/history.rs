use super::database::Database;
use crate::stats::policy::normalize_app_path;
use anyhow::Result;
use rusqlite::params;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub keyboard: i64,
    pub mouse: i64,
    pub wheel: i64,
    pub left: i64,
    pub right: i64,
    pub up: i64,
    pub down: i64,
    pub classified: i64,
}

impl Counts {
    pub fn total(self) -> i64 {
        self.keyboard
            .saturating_add(self.mouse)
            .saturating_add(self.wheel)
    }
    pub fn values(self) -> [i64; 6] {
        [
            self.total(),
            self.keyboard,
            self.left,
            self.right,
            self.up,
            self.down,
        ]
    }
    pub fn add(&mut self, other: Self) {
        self.keyboard += other.keyboard;
        self.mouse += other.mouse;
        self.wheel += other.wheel;
        self.left += other.left;
        self.right += other.right;
        self.up += other.up;
        self.down += other.down;
        self.classified += other.classified;
    }
    pub fn uncategorized(self) -> i64 {
        (self.mouse + self.wheel - self.classified).max(0)
    }
}

impl Database {
    pub fn history_app_totals(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<super::database::AppTotal>> {
        let mut query=self.conn.prepare("SELECT a.executable_name,a.executable_path,SUM(t.keyboard_count),SUM(t.mouse_click_count),SUM(ABS(t.wheel_units)) FROM daily_app_totals t JOIN applications a ON a.id=t.app_id WHERE t.local_date BETWEEN ?1 AND ?2 GROUP BY a.id ORDER BY SUM(t.keyboard_count+t.mouse_click_count+ABS(t.wheel_units)) DESC,a.executable_path")?;
        let rows = query.query_map(params![from, to], |r| {
            Ok(super::database::AppTotal {
                app_name: r.get(0)?,
                app_path: r.get(1)?,
                keyboard: r.get(2)?,
                mouse: r.get(3)?,
                wheel: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn history_apps(&self) -> Result<Vec<(String, String)>> {
        let mut query = self.conn.prepare("SELECT executable_name,executable_path FROM applications ORDER BY executable_name COLLATE NOCASE,executable_path")?;
        let rows = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One row per local calendar day, using the original daily aggregates.
    pub fn history_daily(
        &self,
        from: &str,
        to: &str,
        path: Option<&str>,
    ) -> Result<Vec<(String, Counts)>> {
        let path = path.map(normalize_app_path);
        let mut query = self.conn.prepare(
            "WITH categories AS (
               SELECT local_date,app_id,
                 SUM(CASE WHEN input_code='left' THEN count ELSE 0 END) AS l,
                 SUM(CASE WHEN input_code='right' THEN count ELSE 0 END) AS r,
                 SUM(CASE WHEN input_code='wheel_up' THEN count ELSE 0 END) AS u,
                 SUM(CASE WHEN input_code='wheel_down' THEN count ELSE 0 END) AS d,
                 SUM(count) AS classified
               FROM daily_input_counts WHERE input_type='mouse' AND local_date BETWEEN ?1 AND ?2
               GROUP BY local_date,app_id)
             SELECT t.local_date,SUM(t.keyboard_count),SUM(t.mouse_click_count),SUM(ABS(t.wheel_units)),
               SUM(COALESCE(c.l,0)),SUM(COALESCE(c.r,0)),SUM(COALESCE(c.u,0)),SUM(COALESCE(c.d,0)),SUM(COALESCE(c.classified,0))
             FROM daily_app_totals t JOIN applications a ON t.app_id=a.id
             LEFT JOIN categories c ON c.local_date=t.local_date AND c.app_id=t.app_id
             WHERE t.local_date BETWEEN ?1 AND ?2 AND (?3 IS NULL OR a.executable_path=?3 COLLATE NOCASE)
             GROUP BY t.local_date ORDER BY t.local_date")?;
        let rows = query.query_map(params![from, to, path], read_counts)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Only hour-level totals and mouse categories; no physical key codes.
    pub fn history_hourly(&self, date: &str, path: Option<&str>) -> Result<Vec<(String, Counts)>> {
        let path = path.map(normalize_app_path);
        let mut query = self.conn.prepare(
            "SELECT printf('%02d',t.local_hour),SUM(keyboard_count),SUM(mouse_click_count),SUM(ABS(wheel_units)),
                    SUM(mouse_left),SUM(mouse_right),SUM(wheel_up),SUM(wheel_down),SUM(classified_units)
             FROM hourly_app_totals t JOIN applications a ON a.id=t.app_id
             WHERE t.local_date=?1 AND (?2 IS NULL OR a.executable_path=?2 COLLATE NOCASE)
             GROUP BY t.local_hour ORDER BY t.local_hour")?;
        let rows = query.query_map(params![date, path], read_counts)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn read_counts(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, Counts)> {
    Ok((
        r.get(0)?,
        Counts {
            keyboard: r.get(1)?,
            mouse: r.get(2)?,
            wheel: r.get(3)?,
            left: r.get(4)?,
            right: r.get(5)?,
            up: r.get(6)?,
            down: r.get(7)?,
            classified: r.get(8)?,
        },
    ))
}

#[cfg(test)]
pub(crate) fn sample(
    date: &str,
    hour: Option<u8>,
    path: &str,
    code: &str,
    count: i64,
) -> crate::stats::event::InputEvent {
    let key = code == "key";
    let wheel = code.starts_with("wheel_") || code.starts_with("hwheel_");
    crate::stats::event::InputEvent {
        local_date: date.into(),
        local_hour: hour,
        app_name: "test.exe".into(),
        app_path: path.into(),
        input_type: if key { "keyboard" } else { "mouse" }.into(),
        code: code.into(),
        count,
        keyboard_count: if key { count } else { 0 },
        mouse_count: if !key && !wheel { count } else { 0 },
        wheel_units: if wheel { count } else { 0 },
        detailed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{aggregator::Aggregator, policy::CaptureMode};
    use std::path::Path;

    #[test]
    fn hour_boundaries_categories_unknown_and_app_identity_reach_queries() {
        let mut db = Database::open(Path::new(":memory:")).unwrap();
        let mut aggregate = Aggregator::default();
        for event in [
            sample("2026-10-15", Some(9), "C:/Editor.exe", "key", 10),
            sample("2026-10-15", Some(10), "c:\\EDITOR.exe", "key", 7),
            sample("2026-10-15", Some(10), "C:/Editor.exe", "left", 3),
            sample("2026-10-15", Some(10), "C:/Editor.exe", "right", 2),
            sample("2026-10-15", Some(10), "C:/Editor.exe", "wheel_up", 4),
            sample("2026-10-15", Some(10), "C:/Editor.exe", "wheel_down", 5),
            sample("2026-10-15", Some(10), "Unknown", "left", 1),
            sample("2026-10-16", Some(0), "C:/Editor.exe", "middle", 2),
        ] {
            aggregate.record(event, CaptureMode::Detailed);
        }
        db.record_batch(&aggregate.drain()).unwrap();
        let hours = db
            .history_hourly("2026-10-15", Some("C:/editor.exe"))
            .unwrap();
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0].1.keyboard, 10);
        assert_eq!(hours[1].1.values(), [21, 7, 3, 2, 4, 5]);
        assert_eq!(hours[1].1.uncategorized(), 0);
        let days = db.history_daily("2026-10-15", "2026-10-16", None).unwrap();
        assert_eq!(days[0].1.values(), [32, 17, 4, 2, 4, 5]);
        assert_eq!(days[1].1.mouse, 2);
        assert_eq!(db.history_apps().unwrap().len(), 2);
        let apps = db.history_app_totals("2026-10-15", "2026-10-16").unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!(
            apps.iter()
                .map(|a| a.keyboard + a.mouse + a.wheel)
                .sum::<i64>(),
            34
        );
        assert_eq!(
            db.history_app_totals("2026-10-16", "2026-10-16").unwrap()[0].mouse,
            2
        );
    }

    #[test]
    fn privacy_modes_apply_to_hourly_categories_without_losing_totals() {
        let mut db = Database::open(Path::new(":memory:")).unwrap();
        let mut aggregate = Aggregator::default();
        aggregate.record(
            sample("2026-10-15", Some(8), "C:/browser.exe", "left", 5),
            CaptureMode::TotalsOnly,
        );
        aggregate.record(
            sample("2026-10-15", Some(8), "C:/browser.exe", "wheel_down", 3),
            CaptureMode::TotalsOnly,
        );
        aggregate.record(
            sample("2026-10-15", Some(8), "C:/secret.exe", "key", 99),
            CaptureMode::Excluded,
        );
        db.record_batch(&aggregate.drain()).unwrap();
        let hours = db.history_hourly("2026-10-15", None).unwrap();
        assert_eq!(hours[0].1.values(), [8, 0, 0, 0, 0, 0]);
        assert_eq!(hours[0].1.uncategorized(), 8);
        let days = db.history_daily("2026-10-15", "2026-10-15", None).unwrap();
        assert_eq!(days[0].1, hours[0].1);
        assert_eq!(db.history_apps().unwrap().len(), 1);
    }

    #[test]
    fn failed_hourly_write_rolls_back_daily_totals_and_can_retry() {
        let mut db = Database::open(Path::new(":memory:")).unwrap();
        let mut events = vec![
            sample("2026-10-15", Some(9), "C:/a.exe", "key", 2),
            sample("2026-10-15", Some(24), "C:/a.exe", "left", 3),
        ];
        assert!(db.record_batch(&events).is_err());
        assert!(
            db.history_daily("2026-10-15", "2026-10-15", None)
                .unwrap()
                .is_empty()
        );
        assert!(db.history_hourly("2026-10-15", None).unwrap().is_empty());
        events[1].local_hour = Some(10);
        db.record_batch(&events).unwrap();
        assert_eq!(db.history_hourly("2026-10-15", None).unwrap().len(), 2);
        db.clear_all().unwrap();
        assert!(db.history_hourly("2026-10-15", None).unwrap().is_empty());
    }

    #[test]
    fn version_one_migrates_and_portable_copy_preserves_history_and_policy() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("history-migration-{}", std::process::id()));
        std::fs::create_dir_all(root.join("moved")).unwrap();
        let source = root.join("stats.db");
        {
            let mut db = Database::open(&source).unwrap();
            db.clear_all().unwrap();
            db.record(&sample("2024-02-29", None, "C:/a.exe", "key", 12))
                .unwrap();
            db.set_policy("C:/a.exe", "totals_only").unwrap();
            db.conn
                .execute_batch("DROP TABLE hourly_app_totals; PRAGMA user_version=1;")
                .unwrap();
            db.checkpoint().unwrap();
        }
        {
            let mut db = Database::open(&source).unwrap();
            assert!(db.history_hourly("2024-02-29", None).unwrap().is_empty());
            assert_eq!(
                db.history_daily("2024-02-29", "2024-02-29", None).unwrap()[0]
                    .1
                    .keyboard,
                12
            );
            db.record(&sample("2026-10-16", Some(0), "C:/a.exe", "key", 3))
                .unwrap();
            db.checkpoint().unwrap();
        }
        let moved = root.join("moved/stats.db");
        std::fs::copy(&source, &moved).unwrap();
        {
            let db = Database::open(&moved).unwrap();
            assert_eq!(
                db.history_hourly("2026-10-16", None).unwrap()[0].1.keyboard,
                3
            );
            assert_eq!(
                db.policies().unwrap().get("c:\\a.exe"),
                Some(&CaptureMode::TotalsOnly)
            );
        }
        // These are exact test-owned files inside target; no runtime data.
        for name in [source, moved] {
            std::fs::remove_file(name).unwrap();
        }
    }
}
