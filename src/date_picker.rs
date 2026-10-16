use crate::{CalendarDay, MainWindow};
use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};

// Calendar state is UI-only. Opening or browsing it performs no database work.
struct Calendar {
    selected: NaiveDate,
    month: NaiveDate,
    today: NaiveDate,
    target: i32,
}

impl Calendar {
    fn new(text: &str, today: NaiveDate, target: i32) -> Self {
        let selected = NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .filter(|date| (1..=9999).contains(&date.year()))
            .unwrap_or(today);
        Self {
            selected,
            month: selected.with_day(1).expect("first day"),
            today,
            target,
        }
    }

    fn shift_month(&mut self, delta: i32) {
        let month = if delta >= 0 {
            self.month.checked_add_months(Months::new(delta as u32))
        } else {
            self.month
                .checked_sub_months(Months::new(delta.unsigned_abs()))
        };
        if let Some(month) = month.filter(|date| (1..=9999).contains(&date.year())) {
            // Chrono clamps e.g. January 31 to February 28/29.
            self.selected = if delta >= 0 {
                self.selected.checked_add_months(Months::new(delta as u32))
            } else {
                self.selected
                    .checked_sub_months(Months::new(delta.unsigned_abs()))
            }
            .unwrap_or(month);
            self.month = month;
        }
    }

    fn shift_day(&mut self, delta: i32) {
        if let Some(date) = self
            .selected
            .checked_add_signed(Duration::days(delta.into()))
            .filter(|date| (1..=9999).contains(&date.year()))
        {
            self.selected = date;
            self.month = date.with_day(1).expect("first day");
        }
    }

    fn days(&self) -> Vec<CalendarDay> {
        let start = self.month - Duration::days(self.month.weekday().num_days_from_monday().into());
        (0..42)
            .map(|offset| {
                let date = start + Duration::days(offset);
                let valid = (1..=9999).contains(&date.year());
                CalendarDay {
                    label: if valid {
                        date.day().to_string()
                    } else {
                        String::new()
                    }
                    .into(),
                    date: if valid {
                        date.to_string()
                    } else {
                        String::new()
                    }
                    .into(),
                    in_month: date.month() == self.month.month(),
                    selected: date == self.selected,
                    today: date == self.today,
                }
            })
            .collect()
    }

    fn render(&self, ui: &MainWindow) {
        ui.set_calendar_month(format!("{} 年 {} 月", self.month.year(), self.month.month()).into());
        ui.set_calendar_caption(
            match self.target {
                1 => "选择导出开始日期",
                2 => "选择导出结束日期",
                _ => "选择统计日期",
            }
            .into(),
        );
        ui.set_calendar_selection(self.selected.to_string().into());
        ui.set_calendar_days(Rc::new(slint::VecModel::from(self.days())).into());
    }

    fn apply(&self, ui: &MainWindow, date: NaiveDate) {
        let date = date.to_string().into();
        ui.set_calendar_open(false);
        match self.target {
            1 => ui.set_export_from_date(date),
            2 => ui.set_export_to_date(date),
            _ => {
                ui.set_query_date(date);
                ui.invoke_query_clicked();
            }
        }
    }
}

pub fn bind(ui: &MainWindow) {
    let state = Rc::new(RefCell::new(Calendar::new(
        "",
        Local::now().date_naive(),
        0,
    )));
    ui.on_calendar_requested({
        let weak = ui.as_weak();
        let state = state.clone();
        move |target| {
            let Some(ui) = weak.upgrade() else { return };
            let text = match target {
                1 => ui.get_export_from_date(),
                2 => ui.get_export_to_date(),
                _ => ui.get_query_date(),
            };
            let calendar = Calendar::new(&text, Local::now().date_naive(), target);
            calendar.render(&ui);
            *state.borrow_mut() = calendar;
            ui.set_calendar_open(true);
        }
    });
    ui.on_calendar_month_shift({
        let weak = ui.as_weak();
        let state = state.clone();
        move |delta| {
            if let Some(ui) = weak.upgrade() {
                let mut calendar = state.borrow_mut();
                calendar.shift_month(delta);
                calendar.render(&ui);
            }
        }
    });
    ui.on_calendar_day_shift({
        let weak = ui.as_weak();
        let state = state.clone();
        move |delta| {
            if let Some(ui) = weak.upgrade() {
                let mut calendar = state.borrow_mut();
                calendar.shift_day(delta);
                calendar.render(&ui);
            }
        }
    });
    ui.on_calendar_chosen({
        let weak = ui.as_weak();
        let state = state.clone();
        move |text| {
            if let (Some(ui), Ok(date)) =
                (weak.upgrade(), NaiveDate::parse_from_str(&text, "%Y-%m-%d"))
            {
                state.borrow().apply(&ui, date);
            }
        }
    });
    ui.on_calendar_quick_chosen({
        let weak = ui.as_weak();
        move |offset| {
            if let Some(ui) = weak.upgrade() {
                state.borrow().apply(
                    &ui,
                    Local::now().date_naive() + Duration::days(offset.into()),
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_month_grid_has_monday_columns_and_adjacent_days() {
        let today = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let calendar = Calendar::new("2024-02-29", today, 0);
        let days = calendar.days();
        assert_eq!(days.len(), 42);
        assert_eq!(days[0].date, "2024-01-29");
        assert_eq!(days[41].date, "2024-03-10");
        assert_eq!(days.iter().filter(|day| day.in_month).count(), 29);
        let selected = days.iter().find(|day| day.selected).unwrap();
        assert!(selected.today);
        assert_eq!(selected.date, "2024-02-29");
    }

    #[test]
    fn navigation_clamps_end_of_month_and_crosses_years() {
        let today = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        let mut calendar = Calendar::new("2024-01-31", today, 1);
        calendar.shift_month(1);
        assert_eq!(calendar.selected.to_string(), "2024-02-29");
        calendar.shift_month(-12);
        assert_eq!(calendar.selected.to_string(), "2023-02-28");
        calendar.shift_day(1);
        assert_eq!(calendar.selected.to_string(), "2023-03-01");
        calendar.shift_day(-1);
        assert_eq!(calendar.month.to_string(), "2023-02-01");
    }

    #[test]
    fn invalid_input_falls_back_and_year_bounds_do_not_overflow() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        assert_eq!(Calendar::new("2026-02-30", today, 0).selected, today);
        assert_eq!(Calendar::new("invalid", today, 0).selected, today);
        for (text, delta) in [("0001-01-01", -1), ("9999-12-31", 1)] {
            let mut calendar = Calendar::new(text, today, 0);
            calendar.shift_month(delta);
            calendar.shift_day(delta);
            assert_eq!(calendar.selected.to_string(), text);
            assert_eq!(calendar.days().len(), 42);
        }
    }
}
