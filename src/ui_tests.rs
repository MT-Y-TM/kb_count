use super::*;
use slint::Model;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use std::io::Write;
use std::rc::Rc;

type UiEvent = Box<dyn FnOnce() + Send>;
struct PreviewPlatform(
    Rc<MinimalSoftwareWindow>,
    mpsc::SyncSender<UiEvent>,
    Rc<std::cell::Cell<bool>>,
);
struct PreviewProxy(mpsc::SyncSender<UiEvent>);

impl slint::platform::EventLoopProxy for PreviewProxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: UiEvent) -> Result<(), slint::EventLoopError> {
        self.0
            .try_send(event)
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }
}

impl Platform for PreviewPlatform {
    fn run_event_loop(&self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_event_loop_quit_on_last_window_closed(&self, value: bool) {
        self.2.set(value);
    }
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> {
        Some(Box::new(PreviewProxy(self.1.clone())))
    }
}

// Test-only rendering uses Slint's own software backend, without opening a
// desktop window, registering input devices, or accessing the user's database.
fn render_frame(window: &MinimalSoftwareWindow) -> Vec<slint::Rgb8Pixel> {
    slint::platform::update_timers_and_animations();
    let size = window.size();
    let mut pixels = vec![slint::Rgb8Pixel::default(); (size.width * size.height) as usize];
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, size.width as usize);
    }));
    pixels
}

fn save_preview(window: &MinimalSoftwareWindow, name: &str) {
    if std::env::var_os("KB_COUNT_RENDER_UI").is_none() {
        return;
    }
    let pixels = render_frame(window);
    let size = window.size();
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/ui-review");
    std::fs::create_dir_all(&directory).expect("preview directory");
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(directory.join(format!("{name}.bmp"))).expect("preview file"),
    );
    let stride = (size.width * 3 + 3) & !3;
    let data_size = stride * size.height;
    file.write_all(b"BM").unwrap();
    file.write_all(&(54 + data_size).to_le_bytes()).unwrap();
    file.write_all(&[0; 4]).unwrap();
    file.write_all(&54u32.to_le_bytes()).unwrap();
    file.write_all(&40u32.to_le_bytes()).unwrap();
    file.write_all(&(size.width as i32).to_le_bytes()).unwrap();
    file.write_all(&(size.height as i32).to_le_bytes()).unwrap();
    file.write_all(&1u16.to_le_bytes()).unwrap();
    file.write_all(&24u16.to_le_bytes()).unwrap();
    file.write_all(&[0; 4]).unwrap();
    file.write_all(&data_size.to_le_bytes()).unwrap();
    file.write_all(&[0; 16]).unwrap();
    for row in pixels.chunks(size.width as usize).rev() {
        for pixel in row {
            file.write_all(&[pixel.b, pixel.g, pixel.r]).unwrap();
        }
        file.write_all(&vec![0; (stride - size.width * 3) as usize])
            .unwrap();
    }
    file.flush().unwrap();
}

#[test]
fn refresh_preserves_ui_filters_and_privacy_state() {
    thread::Builder::new()
        .name("ui-regression".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(verify_ui_state)
        .unwrap()
        .join()
        .unwrap();
}

fn verify_close_preferences(ui: &MainWindow, window: &MinimalSoftwareWindow) {
    use config::{CloseAction, Config};
    use std::cell::{Cell, RefCell};
    let saved = Rc::new(RefCell::new(Config::default()));
    let fail = Rc::new(Cell::new(false));
    let writes = Rc::new(Cell::new(0));
    let persist = Rc::new({
        let (saved, fail, writes) = (saved.clone(), fail.clone(), writes.clone());
        move |action, remember| -> anyhow::Result<()> {
            anyhow::ensure!(!fail.get(), "测试目录不可写");
            saved.borrow_mut().close_action = action;
            saved.borrow_mut().remember_close_action = remember;
            writes.set(writes.get() + 1);
            Ok(())
        }
    });
    bind_panel_close(ui, &Config::default(), {
        let persist = persist.clone();
        move |a, r| persist(a, r)
    });
    let exits = Rc::new(Cell::new(0));
    ui.on_exit_requested({
        let exits = exits.clone();
        move || exits.set(exits.get() + 1)
    });
    window.set_size(slint::PhysicalSize::new(820, 440));
    ui.show().unwrap();
    let request = || {
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::CloseRequested);
        slint::platform::update_timers_and_animations();
    };
    let checkbox = || {
        let position = slint::LogicalPosition::new(
            (820.0 - 456.0) / 2.0 + 29.0,
            (440.0 - 240.0) / 2.0 + 118.0,
        );
        let button = slint::platform::PointerEventButton::Left;
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
    };
    request();
    assert!(ui.get_close_open());
    assert!(!ui.get_close_dialog_remember());
    checkbox();
    assert!(
        ui.get_close_dialog_remember(),
        "real remember checkbox is reachable"
    );
    save_preview(window, "close-remember-minimum");
    ui.invoke_close_cancel_clicked();
    assert_eq!(writes.get(), 0, "cancel never saves the tentative choice");
    request();
    assert!(
        !ui.get_close_dialog_remember(),
        "cancel discards the checkbox edit"
    );
    checkbox();
    ui.set_capture_active(false);
    ui.invoke_close_hide_clicked();
    assert!(!ui.window().is_visible());
    assert!(!ui.get_capture_active());
    assert_eq!(writes.get(), 1);
    assert!(saved.borrow().remember_close_action);
    assert_eq!(saved.borrow().close_action, CloseAction::Background);
    // Rebind from serialized configuration to cover the next launch's behavior.
    let reloaded: Config = toml::from_str(&toml::to_string(&*saved.borrow()).unwrap()).unwrap();
    bind_panel_close(ui, &reloaded, {
        let persist = persist.clone();
        move |a, r| persist(a, r)
    });
    ui.show().unwrap();
    request();
    assert!(
        !ui.window().is_visible(),
        "remembered background choice skips confirmation after restart"
    );
    assert!(!ui.get_close_open());
    assert_eq!(writes.get(), 1, "ordinary closes do not rewrite settings");
    ui.show().unwrap();
    ui.invoke_close_preferences_changed(false, 1);
    request();
    assert!(
        ui.get_close_open(),
        "turning off remember restores confirmation"
    );
    assert!(!ui.get_close_dialog_remember());
    ui.invoke_close_exit_clicked();
    assert_eq!(exits.get(), 1);
    assert!(
        !saved.borrow().remember_close_action,
        "one-time exit still asks next time"
    );
    ui.invoke_close_cancel_clicked();
    request();
    checkbox();
    ui.invoke_close_exit_clicked();
    assert_eq!(exits.get(), 2);
    assert_eq!(saved.borrow().close_action, CloseAction::Exit);
    assert!(saved.borrow().remember_close_action);
    ui.invoke_close_cancel_clicked();
    request();
    assert_eq!(
        exits.get(),
        3,
        "remembered exit uses the existing final-save callback"
    );
    ui.invoke_close_preferences_changed(true, 0);
    fail.set(true);
    ui.invoke_close_preferences_changed(false, 1);
    assert!(
        ui.get_close_remember(),
        "failed settings write restores the previous checkbox"
    );
    assert_eq!(ui.get_close_default_action(), 0);
    assert!(ui.get_close_preferences_status().contains("无法保存"));
    request();
    assert!(
        !ui.window().is_visible(),
        "failed settings write cannot change effective behavior"
    );
    ui.show().unwrap();
    fail.set(false);
    ui.invoke_close_preferences_changed(false, 0);
    request();
    checkbox();
    fail.set(true);
    ui.invoke_close_exit_clicked();
    assert_eq!(
        exits.get(),
        3,
        "failed remembered-choice save leaves the panel open for retry"
    );
    assert!(ui.get_close_open());
    assert!(ui.window().is_visible());
    assert!(!ui.get_close_remember());
    assert!(ui.get_close_error().contains("无法保存"));
    ui.set_close_busy(true);
    ui.invoke_close_cancel_clicked();
    checkbox();
    ui.invoke_close_hide_clicked();
    ui.invoke_close_exit_clicked();
    request();
    assert!(ui.get_close_open());
    assert_eq!(exits.get(), 3, "busy exit cannot be duplicated or canceled");
    assert!(
        ui.get_close_dialog_remember(),
        "busy save disables the remember control"
    );
    ui.set_close_busy(false);
    fail.set(false);
    ui.invoke_close_exit_clicked();
    assert_eq!(
        exits.get(),
        4,
        "retry saves preferences before requesting exit"
    );
    ui.invoke_close_cancel_clicked();
    ui.invoke_close_preferences_changed(false, 0);
    request();
    let before = writes.get();
    ui.invoke_close_exit_clicked();
    assert_eq!(
        writes.get(),
        before,
        "a one-time unremembered exit does not rewrite preferences"
    );
    assert_eq!(
        saved.borrow().close_action,
        CloseAction::Background,
        "one-time exit preserves the configured default"
    );
    assert!(!saved.borrow().remember_close_action);
    ui.invoke_close_cancel_clicked();
    ui.set_selected_page(3);
    ui.set_page_scroll_y(0.0);
    ui.set_dark_mode(true);
    save_preview(window, "close-settings-dark-minimum");
    ui.set_dark_mode(false);
    save_preview(window, "close-settings-light-minimum");
    ui.set_selected_page(0);
    ui.set_capture_active(true);
}

