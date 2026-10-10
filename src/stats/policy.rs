#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode { Detailed, TotalsOnly, Excluded }
#[allow(dead_code)]
impl CaptureMode {
 pub fn parse(s:&str)->Self { match s { "totals_only"=>Self::TotalsOnly, "excluded"=>Self::Excluded, _=>Self::Detailed } }
 pub fn as_str(self)->&'static str { match self { Self::Detailed=>"detailed", Self::TotalsOnly=>"totals_only", Self::Excluded=>"excluded" } }
}
