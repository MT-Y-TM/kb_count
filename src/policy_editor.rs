//! Policy editing and app selection. Draft changes use only the cached map.
use crate::stats::policy::{CaptureMode, normalize_app_path};
use crate::{ChartApp, MainWindow, refresh_detail, refresh_heatmap, storage::database::Database};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Editor {
    apps: Vec<ChartApp>,
    overrides: HashMap<String, CaptureMode>,
    ready: bool,
    feedback: Option<(String, String)>,
}

fn identity(text: &str) -> Option<String> {
    if text.trim().trim_matches('"').trim().is_empty() {
        return None;
    }
    let path = normalize_app_path(text);
    (path == "Unknown"
        || (Path::new(&path).is_absolute()
            && Path::new(&path).file_name().is_some()
            && !path.ends_with('\\')))
    .then_some(path)
}

fn label(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::Detailed => "详细统计",
        CaptureMode::TotalsOnly => "仅统计总量",
        CaptureMode::Excluded => "已排除",
    }
}

impl Editor {
    fn message(&mut self, ui: &MainWindow, text: impl Into<String>) {
        self.feedback = Some((ui.get_policy_path().to_string(), text.into()));
        self.render(ui);
    }

    fn render(&mut self, ui: &MainWindow) {
        let path = identity(&ui.get_policy_path());
        ui.set_policy_path_valid(path.is_some());
        ui.set_policy_ready(self.ready);
        let mode = path
            .as_deref()
            .filter(|_| self.ready)
            .map(|path| CaptureMode::from_overrides(path, &self.overrides));
        let manual = path
            .as_ref()
            .is_some_and(|path| self.ready && self.overrides.contains_key(path));
        ui.set_policy_manual(manual);
        ui.set_policy_mode(match mode {
            Some(CaptureMode::Detailed) => 0,
            Some(CaptureMode::TotalsOnly) => 1,
            Some(CaptureMode::Excluded) => 2,
            None => -1,
        });
        ui.set_policy_mode_label(
            match mode {
                Some(mode) => format!(
                    "当前：{}（{}）",
                    label(mode),
                    if manual { "手动策略" } else { "默认" }
                ),
                None if !self.ready => "尚未读取策略，可点击“重新读取”".into(),
                None => "请选择应用或输入完整路径".into(),
            }
            .into(),
        );
        let message = self
            .feedback
            .as_ref()
            .filter(|(draft, _)| draft == ui.get_policy_path().as_str())
            .map(|(_, message)| message.clone());
        if message.is_none() {
            self.feedback = None;
        }
        ui.set_policy_feedback(
            message
                .unwrap_or_else(|| {
                    if path.is_none() && !ui.get_policy_path().trim().is_empty() {
                        "请输入完整应用路径，或点击“选择应用”。Unknown 代表无法识别的应用。".into()
                    } else {
                        "策略影响后续批量统计，不会删除历史数据。选择应用或编辑路径不会保存策略。"
                            .into()
                    }
                })
                .into(),
        );
    }

    fn reload(&mut self, ui: &MainWindow, db: &Mutex<Database>) -> bool {
        let result = db
            .try_lock()
            .map_err(|_| "统计正在保存，请稍后重新读取".to_string())
            .and_then(|db| {
                Ok((
                    db.history_apps().map_err(|e| e.to_string())?,
                    db.policies().map_err(|e| e.to_string())?,
                ))
            });
        match result {
            Ok((apps, overrides)) => {
                self.overrides = overrides;
                self.apps = apps
                    .into_iter()
                    .map(|(name, path)| {
                        let mode = CaptureMode::from_overrides(&path, &self.overrides);
                        let source = if self.overrides.contains_key(&normalize_app_path(&path)) {
                            "手动"
                        } else {
                            "默认"
                        };
                        ChartApp {
                            name: name.into(),
                            path: path.into(),
                            note: format!("{source} · {}", label(mode)).into(),
                        }
                    })
                    .collect();
                self.ready = true;
                self.feedback = None;
                ui.set_policies_text(
                    format!(
                        "{} 项手动策略 · 可在“选择应用”中筛选手动策略与已排除应用",
                        self.overrides.len()
                    )
                    .into(),
                );
                self.render(ui);
                self.filter(ui);
                true
            }
            Err(error) => {
                self.ready = false;
                self.message(ui, format!("无法读取策略：{error}"));
                false
            }
        }
    }

    fn filter(&self, ui: &MainWindow) {
        let search = ui
            .get_policy_search()
            .trim()
            .replace('/', "\\")
            .to_lowercase();
        let filter = ui.get_policy_filter();
        let rows = self
            .apps
            .iter()
            .filter(|app| {
                let path = normalize_app_path(&app.path);
                let matches_filter = match filter {
                    1 => self.overrides.contains_key(&path),
                    2 => {
                        CaptureMode::from_overrides(&path, &self.overrides) == CaptureMode::Excluded
                    }
                    _ => true,
                };
                matches_filter
                    && (app.name.to_lowercase().contains(&search)
                        || path.to_lowercase().contains(&search))
            })
            .cloned()
            .collect::<Vec<_>>();
        ui.set_policy_apps(Rc::new(slint::VecModel::from(rows)).into());
    }

