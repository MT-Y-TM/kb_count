mod config;
mod storage;
mod stats;
mod collector;
slint::include_modules!();
use chrono::Local;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::stats::aggregator::Aggregator;
use crate::stats::policy::CaptureMode;
#[cfg(windows)] use windows::core::w;
#[cfg(windows)] use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)] use windows::Win32::Foundation::GetLastError;
fn main() -> anyhow::Result<()> {
    #[cfg(windows)] { unsafe { let _instance_mutex = CreateMutexW(None, false, w!("Global\\kb_count_single_instance"))?; if GetLastError().0 == 183 { return Ok(()); } let _ = _instance_mutex; } }
    tracing_subscriber::fmt::init();
    let cfg = config::load_or_create()?;
    let dir = config::app_dir()?;
    let db = Arc::new(Mutex::new(storage::database::Database::open(&dir.join("stats.db"))?));
    let events = collector::start();
    let db_writer = Arc::clone(&db);
    thread::Builder::new().name("kb-stats-writer".into()).spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(5));
            let mut aggregator = Aggregator::default();
            while let Ok(event) = events.try_recv() { aggregator.record(event, CaptureMode::Detailed); }
            for event in aggregator.drain() {
                if let Err(error) = db_writer.lock().expect("database mutex poisoned").record(&event) { tracing::error!(%error, "failed to persist aggregate"); }
            }
        }
    })?;
    let ui = MainWindow::new()?;
    let date = Local::now().format("%Y-%m-%d").to_string();
    let (keyboard, mouse, wheel, apps) = db.lock().unwrap().totals(&date)?;
    ui.set_date_text(date.into());
    ui.set_keyboard_text(keyboard.to_string().into());
    ui.set_mouse_text(mouse.to_string().into());
    ui.set_wheel_text(wheel.to_string().into());
    ui.set_app_text(apps.to_string().into());
    ui.set_status_text(if cfg.capture_enabled { "统计运行中" } else { "统计已暂停" }.into());
    let enabled = Arc::new(Mutex::new(cfg.capture_enabled));
    let enabled2 = Arc::clone(&enabled);
    ui.on_pause_clicked(move || { let mut value = enabled2.lock().expect("state mutex poisoned"); *value = !*value; });
    ui.run()?;
    Ok(())
}


