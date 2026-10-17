//! UI-only range selection and validation; never starts an export or a query.
use crate::{MainWindow, format_count};
use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use slint::ComponentHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeError {
    From,
    To,
    Reversed,
}

impl RangeError {
    fn message(self) -> &'static str {
        match self {
            Self::From => "开始日期无效，请使用 YYYY-MM-DD。",
            Self::To => "结束日期无效，请使用 YYYY-MM-DD。",
            Self::Reversed => "开始日期晚于结束日期，可点击“交换日期”。",
        }
    }
}

fn parse(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .ok()
        .filter(|date| (1..=9999).contains(&date.year()))
}

fn validate(from: &str, to: &str) -> Result<DateRange, RangeError> {
    let from = parse(from).ok_or(RangeError::From)?;
    let to = parse(to).ok_or(RangeError::To)?;
    if from > to {
        return Err(RangeError::Reversed);
    }
    Ok(DateRange { from, to })
}

fn preset(index: i32, today: NaiveDate) -> Option<DateRange> {
    let first = today.with_day(1)?;
    let minimum = NaiveDate::from_ymd_opt(1, 1, 1)?;
    let (from, to) = match index {
        0 => (today, today),
        1 | 2 => (
            today
                .checked_sub_signed(Duration::days(if index == 1 { 6 } else { 29 }))?
                .max(minimum),
            today,
        ),
        3 => (first, today),
        4 => (first.checked_sub_months(Months::new(1))?, first.pred_opt()?),
        _ => return None,
    };
    (from >= minimum && from <= to && to.year() <= 9999).then_some(DateRange { from, to })
}

fn refresh(ui: &MainWindow) -> Option<DateRange> {
    let result = validate(&ui.get_export_from_date(), &ui.get_export_to_date());
    ui.set_export_range_valid(result.is_ok());
    ui.set_export_range_reversed(result == Err(RangeError::Reversed));
    match result {
        Ok(range) => {
            ui.set_export_range_status(
                format!(
                    "{} 至 {} · 共 {} 天（含起止日期）",
                    range.from,
                    range.to,
                    format_count((range.to - range.from).num_days() + 1)
                )
                .into(),
            );
            Some(range)
        }
        Err(error) => {
            ui.set_export_range_status(error.message().into());
            None
        }
    }
}

// Revalidate at the action boundary, before opening a native save dialog.
pub fn prepare(ui: &MainWindow) -> Option<DateRange> {
    // Preserve the user's draft. The caller uses canonical dates for filenames
    // and storage, without queuing field-change callbacks after a save result.
    refresh(ui)
}

pub fn bind(ui: &MainWindow) {
    ui.on_export_range_changed({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_export_text("".into());
                let selected = preset(ui.get_export_preset_index(), Local::now().date_naive());
                let current = validate(&ui.get_export_from_date(), &ui.get_export_to_date()).ok();
                if selected.is_none() || selected != current {
                    ui.set_export_preset_index(-1);
                }
                refresh(&ui);
            }
        }
    });
    ui.on_export_range_preset({
        let weak = ui.as_weak();
        move |index| {
            if let (Some(ui), Some(range)) =
                (weak.upgrade(), preset(index, Local::now().date_naive()))
            {
                ui.set_export_from_date(range.from.to_string().into());
                ui.set_export_to_date(range.to.to_string().into());
                ui.set_export_preset_index(index);
                ui.set_export_text("".into());
                refresh(&ui);
            }
        }
    });
    ui.on_export_range_swap({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade()
                && let (Some(from), Some(to)) = (
                    parse(&ui.get_export_from_date()),
                    parse(&ui.get_export_to_date()),
                )
                && from > to
            {
                ui.set_export_from_date(to.to_string().into());
                ui.set_export_to_date(from.to_string().into());
                refresh(&ui);
            }
        }
    });
    refresh(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_compares_dates_and_counts_inclusive_days() {
        assert_eq!(
            validate("2026-2-1", "2026-01-31"),
            Err(RangeError::Reversed)
        );
        assert_eq!(validate("2023-02-29", "2024-01-01"), Err(RangeError::From));
        assert_eq!(validate("2024-02-29", ""), Err(RangeError::To));
        assert_eq!(validate("0000-01-01", "2024-01-01"), Err(RangeError::From));
        assert_eq!(validate("2024-01-01", "10000-01-01"), Err(RangeError::To));
        let range = validate(" 2024-2-28 ", "2024-03-01").unwrap();
        assert_eq!((range.to - range.from).num_days() + 1, 3);
        assert_eq!(range.from.to_string(), "2024-02-28");
        let same = validate("2024-02-29", "2024-02-29").unwrap();
        assert_eq!(same.from, same.to);
    }

    #[test]
    fn quick_ranges_cover_leap_months_year_boundaries_and_valid_year_limits() {
        let march = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        let previous = preset(4, march).unwrap();
        assert_eq!(previous.from.to_string(), "2024-02-01");
        assert_eq!(previous.to.to_string(), "2024-02-29");
        assert_eq!(preset(1, march).unwrap().from.to_string(), "2024-02-24");
        assert_eq!(preset(2, march).unwrap().from.to_string(), "2024-02-01");
        let january = NaiveDate::from_ymd_opt(2026, 1, 3).unwrap();
        assert_eq!(preset(1, january).unwrap().from.to_string(), "2025-12-28");
        assert_eq!(preset(3, january).unwrap().to, january);
        assert_eq!(preset(4, january).unwrap().to.to_string(), "2025-12-31");
        let first = NaiveDate::from_ymd_opt(1, 1, 1).unwrap();
        assert_eq!(preset(2, first).unwrap().from, first);
        assert!(preset(4, first).is_none());
        assert!(preset(8, march).is_none());
    }
}