    fn save(&mut self, ui: &MainWindow, db: &Mutex<Database>, mode: Option<CaptureMode>) {
        let Some(path) = identity(&ui.get_policy_path()) else {
            self.message(ui, "未保存：请选择应用或输入完整路径。");
            return;
        };
        let Ok(database) = db.try_lock() else {
            self.message(ui, "未保存：统计正在保存，请稍后重试。");
            return;
        };
        let result = match mode {
            Some(mode) => database.set_policy(&path, mode.as_str()),
            None => database.reset_policy(&path),
        };
        if let Err(error) = result {
            tracing::warn!(%error, "failed to save application policy");
            self.message(ui, "策略未保存，请检查数据文件后重试。");
            return;
        }
        let date = ui.get_date_text();
        if ui.get_detail_loaded() {
            let _ = refresh_detail(ui, &database, &date, &ui.get_detail_path());
        }
        let _ = refresh_heatmap(ui, &database, &date);
        drop(database);
        ui.set_policy_path(path.into());
        if self.reload(ui, db) {
            self.message(
                ui,
                if mode.is_some() {
                    "已保存手动策略，将用于后续批量统计；历史数据保留。"
                } else {
                    "已恢复默认策略，将用于后续批量统计；历史数据保留。"
                },
            );
        } else {
            self.message(ui, "策略已保存，显示尚未更新，请重新读取。");
        }
    }
}

pub fn bind(ui: &MainWindow, db: Arc<Mutex<Database>>) {
    let editor = Rc::new(RefCell::new(Editor::default()));
    let reload = {
        let weak = ui.as_weak();
        let db = db.clone();
        let editor = editor.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                editor.borrow_mut().reload(&ui, &db);
            }
        }
    };
    ui.on_policy_editor_requested(reload);
    ui.on_policy_path_changed({
        let weak = ui.as_weak();
        let editor = editor.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                editor.borrow_mut().render(&ui);
            }
        }
    });
    ui.on_policy_picker_requested({
        let weak = ui.as_weak();
        let db = db.clone();
        let editor = editor.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                let mut editor = editor.borrow_mut();
                if editor.reload(&ui, &db) {
                    ui.set_policy_search("".into());
                    ui.set_policy_filter(0);
                    editor.filter(&ui);
                    ui.set_policy_picker_open(true);
                }
            }
        }
    });
    ui.on_policy_search_changed({
        let weak = ui.as_weak();
        let editor = editor.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                editor.borrow().filter(&ui);
            }
        }
    });
    ui.on_policy_filter_selected({
        let weak = ui.as_weak();
        let editor = editor.clone();
        move |index| {
            if let Some(ui) = weak.upgrade() {
                ui.set_policy_filter(index);
                editor.borrow().filter(&ui);
            }
        }
    });
    ui.on_policy_app_chosen({
        let weak = ui.as_weak();
        let editor = editor.clone();
        move |path| {
            if let Some(ui) = weak.upgrade() {
                let mut editor = editor.borrow_mut();
                if editor.apps.iter().any(|app| app.path == path) {
                    ui.set_policy_path(path);
                    ui.set_policy_picker_open(false);
                    editor.render(&ui);
                }
            }
        }
    });
    let save = |mode| {
        let weak = ui.as_weak();
        let db = db.clone();
        let editor = editor.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                editor.borrow_mut().save(&ui, &db, mode);
            }
        }
    };
    ui.on_policy_detailed_clicked(save(Some(CaptureMode::Detailed)));
    ui.on_policy_totals_clicked(save(Some(CaptureMode::TotalsOnly)));
    ui.on_policy_excluded_clicked(save(Some(CaptureMode::Excluded)));
    ui.on_policy_reset_clicked(save(None));
    editor.borrow_mut().render(ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_identity_accepts_portable_history_without_accessing_the_filesystem() {
        assert!(identity("").is_none());
        assert!(identity("\"\"").is_none());
        assert!(identity("\" \"").is_none());
        assert!(identity("chrome.exe").is_none());
        assert_eq!(identity(" unknown ").as_deref(), Some("Unknown"));
        #[cfg(windows)]
        {
            assert!(identity(r"C:\").is_none());
            assert!(identity(r"C:\Apps\").is_none());
            assert_eq!(
                identity(r#" "C:/Missing/Editor.EXE" "#).as_deref(),
                Some(r"c:\missing\editor.exe")
            );
        }
    }
}
