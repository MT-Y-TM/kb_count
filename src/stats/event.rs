#[derive(Debug, Clone)]
pub struct InputEvent {
    pub local_date: String,
    /// Local hour bucket only; never a per-event timestamp. None for legacy data.
    pub local_hour: Option<u8>,
    pub app_name: String,
    pub app_path: String,
    pub input_type: String,
    pub code: String,
    pub count: i64,
    pub keyboard_count: i64,
    pub mouse_count: i64,
    pub wheel_units: i64,
    pub detailed: bool,
}
