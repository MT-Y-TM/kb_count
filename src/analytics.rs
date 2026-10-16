use crate::storage::{database::Database, history::Counts};
use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Day,
    Week,
    Month,
    Year,
}

impl Period {
    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Week,
            2 => Self::Month,
            3 => Self::Year,
            _ => Self::Day,
        }
    }
    pub fn range(self, anchor: NaiveDate) -> (NaiveDate, NaiveDate) {
        let start = match self {
            Self::Day => anchor,
            Self::Week => anchor - Duration::days(anchor.weekday().num_days_from_monday().into()),
            Self::Month => anchor.with_day(1).unwrap(),
            Self::Year => NaiveDate::from_ymd_opt(anchor.year(), 1, 1).unwrap(),
        };
        let end = match self {
            Self::Day => start,
            Self::Week => start + Duration::days(6),
            Self::Month => (start + chrono::Months::new(1)) - Duration::days(1),
            Self::Year => NaiveDate::from_ymd_opt(anchor.year(), 12, 31).unwrap(),
        };
        (start, end)
    }
}

pub struct Point {
    pub label: String,
    pub counts: Counts,
    pub available: bool,
}
pub struct ActivityDay {
    pub date: NaiveDate,
    pub value: i64,
    pub week: i32,
    pub weekday: i32,
    pub future: bool,
}
pub struct Snapshot {
    pub days: Vec<ActivityDay>,
    pub points: Vec<Point>,
    pub totals: Counts,
    pub annual_total: i64,
    pub active_days: usize,
    pub columns: i32,
    pub range_label: String,
    pub note: String,
}

pub fn load(
    db: &Database,
    anchor: NaiveDate,
    period: Period,
    path: Option<&str>,
    now: NaiveDateTime,
) -> Result<Snapshot> {
    let year_start = NaiveDate::from_ymd_opt(anchor.year(), 1, 1).unwrap();
    let year_end = NaiveDate::from_ymd_opt(anchor.year(), 12, 31).unwrap();
    let (from, to) = period.range(anchor);
    let daily: HashMap<_, _> = db
        .history_daily(
            &from.min(year_start).to_string(),
            &to.max(year_end).to_string(),
            path,
        )?
        .into_iter()
        .collect();
    let mut days = Vec::new();
    let offset = year_start.weekday().num_days_from_monday() as i64;
    let mut annual_total = 0;
    let mut active_days = 0;
    let mut date = year_start;
    while date <= year_end {
        let value = daily
            .get(&date.to_string())
            .copied()
            .unwrap_or_default()
            .total();
        annual_total += value;
        active_days += usize::from(value > 0);
        let position = offset + date.signed_duration_since(year_start).num_days();
        days.push(ActivityDay {
            date,
            value,
            week: (position / 7) as i32,
            weekday: date.weekday().num_days_from_monday() as i32,
            future: date > now.date() && value == 0,
        });
        date += Duration::days(1);
    }
    let columns = days.last().unwrap().week + 1;
    let mut totals = Counts::default();
    let mut d = from;
    while d <= to {
        totals.add(daily.get(&d.to_string()).copied().unwrap_or_default());
        d += Duration::days(1);
    }
    let mut points = Vec::new();
    let mut note =
        "总输入 = 键盘 + 全部鼠标点击 + 滚轮绝对单位。分类曲线仅含详细采集数据。".to_string();
    if period == Period::Day {
        let hours: HashMap<_, _> = db
            .history_hourly(&anchor.to_string(), path)?
            .into_iter()
            .collect();
        let recorded: i64 = hours.values().map(|c| c.total()).sum();
        let missing = (totals.total() - recorded).max(0);
        if missing > 0 {
            note.push_str(&format!(
                " 旧记录中有 {missing} 次未分时；小时曲线只展示已记录的分时部分。"
            ));
        }
        for hour in 0..24 {
            let counts = hours
                .get(&format!("{hour:02}"))
                .copied()
                .unwrap_or_default();
            let elapsed = anchor < now.date() || (anchor == now.date() && hour <= now.hour());
            points.push(Point {
                label: format!("{hour:02}:00"),
                counts,
                available: counts.total() > 0 || (elapsed && (missing == 0 || recorded > 0)),
            });
        }
    } else if period == Period::Year {
        for month in 1..=12 {
            let mut counts = Counts::default();
            for day in &days {
                if day.date.month() == month {
                    counts.add(
                        daily
                            .get(&day.date.to_string())
                            .copied()
                            .unwrap_or_default(),
                    );
                }
            }
            points.push(Point {
                label: format!("{month}月"),
                counts,
                available: counts.total() > 0
                    || anchor.year() < now.year()
                    || (anchor.year() == now.year() && month <= now.month()),
            });
        }
    } else {
        let mut d = from;
        while d <= to {
            let counts = daily.get(&d.to_string()).copied().unwrap_or_default();
            points.push(Point {
                label: d.format("%m-%d").to_string(),
                counts,
                available: d <= now.date() || counts.total() > 0,
            });
            d += Duration::days(1);
        }
    }
    if totals.uncategorized() > 0 {
        if totals.classified == 0 {
            note.push_str(" 未记录详细数据：此范围的鼠标分类曲线不可用。");
        }
        note.push_str(&format!(
            " 此范围有 {} 次鼠标/滚轮未记录分类，分类曲线不代表这些输入。",
            totals.uncategorized()
        ));
    }
    if totals.total() == 0 {
        note.push_str(" 所选范围暂无输入记录。");
    }
    let granularity = match period {
        Period::Day => "按小时",
        Period::Year => "按月",
        _ => "按日",
    };
    Ok(Snapshot {
        days,
        points,
        totals,
        annual_total,
        active_days,
        columns,
        range_label: format!("{from} — {to} · {granularity}"),
        note,
    })
}