fn verify_capture_limit_notice(ui: &MainWindow, window: &MinimalSoftwareWindow) {
    let (sender, receiver) = mpsc::sync_channel(1);
    ui.set_keyboard_text("12,345".into());
    ui.set_mouse_text("2,468".into());
    ui.set_wheel_text("357".into());
    ui.set_app_text("1".into());
    let row = AppRow {
        name: "Editor.exe".into(),
        path: r"c:\preview\editor.exe".into(),
        initial: "E".into(),
        keyboard: "12,345".into(),
        mouse: "2,468".into(),
        wheel: "357".into(),
        total: "15,170".into(),
        rank: 1,
        share: 1.0,
        share_text: "100%".into(),
        ..Default::default()
    };
    ui.set_app_rows([row.clone()].as_slice().into());
    ui.set_all_app_rows([row].as_slice().into());
    let selected = Rc::new(std::cell::RefCell::new(String::new()));
    ui.on_ranking_app_clicked({
        let selected = selected.clone();
        move |path| *selected.borrow_mut() = path.to_string()
    });
    ui.set_date_text("2026-10-17".into());
    ui.set_query_date("2026-10-17".into());
    ui.set_version_text(env!("CARGO_PKG_VERSION").into());
    ui.set_theme_text("切换主题".into());
    ui.set_startup_text("开机启动：未开启".into());
    ui.hide().unwrap();
    sender
        .try_send(collector::CaptureNotice::ElevatedForegroundObserved)
        .unwrap();
    refresh_capture_notice(ui, &receiver);
    assert!(
        !ui.get_capture_limit_seen(),
        "hidden panels must not update"
    );
    assert!(
        sender
            .try_send(collector::CaptureNotice::ElevatedForegroundObserved)
            .is_err()
    );
    ui.show().unwrap();
    refresh_capture_notice(ui, &receiver);
    assert!(ui.get_capture_limit_seen());
    assert_eq!(
        ui.get_keyboard_text(),
        "12,345",
        "the notice does not alter statistics"
    );
    assert!(
        ui.get_capture_active(),
        "a warning must not pause collection"
    );
    refresh_capture_notice(ui, &receiver);
    assert!(
        ui.get_capture_limit_seen(),
        "returning to our panel retains the warning"
    );
    window.set_size(slint::PhysicalSize::new(820, 440));
    for dark in [false, true] {
        ui.set_dark_mode(dark);
        ui.set_selected_page(0);
        // Hit testing must follow a real layout/render frame, even when PNG
        // preview output is disabled for the normal test run.
        render_frame(window);
        save_preview(
            window,
            if dark {
                "capture-limited-dark"
            } else {
                "capture-limited-light"
            },
        );
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(145.0, 300.0),
                delta_x: 0.0,
                delta_y: -10_000.0,
            });
        slint::platform::update_timers_and_animations();
        render_frame(window);
        assert!(
            ui.get_page_scroll_y() < -64.0,
            "the complete overview remains reachable: offset {}",
            ui.get_page_scroll_y()
        );
        save_preview(
            window,
            if dark {
                "capture-limited-dark-bottom"
            } else {
                "capture-limited-light-bottom"
            },
        );
        let position = slint::LogicalPosition::new(760.0, 412.0);
        let button = slint::platform::PointerEventButton::Left;
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
        render_frame(window);
        assert_eq!(
            ui.get_page_scroll_y(),
            0.0,
            "overview back-to-top is clickable at minimum size"
        );
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(145.0, 300.0),
                delta_x: 0.0,
                delta_y: -10_000.0,
            });
        slint::platform::update_timers_and_animations();
        render_frame(window);
        let position = slint::LogicalPosition::new(370.0, 393.0 + ui.get_page_scroll_y());
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
        assert_eq!(
            ui.get_selected_page(),
            2,
            "the overview row is reachable with a warning"
        );
        assert_eq!(selected.borrow().as_str(), r"c:\preview\editor.exe");
        ui.set_selected_page(3);
        ui.set_page_scroll_y(0.0);
        save_preview(
            window,
            if dark {
                "capture-limited-settings-dark"
            } else {
                "capture-limited-settings-light"
            },
        );
    }
    ui.set_capture_active(false);
    assert!(
        ui.get_capture_limit_seen(),
        "pausing must retain the completeness notice"
    );
    ui.set_capture_limit_seen(false);
    ui.set_capture_active(true);
    ui.set_selected_page(0);
    ui.set_page_scroll_y(0.0);
    ui.set_app_rows(Default::default());
    ui.set_all_app_rows(Default::default());
}

