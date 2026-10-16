#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod analytics;
mod app_icons;
mod charts;
mod collector;
mod config;
mod date_picker;
mod pie;
mod stats;
mod storage;
mod tray;
#[cfg(test)]
mod ui_tests;
mod ui_window;
slint::include_modules!();
use crate::stats::aggregator::Aggregator;
use crate::stats::policy::CaptureMode;
use anyhow::Context;
use chrono::Local;
use slint::{CloseRequestResponse, Model, Timer, TimerMode};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use windows::Win32::Foundation::GetLastError;
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy)]
enum RankingSort {
    Keyboard,
    Mouse,
    Total,
}

enum WriterCommand {
    Clear(mpsc::SyncSender<bool>),
    Shutdown(mpsc::SyncSender<bool>),
}

fn exit_handshake(
    enabled: &AtomicBool,
    commands: &mpsc::SyncSender<WriterCommand>,
    settle: impl FnOnce() -> bool,
    timeout: Duration,
) -> bool {
    let previous = enabled.swap(false, Ordering::AcqRel);
    // Rendezvous: a successful send proves the exit caller received the ACK.
    // With a buffered ACK, it could time out and resume after the writer stops.
    let (ack_tx, ack_rx) = mpsc::sync_channel(0);
    let saved = settle()
        && commands.try_send(WriterCommand::Shutdown(ack_tx)).is_ok()
        && ack_rx.recv_timeout(timeout).unwrap_or(false);
    if !saved {
        enabled.store(previous, Ordering::Release);
    }
    saved
}

fn acknowledge_exit(ack: mpsc::SyncSender<bool>, saved: bool) -> bool {
    ack.send(saved).is_ok() && saved
}

fn heat_count(rows: &[storage::database::KeyTotal], scan_code: &str) -> i32 {
    heat_count_ext(rows, scan_code, false)
}

fn heat_count_ext(rows: &[storage::database::KeyTotal], scan_code: &str, extended: bool) -> i32 {
    rows.iter()
        .find(|row| row.code == format!("scan:{scan_code}:ext:{extended}"))
        .map(|row| row.count.clamp(0, i64::from(i32::MAX)) as i32)
        .unwrap_or(0)
}

fn display_input_code(code: &str) -> String {
    let mut parts = code.split(':');
    if parts.next() != Some("scan") {
        return match code {
            "left" => "鼠标左键",
            "right" => "鼠标右键",
            "middle" => "鼠标中键",
            "x1" => "侧键 XButton1",
            "x2" => "侧键 XButton2",
            "wheel_up" => "滚轮向上",
            "wheel_down" => "滚轮向下",
            "hwheel_up" => "水平滚轮向右",
            "hwheel_down" => "水平滚轮向左",
            _ => code,
        }
        .to_string();
    }
    let Some(scan) = parts
        .next()
        .and_then(|value| u8::from_str_radix(value, 16).ok())
    else {
        return code.to_string();
    };
    let extended = parts
        .nth(1)
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let name = match (scan, extended) {
        (0x01, _) => "Esc",
        (0x02..=0x0b, _) => {
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"][(scan - 0x02) as usize]
        }
        (0x0c, _) => "-",
        (0x0d, _) => "=",
        (0x0e, _) => "Backspace",
        (0x0f, _) => "Tab",
        (0x10..=0x19, _) => {
            ["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P"][(scan - 0x10) as usize]
        }
        (0x1a, _) => "[",
        (0x1b, _) => "]",
        (0x1c, true) => "Numpad Enter",
        (0x1c, false) => "Enter",
        (0x1d, true) => "Right Ctrl",
        (0x1d, false) => "Left Ctrl",
        (0x1e..=0x26, _) => ["A", "S", "D", "F", "G", "H", "J", "K", "L"][(scan - 0x1e) as usize],
        (0x27, _) => ";",
        (0x28, _) => "'",
        (0x29, _) => "`",
        (0x2a, _) => "Left Shift",
        (0x2b, _) => "\\",
        (0x2c..=0x32, _) => ["Z", "X", "C", "V", "B", "N", "M"][(scan - 0x2c) as usize],
        (0x33, _) => ",",
        (0x34, _) => ".",
        (0x35, true) => "Numpad /",
        (0x35, false) => "/",
        (0x36, _) => "Right Shift",
        (0x37, true) => "Print Screen",
        (0x37, false) => "Numpad *",
        (0x38, true) => "Right Alt",
        (0x38, false) => "Left Alt",
        (0x39, _) => "Space",
        (0x3a, _) => "Caps Lock",
        (0x3b..=0x44, _) => return format!("F{}", scan - 0x3a),
        (0x45, _) => "Num Lock",
        (0x46, _) => "Scroll Lock",
        (0x47, true) => "Home",
        (0x48, true) => "↑",
        (0x49, true) => "Page Up",
        (0x4b, true) => "←",
        (0x4d, true) => "→",
        (0x4f, true) => "End",
        (0x50, true) => "↓",
        (0x51, true) => "Page Down",
        (0x52, true) => "Insert",
        (0x53, true) => "Delete",
        (0x57, _) => "F11",
        (0x58, _) => "F12",
        (0x5b, true) => "Left Win",
        (0x5c, true) => "Right Win",
        _ => return code.to_string(),
    };
    name.to_string()
}