/// SVG commands use a shared fixed view box so series never autoscale separately.
pub fn path(values: &[Option<i64>], maximum: i64) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    let mut connected = false;
    for (index, value) in values.iter().enumerate() {
        if let Some(value) = value {
            let x = 1000.0 * index as f64 / values.len().saturating_sub(1).max(1) as f64;
            let y = 200.0 * (1.0 - *value as f64 / maximum.max(1) as f64);
            let _ = write!(
                result,
                "{} {x:.2} {y:.2} ",
                if connected { "L" } else { "M" }
            );
            // Single isolated observations need a visible mark too.
            if !connected {
                let _ = write!(result, "L {:.2} {y:.2} ", x + 0.4);
            }
            connected = true;
        } else {
            connected = false;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::history::sample;
    #[test]
    fn all_periods_and_single_app_use_real_aggregates_with_zero_filled_gaps() {
        let mut db = Database::open(std::path::Path::new(":memory:")).unwrap();
        db.record_batch(&[
            sample("2024-02-28", Some(8), "C:/editor.exe", "key", 4),
            sample("2024-02-29", Some(9), "C:/editor.exe", "key", 10),
            sample("2024-02-29", Some(9), "C:/editor.exe", "left", 2),
            sample("2024-02-29", Some(10), "C:/terminal.exe", "key", 5),
        ])
        .unwrap();
        let anchor = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let now = NaiveDate::from_ymd_opt(2024, 3, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        for (period, points, total, app_total) in [
            (Period::Day, 24, 17, 12),
            (Period::Week, 7, 21, 16),
            (Period::Month, 29, 21, 16),
            (Period::Year, 12, 21, 16),
        ] {
            let all = load(&db, anchor, period, None, now).unwrap();
            assert_eq!(all.days.len(), 366);
            assert_eq!(all.points.len(), points);
            assert_eq!(all.totals.total(), total);
            assert_eq!(
                all.points.iter().map(|p| p.counts.total()).sum::<i64>(),
                total
            );
            assert_eq!(all.annual_total, 21);
            assert_eq!(all.active_days, 2);
            let app = load(&db, anchor, period, Some("C:\\EDITOR.EXE"), now).unwrap();
            assert_eq!(app.totals.total(), app_total);
            assert_eq!(
                app.points.iter().map(|p| p.counts.total()).sum::<i64>(),
                app_total
            );
        }
        let day = load(&db, anchor, Period::Day, None, now).unwrap();
        assert_eq!(day.points[9].counts.values(), [12, 10, 2, 0, 0, 0]);
        assert_eq!(day.points[0].counts, Counts::default());
        let future = load(
            &db,
            NaiveDate::from_ymd_opt(2024, 3, 4).unwrap(),
            Period::Day,
            None,
            now,
        )
        .unwrap();
        assert!(future.points.iter().all(|p| !p.available));
    }
    #[test]
    fn legacy_hourly_gaps_are_explicit_without_altering_daily_history() {
        let mut db = Database::open(std::path::Path::new(":memory:")).unwrap();
        db.record(&sample("2024-02-29", None, "C:/a.exe", "key", 10))
            .unwrap();
        let anchor = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let now = anchor.and_hms_opt(23, 0, 0).unwrap();
        let old = load(&db, anchor, Period::Day, None, now).unwrap();
        assert!(old.points.iter().all(|p| !p.available));
        assert_eq!(old.totals.total(), 10);
        assert!(old.note.contains("10 次未分时"));
        db.record(&sample("2024-02-29", Some(9), "C:/a.exe", "key", 3))
            .unwrap();
        let mixed = load(&db, anchor, Period::Day, None, now).unwrap();
        assert_eq!(mixed.totals.total(), 13);
        assert_eq!(mixed.points[9].counts.keyboard, 3);
        assert_eq!(
            load(&db, anchor, Period::Month, None, now)
                .unwrap()
                .totals
                .total(),
            13
        );
    }
    #[test]
    fn periods_cover_leap_month_and_iso_week_across_year_boundary() {
        let leap = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(Period::Month.range(leap).1.to_string(), "2024-02-29");
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let (start, end) = Period::Week.range(date);
        assert_eq!(start.to_string(), "2025-12-29");
        assert_eq!(end.to_string(), "2026-01-04");
        let mut db = Database::open(std::path::Path::new(":memory:")).unwrap();
        db.record_batch(&[
            sample("2025-12-31", Some(8), "C:/a.exe", "key", 5),
            sample("2026-01-02", Some(9), "C:/a.exe", "key", 3),
        ])
        .unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 1, 3)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let week = load(&db, date, Period::Week, None, now).unwrap();
        assert_eq!(week.totals.total(), 8);
        assert_eq!(
            week.annual_total, 3,
            "annual wall excludes the previous year's week tail"
        );
        assert_eq!(week.points[2].counts.total(), 5);
        assert_eq!(week.points[4].counts.total(), 3);
    }
    #[test]
    fn paths_share_scale_and_do_not_connect_missing_data() {
        assert!(path(&[Some(0), Some(50)], 100).contains("1000.00 100.00"));
        assert_eq!(path(&[None, None], 100), "");
        let text = path(&[Some(10), None, Some(20)], 100);
        assert_eq!(text.matches('M').count(), 2);
    }
}
