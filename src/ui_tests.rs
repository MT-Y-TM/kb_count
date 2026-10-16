use super::*;
use slint::Model;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use std::io::Write;
use std::rc::Rc;

type UiEvent = Box<dyn FnOnce() + Send>;
struct PreviewPlatform(Rc<MinimalSoftwareWindow>, mpsc::SyncSender<UiEvent>);
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
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> {
        Some(Box::new(PreviewProxy(self.1.clone())))
    }
}

// Test-only rendering uses Slint's own software backend, without opening a
// desktop window, registering input devices, or accessing the user's database.
fn save_preview(window: &MinimalSoftwareWindow, name: &str) {
    if std::env::var_os("KB_COUNT_RENDER_UI").is_none() {
        return;
    }
    slint::platform::update_timers_and_animations();
    let size = window.size();
    let mut pixels = vec![slint::Rgb8Pixel::default(); (size.width * size.height) as usize];
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, size.width as usize);
    }));
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

fn verify_ui_state() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let (event_sender, event_receiver) = mpsc::sync_channel::<UiEvent>(64);
    slint::platform::set_platform(Box::new(PreviewPlatform(window.clone(), event_sender)))
        .expect("preview backend");
    let ui = MainWindow::new().expect("UI");
    bind_app_search(&ui);
    bind_panel_close(&ui);
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
                    assert!(!ui.window().is_visible(), "closing hides the panel");
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
    ui.hide().unwrap();
}

#[test]
fn input_display_labels_keep_physical_keys_distinct() {
    assert_eq!(display_input_code("scan:45:ext:false"), "Num Lock");
    assert_eq!(display_input_code("scan:57:ext:false"), "F11");
    assert_eq!(display_input_code("scan:1d:ext:true"), "Right Ctrl");
    assert_eq!(display_input_code("hwheel_up"), "水平滚轮向右");
    assert_eq!(format_count(12_840), "12,840");
}