fn verify_ui_state() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let (event_sender, event_receiver) = mpsc::sync_channel::<UiEvent>(64);
    let quit_on_last_window = Rc::new(std::cell::Cell::new(true));
    slint::platform::set_platform(Box::new(PreviewPlatform(
        window.clone(),
        event_sender,
        quit_on_last_window.clone(),
    )))
    .expect("preview backend");
    let ui = MainWindow::new().expect("UI");
    bind_app_search(&ui);
    verify_close_preferences(&ui, &window);
    verify_capture_limit_notice(&ui, &window);
    if std::env::var_os("KB_COUNT_CAPTURE_NOTICE_ONLY").is_some() {
        ui.hide().unwrap();
        return;
    }
    date_picker::bind(&ui);
    if std::env::var_os("KB_COUNT_ACTIVITY_ONLY").is_some() {
        verify_activity_and_close(&ui, &window);
        ui.hide().unwrap();
        run_desktop_event_loop().unwrap();
        assert!(
            !quit_on_last_window.get(),
            "tray mode must require an explicit event-loop quit"
        );
        return;
    }
    window.set_size(slint::PhysicalSize::new(1060, 780));
    let mut db = storage::database::Database::open(std::path::Path::new(":memory:")).unwrap();
    let date = "2026-10-15";
    for (index, name) in [
        "Editor.exe",
        "Terminal.exe",
        "Design.exe",
        "Browser.exe",
        "Notes.exe",
        "Player.exe",
        "Mail.exe",
        "Files.exe",
        "Reader.exe",
        "ToolsWithAnIntentionallyLongExecutableName.exe",
    ]
    .iter()
    .enumerate()
    {
        let keyboard = 12_840 / (index as i64 + 1);
        db.record_batch(&[
            stats::event::InputEvent {
                local_date: date.into(),
                local_hour: None,
                app_name: (*name).into(),
                app_path: format!("C:/Preview/{name}"),
                input_type: "keyboard".into(),
                code: "scan:1e:ext:false".into(),
                count: keyboard,
                keyboard_count: keyboard,
                mouse_count: 0,
                wheel_units: 0,
                detailed: true,
            },
            stats::event::InputEvent {
                local_date: date.into(),
                local_hour: None,
                app_name: (*name).into(),
                app_path: format!("C:/Preview/{name}"),
                input_type: "mouse".into(),
                code: "left".into(),
                count: 830 / (index as i64 + 1),
                keyboard_count: 0,
                mouse_count: 830 / (index as i64 + 1),
                wheel_units: 0,
                detailed: true,
            },
            stats::event::InputEvent {
                local_date: date.into(),
                local_hour: None,
                app_name: (*name).into(),
                app_path: format!("C:/Preview/{name}"),
                input_type: "mouse".into(),
                code: "wheel_up".into(),
                count: 120 / (index as i64 + 1),
                keyboard_count: 0,
                mouse_count: 0,
                wheel_units: 120 / (index as i64 + 1),
                detailed: true,
            },
        ])
        .unwrap();
    }
    ui.set_date_text(date.into());
    ui.set_query_date("2026-".into());
    ui.set_export_from_date("2026-10-01".into());
    ui.set_export_to_date(date.into());
    ui.set_status_text("UI 离线预览 · 测试聚合数据".into());
    ui.set_version_text(env!("CARGO_PKG_VERSION").into());
    ui.set_data_directory(r"D:\Portable\kb_count".into());
    ui.set_startup_text("开机启动：未开启".into());
    ui.set_theme_text("主题：浅色".into());
    ui.set_clear_button_text("清空统计数据".into());
    ui.set_export_text("选择日期范围后导出".into());
    ui.set_policies_text("当前没有手动应用策略".into());
    refresh_overview(&ui, &db, date, RankingSort::Total).unwrap();
    assert_eq!(ui.get_query_date(), "2026-");
    assert_eq!(ui.get_export_from_date(), "2026-10-01");
    assert_eq!(ui.get_export_to_date(), date);
    assert_eq!(ui.get_app_rows().row_count(), 10);
    let original_terminal = ui.get_app_rows().row_data(1).unwrap();
    assert_eq!(ui.get_app_rows().row_data(0).unwrap().total, "13,790");
    let daily_keyboard = ui.get_keyboard_text();
    ui.set_app_search("  TeRmiNaL  ".into());
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_app_rows().row_count(), 1);
    let filtered = ui.get_app_rows().row_data(0).unwrap();
    assert_eq!(filtered.rank, 2, "search preserves the daily rank");
    assert_eq!(filtered.total, "6,895", "total includes wheel units");
    assert_eq!(
        filtered.share, original_terminal.share,
        "search preserves the daily denominator"
    );
    assert_eq!(ui.get_keyboard_text(), daily_keyboard);
    assert_eq!(ui.get_app_text(), "10");
    refresh_overview(&ui, &db, date, RankingSort::Total).unwrap();
    assert_eq!(ui.get_app_rows().row_count(), 1, "refresh preserves search");
    ui.set_app_search("C:/Preview/TER".into());
    slint::platform::update_timers_and_animations();
    assert_eq!(
        ui.get_app_rows().row_count(),
        1,
        "search accepts Windows path separators and case"
    );
    ui.set_app_search("".into());
    slint::platform::update_timers_and_animations();
    refresh_detail(&ui, &db, date, "C:/Preview/Editor.exe").unwrap();
    ui.set_app_query_path("C:/Unsubmitted/".into());
    ui.set_heatmap_scope(1);
    refresh_overview(&ui, &db, date, RankingSort::Mouse).unwrap();
    assert!(ui.get_detail_loaded());
    assert_eq!(ui.get_detail_name(), "editor.exe");
    assert_eq!(ui.get_app_query_path(), "C:/Unsubmitted/");
    assert_eq!(ui.get_heatmap_scope(), 1);
    assert_eq!(ui.get_detail_keyboard(), "12,840");
    ui.set_query_date(date.into());
    ui.set_app_query_path(ui.get_detail_path());
    ui.show().unwrap();
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        let button = slint::platform::PointerEventButton::Left;
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
    };
    click(80.0, 215.0);
    assert_eq!(ui.get_selected_page(), 1, "sidebar opens key distribution");
    click(80.0, 159.0);
    assert_eq!(ui.get_selected_page(), 0);
    let selected = Rc::new(std::cell::RefCell::new(String::new()));
    ui.on_ranking_app_clicked({
        let selected = selected.clone();
        move |path| *selected.borrow_mut() = path.to_string()
    });
    click(370.0, 431.0);
    assert_eq!(ui.get_selected_page(), 2, "application row opens details");
    assert_eq!(selected.borrow().as_str(), r"c:\preview\editor.exe");
    for dark in [false, true] {
        ui.set_dark_mode(dark);
        ui.set_theme_text(
            if dark {
                "主题：深色"
            } else {
                "主题：浅色"
            }
            .into(),
        );
        if std::env::var_os("KB_COUNT_RENDER_UI").is_some() {
            slint::platform::update_timers_and_animations();
            thread::sleep(Duration::from_millis(250));
            slint::platform::update_timers_and_animations();
        }
        for page in 0..4 {
            ui.set_selected_page(page);
            save_preview(
                &window,
                &format!("{}-{page}", if dark { "dark" } else { "light" }),
            );
        }
    }
    ui.set_selected_page(0);
    ui.set_app_search("terminal".into());
    slint::platform::update_timers_and_animations();
    save_preview(&window, "search-filtered");
    ui.set_app_search("no-matching-application".into());
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_app_rows().row_count(), 0);
    assert_eq!(ui.get_all_app_rows().row_count(), 10);
    assert_eq!(ui.get_ranking_result(), "匹配 0 / 10 个应用");
    save_preview(&window, "search-no-match");
    ui.set_app_search("".into());
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.get_app_rows().row_count(), 10);
    window.set_size(slint::PhysicalSize::new(960, 680));
    ui.set_selected_page(0);
    save_preview(&window, "dark-minimum");
    let pauses = Rc::new(std::cell::Cell::new(0));
    ui.on_pause_clicked({
        let pauses = pauses.clone();
        move || pauses.set(pauses.get() + 1)
    });
    let queries = Rc::new(std::cell::Cell::new(0));
    ui.on_query_clicked({
        let queries = queries.clone();
        move || queries.set(queries.get() + 1)
    });
    let exports = Rc::new(std::cell::Cell::new(0));
    ui.on_export_clicked({
        let exports = exports.clone();
        move || exports.set(exports.get() + 1)
    });
    let app_queries = Rc::new(std::cell::Cell::new(0));
    ui.on_query_app_clicked({
        let app_queries = app_queries.clone();
        move || app_queries.set(app_queries.get() + 1)
    });
    let confirmed_clears = Rc::new(std::cell::Cell::new(0));
    let pending_clear = Arc::new(AtomicBool::new(false));
    ui.on_clear_data_clicked({
        let weak = ui.as_weak();
        let pending = pending_clear.clone();
        let confirmed = confirmed_clears.clone();
        move || {
            let ui = weak.upgrade().unwrap();
            if !arm_clear_confirmation(&ui, &pending) {
                confirmed.set(confirmed.get() + 1);
                reset_clear_confirmation(&ui, &pending);
            }
        }
    });
    ui.on_clear_cancel_clicked({
        let weak = ui.as_weak();
        let pending = pending_clear.clone();
        move || reset_clear_confirmation(&weak.upgrade().unwrap(), &pending)
    });
    ui.set_settings_text("应用策略反馈保持在策略区域".into());
    // Exercise logical sizing at the same physical client size: increased DPI
    // must switch to the compact layout rather than enlarge the window minimum.
    for (label, scale, width, height) in [
        ("100", 1.0, 1280, 720),
        ("125", 1.25, 1280, 720),
        ("150", 1.5, 1280, 720),
        ("175", 1.75, 1680, 945),
        ("200", 2.0, 1920, 1080),
        ("minimum", 1.0, 820, 440),
    ] {
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
        window.set_size(slint::PhysicalSize::new(width, height));
        slint::platform::update_timers_and_animations();
        assert_eq!(window.size(), slint::PhysicalSize::new(width, height));
        assert_eq!(ui.get_compact_layout(), label != "100");
        let compact = ui.get_compact_layout();
        let logical_width = width as f32 / scale;
        let narrow = logical_width < 960.0;
        ui.set_selected_page(3);
        slint::platform::update_timers_and_animations();
        click(65.0, if narrow { 132.0 } else { 159.0 });
        assert_eq!(ui.get_selected_page(), 0, "navigation at {label}%");
        let before = pauses.get();
        click(logical_width - 52.0, if compact { 31.0 } else { 51.0 });
        assert_eq!(pauses.get(), before + 1, "pause remains usable at {label}%");
        let content_x = if narrow { 129.0 } else { 169.0 } + if compact { 12.0 } else { 26.0 };
        let before = queries.get();
        click(content_x + 288.0, if compact { 82.0 } else { 116.0 });
        assert_eq!(queries.get(), before + 1, "date query at {label}%");
        let logical_height = height as f32 / scale;
        let calendar_x = (logical_width - 352.0) / 2.0;
        let calendar_y = (logical_height - 392.0) / 2.0;
        ui.set_query_date("2024-02-29".into());
        click(content_x + 352.0, if compact { 82.0 } else { 116.0 });
        assert!(ui.get_calendar_open(), "date picker opens at {label}%");
        assert_eq!(ui.get_calendar_caption(), "选择统计日期");
        assert_eq!(ui.get_calendar_days().row_count(), 42);
        save_preview(&window, &format!("calendar-{label}"));
        if label == "minimum" {
            ui.set_dark_mode(false);
            slint::platform::update_timers_and_animations();
            thread::sleep(Duration::from_millis(250));
            save_preview(&window, "calendar-minimum-light");
            ui.set_dark_mode(true);
            slint::platform::update_timers_and_animations();
            thread::sleep(Duration::from_millis(250));
        }
        let before = queries.get();
        // Browse a month and a year in each direction without running queries.
        click(calendar_x + 280.0, calendar_y + 64.0);
        assert_eq!(ui.get_calendar_month(), "2024 年 3 月");
        click(calendar_x + 68.0, calendar_y + 64.0);
        assert_eq!(ui.get_calendar_month(), "2024 年 2 月");
        click(calendar_x + 320.0, calendar_y + 64.0);
        assert_eq!(ui.get_calendar_month(), "2025 年 2 月");
        click(calendar_x + 32.0, calendar_y + 64.0);
        assert_eq!(ui.get_calendar_month(), "2024 年 2 月");
        assert_eq!(queries.get(), before);
        // Leap day is the Thursday of row five, using Monday-based columns.
        click(calendar_x + 176.0, calendar_y + 271.0);
        assert!(!ui.get_calendar_open());
        assert_eq!(ui.get_query_date(), "2024-02-29");
        assert_eq!(queries.get(), before + 1, "selection queries immediately");
        // Outside dismissal consumes the click, leaving query/date unchanged.
        ui.invoke_calendar_requested(0);
        slint::platform::update_timers_and_animations();
        click(12.0, 12.0);
        assert!(!ui.get_calendar_open());
        assert_eq!(ui.get_query_date(), "2024-02-29");
        assert_eq!(queries.get(), before + 1);
        ui.invoke_calendar_requested(0);
        slint::platform::update_timers_and_animations();
        click(calendar_x + 300.0, calendar_y + 360.0);
        assert!(!ui.get_calendar_open());
        assert_eq!(ui.get_query_date(), "2024-02-29");
        // Escape must work after reopening, not just on the first instance.
        ui.invoke_calendar_requested(0);
        slint::platform::update_timers_and_animations();
        let key = |key: slint::platform::Key| {
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.into() });
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key.into() });
            slint::platform::update_timers_and_animations();
        };
        key(slint::platform::Key::Escape);
        assert!(
            !ui.get_calendar_open(),
            "Escape closes calendar at {label}%"
        );
        ui.invoke_calendar_requested(0);
        slint::platform::update_timers_and_animations();
        key(slint::platform::Key::RightArrow);
        assert_eq!(ui.get_calendar_selection(), "2024-03-01");
        key(slint::platform::Key::Return);
        assert!(!ui.get_calendar_open());
        assert_eq!(ui.get_query_date(), "2024-03-01");
        // CSV dates are independent and never start export or a stats query.
        let before = queries.get();
        let exports_before = exports.get();
        ui.set_export_from_date("not-a-date".into());
        ui.invoke_calendar_requested(1);
        assert_eq!(
            ui.get_calendar_selection(),
            Local::now().date_naive().to_string()
        );
        ui.set_calendar_open(false);
        ui.set_export_from_date("2024-02-29".into());
        ui.set_export_to_date("2024-03-31".into());
        ui.invoke_calendar_requested(1);
        slint::platform::update_timers_and_animations();
        click(calendar_x + 176.0, calendar_y + 271.0);
        assert_eq!(ui.get_export_from_date(), "2024-02-29");
        assert_eq!(ui.get_export_to_date(), "2024-03-31");
        ui.invoke_calendar_requested(2);
        slint::platform::update_timers_and_animations();
        assert_eq!(ui.get_calendar_caption(), "选择导出结束日期");
        click(calendar_x + 45.0, calendar_y + 360.0);
        assert_eq!(
            ui.get_export_to_date(),
            Local::now().date_naive().to_string()
        );
        ui.invoke_calendar_requested(1);
        slint::platform::update_timers_and_animations();
        click(calendar_x + 112.0, calendar_y + 360.0);
        assert_eq!(
            ui.get_export_from_date(),
            (Local::now().date_naive() - chrono::Duration::days(1)).to_string()
        );
        assert_eq!(queries.get(), before);
        assert_eq!(exports.get(), exports_before);
        ui.set_query_date(date.into());
        click(content_x + 150.0, if compact { 347.0 } else { 431.0 });
        assert_eq!(
            ui.get_selected_page(),
            2,
            "complete application row at {label}%"
        );
        for page in 0..4 {
            ui.set_selected_page(page);
            save_preview(&window, &format!("scale-{label}-{page}"));
        }
        // Cover both spacious and compact layouts, rather than only endpoints.
        {
            let padding = if compact { 12.0 } else { 26.0 };
            let gap = if compact { 10.0 } else { 18.0 };
            for page in [1, 2, 3] {
                ui.set_selected_page(page);
                slint::platform::update_timers_and_animations();
                assert_eq!(ui.get_page_scroll_y(), 0.0, "new page begins at the top");
                if page == 1 {
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                            position: slint::LogicalPosition::new(
                                content_x + 180.0,
                                height as f32 / scale / 2.0,
                            ),
                            delta_x: -1_000.0,
                            delta_y: 0.0,
                        });
                    slint::platform::update_timers_and_animations();
                    assert_eq!(
                        ui.get_page_scroll_y(),
                        0.0,
                        "horizontal pan stays inside the keyboard"
                    );
                    save_preview(&window, &format!("keyboard-{label}-right"));
                }
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                        position: slint::LogicalPosition::new(
                            content_x + if page == 1 { 180.0 } else { 4.0 },
                            height as f32 / scale / 2.0,
                        ),
                        delta_x: 0.0,
                        delta_y: -10_000.0,
                    });
                slint::platform::update_timers_and_animations();
                assert!(
                    ui.get_page_scroll_y() < -64.0,
                    "wheel reaches the page below the keyboard"
                );
                save_preview(&window, &format!("scroll-{label}-{page}-bottom"));
                let logical_height = height as f32 / scale;
                if page == 2 {
                    let before = app_queries.get();
                    click(
                        logical_width - padding - 154.0,
                        logical_height - padding - gap - 79.0,
                    );
                    assert_eq!(
                        app_queries.get(),
                        before + 1,
                        "app query is reachable at {label}"
                    );
                }
                if page == 3 {
                    let scroll = ui.get_page_scroll_y();
                    let date_y = logical_height - padding - gap - 313.0;
                    click((content_x + logical_width - padding) / 2.0 - 84.5, date_y);
                    assert!(ui.get_calendar_open(), "CSV start picker at {label}%");
                    assert_eq!(ui.get_calendar_caption(), "选择导出开始日期");
                    click(calendar_x + 300.0, calendar_y + 360.0);
                    assert!(!ui.get_calendar_open());
                    click(logical_width - padding - 135.0, date_y);
                    assert!(ui.get_calendar_open(), "CSV end picker at {label}%");
                    assert_eq!(ui.get_calendar_caption(), "选择导出结束日期");
                    save_preview(&window, &format!("calendar-export-{label}"));
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::CloseRequested);
                    assert!(!ui.get_calendar_open(), "closing panel dismisses calendar");
                    assert!(ui.get_close_open());
                    ui.invoke_close_hide_clicked();
                    ui.show().unwrap();
                    slint::platform::update_timers_and_animations();
                    assert_eq!(
                        ui.get_page_scroll_y(),
                        scroll,
                        "calendar preserves scroll position"
                    );
                    let before = exports.get();
                    click(
                        logical_width - padding - 64.0,
                        logical_height - padding - gap - 313.0,
                    );
                    assert_eq!(
                        exports.get(),
                        before + 1,
                        "CSV export is reachable at {label}"
                    );
                    let (x, y) = (
                        logical_width - padding - 100.0,
                        logical_height - padding - gap - 68.0,
                    );
                    let before = confirmed_clears.get();
                    let scroll = ui.get_page_scroll_y();
                    click(x, y);
                    assert!(ui.get_clear_confirmation(), "clear asks for confirmation");
                    assert_eq!(confirmed_clears.get(), before);
                    assert!(ui.get_clear_status().contains("不可撤销"));
                    assert_eq!(ui.get_settings_text(), "应用策略反馈保持在策略区域");
                    assert_eq!(
                        ui.get_page_scroll_y(),
                        scroll,
                        "confirmation does not move the page"
                    );
                    save_preview(&window, &format!("clear-{label}-confirm"));
                    click(x, y);
                    assert_eq!(
                        confirmed_clears.get(),
                        before + 1,
                        "confirmation stays under the pointer"
                    );
                    assert!(!ui.get_clear_confirmation());
                    click(x, y);
                    click(x - 118.0, y);
                    assert!(!ui.get_clear_confirmation(), "cancel is reachable");
                    assert_eq!(confirmed_clears.get(), before + 1);
                    click(x, y);
                    assert!(ui.get_clear_confirmation());
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::CloseRequested);
                    assert!(ui.window().is_visible(), "close first asks for a choice");
                    assert!(ui.get_close_open());
                    ui.invoke_close_hide_clicked();
                    assert!(
                        !ui.window().is_visible(),
                        "hide choice keeps background mode"
                    );
                    assert!(
                        !ui.get_clear_confirmation(),
                        "closing cancels a pending clear"
                    );
                    assert!(!pending_clear.load(Ordering::Relaxed));
                    ui.show().unwrap();
                    slint::platform::update_timers_and_animations();
                }
                click(
                    logical_width - padding - 35.0,
                    logical_height - padding - 16.0,
                );
                assert_eq!(ui.get_page_scroll_y(), 0.0, "back to top at {label}");
                if page == 2 {
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                            position: slint::LogicalPosition::new(
                                content_x + 4.0,
                                logical_height / 2.0,
                            ),
                            delta_x: 0.0,
                            delta_y: -10_000.0,
                        });
                    slint::platform::update_timers_and_animations();
                    click(
                        logical_width - padding - 57.0,
                        logical_height - padding - gap - 79.0,
                    );
                    assert_eq!(ui.get_selected_page(), 3, "policy action reaches settings");
                    assert_eq!(ui.get_policy_path(), ui.get_detail_path());
                    assert_eq!(ui.get_page_scroll_y(), 0.0);
                }
            }
        }
        if label == "minimum" {
            ui.set_selected_page(0);
            ui.set_app_search("no-match".into());
            slint::platform::update_timers_and_animations();
            save_preview(&window, "scale-minimum-no-match");
            assert_eq!(window.size(), slint::PhysicalSize::new(width, height));
            ui.set_app_search("".into());
            slint::platform::update_timers_and_animations();
        }
    }
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.set_size(slint::PhysicalSize::new(960, 680));
    db.set_policy("C:/Preview/Editor.exe", "totals_only")
        .unwrap();
    refresh_overview(&ui, &db, date, RankingSort::Total).unwrap();
    assert_eq!(ui.get_detail_key_rows().row_count(), 0);
    assert_eq!(ui.get_detail_mouse_rows().row_count(), 0);
    assert_eq!(ui.get_key_rows().row_count(), 0);
    assert!(ui.get_detail_note().contains("未记录详细数据"));
    ui.set_selected_page(2);
    save_preview(&window, "totals-only");
    db.clear_all().unwrap();
    refresh_overview(&ui, &db, date, RankingSort::Total).unwrap();
    ui.set_selected_page(0);
    save_preview(&window, "empty");
    assert_eq!(ui.get_app_rows().row_count(), 0);
    #[cfg(windows)]
    {
        // The aggregates remain test data; only the embedded icons come from
        // actual local Windows executables. No program is launched.
        let system = std::env::var("SystemRoot").unwrap();
        let paths = [
            format!(r"{system}\System32\WindowsPowerShell\v1.0\powershell.exe"),
            format!(r"{system}\explorer.exe"),
            format!(r"{system}\System32\notepad.exe"),
        ];
        for (index, path) in paths.iter().enumerate() {
            for (code, divisor) in [("1e", 1), ("1f", 2), ("20", 4), ("21", 20)] {
                let count = 12_345 / ((index as i64 + 1) * divisor);
                db.record_batch(&[stats::event::InputEvent {
                    local_date: date.into(),
                    local_hour: None,
                    app_name: path.rsplit('\\').next().unwrap().into(),
                    app_path: path.clone(),
                    input_type: "keyboard".into(),
                    code: format!("scan:{code}:ext:false"),
                    count,
                    keyboard_count: count,
                    mouse_count: 0,
                    wheel_units: 0,
                    detailed: true,
                }])
                .unwrap();
            }
        }
        refresh_overview(&ui, &db, date, RankingSort::Total).unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while app_icons::pending_count() > 0 {
            let remaining = end.saturating_duration_since(std::time::Instant::now());
            let event = event_receiver
                .recv_timeout(remaining)
                .expect("icon worker completion");
            event();
        }
        assert_eq!(ui.get_app_rows().row_data(0).unwrap().icon.size().width, 96);
        ui.set_app_search("PowerShell".into());
        slint::platform::update_timers_and_animations();
        assert!(
            ui.get_app_rows().row_data(0).unwrap().has_icon,
            "search retains cached icon"
        );
        ui.set_app_search("".into());
        slint::platform::update_timers_and_animations();
        refresh_detail(&ui, &db, date, &paths[0]).unwrap();
        assert!(ui.get_detail_has_icon(), "details reuse the cached icon");
        for (label, scale, width, height) in [
            ("100", 1.0, 1280, 720),
            ("125", 1.25, 1280, 720),
            ("150", 1.5, 1280, 720),
            ("175", 1.75, 1680, 945),
            ("200", 2.0, 1920, 1080),
        ] {
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                    scale_factor: scale,
                });
            window.set_size(slint::PhysicalSize::new(width, height));
            for page in [0, 2] {
                ui.set_selected_page(page);
                save_preview(&window, &format!("icons-{label}-{page}"));
            }
        }
        ui.set_capture_active(false);
        ui.set_selected_page(0);
        save_preview(&window, "paused-dark-200");
        ui.set_dark_mode(false);
        ui.set_theme_text("主题：浅色".into());
        slint::platform::update_timers_and_animations();
        if std::env::var_os("KB_COUNT_RENDER_UI").is_some() {
            thread::sleep(Duration::from_millis(250));
        }
        ui.set_selected_page(0);
        save_preview(&window, "paused-light-200");
        ui.set_capture_active(true);
        save_preview(&window, "icons-light-200");
        refresh_heatmap(&ui, &db, date).unwrap();
        ui.set_selected_page(1);
        save_preview(&window, "heatmap-readable-200");
        assert!(
            ui.get_all_app_rows().iter().all(|row| !row.has_icon),
            "the base model does not retain a second icon cache"
        );
        ui.hide().unwrap();
        let mut row = ui.get_app_rows().row_data(0).unwrap();
        row.path = r"C:\NotYetRequested\app.exe".into();
        ui.set_app_rows([row].as_slice().into());
        app_icons::populate(&ui);
        assert_eq!(
            app_icons::pending_count(),
            0,
            "hidden panels do not request icons"
        );
    }
    verify_activity_and_close(&ui, &window);
    ui.hide().unwrap();
    run_desktop_event_loop().unwrap();
    assert!(
        !quit_on_last_window.get(),
        "tray mode must require an explicit event-loop quit"
    );
}

