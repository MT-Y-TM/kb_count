//! Date drafts are separate from the date of the displayed statistics.
use crate::{MainWindow, RankingSort, refresh_overview, storage::database::Database};
use chrono::{Datelike, Duration, Local, NaiveDate};
use slint::ComponentHandle;
use std::sync::{Arc, Mutex, TryLockError};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Feedback {
    failed: Option<(String, String, &'static str)>,
}

pub fn parse(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .ok()
        .filter(|date| (1..=9999).contains(&date.year()))
}

fn validate(ui: &MainWindow) -> Option<NaiveDate> {
    let date = parse(&ui.get_query_date());
    ui.set_query_date_valid(date.is_some());
    ui.set_query_date_pending(date.is_some_and(|date| Some(date) != parse(&ui.get_date_text())));
    date
}

fn refresh(ui: &MainWindow, feedback: &RefCell<Feedback>) {
    validate(ui);
    let mut feedback = feedback.borrow_mut();
    if let Some((draft, displayed, message)) = &feedback.failed
        && draft == ui.get_query_date().as_str()
        && displayed == ui.get_date_text().as_str()
    {
        ui.set_query_date_failed(true);
        ui.set_query_date_hint((*message).into());
        return;
    }
    feedback.failed = None;
    ui.set_query_date_failed(false);
    ui.set_query_date_hint(
        if !ui.get_query_date_valid() {
            "请输入有效日期 YYYY-MM-DD"
        } else if ui.get_query_date_pending() {
            "尚未查询 · 回车或点击查询"
        } else {
            "回车或点击查询可刷新"
        }
        .into(),
    );
}

fn failure(ui: &MainWindow, feedback: &RefCell<Feedback>, message: &'static str) {
    // Deferred Slint changed handlers for this same draft must not erase a
    // failed attempt. Editing or displaying another date invalidates it.
    feedback.borrow_mut().failed = Some((
        ui.get_query_date().to_string(),
        ui.get_date_text().to_string(),
        message,
    ));
    ui.set_query_date_failed(true);
    ui.set_query_date_hint(message.into());
    ui.set_status_text(message.into());
}

fn query(
    ui: &MainWindow,
    database: &Mutex<Database>,
    sort: &Mutex<RankingSort>,
    feedback: &RefCell<Feedback>,
) {
    feedback.borrow_mut().failed = None;
    refresh(ui, feedback);
    let Some(date) = validate(ui) else {
        return;
    };
    // Short-lived writer contention must not block the UI event loop. The
    // displayed date remains explicit while the user retries the new draft.
    let database = match database.try_lock() {
        Ok(database) => database,
        Err(TryLockError::WouldBlock) => {
            failure(ui, feedback, "统计正在保存，请稍后重试");
            return;
        }
        Err(TryLockError::Poisoned(_)) => {
            failure(ui, feedback, "数据库不可用，请重新启动");
            return;
        }
    };
    let sort = *sort.lock().expect("ranking state poisoned");
    match refresh_overview(ui, &database, &date.to_string(), sort) {
        Ok(()) => {
            ui.set_query_date(date.to_string().into());
            refresh(ui, feedback);
            ui.set_status_text(format!("已显示 {date} 的统计").into());
        }
        Err(error) => {
            tracing::warn!(%error, "statistics date query failed");
            failure(ui, feedback, "查询失败，请检查数据文件");
        }
    }
}

pub fn bind(ui: &MainWindow, database: Arc<Mutex<Database>>, sort: Arc<Mutex<RankingSort>>) {
    let feedback = Rc::new(RefCell::new(Feedback::default()));
    ui.on_query_date_changed({
        let weak = ui.as_weak();
        let feedback = feedback.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &feedback);
            }
        }
    });
    ui.on_query_clicked({
        let weak = ui.as_weak();
        let database = database.clone();
        let sort = sort.clone();
        let feedback = feedback.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                query(&ui, &database, &sort, &feedback);
            }
        }
    });
    let select = |offset| {
        let weak = ui.as_weak();
        let database = database.clone();
        let sort = sort.clone();
        let feedback = feedback.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_query_date(
                    (Local::now().date_naive() + Duration::days(offset))
                        .to_string()
                        .into(),
                );
                query(&ui, &database, &sort, &feedback);
            }
        }
    };
    ui.on_today_clicked(select(0));
    ui.on_yesterday_clicked(select(-1));
    refresh(ui, &feedback);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_dates_normalize_padding_and_spaces_without_accepting_invalid_years() {
        assert_eq!(parse(" 2024-2-29 ").unwrap().to_string(), "2024-02-29");
        for text in [
            "",
            "2024-",
            "2023-02-29",
            "2024-04-31",
            "0000-01-01",
            "+10000-01-01",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
        for text in ["0001-01-01", "9999-12-31"] {
            assert_eq!(parse(text).unwrap().to_string(), text);
        }
    }
}
