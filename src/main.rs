mod collector;
mod config;
mod stats;
mod storage;
mod tray;
slint::include_modules!();
use crate::stats::aggregator::Aggregator;
use crate::stats::policy::CaptureMode;
use chrono::Local;
use slint::{Timer, TimerMode};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use windows::Win32::Foundation::GetLastError;
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
#[cfg(windows)]
use windows::core::w;

#[derive(Clone, Copy)]
enum RankingSort {
    Keyboard,
    Mouse,
    Total,
}

fn heat_count(rows: &[storage::database::KeyTotal], scan_code: &str) -> i32 {
    rows.iter()
        .find(|row| row.code == format!("scan:{scan_code}:ext:false"))
        .map(|row| row.count.clamp(0, i64::from(i32::MAX)) as i32)
        .unwrap_or(0)
}

#[cfg(windows)]
fn choose_csv_path(default_name: &str) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows::core::{PCWSTR, PWSTR, w};
    let mut buffer = [0u16; 32768];
    let encoded = default_name.encode_utf16().collect::<Vec<_>>();
    let length = encoded.len().min(buffer.len().saturating_sub(1));
    buffer[..length].copy_from_slice(&encoded[..length]);
    let filter: PCWSTR = w!("CSV 文件 (*.csv)\0*.csv\0所有文件 (*.*)\0*.*\0\0");
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrDefExt: w!("csv"),
        Flags: OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    unsafe {
        if !GetSaveFileNameW(&mut dialog).as_bool() {
            return None;
        }
    }
    let length = buffer.iter().position(|value| *value == 0).unwrap_or(0);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..length])))
}

fn set_heatmap(ui: &MainWindow, rows: &[storage::database::KeyTotal]) {
    for (name, scan) in [
        ("q", "10"),
        ("w", "11"),
        ("e", "12"),
        ("r", "13"),
        ("t", "14"),
        ("y", "15"),
        ("u", "16"),
        ("i", "17"),
        ("o", "18"),
        ("p", "19"),
        ("a", "1e"),
        ("s", "1f"),
        ("d", "20"),
        ("f", "21"),
        ("g", "22"),
        ("h", "23"),
        ("j", "24"),
        ("k", "25"),
        ("l", "26"),
        ("z", "2c"),
        ("x", "2d"),
        ("c", "2e"),
        ("v", "2f"),
        ("b", "30"),
        ("n", "31"),
        ("m", "32"),
    ] {
        let value = heat_count(rows, scan);
        match name {
            "q" => ui.set_heat_q(value),
            "w" => ui.set_heat_w(value),
            "e" => ui.set_heat_e(value),
            "r" => ui.set_heat_r(value),
            "t" => ui.set_heat_t(value),
            "y" => ui.set_heat_y(value),
            "u" => ui.set_heat_u(value),
            "i" => ui.set_heat_i(value),
            "o" => ui.set_heat_o(value),
            "p" => ui.set_heat_p(value),
            "a" => ui.set_heat_a(value),
            "s" => ui.set_heat_s(value),
            "d" => ui.set_heat_d(value),
            "f" => ui.set_heat_f(value),
            "g" => ui.set_heat_g(value),
            "h" => ui.set_heat_h(value),
            "j" => ui.set_heat_j(value),
            "k" => ui.set_heat_k(value),
            "l" => ui.set_heat_l(value),
            "z" => ui.set_heat_z(value),
            "x" => ui.set_heat_x(value),
            "c" => ui.set_heat_c(value),
            "v" => ui.set_heat_v(value),
            "b" => ui.set_heat_b(value),
            "n" => ui.set_heat_n(value),
            "m" => ui.set_heat_m(value),
            _ => {}
        }
    }
    ui.set_heat_space(heat_count(rows, "39"));
}

impl RankingSort {
    fn as_query(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
            Self::Total => "total",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Keyboard => "当前按键盘排序",
            Self::Mouse => "当前按鼠标排序",
            Self::Total => "当前按总量排序",
        }
    }
}