fn verify_activity_and_close(ui: &MainWindow, window: &MinimalSoftwareWindow) {
    use crate::storage::history::sample;
    use chrono::NaiveDate;
    ui.set_version_text(env!("CARGO_PKG_VERSION").into());
    ui.set_status_text("离线预览 · 测试聚合数据".into());
    let mut db = storage::database::Database::open(std::path::Path::new(":memory:")).unwrap();
    let mut batch = Vec::new();
    for ordinal in 1..=366 {
        let date = NaiveDate::from_yo_opt(2024, ordinal).unwrap();
        let hour = Some((ordinal % 10 + 8) as u8);
        for (code, value) in [
            ("key", 400 + i64::from(ordinal % 29) * 50),
            ("left", 80 + i64::from(ordinal % 7) * 12),
            ("right", 25),
            ("wheel_up", 36),
            ("wheel_down", 27),
        ] {
            batch.push(sample(
                &date.to_string(),
                hour,
                "C:/Preview/Editor.exe",
                code,
                value,
            ));
        }
        if ordinal % 3 == 0 {
            batch.push(sample(
                &date.to_string(),
                Some(19),
                "C:/Preview/Terminal.exe",
                "key",
                210,
            ));
        }
    }
    for index in 0..9 {
        batch.push(sample(
            "2024-10-15",
            Some(18),
            &format!("C:/Preview/Tool{index}.exe"),
            "key",
            index + 1,
        ));
    }
    db.record_batch(&batch).unwrap();
    let db = Arc::new(Mutex::new(db));
    charts::bind(ui, db.clone());
    let weak = ui.as_weak();
    let query_db = db.clone();
    ui.on_query_clicked(move || {
        let ui = weak.upgrade().unwrap();
        let date = ui.get_query_date();
        refresh_overview(&ui, &query_db.lock().unwrap(), &date, RankingSort::Total).unwrap();
    });
    let exits = Rc::new(std::cell::Cell::new(0));
    ui.on_exit_requested({
        let exits = exits.clone();
        move || exits.set(exits.get() + 1)
    });
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        let button = slint::platform::PointerEventButton::Left;
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
    };
    for (label, scale, width, height) in [
        ("100", 1.0, 1280, 720),
        ("125", 1.25, 1280, 720),
        ("150", 1.5, 1280, 720),
        ("175", 1.75, 1680, 945),
        ("200", 2.0, 1920, 1080),
        ("minimum", 1.0, 820, 440),
    ] {
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
        window.set_size(slint::PhysicalSize::new(width, height));
        let w = width as f32 / scale;
        let h = height as f32 / scale;
        ui.show().unwrap();
        for dark in [false, true] {
            ui.set_dark_mode(dark);
            slint::platform::update_timers_and_animations();
            if std::env::var_os("KB_COUNT_RENDER_UI").is_some() {
                thread::sleep(Duration::from_millis(250));
            }
            ui.set_date_text("2024-10-15".into());
            ui.set_query_date("2024-10-15".into());
            ui.set_chart_view(0);
            ui.set_pie_mode(0);
            ui.set_selected_page(4);
            ui.invoke_chart_all_apps_selected();
            ui.invoke_charts_requested();
            assert_eq!(ui.get_activity_cells().row_count(), 366);
            assert_eq!(ui.get_trend_series().row_count(), 6);
            assert_eq!(ui.get_activity_columns(), 53);
            ui.invoke_chart_year_shift(-12);
            assert_eq!(
                ui.get_activity_columns(),
                54,
                "Sunday-start leap year needs 54 columns"
            );
            ui.invoke_chart_year_shift(12);
            let compact = ui.get_compact_layout();
            let padding = if compact { 12.0 } else { 26.0 };
            let content_x = if w < 960.0 { 129.0 } else { 169.0 } + padding;
            let content_y = if compact { 114.0 } else { 156.0 };
            for period in 0..4 {
                ui.set_page_scroll_y(0.0);
                slint::platform::update_timers_and_animations();
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                        position: slint::LogicalPosition::new(content_x + 4.0, content_y + 10.0),
                        delta_x: 0.0,
                        delta_y: -316.0,
                    });
                slint::platform::update_timers_and_animations();
                if period == 0 {
                    save_preview(window, &format!("activity-controls-{label}"));
                }
                click(
                    w - padding - 187.0 + 48.0 * period as f32,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                assert_eq!(
                    ui.get_trend_period(),
                    period,
                    "period selector {period} at {label}"
                );
                let length = match period {
                    0 => 24,
                    1 => 7,
                    2 => 31,
                    _ => 12,
                };
                assert_eq!(ui.get_trend_points().row_count(), length);
                assert!(
                    ui.get_trend_series()
                        .row_data(0)
                        .unwrap()
                        .commands
                        .contains('L')
                );
                let anchor = NaiveDate::from_ymd_opt(2024, 10, 15).unwrap();
                let next = analytics::Period::from_index(period)
                    .shift(anchor, 1)
                    .unwrap();
                click(
                    w - padding - 247.0,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                assert_eq!(
                    ui.get_date_text(),
                    next.to_string(),
                    "next period at {label}"
                );
                assert_eq!(ui.get_query_date(), ui.get_date_text());
                assert_eq!(ui.get_trend_period(), period);
                click(
                    w - padding - 385.0,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                assert_eq!(
                    ui.get_date_text(),
                    "2024-10-15",
                    "previous period at {label}"
                );
                click(
                    w - padding - 316.0,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                assert_eq!(
                    ui.get_date_text(),
                    chrono::Local::now().date_naive().to_string(),
                    "current period at {label}"
                );
                assert_eq!(ui.get_trend_period(), period);
                ui.set_query_date("2024-10-15".into());
                ui.invoke_query_clicked();
            }
            ui.invoke_chart_period_selected(2);
            ui.set_page_scroll_y(0.0);
            slint::platform::update_timers_and_animations();
            let theme = if dark { "dark" } else { "light" };
            save_preview(window, &format!("activity-{theme}-{label}-annual"));
            click(w - padding - 135.0, content_y + 76.0);
            assert_eq!(ui.get_activity_year(), "2023");
            assert_eq!(
                ui.get_keyboard_text(),
                "0",
                "year selection refreshes the overview too"
            );
            click(w - padding - 61.0, content_y + 76.0);
            assert_eq!(ui.get_activity_cells().row_count(), 366);
            assert_eq!(
                ui.get_keyboard_text(),
                format_count(db.lock().unwrap().totals("2024-10-15").unwrap().0)
            );
            if label == "100" {
                click(content_x + 50.0, content_y + 152.0);
                assert_eq!(
                    ui.get_query_date(),
                    "2024-01-01",
                    "heatmap cell opens its actual date"
                );
                assert_eq!(ui.get_trend_period(), 0);
                ui.set_query_date("2024-10-15".into());
                ui.invoke_query_clicked();
                ui.invoke_chart_period_selected(2);
            }
            // Open the real application picker and choose a search result.
            click(content_x + 125.0, content_y + 16.0);
            assert!(ui.get_app_picker_open(), "app picker at {label}");
            ui.set_chart_app_search("editor".into());
            slint::platform::update_timers_and_animations();
            assert_eq!(ui.get_chart_apps().row_count(), 1);
            save_preview(window, &format!("activity-{theme}-{label}-apps"));
            click((w - 600.0) / 2.0 + 160.0, (h - 392.0) / 2.0 + 121.0);
            assert!(!ui.get_app_picker_open());
            assert!(ui.get_trend_app_path().ends_with("editor.exe"));
            assert_eq!(ui.get_trend_app_name(), "editor.exe");
            for period in 0..4 {
                ui.invoke_chart_period_selected(period);
                assert_eq!(
                    ui.get_trend_points().row_count(),
                    match period {
                        0 => 24,
                        1 => 7,
                        2 => 31,
                        _ => 12,
                    }
                );
            }
            ui.invoke_chart_period_selected(2);
            ui.set_page_scroll_y(-10_000.0);
            slint::platform::update_timers_and_animations();
            save_preview(window, &format!("activity-{theme}-{label}-trend"));
            if label == "100" {
                click(
                    content_x + 60.0,
                    h - padding - 32.0 - if compact { 10.0 } else { 18.0 } - 74.0,
                );
            } else {
                ui.invoke_chart_series_toggled(0);
            }
            assert!(!ui.get_trend_series().row_data(0).unwrap().shown);
            ui.invoke_chart_series_toggled(1);
            assert!(
                ui.get_trend_maximum().parse::<i32>().unwrap() < 200,
                "hiding large series rescales mouse curves"
            );
            if ["100", "minimum", "200"].contains(&label) {
                save_preview(window, &format!("activity-{theme}-{label}-mouse"));
            }
            ui.invoke_chart_period_selected(1);
            assert!(
                !ui.get_trend_series().row_data(0).unwrap().shown,
                "visibility survives period changes"
            );
            ui.invoke_chart_period_shift(-1);
            assert_eq!(ui.get_date_text(), "2024-10-08");
            assert!(ui.get_trend_app_path().ends_with("editor.exe"));
            assert!(!ui.get_trend_series().row_data(0).unwrap().shown);
            ui.invoke_chart_period_shift(1);
            ui.invoke_chart_series_toggled(0);
            ui.invoke_chart_series_toggled(1);
            ui.invoke_chart_all_apps_selected();
            assert_eq!(ui.get_trend_app_path(), "");
            // Pie views use complete daily totals, not the hourly subset.
            ui.set_page_scroll_y(0.0);
            slint::platform::update_timers_and_animations();
            click(w - padding - 38.0, content_y + 16.0);
            assert_eq!(ui.get_chart_view(), 1, "pie view button at {label}");
            for period in 0..4 {
                ui.set_page_scroll_y(0.0);
                ui.window()
                    .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                        position: slint::LogicalPosition::new(content_x + 4.0, content_y + 10.0),
                        delta_x: 0.0,
                        delta_y: -316.0,
                    });
                slint::platform::update_timers_and_animations();
                click(
                    w - padding - 187.0 + 48.0 * period as f32,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                assert_eq!(ui.get_trend_period(), period);
                let (from, to) = analytics::Period::from_index(period)
                    .range(NaiveDate::from_ymd_opt(2024, 10, 15).unwrap());
                let expected = db
                    .lock()
                    .unwrap()
                    .history_daily(&from.to_string(), &to.to_string(), None)
                    .unwrap()
                    .iter()
                    .map(|(_, c)| c.total())
                    .sum::<i64>();
                assert_eq!(ui.get_pie_rows().row_count(), 3);
                assert_eq!(
                    ui.get_pie_rows()
                        .iter()
                        .map(|r| r.value.replace(',', "").parse::<i64>().unwrap())
                        .sum::<i64>(),
                    expected
                );
                assert!(ui.get_pie_has_data());
                assert_eq!(ui.get_pie_source().size().width, 240);
                click(
                    w - padding - 385.0,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                let previous = analytics::Period::from_index(period)
                    .shift(NaiveDate::from_ymd_opt(2024, 10, 15).unwrap(), -1)
                    .unwrap();
                assert_eq!(ui.get_date_text(), previous.to_string());
                assert_eq!(ui.get_chart_view(), 1);
                let (from, to) = analytics::Period::from_index(period).range(previous);
                let expected = db
                    .lock()
                    .unwrap()
                    .history_daily(&from.to_string(), &to.to_string(), None)
                    .unwrap()
                    .iter()
                    .map(|(_, c)| c.total())
                    .sum::<i64>();
                assert_eq!(
                    ui.get_pie_rows()
                        .iter()
                        .map(|row| row.value.replace(',', "").parse::<i64>().unwrap())
                        .sum::<i64>(),
                    expected,
                    "pie follows previous period"
                );
                click(
                    w - padding - 247.0,
                    content_y + 348.0 + ui.get_page_scroll_y(),
                );
                assert_eq!(ui.get_date_text(), "2024-10-15");
            }
            ui.invoke_chart_period_selected(2);
            ui.set_page_scroll_y(-10_000.0);
            slint::platform::update_timers_and_animations();
            save_preview(window, &format!("pie-{theme}-{label}-input"));
            click(
                content_x + 156.0,
                content_y + 390.0 + ui.get_page_scroll_y(),
            );
            assert_eq!(ui.get_pie_mode(), 1, "application pie mode at {label}");
            assert_eq!(ui.get_pie_rows().row_count(), 8);
            ui.invoke_chart_app_requested();
            ui.set_chart_app_search("editor".into());
            slint::platform::update_timers_and_animations();
            ui.invoke_chart_app_chosen(ui.get_chart_apps().row_data(0).unwrap().path);
            for period in 0..4 {
                ui.invoke_chart_period_selected(period);
                let (from, to) = analytics::Period::from_index(period)
                    .range(NaiveDate::from_ymd_opt(2024, 10, 15).unwrap());
                let expected = db
                    .lock()
                    .unwrap()
                    .history_daily(&from.to_string(), &to.to_string(), None)
                    .unwrap()
                    .iter()
                    .map(|(_, c)| c.total())
                    .sum::<i64>();
                assert_eq!(
                    ui.get_pie_rows()
                        .iter()
                        .map(|r| r.value.replace(',', "").parse::<i64>().unwrap())
                        .sum::<i64>(),
                    expected,
                    "app pie keeps the global denominator"
                );
                assert!(
                    ui.get_pie_rows()
                        .iter()
                        .any(|r| r.selected && r.label == "editor.exe")
                );
            }
            ui.invoke_chart_period_selected(2);
            ui.set_page_scroll_y(-10_000.0);
            slint::platform::update_timers_and_animations();
            save_preview(window, &format!("pie-{theme}-{label}-apps"));
            ui.invoke_chart_pie_mode_selected(0);
            for period in 0..4 {
                ui.invoke_chart_period_selected(period);
                let (from, to) = analytics::Period::from_index(period)
                    .range(NaiveDate::from_ymd_opt(2024, 10, 15).unwrap());
                let expected = db
                    .lock()
                    .unwrap()
                    .history_daily(
                        &from.to_string(),
                        &to.to_string(),
                        Some("C:/Preview/Editor.exe"),
                    )
                    .unwrap()
                    .iter()
                    .map(|(_, c)| c.total())
                    .sum::<i64>();
                assert_eq!(
                    ui.get_pie_rows()
                        .iter()
                        .map(|r| r.value.replace(',', "").parse::<i64>().unwrap())
                        .sum::<i64>(),
                    expected,
                    "composition follows the selected app"
                );
            }
            ui.invoke_chart_period_selected(2);
            ui.set_page_scroll_y(-10_000.0);
            slint::platform::update_timers_and_animations();
            save_preview(window, &format!("pie-{theme}-{label}-selected"));
            ui.invoke_chart_view_selected(0);
            ui.invoke_chart_day_selected("2024-02-29".into());
            assert_eq!(ui.get_trend_period(), 0);
            assert_eq!(ui.get_trend_points().row_count(), 24);
            assert_eq!(ui.get_query_date(), "2024-02-29");
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::CloseRequested);
            slint::platform::update_timers_and_animations();
            assert!(ui.get_close_open());
            assert!(ui.window().is_visible());
            save_preview(window, &format!("close-{theme}-{label}"));
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                    text: slint::platform::Key::Escape.into(),
                });
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                    text: slint::platform::Key::Escape.into(),
                });
            slint::platform::update_timers_and_animations();
            assert!(!ui.get_close_open(), "Escape cancels closing");
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::CloseRequested);
            slint::platform::update_timers_and_animations();
            let (left, top) = ((w - 456.0) / 2.0, (h - 240.0) / 2.0);
            click(left + 45.0, top + 204.0);
            assert!(!ui.get_close_open(), "cancel close at {label}");
            assert!(ui.window().is_visible());
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::CloseRequested);
            slint::platform::update_timers_and_animations();
            let before = exits.get();
            click(left + 396.0, top + 204.0);
            assert_eq!(exits.get(), before + 1, "exit choice at {label}");
            assert!(
                ui.window().is_visible(),
                "test exit callback never terminates the test process"
            );
            ui.invoke_close_cancel_clicked();
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::CloseRequested);
            slint::platform::update_timers_and_animations();
            ui.set_capture_active(false);
            click(left + 298.0, top + 204.0);
            assert!(!ui.window().is_visible(), "background choice at {label}");
            assert!(
                !ui.get_capture_active(),
                "background choice preserves pause state"
            );
            assert!(
                ui.get_trend_series()
                    .iter()
                    .all(|s| s.source.size().width == 0),
                "hiding the panel releases chart images"
            );
            ui.show().unwrap();
            ui.set_capture_active(true);
        }
    }
    let mut browser = sample("2024-10-15", Some(12), "C:/browser.exe", "left", 5);
    browser.code = "total".into();
    browser.detailed = false;
    db.lock().unwrap().record(&browser).unwrap();
    ui.set_date_text("2024-10-15".into());
    ui.set_query_date("2024-10-15".into());
    ui.set_trend_period(0);
    ui.invoke_chart_app_requested();
    ui.set_chart_app_search("browser".into());
    slint::platform::update_timers_and_animations();
    ui.invoke_chart_app_chosen(ui.get_chart_apps().row_data(0).unwrap().path);
    assert!(
        ui.get_trend_series()
            .iter()
            .skip(2)
            .all(|s| s.commands.is_empty()),
        "Totals Only never invents categorized curves"
    );
    assert!(
        ui.get_trend_points()
            .row_data(12)
            .unwrap()
            .description
            .contains("未记录详细数据")
    );
    assert!(ui.get_trend_note().contains("未记录详细数据"));
    ui.set_page_scroll_y(-10_000.0);
    slint::platform::update_timers_and_animations();
    save_preview(window, "activity-totals-only");
    ui.invoke_chart_view_selected(1);
    ui.invoke_chart_pie_mode_selected(0);
    assert_eq!(ui.get_pie_rows().row_data(1).unwrap().value, "5");
    assert_eq!(ui.get_pie_rows().row_data(1).unwrap().percent, "100.0%");
    ui.set_page_scroll_y(-10_000.0);
    slint::platform::update_timers_and_animations();
    save_preview(window, "pie-totals-only");
    db.lock()
        .unwrap()
        .set_policy("C:/empty.exe", "detailed")
        .unwrap();
    ui.invoke_chart_app_requested();
    ui.set_chart_app_search("empty".into());
    slint::platform::update_timers_and_animations();
    ui.invoke_chart_app_chosen(ui.get_chart_apps().row_data(0).unwrap().path);
    assert!(!ui.get_pie_has_data());
    assert_eq!(ui.get_pie_source().size().width, 0);
    assert!(ui.get_pie_rows().iter().all(|r| r.percent == "0.0%"));
    ui.set_page_scroll_y(-10_000.0);
    slint::platform::update_timers_and_animations();
    save_preview(window, "pie-empty");
    ui.invoke_chart_pie_mode_selected(1);
    assert!(
        ui.get_pie_rows()
            .iter()
            .any(|r| r.selected && r.label == "empty.exe" && r.value == "0" && r.percent == "0.0%")
    );
    assert!(
        ui.get_pie_has_data(),
        "global application pie includes other active apps"
    );
    ui.invoke_chart_pie_mode_selected(0);
    db.lock()
        .unwrap()
        .record(&sample("2024-10-15", None, "C:/legacy.exe", "key", 10))
        .unwrap();
    ui.invoke_chart_app_requested();
    ui.set_chart_app_search("legacy".into());
    slint::platform::update_timers_and_animations();
    ui.invoke_chart_app_chosen(ui.get_chart_apps().row_data(0).unwrap().path);
    assert!(ui.get_trend_note().contains("10 次未分时"));
    assert_eq!(ui.get_pie_rows().row_data(0).unwrap().value, "10");
    assert_eq!(ui.get_pie_rows().row_data(0).unwrap().percent, "100.0%");
    assert!(
        ui.get_pie_has_data(),
        "legacy daily data is still complete in pie charts"
    );
    ui.set_page_scroll_y(-10_000.0);
    slint::platform::update_timers_and_animations();
    save_preview(window, "pie-legacy");
    ui.invoke_chart_pie_mode_selected(1);
    ui.invoke_chart_app_requested();
    ui.set_chart_app_search("tool0".into());
    slint::platform::update_timers_and_animations();
    ui.invoke_chart_app_chosen(ui.get_chart_apps().row_data(0).unwrap().path);
    assert_eq!(ui.get_pie_rows().row_count(), 8);
    assert!(
        ui.get_pie_rows()
            .iter()
            .any(|r| r.selected && r.label == "tool0.exe")
    );
    ui.set_page_scroll_y(-10_000.0);
    slint::platform::update_timers_and_animations();
    let scroll = ui.get_page_scroll_y();
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(450.0, 250.0),
            delta_x: 0.0,
            delta_y: -10_000.0,
        });
    slint::platform::update_timers_and_animations();
    assert_eq!(
        ui.get_page_scroll_y(),
        scroll,
        "pie legend scroll does not move the whole page"
    );
    save_preview(window, "pie-many-apps");
}

