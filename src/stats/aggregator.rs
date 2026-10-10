use std::collections::HashMap;
use crate::stats::event::InputEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode { Detailed, TotalsOnly, Excluded }

#[derive(Default)]
pub struct Aggregator { counts: HashMap<(String,String,String,String), i64> }
impl Aggregator {
    pub fn record(&mut self, mut event: InputEvent, mode: CaptureMode) {
        if mode == CaptureMode::Excluded { return; }
        if mode == CaptureMode::TotalsOnly { event.code = "total".into(); event.input_type = "total".into(); }
        let key=(event.local_date.clone(),event.app_path.clone(),event.input_type.clone(),event.code.clone());
        *self.counts.entry(key).or_default() += event.count;
    }
    pub fn drain(&mut self) -> Vec<InputEvent> {
        let mut out=Vec::with_capacity(self.counts.len());
        for ((date,path,kind,code),count) in self.counts.drain() {
            let name=std::path::Path::new(&path).file_name().and_then(|x|x.to_str()).unwrap_or("Unknown").to_string();
            out.push(InputEvent { local_date:date,app_name:name,app_path:path,input_type:kind.clone(),code,count,keyboard_count:if kind=="keyboard" {count} else {0},mouse_count:if kind=="mouse" {count} else {0},wheel_units:if kind=="wheel" {count} else {0} });
        }
        out
    }
}
