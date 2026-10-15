use super::*;
use slint::Model;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use std::io::Write;
use std::rc::Rc;

struct PreviewPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for PreviewPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
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
    slint::platform::set_platform(Box::new(PreviewPlatform(window.clone())))
        .expect("preview backend");
    let ui = MainWindow::new().expect("UI");
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
        "Tools.exe",
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
    click(370.0, 410.0);
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
    window.set_size(slint::PhysicalSize::new(960, 680));
    ui.set_selected_page(0);
    save_preview(&window, "dark-minimum");
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