#[test]
fn input_display_labels_keep_physical_keys_distinct() {
    assert_eq!(display_input_code("scan:45:ext:false"), "Num Lock");
    assert_eq!(display_input_code("scan:57:ext:false"), "F11");
    assert_eq!(display_input_code("scan:1d:ext:true"), "Right Ctrl");
    assert_eq!(display_input_code("hwheel_up"), "水平滚轮向右");
    assert_eq!(format_count(12_840), "12,840");
}

#[test]
fn exit_waits_for_committed_batch_and_restores_capture_on_failure() {
    let enabled = AtomicBool::new(true);
    let (tx, rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let WriterCommand::Shutdown(ack) = rx.recv().unwrap() else {
            panic!("shutdown command")
        };
        let mut db = storage::database::Database::open(std::path::Path::new(":memory:")).unwrap();
        let result = db.record(&storage::history::sample(
            "2026-10-16",
            Some(8),
            "C:/a.exe",
            "key",
            13,
        ));
        ack.send(result.is_ok()).unwrap();
        db.totals("2026-10-16").unwrap().0
    });
    assert!(exit_handshake(
        &enabled,
        &tx,
        || {
            assert!(!enabled.load(Ordering::Acquire));
            true
        },
        Duration::from_secs(1)
    ));
    assert!(!enabled.load(Ordering::Acquire));
    assert_eq!(worker.join().unwrap(), 13);
    let (tx, rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let WriterCommand::Shutdown(ack) = rx.recv().unwrap() else {
            panic!("shutdown")
        };
        ack.send(false).unwrap();
    });
    enabled.store(true, Ordering::Release);
    assert!(!exit_handshake(
        &enabled,
        &tx,
        || true,
        Duration::from_secs(1)
    ));
    assert!(enabled.load(Ordering::Acquire));
    worker.join().unwrap();
}

