mod collector;
mod config;
mod stats;
mod storage;
slint::include_modules!();
use crate::stats::aggregator::Aggregator;
use crate::stats::policy::CaptureMode;
use chrono::Local;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use windows::Win32::Foundation::GetLastError;
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::core::w;
fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        unsafe {
            let _instance_mutex =
                CreateMutexW(None, false, w!("Global\\kb_count_single_instance"))?;
            if GetLastError().0 == 183 {
                return Ok(());
            }
            let _ = _instance_mutex;
        }
    }
    tracing_subscriber::fmt::init();
    let cfg = config::load_or_create()?;
    let dir = config::app_dir()?;
    let db = Arc::new(Mutex::new(storage::database::Database::open(
        &dir.join("stats.db"),
    )?));
    let enabled = Arc::new(AtomicBool::new(cfg.capture_enabled));
    let events = collector::start();
    let db_writer = Arc::clone(&db);
    let writer_enabled = Arc::clone(&enabled);
    let flush_seconds = cfg.flush_interval_seconds.max(1);
    thread::Builder::new()
        .name("kb-stats-writer".into())
        .spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(flush_seconds));
                let mut aggregator = Aggregator::default();
                while let Ok(event) = events.try_recv() {
                    let mode = CaptureMode::for_application(&event.app_path);
                    aggregator.record(event, mode);
                }
                if !writer_enabled.load(Ordering::Relaxed) {
                    continue;
                }
                for event in aggregator.drain() {
                    if let Err(error) = db_writer
                        .lock()
                        .expect("database mutex poisoned")
                        .record(&event)
                    {
                        tracing::error!(%error, "failed to persist aggregate");
                    }
                }
            }
        })?;
    let ui = MainWindow::new()?;
    let date = Local::now().format("%Y-%m-%d").to_string();
    let (keyboard, mouse, wheel, apps) = db.lock().unwrap().totals(&date)?;
    ui.set_date_text(date.clone().into());
    ui.set_keyboard_text(keyboard.to_string().into());
    ui.set_mouse_text(mouse.to_string().into());
    ui.set_wheel_text(wheel.to_string().into());
    ui.set_app_text(apps.to_string().into());
    ui.set_status_text(
        if cfg.capture_enabled {
            "统计运行中"
        } else {
            "统计已暂停"
        }
        .into(),
    );
    let ranking = db
        .lock()
        .unwrap()
        .app_totals(&date)?
        .into_iter()
        .take(8)
        .enumerate()
        .map(|(index, row)| {
            format!(
                "{}. {}  键盘:{} 鼠标:{} 滚轮:{}",
                index + 1,
                row.app_name,
                row.keyboard,
                row.mouse,
                row.wheel
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ui.set_export_text("未导出".into());
    ui.set_ranking_text(ranking.into());
    let enabled_for_ui = Arc::clone(&enabled);
    let ui_for_state = ui.as_weak();
    let db_for_export = Arc::clone(&db);
    let dir_for_export = dir.clone();
    let date_for_export = date.clone();
    let ui_for_export = ui.as_weak();
    let db_for_date = Arc::clone(&db);
    let ui_for_date = ui.as_weak();
    ui.on_today_clicked({
        let db = Arc::clone(&db_for_date);
        let ui = ui_for_date.clone();
        move || {
            let date = Local::now().format("%Y-%m-%d").to_string();
            if let Ok((k, m, w, a)) = db.lock().expect("database mutex poisoned").totals(&date) {
                if let Some(ui) = ui.upgrade() {
                    ui.set_date_text(date.into());
                    ui.set_keyboard_text(k.to_string().into());
                    ui.set_mouse_text(m.to_string().into());
                    ui.set_wheel_text(w.to_string().into());
                    ui.set_app_text(a.to_string().into());
                }
            }
        }
    });
    ui.on_yesterday_clicked({
        let db = db_for_date;
        let ui = ui_for_date;
        move || {
            let date = (Local::now() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            if let Ok((k, m, w, a)) = db.lock().expect("database mutex poisoned").totals(&date) {
                if let Some(ui) = ui.upgrade() {
                    ui.set_date_text(date.into());
                    ui.set_keyboard_text(k.to_string().into());
                    ui.set_mouse_text(m.to_string().into());
                    ui.set_wheel_text(w.to_string().into());
                    ui.set_app_text(a.to_string().into());
                }
            }
        }
    });
    ui.on_pause_clicked(move || {
        let value = !enabled_for_ui.load(Ordering::Relaxed);
        enabled_for_ui.store(value, Ordering::Relaxed);
        if let Some(ui) = ui_for_state.upgrade() {
            ui.set_status_text(
                if value {
                    "统计运行中"
                } else {
                    "统计已暂停"
                }
                .into(),
            );
        }
    });
    ui.on_export_clicked(move || {
        let path = dir_for_export.join(format!("kb_count-{}.csv", date_for_export));
        let text = match storage::export::export_date(
            &db_for_export.lock().expect("database mutex poisoned"),
            &date_for_export,
            &path,
        ) {
            Ok(()) => format!("已导出：{}", path.display()),
            Err(error) => format!("导出失败：{error}"),
        };
        tracing::info!(%text);
        if let Some(ui) = ui_for_export.upgrade() {
            ui.set_export_text(text.into());
        }
    });
    ui.run()?;
    Ok(())
}
