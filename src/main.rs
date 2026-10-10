mod config; mod storage; mod stats; mod collector;
slint::include_modules!();
use chrono::Local;
use std::sync::{Arc,Mutex};
fn main() -> anyhow::Result<()> {
 tracing_subscriber::fmt::init();
 let cfg=config::load_or_create()?; let dir=config::app_dir()?; let db=Arc::new(Mutex::new(storage::database::Database::open(&dir.join("stats.db"))?));
 collector::raw_input::start();
 let ui=MainWindow::new()?; let date=Local::now().format("%Y-%m-%d").to_string(); let (k,m,w,a)=db.lock().unwrap().totals(&date)?;
 ui.set_date_text(date.into()); ui.set_keyboard_text(k.to_string().into()); ui.set_mouse_text(m.to_string().into()); ui.set_wheel_text(w.to_string().into()); ui.set_app_text(a.to_string().into()); ui.set_status_text(if cfg.capture_enabled {"统计运行中"} else {"统计已暂停"}.into());
 let enabled=Arc::new(Mutex::new(cfg.capture_enabled)); let enabled2=enabled.clone(); ui.on_pause_clicked(move || { let mut v=enabled2.lock().unwrap(); *v=!*v; }); ui.run()?; Ok(())
}