#[test]
fn exit_barrier_queue_and_timeout_failures_preserve_previous_pause_state() {
    let enabled = AtomicBool::new(true);
    let (tx, rx) = mpsc::sync_channel(1);
    assert!(!exit_handshake(
        &enabled,
        &tx,
        || false,
        Duration::from_millis(1)
    ));
    assert!(enabled.load(Ordering::Acquire));
    assert!(
        rx.try_recv().is_err(),
        "failed collector barrier never requests exit Flush"
    );
    enabled.store(false, Ordering::Release);
    assert!(!exit_handshake(
        &enabled,
        &tx,
        || true,
        Duration::from_millis(1)
    ));
    assert!(
        !enabled.load(Ordering::Acquire),
        "timeout preserves a previously paused collector"
    );
    assert!(
        !exit_handshake(&enabled, &tx, || true, Duration::from_millis(1)),
        "full bounded command queue prevents exit"
    );
    assert!(!enabled.load(Ordering::Acquire));
}

#[test]
fn writer_stops_only_after_exit_ack_is_received_successfully() {
    let (tx, rx) = mpsc::sync_channel(0);
    let worker = thread::spawn(move || acknowledge_exit(tx, true));
    assert!(
        !worker.is_finished(),
        "rendezvous cannot stop the writer before the caller receives"
    );
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert!(
        worker.join().unwrap(),
        "received successful save allows the writer loop to break"
    );
    let (tx, rx) = mpsc::sync_channel(0);
    drop(rx);
    assert!(
        !acknowledge_exit(tx, true),
        "a timed-out exit must not stop a successfully flushed writer"
    );
    let (tx, rx) = mpsc::sync_channel(0);
    let worker = thread::spawn(move || acknowledge_exit(tx, false));
    assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert!(
        !worker.join().unwrap(),
        "failed save keeps the writer available for retry"
    );
}
