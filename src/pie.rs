use crate::stats::policy::normalize_app_path;
use crate::storage::{database::AppTotal, history::Counts};

pub struct Slice {
    pub label: String,
    pub detail: String,
    pub value: i64,
    pub selected: bool,
    pub color: [u8; 3],
}

pub fn input_slices(counts: Counts) -> Vec<Slice> {
    [
        ("键盘", "键盘敲击", counts.keyboard, [188, 97, 216]),
        ("鼠标点击", "全部鼠标按钮按下", counts.mouse, [22, 160, 133]),
        (
            "滚轮",
            "垂直及水平滚轮绝对单位",
            counts.wheel,
            [31, 159, 183],
        ),
    ]
    .into_iter()
    .map(|(label, detail, value, color)| Slice {
        label: label.into(),
        detail: detail.into(),
        value,
        selected: false,
        color,
    })
    .collect()
}

/// Keep at most seven named applications plus Others, pinning a selected app
/// even if it is below the top seven. The denominator always remains global.
pub fn app_slices(apps: Vec<AppTotal>, selected: &str) -> Vec<Slice> {
    let selected = (!selected.is_empty()).then(|| normalize_app_path(selected));
    let mut apps = apps
        .into_iter()
        .filter(|a| {
            let value = a.keyboard + a.mouse + a.wheel;
            value > 0
                || (value == 0
                    && selected
                        .as_deref()
                        .is_some_and(|path| normalize_app_path(&a.app_path) == path))
        })
        .collect::<Vec<_>>();
    apps.sort_by(|a, b| {
        (b.keyboard + b.mouse + b.wheel)
            .cmp(&(a.keyboard + a.mouse + a.wheel))
            .then_with(|| a.app_path.cmp(&b.app_path))
    });
    let pinned = apps.iter().position(|a| {
        selected
            .as_deref()
            .is_some_and(|path| normalize_app_path(&a.app_path) == path)
    });
    let mut rows = Vec::new();
    let mut other = 0;
    let mut other_count = 0;
    let palette = [
        [79, 131, 232],
        [188, 97, 216],
        [22, 160, 133],
        [224, 139, 43],
        [216, 83, 116],
        [31, 159, 183],
        [181, 154, 40],
        [120, 137, 164],
    ];
    for (index, app) in apps.into_iter().enumerate() {
        let retain =
            index < if pinned.is_some_and(|i| i >= 7) { 6 } else { 7 } || Some(index) == pinned;
        let value = app.keyboard + app.mouse + app.wheel;
        if retain {
            rows.push(Slice {
                label: app.app_name,
                detail: app.app_path,
                value,
                selected: Some(index) == pinned,
                color: palette[rows.len()],
            });
        } else {
            other += value;
            other_count += 1;
        }
    }
    if other_count > 0 {
        rows.push(Slice {
            label: format!("其他应用（{other_count}）"),
            detail: "其余应用合计；占比分母包含全部应用".into(),
            value: other,
            selected: false,
            color: palette[7],
        });
    }
    rows
}

pub fn svg(slices: &[Slice]) -> String {
    use std::fmt::Write;
    let total: i64 = slices.iter().map(|s| s.value).sum();
    if total <= 0 {
        return String::new();
    }
    let circumference = std::f64::consts::TAU * 90.0;
    let mut result =
        "<svg xmlns='http://www.w3.org/2000/svg' width='240' height='240' viewBox='0 0 240 240'>"
            .to_string();
    let mut offset = 0.0;
    for slice in slices.iter().filter(|s| s.value > 0) {
        let length = circumference * slice.value as f64 / total as f64;
        let _ = write!(
            result,
            "<circle cx='120' cy='120' r='90' fill='none' stroke='#{:02x}{:02x}{:02x}' stroke-width='36' stroke-dasharray='{length:.6} {:.6}' stroke-dashoffset='{:.6}' transform='rotate(-90 120 120)'/>",
            slice.color[0],
            slice.color[1],
            slice.color[2],
            (circumference - length).max(0.0),
            -offset
        );
        offset += length;
    }
    result.push_str("</svg>");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_composition_is_disjoint_and_zero_has_no_fake_chart() {
        let counts = Counts {
            keyboard: 10,
            mouse: 6,
            wheel: 4,
            ..Default::default()
        };
        let rows = input_slices(counts);
        assert_eq!(
            rows.iter().map(|r| r.value).collect::<Vec<_>>(),
            vec![10, 6, 4]
        );
        assert_eq!(rows.iter().map(|r| r.value).sum::<i64>(), counts.total());
        assert!(!svg(&rows).is_empty());
        assert!(svg(&input_slices(Counts::default())).is_empty());
        assert!(
            !svg(&input_slices(Counts {
                keyboard: 1,
                ..Default::default()
            }))
            .is_empty()
        );
    }
    #[test]
    fn small_selected_app_is_visible_without_changing_the_denominator() {
        let apps = (0..10)
            .map(|i| AppTotal {
                app_name: format!("app{i}"),
                app_path: format!("C:/app{i}.exe"),
                keyboard: (10 - i) * 10,
                mouse: 0,
                wheel: 0,
            })
            .collect();
        let rows = app_slices(apps, "c:\\APP9.exe");
        assert_eq!(rows.len(), 8);
        assert_eq!(rows.iter().map(|r| r.value).sum::<i64>(), 550);
        assert_eq!(rows.iter().find(|r| r.selected).unwrap().value, 10);
        assert_eq!(rows.last().unwrap().value, 90);
        assert!(app_slices(Vec::new(), "").is_empty());
        let unknown = app_slices(
            vec![AppTotal {
                app_name: "Unknown".into(),
                app_path: "Unknown".into(),
                keyboard: 1,
                mouse: 0,
                wheel: 0,
            }],
            "",
        );
        assert!(
            !unknown[0].selected,
            "all-app scope must not implicitly select Unknown"
        );
    }
}