fn refresh_overview(
    ui: &MainWindow,
    database: &storage::database::Database,
    date: &str,
    sort: RankingSort,
) -> anyhow::Result<()> {
    let (keyboard, mouse, wheel, apps) = database.totals(date)?;
    ui.set_date_text(date.into());
    ui.set_query_date(date.into());
    ui.set_keyboard_text(keyboard.to_string().into());
    ui.set_mouse_text(mouse.to_string().into());
    ui.set_wheel_text(wheel.to_string().into());
    ui.set_app_text(apps.to_string().into());
    let ranking = database
        .app_totals_sorted(date, sort.as_query())?
        .into_iter()
        .take(8)
        .enumerate()
        .map(|(index, row)| {
            format!(
                "{}. {}\n   路径: {}\n   键盘:{} 鼠标:{} 滚轮:{}",
                index + 1,
                row.app_name,
                row.app_path,
                row.keyboard,
                row.mouse,
                row.wheel
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    ui.set_ranking_text(ranking.into());
    ui.set_ranking_sort_text(sort.label().into());
    ui.set_heatmap_title("键盘热力图（当前日期，全局）".into());
    let all_keys = database.input_ranking(date, None, "keyboard")?;
    let keys = all_keys
        .iter()
        .take(10)
        .map(|row| format!("{}:{}", row.code, row.count))
        .collect::<Vec<_>>()
        .join("  ");
    let mouse = database
        .input_ranking(date, None, "mouse")?
        .into_iter()
        .map(|row| format!("{}:{}", row.code, row.count))
        .collect::<Vec<_>>()
        .join("  ");
    ui.set_frequency_text(
        format!(
            "全局按键排行: {}\n鼠标分类统计: {}",
            if keys.is_empty() {
                "无详细数据"
            } else {
                &keys
            },
            if mouse.is_empty() {
                "无详细数据"
            } else {
                &mouse
            }
        )
        .into(),
    );
    set_heatmap(ui, &all_keys);
    Ok(())
}

fn refresh_detail(
    ui: &MainWindow,
    database: &storage::database::Database,
    date: &str,
    path: &str,
) -> anyhow::Result<()> {
    let Some(detail) = database.app_detail(date, path)? else {
        ui.set_detail_text("未找到该应用。请先在该应用中产生输入，或检查完整路径。".into());
        return Ok(());
    };
    let keys = database.input_ranking(date, Some(path), "keyboard")?;
    let mouse = database.input_ranking(date, Some(path), "mouse")?;
    let key_text = if detail.policy == CaptureMode::TotalsOnly {
        "未记录详细数据".to_string()
    } else {
        keys.iter()
            .take(12)
            .map(|row| format!("{}:{}", row.code, row.count))
            .collect::<Vec<_>>()
            .join("  ")
    };
    ui.set_heatmap_title("键盘热力图（当前日期，当前应用）".into());
    set_heatmap(ui, &keys);
    let mouse_text = mouse
        .into_iter()
        .map(|row| format!("{}:{}", row.code, row.count))
        .collect::<Vec<_>>()
        .join("  ");
    ui.set_detail_text(
        format!(
            "{}\n路径: {}\n策略: {}\n键盘: {}  鼠标: {}  滚轮: {}\n按键: {}\n鼠标分类: {}",
            detail.app_name,
            detail.app_path,
            detail.policy.as_str(),
            detail.keyboard,
            detail.mouse,
            detail.wheel,
            key_text,
            if mouse_text.is_empty() {
                "无详细数据"
            } else {
                &mouse_text
            }
        )
        .into(),
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        unsafe {
            let _instance_mutex =
                CreateMutexW(None, false, w!("Global\\kb_count_single_instance"))?;
            if GetLastError().0 == 183 {
                if let Ok(window) = FindWindowW(None, w!("kb_count"))
                    && !window.0.is_null()
                {
                    let _ = ShowWindow(window, SW_RESTORE);
                    let _ = SetForegroundWindow(window);
                }
                return Ok(());
            }
            let _ = _instance_mutex;
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info,fontdb=off"))
        .init();
    tray::start();
    let cfg = config::load_or_create()?;
    let dir = config::app_dir()?;
    tracing::info!(data_dir = %dir.display(), "portable data directory initialized");
    let config_state = Arc::new(Mutex::new(cfg.clone()));
    let db = Arc::new(Mutex::new(storage::database::Database::open(
        &dir.join("stats.db"),
    )?));
    let enabled = Arc::new(AtomicBool::new(cfg.capture_enabled));
    let events = collector::start();
    let db_writer = Arc::clone(&db);
    let writer_enabled = Arc::clone(&enabled);
    let flush_seconds = cfg.flush_interval_seconds.max(1);
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel(1);
    let (shutdown_ack_tx, shutdown_ack_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("kb-stats-writer".into())
        .spawn(move || {
            let flush = |events: &mpsc::Receiver<stats::event::InputEvent>,
                         db: &Arc<Mutex<storage::database::Database>>,
                         enabled: &AtomicBool| {
                let mut aggregator = Aggregator::default();
                let policies = db
                    .lock()
                    .ok()
                    .and_then(|database| database.policies().ok())
                    .unwrap_or_default();
                while let Ok(event) = events.try_recv() {
                    let mode = CaptureMode::from_overrides(&event.app_path, &policies);
                    aggregator.record(event, mode);
                }
                if !enabled.load(Ordering::Relaxed) {
                    return;
                }
                let events = aggregator.drain();
                if let Err(error) = db
                    .lock()
                    .expect("database mutex poisoned")
                    .record_batch(&events)
                {
                    tracing::error!(%error, "failed to persist aggregate batch");
                }
            };
            loop {
                match shutdown_rx.recv_timeout(Duration::from_secs(flush_seconds)) {
                    Ok(()) => {
                        flush(&events, &db_writer, &writer_enabled);
                        let _ = shutdown_ack_tx.send(());
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        flush(&events, &db_writer, &writer_enabled);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
    let ui = MainWindow::new()?;
    let date = Local::now().format("%Y-%m-%d").to_string();
    let sort_state = Arc::new(Mutex::new(RankingSort::Total));
    refresh_overview(
        &ui,
        &db.lock().expect("database mutex poisoned"),
        &date,
        RankingSort::Total,
    )?;
    ui.set_status_text(
        if cfg.capture_enabled {
            "统计运行中"
        } else {
            "统计已暂停"
        }
        .into(),
    );
    ui.set_export_text("未导出".into());
    ui.set_detail_text("请输入应用完整路径查询详情。".into());
    let startup_matches = std::env::current_exe()
        .ok()
        .and_then(|exe| config::startup::windows::is_configured(&exe).ok())
        .unwrap_or(false);
    ui.set_settings_text(
        format!(
            "数据目录: {}\n数据库: stats.db (WAL)\n版本: {}{}",
            dir.display(),
            env!("CARGO_PKG_VERSION"),
            if cfg.start_with_windows && !startup_matches {
                "\n启动项路径与当前 EXE 不一致，请重新启用。"
            } else {
                ""
            }
        )
        .into(),
    );
    ui.set_startup_text(
        if cfg.start_with_windows && startup_matches {
            "开机启动：已开启"
        } else if cfg.start_with_windows {
            "开机启动：路径不一致"
        } else {
            "开机启动：未开启"
        }
        .into(),
    );
    ui.set_clear_button_text("清空统计数据".into());
    let enabled_for_ui = Arc::clone(&enabled);
    let ui_for_state = ui.as_weak();
    let db_for_export = Arc::clone(&db);
    #[cfg(not(windows))]
    let dir_for_export = dir.clone();
    let ui_for_export = ui.as_weak();
    let db_for_date = Arc::clone(&db);
    let ui_for_date = ui.as_weak();
    ui.on_today_clicked({
        let db = Arc::clone(&db_for_date);
        let ui = ui_for_date.clone();
        let sort = Arc::clone(&sort_state);
        move || {
            let date = Local::now().format("%Y-%m-%d").to_string();
            if let Some(ui) = ui.upgrade() {
                let database = db.lock().expect("database mutex poisoned");
                let _ = refresh_overview(
                    &ui,
                    &database,
                    &date,
                    *sort.lock().expect("ranking state poisoned"),
                );
            }
        }
    });
    ui.on_yesterday_clicked({
        let db = db_for_date;
        let ui = ui_for_date;
        let sort = Arc::clone(&sort_state);
        move || {
            let date = (Local::now() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            if let Some(ui) = ui.upgrade() {
                let database = db.lock().expect("database mutex poisoned");
                let _ = refresh_overview(
                    &ui,
                    &database,
                    &date,
                    *sort.lock().expect("ranking state poisoned"),
                );
            }
        }
    });
    let db_for_query = Arc::clone(&db);
    let ui_for_query = ui.as_weak();
    let sort_for_query = Arc::clone(&sort_state);
    ui.on_query_clicked(move || {
        let Some(ui) = ui_for_query.upgrade() else {
            return;
        };
        let date = ui.get_query_date().to_string();
        if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
            ui.set_status_text("日期格式应为 YYYY-MM-DD".into());
            return;
        }
        let database = db_for_query.lock().expect("database mutex poisoned");
        match refresh_overview(
            &ui,
            &database,
            &date,
            *sort_for_query.lock().expect("ranking state poisoned"),
        ) {
            Ok(()) => {
                ui.set_status_text("已查询指定日期".into());
            }
            Err(error) => ui.set_status_text(format!("查询失败：{error}").into()),
        }
    });
    let db_for_sort = Arc::clone(&db);
    let ui_for_sort = ui.as_weak();
    let sort_for_keyboard = Arc::clone(&sort_state);
    ui.on_sort_keyboard_clicked(move || {
        *sort_for_keyboard.lock().expect("ranking state poisoned") = RankingSort::Keyboard;
        if let Some(ui) = ui_for_sort.upgrade() {
            let date = ui.get_date_text().to_string();
            if let Ok(database) = db_for_sort.lock() {
                let _ = refresh_overview(&ui, &database, &date, RankingSort::Keyboard);
            }
        }
    });
    let db_for_sort = Arc::clone(&db);
    let ui_for_sort = ui.as_weak();
    let sort_for_mouse = Arc::clone(&sort_state);
    ui.on_sort_mouse_clicked(move || {
        *sort_for_mouse.lock().expect("ranking state poisoned") = RankingSort::Mouse;
        if let Some(ui) = ui_for_sort.upgrade() {
            let date = ui.get_date_text().to_string();
            if let Ok(database) = db_for_sort.lock() {
                let _ = refresh_overview(&ui, &database, &date, RankingSort::Mouse);
            }
        }
    });
    let db_for_sort = Arc::clone(&db);
    let ui_for_sort = ui.as_weak();
    let sort_for_total = Arc::clone(&sort_state);
    ui.on_sort_total_clicked(move || {
        *sort_for_total.lock().expect("ranking state poisoned") = RankingSort::Total;
        if let Some(ui) = ui_for_sort.upgrade() {
            let date = ui.get_date_text().to_string();
            if let Ok(database) = db_for_sort.lock() {
                let _ = refresh_overview(&ui, &database, &date, RankingSort::Total);
            }
        }
    });
    let db_for_detail = Arc::clone(&db);
    let ui_for_detail = ui.as_weak();
    ui.on_query_app_clicked(move || {
        if let Some(ui) = ui_for_detail.upgrade() {
            let path = ui.get_detail_path().to_string();
            let date = ui.get_date_text().to_string();
            if path.trim().is_empty() {
                ui.set_detail_text("请输入应用完整路径查询详情。".into());
            } else if let Ok(database) = db_for_detail.lock() {
                let _ = refresh_detail(&ui, &database, &date, path.trim());
            }
        }
    });
    let db_for_policy = Arc::clone(&db);
    let ui_for_policy = ui.as_weak();
    let set_policy = Arc::new(move |mode: CaptureMode| {
        if let Some(ui) = ui_for_policy.upgrade() {
            let path = ui.get_policy_path().to_string();
            if path.trim().is_empty() {
                ui.set_settings_text("请输入应用完整路径后设置采集策略。".into());
            } else {
                let result = db_for_policy
                    .lock()
                    .expect("database mutex poisoned")
                    .set_policy(path.trim(), mode.as_str());
                match result {
                    Ok(()) => ui.set_settings_text(
                        format!("已设置 {} 的采集策略为 {}。", path.trim(), mode.as_str()).into(),
                    ),
                    Err(error) => ui.set_settings_text(format!("设置策略失败：{error}").into()),
                }
            }
        }
    });
    ui.on_policy_detailed_clicked({
        let set_policy = Arc::clone(&set_policy);
        move || set_policy(CaptureMode::Detailed)
    });
    ui.on_policy_totals_clicked({
        let set_policy = Arc::clone(&set_policy);
        move || set_policy(CaptureMode::TotalsOnly)
    });
    ui.on_policy_excluded_clicked({
        let set_policy = Arc::clone(&set_policy);
        move || set_policy(CaptureMode::Excluded)
    });
    let startup_config = Arc::clone(&config_state);
    let startup_ui = ui.as_weak();
    let startup_exe = std::env::current_exe().ok();
    ui.on_startup_clicked(move || {
        let Some(exe) = startup_exe.as_ref() else {
            return;
        };
        let mut config = startup_config.lock().expect("config mutex poisoned");
        let configured = config::startup::windows::is_configured(exe).unwrap_or(false);
        let next = if config.start_with_windows && !configured {
            true
        } else {
            !config.start_with_windows
        };
        match config::startup::windows::set_enabled(exe, next) {
            Ok(()) => {
                config.start_with_windows = next;
                if let Err(error) = config::save(&config) {
                    tracing::error!(%error, "failed to save startup configuration");
                }
                if let Some(ui) = startup_ui.upgrade() {
                    ui.set_startup_text(
                        if next {
                            "开机启动：已开启"
                        } else {
                            "开机启动：未开启"
                        }
                        .into(),
                    );
                    ui.set_settings_text("开机启动设置已更新。移动 EXE 后需要重新开启。".into());
                }
            }
            Err(error) => {
                if let Some(ui) = startup_ui.upgrade() {
                    ui.set_settings_text(format!("开机启动设置失败：{error}").into());
                }
            }
        }
    });
    let clear_pending = Arc::new(AtomicBool::new(false));
    let clear_db = Arc::clone(&db);
    let clear_ui = ui.as_weak();
    let clear_sort = Arc::clone(&sort_state);
    let clear_pending_for_ui = Arc::clone(&clear_pending);
    ui.on_clear_data_clicked(move || {
        let Some(ui) = clear_ui.upgrade() else {
            return;
        };
        if !clear_pending_for_ui.swap(true, Ordering::Relaxed) {
            ui.set_clear_button_text("再次点击确认清空".into());
            ui.set_settings_text("清空操作不可撤销，请再次点击确认。".into());
            return;
        }
        let result = clear_db
            .lock()
            .expect("database mutex poisoned")
            .clear_all();
        clear_pending_for_ui.store(false, Ordering::Relaxed);
        ui.set_clear_button_text("清空统计数据".into());
        match result {
            Ok(()) => {
                let date = ui.get_date_text().to_string();
                if let Ok(database) = clear_db.lock() {
                    let _ = refresh_overview(
                        &ui,
                        &database,
                        &date,
                        *clear_sort.lock().expect("ranking state poisoned"),
                    );
                }
                ui.set_detail_text("请输入应用完整路径查询详情。".into());
                ui.set_settings_text("统计数据已清空。".into());
            }
            Err(error) => ui.set_settings_text(format!("清空失败：{error}").into()),
        }
    });
    let pause_config = Arc::clone(&config_state);
    ui.on_pause_clicked(move || {
        let value = !enabled_for_ui.load(Ordering::Relaxed);
        enabled_for_ui.store(value, Ordering::Relaxed);
        if let Ok(mut config) = pause_config.lock() {
            config.capture_enabled = value;
            if let Err(error) = config::save(&config) {
                tracing::error!(%error, "failed to save capture state");
            }
        }
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
        let selected_date = ui_for_export
            .upgrade()
            .map(|ui| ui.get_date_text().to_string())
            .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
        let default_name = format!("kb_count-{}.csv", selected_date);
        #[cfg(windows)]
        let Some(path) = choose_csv_path(&default_name) else {
            if let Some(ui) = ui_for_export.upgrade() {
                ui.set_export_text("已取消导出".into());
            }
            return;
        };
        #[cfg(not(windows))]
        let path = dir_for_export.join(default_name);
        let text = match storage::export::export_date(
            &db_for_export.lock().expect("database mutex poisoned"),
            &selected_date,
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
    let tray_enabled = Arc::clone(&enabled);
    let tray_ui = ui.as_weak();
    let tray_db = Arc::clone(&db);
    let tray_shutdown = shutdown_tx;
    let tray_config = Arc::clone(&config_state);
    let tray_timer = Timer::default();
    tray_timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        let Some(command) = tray::take_request() else {
            return;
        };
        match command {
            tray::Command::Open | tray::Command::Settings => {
                if let Some(ui) = tray_ui.upgrade() {
                    let _ = ui.show();
                }
            }
            tray::Command::TogglePause => {
                let value = !tray_enabled.load(Ordering::Relaxed);
                tray_enabled.store(value, Ordering::Relaxed);
                if let Ok(mut config) = tray_config.lock() {
                    config.capture_enabled = value;
                    if let Err(error) = config::save(&config) {
                        tracing::error!(%error, "failed to save capture state");
                    }
                }
                if let Some(ui) = tray_ui.upgrade() {
                    ui.set_status_text(
                        if value {
                            "统计运行中"
                        } else {
                            "统计已暂停"
                        }
                        .into(),
                    );
                }
            }
            tray::Command::Exit => {
                let _ = tray_shutdown.try_send(());
                let _ = shutdown_ack_rx.recv_timeout(Duration::from_secs(2));
                let _ = tray_db
                    .lock()
                    .expect("database mutex poisoned")
                    .checkpoint();
                std::process::exit(0);
            }
        }
    });
    ui.hide()?;
    ui.run()?;
    Ok(())
}