fn policy_summary(database: &storage::database::Database) -> String {
    match database.policy_rows() {
        Ok(rows) if rows.is_empty() => {
            "当前没有手动应用策略（浏览器和密码管理器使用默认策略）".to_string()
        }
        Ok(rows) => rows
            .into_iter()
            .map(|(path, mode)| format!("{}: {}", path, mode.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(error) => format!("无法读取应用策略：{error}"),
    }
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
    let functions = [
        ("Esc", "01"),
        ("F1", "3b"),
        ("F2", "3c"),
        ("F3", "3d"),
        ("F4", "3e"),
        ("F5", "3f"),
        ("F6", "40"),
        ("F7", "41"),
        ("F8", "42"),
        ("F9", "43"),
        ("F10", "44"),
        ("F11", "57"),
        ("F12", "58"),
    ]
    .map(|(label, scan)| HeatCell {
        label: label.into(),
        count: heat_count(rows, scan),
    });
    ui.set_heat_function_keys(functions.as_slice().into());
    ui.set_heat_1(heat_count(rows, "02"));
    ui.set_heat_2(heat_count(rows, "03"));
    ui.set_heat_3(heat_count(rows, "04"));
    ui.set_heat_4(heat_count(rows, "05"));
    ui.set_heat_5(heat_count(rows, "06"));
    ui.set_heat_6(heat_count(rows, "07"));
    ui.set_heat_7(heat_count(rows, "08"));
    ui.set_heat_8(heat_count(rows, "09"));
    ui.set_heat_9(heat_count(rows, "0a"));
    ui.set_heat_0(heat_count(rows, "0b"));
    ui.set_heat_minus(heat_count(rows, "0c"));
    ui.set_heat_equal(heat_count(rows, "0d"));
    ui.set_heat_backspace(heat_count(rows, "0e"));
    ui.set_heat_tab(heat_count(rows, "0f"));
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
    ui.set_heat_left_bracket(heat_count(rows, "1a"));
    ui.set_heat_right_bracket(heat_count(rows, "1b"));
    ui.set_heat_enter(heat_count(rows, "1c"));
    ui.set_heat_caps(heat_count(rows, "3a"));
    ui.set_heat_semicolon(heat_count(rows, "27"));
    ui.set_heat_quote(heat_count(rows, "28"));
    ui.set_heat_backtick(heat_count(rows, "29"));
    ui.set_heat_left_shift(heat_count(rows, "2a"));
    ui.set_heat_backslash(heat_count(rows, "2b"));
    ui.set_heat_comma(heat_count(rows, "33"));
    ui.set_heat_period(heat_count(rows, "34"));
    ui.set_heat_slash(heat_count(rows, "35"));
    ui.set_heat_right_shift(heat_count(rows, "36"));
    ui.set_heat_ctrl(heat_count(rows, "1d"));
    ui.set_heat_alt(heat_count(rows, "38"));
    ui.set_heat_right_ctrl(heat_count_ext(rows, "1d", true));
    ui.set_heat_right_alt(heat_count_ext(rows, "38", true));
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
}

fn format_count(value: i64) -> String {
    let text = value.max(0).to_string();
    let mut formatted = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().enumerate() {
        if index > 0 && (text.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted
}

fn input_rows(rows: &[storage::database::KeyTotal]) -> Vec<InputRow> {
    let maximum = rows.iter().map(|row| row.count).max().unwrap_or(1).max(1) as f32;
    rows.iter()
        .map(|row| InputRow {
            label: display_input_code(&row.code).into(),
            count: format_count(row.count).into(),
            share: (row.count as f32 / maximum).clamp(0.0, 1.0),
        })
        .collect()
}

fn filter_app_rows(ui: &MainWindow) {
    let all = ui.get_all_app_rows();
    let query = ui.get_app_search().trim().to_lowercase().replace('/', "\\");
    let visible: slint::ModelRc<AppRow> = all
        .iter()
        .filter(|row| {
            query.is_empty()
                || row.name.to_lowercase().contains(&query)
                || row.path.to_lowercase().contains(&query)
        })
        .map(|mut row| {
            row.icon = app_icons::cached(row.path.as_str());
            row.has_icon = row.icon.size().width > 0;
            row
        })
        .collect::<Vec<_>>()
        .as_slice()
        .into();
    ui.set_ranking_result(
        if query.is_empty() {
            format!("{} 个应用", all.row_count())
        } else {
            format!("匹配 {} / {} 个应用", visible.row_count(), all.row_count())
        }
        .into(),
    );
    ui.set_app_rows(visible);
    app_icons::populate(ui);
}

fn bind_app_search(ui: &MainWindow) {
    let weak = ui.as_weak();
    ui.on_app_search_changed(move || {
        if let Some(ui) = weak.upgrade() {
            filter_app_rows(&ui);
        }
    });
}

fn arm_clear_confirmation(ui: &MainWindow, pending: &AtomicBool) -> bool {
    if pending.swap(true, Ordering::Relaxed) {
        return false;
    }
    ui.set_clear_confirmation(true);
    ui.set_clear_button_text("再次点击确认清空".into());
    ui.set_clear_status("清空操作不可撤销，请再次点击确认。".into());
    true
}

fn reset_clear_confirmation(ui: &MainWindow, pending: &AtomicBool) {
    pending.store(false, Ordering::Relaxed);
    ui.set_clear_confirmation(false);
    ui.set_clear_button_text("清空统计数据".into());
    ui.set_clear_status("".into());
}

fn hide_panel(ui: &MainWindow) {
    ui.set_close_open(false);
    charts::release_images(ui);
    let _ = ui.hide();
}

fn bind_panel_close(
    ui: &MainWindow,
    initial: &config::Config,
    save: impl Fn(config::CloseAction, bool) -> anyhow::Result<()> + 'static,
) {
    use config::CloseAction;
    use std::{cell::Cell, rc::Rc};
    let preferences = Rc::new(Cell::new((
        initial.close_action,
        initial.remember_close_action,
    )));
    ui.set_close_default_action(initial.close_action.index());
    ui.set_close_remember(initial.remember_close_action);
    let persist = Rc::new({
        let preferences = preferences.clone();
        move |ui: &MainWindow, action: CloseAction, remember: bool| {
            let previous = preferences.get();
            let result = if previous == (action, remember) {
                Ok(())
            } else {
                save(action, remember)
            };
            if let Err(error) = result {
                ui.set_close_default_action(previous.0.index());
                ui.set_close_remember(previous.1);
                let message = format!("无法保存关闭偏好：{error}。请检查程序目录的写入权限。");
                ui.set_close_preferences_status(message.clone().into());
                ui.set_close_error("无法保存关闭偏好，请检查程序目录权限或磁盘状态后重试。".into());
                return false;
            }
            preferences.set((action, remember));
            ui.set_close_default_action(action.index());
            ui.set_close_remember(remember);
            ui.set_close_preferences_status("".into());
            ui.set_close_error("".into());
            true
        }
    });
    ui.on_close_preferences_changed({
        let weak = ui.as_weak();
        let persist = persist.clone();
        move |remember, index| {
            if let Some(ui) = weak.upgrade()
                && let Some(action) = CloseAction::from_index(index)
            {
                persist(&ui, action, remember);
            }
        }
    });
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_close_busy() || ui.get_close_open() {
                return CloseRequestResponse::KeepWindowShown;
            }
            ui.set_calendar_open(false);
            ui.set_app_picker_open(false);
            ui.invoke_clear_cancel_clicked();
            ui.set_close_error("".into());
            let (action, remember) = preferences.get();
            ui.set_close_dialog_remember(remember);
            if remember {
                match action {
                    CloseAction::Background => hide_panel(&ui),
                    CloseAction::Exit => ui.invoke_exit_requested(),
                }
            } else {
                ui.set_close_open(true);
            }
        }
        CloseRequestResponse::KeepWindowShown
    });
    ui.on_close_hide_clicked({
        let weak = ui.as_weak();
        let persist = persist.clone();
        move || {
            if let Some(ui) = weak.upgrade()
                && !ui.get_close_busy()
            {
                let remember = ui.get_close_dialog_remember();
                let action = if remember {
                    CloseAction::Background
                } else {
                    CloseAction::from_index(ui.get_close_default_action()).unwrap_or_default()
                };
                if persist(&ui, action, remember) {
                    hide_panel(&ui);
                }
            }
        }
    });
    ui.on_close_exit_clicked({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade()
                && !ui.get_close_busy()
            {
                let remember = ui.get_close_dialog_remember();
                let action = if remember {
                    CloseAction::Exit
                } else {
                    CloseAction::from_index(ui.get_close_default_action()).unwrap_or_default()
                };
                if persist(&ui, action, remember) {
                    ui.invoke_exit_requested();
                }
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_close_cancel_clicked(move || {
        if let Some(ui) = weak.upgrade()
            && !ui.get_close_busy()
        {
            ui.set_close_open(false);
        }
    });
}

fn refresh_heatmap(
    ui: &MainWindow,
    database: &storage::database::Database,
    date: &str,
) -> anyhow::Result<()> {
    let path = ui.get_detail_path().to_string();
    let app = (ui.get_heatmap_scope() == 1 && ui.get_detail_loaded()).then_some(path.as_str());
    let totals_only = app
        .map(|path| database.app_detail(date, path))
        .transpose()?
        .flatten()
        .is_some_and(|detail| detail.policy == CaptureMode::TotalsOnly);
    let keys = if totals_only {
        Vec::new()
    } else {
        database.input_ranking(date, app, "keyboard")?
    };
    let mut mouse = if totals_only {
        Vec::new()
    } else {
        database.input_ranking(date, app, "mouse")?
    };
    if !totals_only {
        mouse.extend(database.input_ranking(date, app, "wheel")?);
        mouse.sort_by(|left, right| right.count.cmp(&left.count));
    }
    ui.set_heatmap_max(
        keys.iter()
            .map(|row| row.count)
            .max()
            .unwrap_or(1)
            .clamp(1, i64::from(i32::MAX)) as i32,
    );
    ui.set_heatmap_note(
        if totals_only {
            "未记录详细数据：该应用仅保存输入总量。"
        } else {
            "颜色按当前筛选下的最高频次分级；特殊键位可在下方完整列表中查看。"
        }
        .into(),
    );
    ui.set_key_rows(input_rows(&keys).as_slice().into());
    ui.set_mouse_rows(input_rows(&mouse).as_slice().into());
    set_heatmap(ui, &keys);
    Ok(())
}

fn refresh_overview(
    ui: &MainWindow,
    database: &storage::database::Database,
    date: &str,
    sort: RankingSort,
) -> anyhow::Result<()> {
    let (keyboard, mouse, wheel, apps) = database.totals(date)?;
    if ui.get_date_text().as_str() != date {
        ui.set_query_date(date.into());
    }
    ui.set_date_text(date.into());
    ui.set_keyboard_text(format_count(keyboard).into());
    ui.set_mouse_text(format_count(mouse).into());
    ui.set_wheel_text(format_count(wheel).into());
    ui.set_app_text(format_count(apps).into());
    let totals = database.app_totals_sorted(date, sort.as_query())?;
    let metric = |row: &storage::database::AppTotal| match sort {
        RankingSort::Keyboard => row.keyboard,
        RankingSort::Mouse => row.mouse,
        RankingSort::Total => row.keyboard + row.mouse + row.wheel.abs(),
    };
    let sum = totals.iter().map(&metric).sum::<i64>().max(1) as f32;
    let rows = totals
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let share = metric(&row) as f32 / sum;
            let total = row
                .keyboard
                .saturating_add(row.mouse)
                .saturating_add(row.wheel.saturating_abs());
            AppRow {
                icon: slint::Image::default(),
                has_icon: false,
                rank: (index + 1) as i32,
                initial: row
                    .app_name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string()
                    .into(),
                name: row.app_name.into(),
                path: row.app_path.into(),
                keyboard: format_count(row.keyboard).into(),
                mouse: format_count(row.mouse).into(),
                wheel: format_count(row.wheel).into(),
                total: format_count(total).into(),
                share: share.clamp(0.0, 1.0),
                share_text: if share > 0.0 && share < 0.001 {
                    "<0.1%".into()
                } else {
                    format!("{:.1}%", share * 100.0).into()
                },
            }
        })
        .collect::<Vec<_>>();
    ui.set_all_app_rows(rows.as_slice().into());
    filter_app_rows(ui);
    ui.set_ranking_share_label(
        match sort {
            RankingSort::Total => "总输入占比",
            RankingSort::Keyboard => "键盘占比",
            RankingSort::Mouse => "点击占比",
        }
        .into(),
    );
    ui.set_ranking_sort(match sort {
        RankingSort::Total => 0,
        RankingSort::Keyboard => 1,
        RankingSort::Mouse => 2,
    });
    if ui.get_detail_loaded() {
        refresh_detail(ui, database, date, ui.get_detail_path().as_str())?;
    }
    refresh_heatmap(ui, database, date)?;
    if ui.get_selected_page() == 4 {
        charts::refresh(ui, database)?;
    }
    Ok(())
}

fn refresh_detail(
    ui: &MainWindow,
    database: &storage::database::Database,
    date: &str,
    path: &str,
) -> anyhow::Result<()> {
    let Some(detail) = database.app_detail(date, path)? else {
        ui.set_detail_loaded(false);
        ui.set_heatmap_scope(0);
        ui.set_detail_text("未找到该应用，请从应用排行选择，或检查完整路径。".into());
        return Ok(());
    };
    let totals_only = detail.policy == CaptureMode::TotalsOnly;
    let keys = if totals_only {
        Vec::new()
    } else {
        database.input_ranking(date, Some(path), "keyboard")?
    };
    let mut mouse = if totals_only {
        Vec::new()
    } else {
        database.input_ranking(date, Some(path), "mouse")?
    };
    if !totals_only {
        mouse.extend(database.input_ranking(date, Some(path), "wheel")?);
        mouse.sort_by(|left, right| right.count.cmp(&left.count));
    }
    ui.set_detail_loaded(true);
    let icon = app_icons::cached(&detail.app_path);
    ui.set_detail_has_icon(icon.size().width > 0);
    ui.set_detail_icon(icon);
    ui.set_detail_initial(
        detail
            .app_name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string()
            .into(),
    );
    ui.set_detail_name(detail.app_name.into());
    ui.set_detail_path(detail.app_path.into());
    ui.set_detail_keyboard(format_count(detail.keyboard).into());
    ui.set_detail_mouse(format_count(detail.mouse).into());
    ui.set_detail_wheel(format_count(detail.wheel).into());
    ui.set_detail_mode_label(
        match detail.policy {
            CaptureMode::Detailed => "详细统计",
            CaptureMode::TotalsOnly => "仅统计总量",
            CaptureMode::Excluded => "已排除",
        }
        .into(),
    );
    ui.set_detail_note(
        if totals_only {
            "未记录详细数据：该应用仅保存输入总量。"
        } else if detail.policy == CaptureMode::Excluded {
            "此应用当前已排除；下方仅展示此前保留的历史记录。"
        } else if keys.is_empty() && mouse.is_empty() {
            "所选日期暂无详细输入记录。"
        } else {
            "仅保存分类计数，不记录输入文字、按键顺序或鼠标轨迹。"
        }
        .into(),
    );
    ui.set_detail_key_rows(input_rows(&keys).as_slice().into());
    ui.set_detail_mouse_rows(input_rows(&mouse).as_slice().into());
    ui.set_detail_text("".into());
    app_icons::populate(ui);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        #[cfg(windows)]
        show_startup_error(&error);
        #[cfg(not(windows))]
        eprintln!("kb_count 启动失败：{error:#}");
    }
}

fn run_desktop_event_loop() -> Result<(), slint::platform::PlatformError> {
    slint::run_event_loop_until_quit()
}

#[cfg(windows)]
fn show_startup_error(error: &anyhow::Error) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let message = format!("kb_count 启动失败：{error:#}");
    let mut text = message.encode_utf16().collect::<Vec<_>>();
    text.push(0);
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("kb_count"),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn run() -> anyhow::Result<()> {
    #[cfg(windows)]
    let _instance_mutex = unsafe {
        let mutex = CreateMutexW(None, false, w!("Global\\kb_count_single_instance"))?;
        if GetLastError().0 == 183 {
            // Ask the UI event loop to show its window. Showing the native
            // HWND directly leaves Slint unaware of the visibility change.
            for _ in 0..20 {
                if tray::activate_existing() {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            return Ok(());
        }
        mutex
    };
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info,fontdb=off"))
        .init();
    let cfg = config::load_or_create()?;
    let dir = config::app_dir()?;
    tracing::info!(
        data_dir = %dir.display(),
        capture_enabled = cfg.capture_enabled,
        "portable data directory initialized"
    );
    let config_state = Arc::new(Mutex::new(cfg.clone()));
    let db_path = dir.join("stats.db");
    let db = Arc::new(Mutex::new(
        storage::database::Database::open(&db_path).with_context(|| {
            format!(
                "无法在 EXE 所在目录初始化数据库 {}。当前目录没有写入权限，请将 kb_count 移动到具有读写权限的文件夹。",
                db_path.display()
            )
        })?,
    ));
    // Slint's Windows backend may register the mouse Raw Input device while
    // constructing its window. The collector must register its own HWND last.
    let ui = MainWindow::new()?;
    bind_app_search(&ui);
    bind_panel_close(&ui, &cfg, {
        let state = config_state.clone();
        move |action, remember| {
            let mut config = state.lock().map_err(|_| anyhow::anyhow!("配置暂不可用"))?;
            let mut next = config.clone();
            next.close_action = action;
            next.remember_close_action = remember;
            config::save(&next)?;
            *config = next;
            Ok(())
        }
    });
    date_picker::bind(&ui);
    charts::bind(&ui, Arc::clone(&db));
    ui.on_exit_requested({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_close_open(true);
                ui.set_close_busy(true);
            }
            tray::request_exit();
        }
    });
    let enabled = Arc::new(AtomicBool::new(cfg.capture_enabled));
    tray::start(Arc::clone(&enabled));
    let events = collector::start(Arc::clone(&enabled));
    let db_writer = Arc::clone(&db);
    let data_generation = Arc::new(AtomicU64::new(0));
    let writer_generation = Arc::clone(&data_generation);
    let flush_seconds = cfg.flush_interval_seconds.max(1);
    let (writer_command_tx, writer_command_rx) = mpsc::sync_channel(1);
    let writer_handle = thread::Builder::new()
        .name("kb-stats-writer".into())
        .spawn(move || {
            let mut pending = Vec::<stats::event::InputEvent>::new();
            let flush = |events: &mpsc::Receiver<stats::event::InputEvent>,
                         db: &Arc<Mutex<storage::database::Database>>,
                         pending: &mut Vec<stats::event::InputEvent>| {
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
                // Pausing stops the collector from creating new events. Events
                // already queued before the pause are still valid and must be
                // persisted during the next flush or shutdown.
                pending.extend(aggregator.drain());
                if pending.is_empty() {
                    return;
                }
                let result = db
                    .lock()
                    .expect("database mutex poisoned")
                    .record_batch(pending);
                if let Err(error) = result {
                    let dropped = pending.len().saturating_sub(16_384);
                    if dropped > 0 {
                        pending.drain(..dropped);
                        tracing::error!(%error, dropped, "database unavailable; aggregate batch was bounded");
                    } else {
                        tracing::error!(%error, "failed to persist aggregate batch; will retry");
                    }
                } else {
                    pending.clear();
                    writer_generation.fetch_add(1, Ordering::Release);
                }
            };
            loop {
                match writer_command_rx.recv_timeout(Duration::from_secs(flush_seconds)) {
                    Ok(WriterCommand::Shutdown(ack)) => {
                        flush(&events, &db_writer, &mut pending);
                        // Other UI callbacks retain command sender clones. Stop
                        // explicitly after a received successful rendezvous ACK;
                        // a failed write or abandoned exit keeps the writer alive.
                        if acknowledge_exit(ack, pending.is_empty()) { break; }
                    }
                    Ok(WriterCommand::Clear(ack)) => {
                        while events.try_recv().is_ok() {}
                        pending.clear();
                        let result = db_writer
                            .lock()
                            .map_err(|_| "database mutex poisoned")
                            .and_then(|mut database| database.clear_all().map_err(|_| "clear failed"));
                        if result.is_err() {
                            tracing::error!("failed to clear statistics database");
                        }
                        if result.is_ok() {
                            writer_generation.fetch_add(1, Ordering::Release);
                        }
                        let _ = ack.send(result.is_ok());
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        flush(&events, &db_writer, &mut pending);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
    let date = Local::now().format("%Y-%m-%d").to_string();
    let sort_state = Arc::new(Mutex::new(RankingSort::Total));
    ui.set_capture_active(cfg.capture_enabled);
    ui.set_data_directory(dir.display().to_string().into());
    ui.set_version_text(env!("CARGO_PKG_VERSION").into());
    ui.set_date_text(date.clone().into());
    ui.set_query_date(date.clone().into());
    ui.set_keyboard_text("0".into());
    ui.set_mouse_text("0".into());
    ui.set_wheel_text("0".into());
    ui.set_app_text("0".into());
    ui.set_status_text(
        if cfg.capture_enabled {
            "统计运行中"
        } else {
            "统计已暂停"
        }
        .into(),
    );
    ui.set_export_text("未导出".into());
    ui.set_export_from_date(date.clone().into());
    ui.set_export_to_date(date.clone().into());
    ui.set_detail_text("".into());
    let startup_matches = std::env::current_exe()
        .ok()
        .and_then(|exe| config::startup::windows::is_configured(&exe).ok())
        .unwrap_or(false);
    let initial_policy_summary = policy_summary(&db.lock().expect("database mutex poisoned"));
    ui.set_policies_text(initial_policy_summary.into());
    ui.set_settings_text(
        if cfg.start_with_windows && !startup_matches {
            "启动项路径与当前 EXE 不一致，请重新启用。"
        } else {
            "可从应用详情直接带入路径，修改采集策略。"
        }
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
    ui.set_dark_mode(cfg.dark_mode);
    ui.set_theme_text(
        if cfg.dark_mode {
            "主题：深色"
        } else {
            "主题：浅色"
        }
        .into(),
    );
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
    let ui_for_ranking = ui.as_weak();
    let db_for_ranking = Arc::clone(&db);
    ui.on_ranking_app_clicked(move |path| {
        let Some(ui) = ui_for_ranking.upgrade() else {
            return;
        };
        if !path.is_empty() {
            ui.set_detail_path(path.clone());
            ui.set_app_query_path(path.clone());
            ui.set_policy_path(path.clone());
            let date = ui.get_date_text().to_string();
            if let Ok(database) = db_for_ranking.lock() {
                let _ = refresh_detail(&ui, &database, &date, path.as_str());
                let _ = refresh_heatmap(&ui, &database, &date);
            }
            ui.set_status_text("已打开应用详情".into());
        }
    });
    let db_for_detail = Arc::clone(&db);
    let ui_for_detail = ui.as_weak();
    ui.on_query_app_clicked(move || {
        if let Some(ui) = ui_for_detail.upgrade() {
            let path = ui.get_app_query_path().to_string();
            let date = ui.get_date_text().to_string();
            if path.trim().is_empty() {
                ui.set_detail_text("请输入应用完整路径查询详情。".into());
            } else if let Ok(database) = db_for_detail.lock() {
                let _ = refresh_detail(&ui, &database, &date, path.trim());
                let _ = refresh_heatmap(&ui, &database, &date);
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
                let database = db_for_policy.lock().expect("database mutex poisoned");
                let result = database.set_policy(path.trim(), mode.as_str());
                match result {
                    Ok(()) => {
                        ui.set_settings_text("应用采集策略已更新。".into());
                        ui.set_policies_text(policy_summary(&database).into());
                        let date = ui.get_date_text().to_string();
                        if ui.get_detail_loaded() {
                            let _ = refresh_detail(
                                &ui,
                                &database,
                                &date,
                                ui.get_detail_path().as_str(),
                            );
                        }
                        let _ = refresh_heatmap(&ui, &database, &date);
                    }
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
    let heatmap_ui = ui.as_weak();
    let heatmap_db = Arc::clone(&db);
    let refresh_scope = Arc::new(move || {
        if let Some(ui) = heatmap_ui.upgrade()
            && let Ok(database) = heatmap_db.lock()
        {
            let _ = refresh_heatmap(&ui, &database, ui.get_date_text().as_str());
        }
    });
    ui.on_heatmap_all_clicked({
        let refresh_scope = Arc::clone(&refresh_scope);
        move || refresh_scope()
    });
    ui.on_heatmap_app_clicked(move || refresh_scope());
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
    let theme_config = Arc::clone(&config_state);
    let theme_ui = ui.as_weak();
    ui.on_theme_clicked(move || {
        let mut config = theme_config.lock().expect("config mutex poisoned");
        config.dark_mode = !config.dark_mode;
        if let Err(error) = config::save(&config) {
            tracing::error!(%error, "failed to save theme configuration");
        }
        if let Some(ui) = theme_ui.upgrade() {
            ui.set_dark_mode(config.dark_mode);
            ui.set_theme_text(
                if config.dark_mode {
                    "主题：深色"
                } else {
                    "主题：浅色"
                }
                .into(),
            );
        }
    });
    let clear_pending = Arc::new(AtomicBool::new(false));
    let clear_db = Arc::clone(&db);
    let clear_ui = ui.as_weak();
    let clear_sort = Arc::clone(&sort_state);
    let clear_pending_for_ui = Arc::clone(&clear_pending);
    let clear_command = writer_command_tx.clone();
    ui.on_clear_data_clicked(move || {
        let Some(ui) = clear_ui.upgrade() else {
            return;
        };
        if arm_clear_confirmation(&ui, &clear_pending_for_ui) {
            return;
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let result = if clear_command.send(WriterCommand::Clear(ack_tx)).is_ok() {
            ack_rx
                .recv_timeout(Duration::from_secs(2))
                .map(|success| {
                    if success {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!("统计线程清空数据库失败"))
                    }
                })
                .unwrap_or_else(|_| Err(anyhow::anyhow!("统计线程未响应清空请求")))
        } else {
            Err(anyhow::anyhow!("统计线程已停止"))
        };
        reset_clear_confirmation(&ui, &clear_pending_for_ui);
        match result {
            Ok(()) => {
                ui.set_detail_loaded(false);
                ui.set_detail_path("".into());
                ui.set_heatmap_scope(0);
                let date = ui.get_date_text().to_string();
                if let Ok(database) = clear_db.lock() {
                    let _ = refresh_overview(
                        &ui,
                        &database,
                        &date,
                        *clear_sort.lock().expect("ranking state poisoned"),
                    );
                }
                ui.set_detail_text("".into());
                ui.set_clear_status("统计数据已清空。".into());
            }
            Err(error) => ui.set_clear_status(format!("清空失败：{error}").into()),
        }
    });
    let cancel_ui = ui.as_weak();
    ui.on_clear_cancel_clicked(move || {
        if let Some(ui) = cancel_ui.upgrade() {
            reset_clear_confirmation(&ui, &clear_pending);
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
            ui.set_capture_active(value);
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
        let Some(ui) = ui_for_export.upgrade() else {
            return;
        };
        let from = ui.get_export_from_date().to_string();
        let to = ui.get_export_to_date().to_string();
        if chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").is_err()
            || chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d").is_err()
        {
            ui.set_export_text("导出日期格式应为 YYYY-MM-DD".into());
            return;
        }
        let default_name = if from == to {
            format!("kb_count-{}.csv", from)
        } else {
            format!("kb_count-{}-{}.csv", from, to)
        };
        #[cfg(windows)]
        let Some(path) = choose_csv_path(&default_name) else {
            ui.set_export_text("已取消导出".into());
            return;
        };
        #[cfg(not(windows))]
        let path = dir_for_export.join(default_name);
        let text = match storage::export::export_range(
            &db_for_export.lock().expect("database mutex poisoned"),
            &from,
            &to,
            &path,
        ) {
            Ok(()) => format!("已导出：{}", path.display()),
            Err(error) => format!("导出失败：{error}"),
        };
        tracing::info!(%text);
        ui.set_export_text(text.into());
    });
    let tray_enabled = Arc::clone(&enabled);
    let tray_ui = ui.as_weak();
    let tray_db = Arc::clone(&db);
    let tray_sort = Arc::clone(&sort_state);
    let tray_shutdown = writer_command_tx.clone();
    let tray_config = Arc::clone(&config_state);
    let tray_timer = Timer::default();
    tray_timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        let Some(command) = tray::take_request() else {
            return;
        };
        match command {
            tray::Command::Open | tray::Command::Settings => {
                if let Some(ui) = tray_ui.upgrade() {
                    if command == tray::Command::Settings {
                        ui.set_selected_page(3);
                    }
                    let date = ui.get_date_text().to_string();
                    if let Ok(database) = tray_db.lock()
                        && let Err(error) = refresh_overview(
                            &ui,
                            &database,
                            &date,
                            *tray_sort.lock().expect("ranking state poisoned"),
                        )
                    {
                        ui.set_status_text(format!("查询失败：{error}").into());
                    }
                    let _ = ui.show();
                    ui.window().set_minimized(false);
                    app_icons::populate(&ui);
                    #[cfg(windows)]
                    ui_window::fit_to_work_area(ui.window());
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
                    ui.set_capture_active(value);
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
                let Some(ui) = tray_ui.upgrade() else { return };
                if !ui.get_close_open() {
                    ui.set_close_dialog_remember(ui.get_close_remember());
                }
                ui.set_close_open(true);
                ui.set_close_busy(true);
                let saved = exit_handshake(
                    &tray_enabled,
                    &tray_shutdown,
                    collector::settle,
                    Duration::from_secs(2),
                );
                if saved {
                    let _ = ui.hide();
                    let _ = slint::quit_event_loop();
                } else {
                    ui.set_close_busy(false);
                    ui.set_close_error(
                        "保存尚未完成，应用继续运行。请检查目录/磁盘状态后重试退出。".into(),
                    );
                    let _ = ui.show();
                }
            }
        }
    });
    let refresh_timer_ui = ui.as_weak();
    let refresh_timer_db = Arc::clone(&db);
    let refresh_timer_sort = Arc::clone(&sort_state);
    let refresh_timer_generation = Arc::clone(&data_generation);
    let refresh_timer = Timer::default();
    let mut last_generation = refresh_timer_generation.load(Ordering::Acquire);
    refresh_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
        let Some(ui) = refresh_timer_ui.upgrade() else {
            return;
        };
        if !ui.window().is_visible() {
            return;
        }
        let generation = refresh_timer_generation.load(Ordering::Acquire);
        if generation == last_generation {
            return;
        }
        let date = ui.get_date_text().to_string();
        if let Ok(database) = refresh_timer_db.try_lock()
            && refresh_overview(
                &ui,
                &database,
                &date,
                *refresh_timer_sort.lock().expect("ranking state poisoned"),
            )
            .is_ok()
        {
            last_generation = generation;
        }
    });
    ui.hide()?;
    run_desktop_event_loop()?;
    drop(tray_timer);
    drop(refresh_timer);
    drop(writer_command_tx);
    let _ = writer_handle.join();
    if let Err(error) = db.lock().expect("database mutex poisoned").checkpoint() {
        tracing::warn!(%error,"final WAL checkpoint deferred; committed data remains in WAL");
    }
    tray::remove_icon();
    Ok(())
}
