use crate::storage::database::Database;
use crate::{
    ActivityCell, ChartApp, MainWindow, PieRow, TrendPoint, TrendSeries, TrendTick, analytics,
    format_count, pie,
};
use anyhow::Result;
use chrono::{Datelike, Local, NaiveDate};
use slint::{ComponentHandle, Model};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

pub fn refresh(ui: &MainWindow, db: &Database) -> Result<()> {
    let anchor = NaiveDate::parse_from_str(&ui.get_date_text(), "%Y-%m-%d")?;
    let app = ui.get_trend_app_path();
    let snapshot = analytics::load(
        db,
        anchor,
        analytics::Period::from_index(ui.get_trend_period()),
        (!app.is_empty()).then_some(app.as_str()),
        Local::now().naive_local(),
    )?;
    let peak = snapshot
        .days
        .iter()
        .map(|d| d.value)
        .max()
        .unwrap_or(0)
        .max(1);
    let cells = snapshot
        .days
        .iter()
        .map(|d| ActivityCell {
            date: d.date.to_string().into(),
            value: format_count(d.value).into(),
            level: if d.value == 0 {
                0
            } else {
                ((d.value as f64 / peak as f64 * 4.0).ceil() as i32).clamp(1, 4)
            },
            week: d.week,
            weekday: d.weekday,
            future: d.future,
            month_label: if d.date.day() == 1 {
                format!("{}月", d.date.month())
            } else {
                String::new()
            }
            .into(),
        })
        .collect::<Vec<_>>();
    ui.set_activity_cells(Rc::new(slint::VecModel::from(cells)).into());
    ui.set_activity_year(anchor.year().to_string().into());
    ui.set_activity_columns(snapshot.columns);
    ui.set_activity_summary(
        format!(
            "全年输入 {} · 活跃 {} 天 · {}",
            format_count(snapshot.annual_total),
            snapshot.active_days,
            ui.get_trend_app_name()
        )
        .into(),
    );
    ui.set_trend_summary(
        format!(
            "总输入 {} · 键盘 {} · 鼠标点击 {} · 滚轮 {}",
            format_count(snapshot.totals.total()),
            format_count(snapshot.totals.keyboard),
            format_count(snapshot.totals.mouse),
            format_count(snapshot.totals.wheel)
        )
        .into(),
    );
    ui.set_trend_range(snapshot.range_label.into());
    ui.set_trend_note(snapshot.note.into());
    let old = ui.get_trend_series();
    let shown: Vec<_> = (0..6)
        .map(|i| old.row_data(i).is_none_or(|s| s.shown))
        .collect();
    let maximum = snapshot
        .points
        .iter()
        .filter(|p| p.available)
        .flat_map(|p| p.counts.values().into_iter().enumerate())
        .filter(|(i, _)| shown[*i])
        .map(|(_, v)| v)
        .max()
        .unwrap_or(1)
        .max(1);
    ui.set_trend_maximum(axis_count(maximum).into());
    ui.set_trend_middle(
        if maximum == 1 {
            "0.5".to_string()
        } else {
            axis_count(maximum / 2)
        }
        .into(),
    );
    let labels = ["总输入", "键盘", "左键", "右键", "滚轮上", "滚轮下"];
    let no_categories = snapshot.totals.classified == 0 && snapshot.totals.uncategorized() > 0;
    let colors = [
        [79, 131, 232],
        [188, 97, 216],
        [22, 160, 133],
        [224, 139, 43],
        [216, 83, 116],
        [31, 159, 183],
    ];
    let series = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let values = snapshot
                .points
                .iter()
                .map(|p| {
                    let categorized = i < 2
                        || (!no_categories
                            && (p.counts.classified > 0 || p.counts.uncategorized() == 0));
                    (p.available && categorized).then_some(p.counts.values()[i])
                })
                .collect::<Vec<_>>();
            let commands=analytics::path(&values,maximum);
            let source=if commands.is_empty() {slint::Image::default()} else {
                let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='1000' height='200' viewBox='0 0 1000 200' preserveAspectRatio='none'><path d='{commands}' fill='none' stroke='#{:02x}{:02x}{:02x}' stroke-width='3' stroke-linecap='round' stroke-linejoin='round'/></svg>",colors[i][0],colors[i][1],colors[i][2]);
                slint::Image::load_from_svg_data(svg.as_bytes())?
            };
            Ok(TrendSeries {
                label: (*label).into(),
                tint: slint::Color::from_rgb_u8(colors[i][0], colors[i][1], colors[i][2]),
                commands: commands.into(),
                source,
                shown: shown[i],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ui.set_trend_series(Rc::new(slint::VecModel::from(series)).into());
    let points = snapshot
        .points
        .iter()
        .map(|p| {
            let description = if !p.available {
                format!("{} · 暂无可用数据", p.label)
            } else if no_categories {
                format!(
                    "{} · 总输入 {} · 键盘 {} · 鼠标分类：未记录详细数据",
                    p.label,
                    format_count(p.counts.total()),
                    format_count(p.counts.keyboard)
                )
            } else {
                format!(
                    "{} · 总输入 {} · 键盘 {} · 左键 {} · 右键 {} · 上滚 {} · 下滚 {}{}",
                    p.label,
                    format_count(p.counts.total()),
                    format_count(p.counts.keyboard),
                    format_count(p.counts.left),
                    format_count(p.counts.right),
                    format_count(p.counts.up),
                    format_count(p.counts.down),
                    if p.counts.uncategorized() > 0 {
                        "（分类仅含已记录部分）"
                    } else {
                        ""
                    }
                )
            };
            TrendPoint {
                label: p.label.clone().into(),
                description: description.into(),
            }
        })
        .collect::<Vec<_>>();
    let n = points.len();
    let indices: Vec<_> = (0..6).map(|i| i * (n - 1) / 5).collect();
    let ticks = indices
        .into_iter()
        .map(|i| TrendTick {
            position: i as f32 / (n - 1) as f32,
            label: points[i].label.clone(),
        })
        .collect::<Vec<_>>();
    ui.set_trend_points(Rc::new(slint::VecModel::from(points)).into());
    ui.set_trend_ticks(Rc::new(slint::VecModel::from(ticks)).into());
    if ui.get_chart_view() == 1 {
        refresh_pie(ui, db, anchor, snapshot.totals)?;
    }
    Ok(())
}

fn refresh_pie(
    ui: &MainWindow,
    db: &Database,
    anchor: NaiveDate,
    counts: crate::storage::history::Counts,
) -> Result<()> {
    let (from, to) = analytics::Period::from_index(ui.get_trend_period()).range(anchor);
    let slices = if ui.get_pie_mode() == 0 {
        pie::input_slices(counts)
    } else {
        let mut apps = db.history_app_totals(&from.to_string(), &to.to_string())?;
        let selected = ui.get_trend_app_path();
        if !selected.is_empty() && !apps.iter().any(|app| app.app_path == selected.as_str()) {
            apps.push(crate::storage::database::AppTotal {
                app_name: ui.get_trend_app_name().to_string(),
                app_path: selected.to_string(),
                keyboard: 0,
                mouse: 0,
                wheel: 0,
            });
        }
        pie::app_slices(apps, &selected)
    };
    let total: i64 = slices.iter().map(|s| s.value).sum();
    let svg = pie::svg(&slices);
    ui.set_pie_source(if svg.is_empty() {
        slint::Image::default()
    } else {
        slint::Image::load_from_svg_data(svg.as_bytes())?
    });
    ui.set_pie_has_data(total > 0);
    ui.set_pie_total(axis_count(total).into());
    ui.set_pie_range(format!("{from} — {to} · 完整日汇总").into());
    let scope = if ui.get_pie_mode() == 0 {
        ui.get_trend_app_name().to_string()
    } else {
        "全应用分布".into()
    };
    ui.set_pie_scope(format!("总输入 {} · {scope}", format_count(total)).into());
    let rows = slices
        .into_iter()
        .map(|s| {
            let percent = if total > 0 {
                s.value as f64 * 100.0 / total as f64
            } else {
                0.0
            };
            PieRow {
                label: s.label.into(),
                detail: s.detail.into(),
                value: format_count(s.value).into(),
                percent: if percent > 0.0 && percent < 0.1 {
                    "<0.1%".into()
                } else {
                    format!("{percent:.1}%").into()
                },
                tint: slint::Color::from_rgb_u8(s.color[0], s.color[1], s.color[2]),
                selected: s.selected,
            }
        })
        .collect::<Vec<_>>();
    ui.set_pie_rows(Rc::new(slint::VecModel::from(rows)).into());
    Ok(())
}

fn axis_count(count: i64) -> String {
    if count >= 100_000_000 {
        format!("{:.1}亿", count as f64 / 100_000_000.0)
    } else if count >= 10_000 {
        format!("{:.1}万", count as f64 / 10_000.0)
    } else {
        count.to_string()
    }
}

pub fn release_images(ui: &MainWindow) {
    ui.set_pie_source(slint::Image::default());
    let mut rows = ui.get_trend_series().iter().collect::<Vec<_>>();
    for row in &mut rows {
        row.source = slint::Image::default();
    }
    ui.set_trend_series(Rc::new(slint::VecModel::from(rows)).into());
}

fn reload(ui: &MainWindow, db: &Arc<Mutex<Database>>) {
    match db.lock() {
        Ok(db) => {
            if let Err(error) = refresh(ui, &db) {
                ui.set_status_text(format!("趋势查询失败：{error}").into());
            }
        }
        Err(_) => ui.set_status_text("趋势查询失败：数据库不可用".into()),
    }
}

fn filter_apps(ui: &MainWindow, apps: &[ChartApp]) {
    let query = ui.get_chart_app_search().trim().to_lowercase();
    let filtered = apps
        .iter()
        .filter(|a| {
            a.name.to_lowercase().contains(&query)
                || a.path
                    .to_lowercase()
                    .replace('/', "\\")
                    .contains(&query.replace('/', "\\"))
        })
        .cloned()
        .collect::<Vec<_>>();
    ui.set_chart_apps(Rc::new(slint::VecModel::from(filtered)).into());
}

pub fn bind(ui: &MainWindow, db: Arc<Mutex<Database>>) {
    ui.on_chart_view_selected({
        let weak = ui.as_weak();
        let db = db.clone();
        move |view| {
            if let Some(ui) = weak.upgrade() {
                ui.set_chart_view(view);
                reload(&ui, &db);
                ui.set_page_scroll_y(-316.0);
            }
        }
    });
    ui.on_chart_pie_mode_selected({
        let weak = ui.as_weak();
        let db = db.clone();
        move |mode| {
            if let Some(ui) = weak.upgrade() {
                ui.set_pie_mode(mode);
                reload(&ui, &db);
            }
        }
    });
    ui.on_charts_requested({
        let weak = ui.as_weak();
        let db = db.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                reload(&ui, &db);
            }
        }
    });
    ui.on_chart_period_selected({
        let weak = ui.as_weak();
        let db = db.clone();
        move |period| {
            if let Some(ui) = weak.upgrade() {
                ui.set_trend_period(period);
                reload(&ui, &db);
            }
        }
    });
    ui.on_chart_period_shift({
        let weak = ui.as_weak();
        move |delta| {
            if let Some(ui) = weak.upgrade()
                && let Ok(date) = NaiveDate::parse_from_str(&ui.get_date_text(), "%Y-%m-%d")
                && let Some(date) =
                    analytics::Period::from_index(ui.get_trend_period()).shift(date, delta)
            {
                ui.set_query_date(date.to_string().into());
                ui.invoke_query_clicked();
            }
        }
    });
    ui.on_chart_period_current({
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_query_date(Local::now().date_naive().to_string().into());
                ui.invoke_query_clicked();
            }
        }
    });
    ui.on_chart_year_shift({
        let weak = ui.as_weak();
        move |delta| {
            if let Some(ui) = weak.upgrade()
                && let Ok(date) = NaiveDate::parse_from_str(&ui.get_date_text(), "%Y-%m-%d")
            {
                let year = date.year() + delta;
                if (1..=9999).contains(&year) {
                    let new_date = date
                        .with_year(year)
                        .or_else(|| NaiveDate::from_ymd_opt(year, 2, 28))
                        .unwrap();
                    ui.set_query_date(new_date.to_string().into());
                    ui.invoke_query_clicked();
                }
            }
        }
    });
    ui.on_chart_day_selected({
        let weak = ui.as_weak();
        move |date| {
            if let Some(ui) = weak.upgrade() {
                ui.set_trend_period(0);
                ui.set_chart_view(0);
                ui.set_query_date(date);
                ui.invoke_query_clicked();
            }
        }
    });
    ui.on_chart_series_toggled({
        let weak = ui.as_weak();
        let db = db.clone();
        move |index| {
            if let Some(ui) = weak.upgrade() {
                let mut rows = ui.get_trend_series().iter().collect::<Vec<_>>();
                if let Some(row) = rows.get_mut(index as usize) {
                    row.shown = !row.shown;
                    ui.set_trend_series(Rc::new(slint::VecModel::from(rows)).into());
                    reload(&ui, &db);
                }
            }
        }
    });
    ui.on_chart_all_apps_selected({
        let weak = ui.as_weak();
        let db = db.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_trend_app_path("".into());
                ui.set_trend_app_name("全部应用".into());
                reload(&ui, &db);
            }
        }
    });
    let catalog = Rc::new(RefCell::new(Vec::<ChartApp>::new()));
    ui.on_chart_app_requested({
        let weak = ui.as_weak();
        let db = db.clone();
        let catalog = catalog.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let result = db
                    .lock()
                    .map_err(|_| "数据库不可用".to_string())
                    .and_then(|db| db.history_apps().map_err(|e| e.to_string()));
                match result {
                    Ok(apps) => {
                        *catalog.borrow_mut() = apps
                            .into_iter()
                            .map(|(name, path)| ChartApp {
                                name: name.into(),
                                path: path.into(),
                                note: Default::default(),
                            })
                            .collect();
                        ui.set_chart_app_search("".into());
                        filter_apps(&ui, &catalog.borrow());
                        ui.set_app_picker_open(true);
                    }
                    Err(error) => ui.set_status_text(format!("应用查询失败：{error}").into()),
                }
            }
        }
    });
    ui.on_chart_app_search_changed({
        let weak = ui.as_weak();
        let catalog = catalog.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                filter_apps(&ui, &catalog.borrow());
            }
        }
    });
    ui.on_chart_app_chosen({
        let weak = ui.as_weak();
        move |path| {
            if let Some(ui) = weak.upgrade()
                && let Some(app) = catalog.borrow().iter().find(|a| a.path == path)
            {
                ui.set_trend_app_path(path);
                ui.set_trend_app_name(app.name.clone());
                ui.set_app_picker_open(false);
                reload(&ui, &db);
            }
        }
    });
}
