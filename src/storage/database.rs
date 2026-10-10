use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use crate::stats::event::InputEvent;
#[derive(Debug, Clone)]
pub struct AppTotal { pub app_name:String, pub app_path:String, pub keyboard:i64, pub mouse:i64, pub wheel:i64 }
#[derive(Debug, Clone)]
pub struct KeyTotal { pub code:String, pub count:i64 }
pub struct Database { conn: Connection }
impl Database {
 pub fn open(path: &Path) -> Result<Self> { let conn=Connection::open(path)?; conn.pragma_update(None,"journal_mode","WAL")?; conn.execute_batch("CREATE TABLE IF NOT EXISTS applications(id INTEGER PRIMARY KEY, executable_name TEXT NOT NULL, executable_path TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, first_seen_at TEXT NOT NULL, last_seen_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS daily_input_counts(local_date TEXT NOT NULL, app_id INTEGER NOT NULL, input_type TEXT NOT NULL, input_code TEXT NOT NULL, count INTEGER NOT NULL, PRIMARY KEY(local_date,app_id,input_type,input_code)); CREATE TABLE IF NOT EXISTS daily_app_totals(local_date TEXT NOT NULL, app_id INTEGER NOT NULL, keyboard_count INTEGER NOT NULL DEFAULT 0, mouse_click_count INTEGER NOT NULL DEFAULT 0, wheel_units INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(local_date,app_id)); CREATE TABLE IF NOT EXISTS app_policies(app_id INTEGER PRIMARY KEY, capture_mode TEXT NOT NULL DEFAULT 'detailed');")?; Ok(Self{conn}) }
 pub fn record(&mut self, event:&InputEvent) -> Result<()> { let tx=self.conn.transaction()?; tx.execute("INSERT INTO applications(executable_name,executable_path,display_name,first_seen_at,last_seen_at) VALUES (?1,?2,?3,datetime('now'),datetime('now')) ON CONFLICT(executable_path) DO UPDATE SET last_seen_at=datetime('now')", params![event.app_name,event.app_path,event.app_name])?; let id:i64=tx.query_row("SELECT id FROM applications WHERE executable_path=?1",[&event.app_path],|r|r.get(0))?; tx.execute("INSERT INTO daily_input_counts VALUES (?1,?2,?3,?4,?5) ON CONFLICT(local_date,app_id,input_type,input_code) DO UPDATE SET count=count+excluded.count",params![event.local_date,id,event.input_type,event.code,event.count])?; tx.execute("INSERT INTO daily_app_totals(local_date,app_id,keyboard_count,mouse_click_count,wheel_units) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(local_date,app_id) DO UPDATE SET keyboard_count=keyboard_count+excluded.keyboard_count,mouse_click_count=mouse_click_count+excluded.mouse_click_count,wheel_units=wheel_units+excluded.wheel_units",params![event.local_date,id,event.keyboard_count,event.mouse_count,event.wheel_units])?; tx.commit()?; Ok(()) }
 pub fn totals(&self,date:&str)->Result<(i64,i64,i64,i64)>{ self.conn.query_row("SELECT COALESCE(SUM(keyboard_count),0),COALESCE(SUM(mouse_click_count),0),COALESCE(SUM(wheel_units),0),COUNT(DISTINCT app_id) FROM daily_app_totals WHERE local_date=?1",[date],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(Into::into) }
 #[allow(dead_code)] pub fn app_totals(&self,date:&str)->Result<Vec<AppTotal>> { let mut stmt=self.conn.prepare("SELECT a.executable_name,a.executable_path,t.keyboard_count,t.mouse_click_count,t.wheel_units FROM daily_app_totals t JOIN applications a ON a.id=t.app_id WHERE t.local_date=?1 ORDER BY (t.keyboard_count+t.mouse_click_count+ABS(t.wheel_units)) DESC")?; let rows=stmt.query_map([date],|r|Ok(AppTotal{app_name:r.get(0)?,app_path:r.get(1)?,keyboard:r.get(2)?,mouse:r.get(3)?,wheel:r.get(4)?}))?; Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?) }
 #[allow(dead_code)] pub fn key_ranking(&self,date:&str,app_path:Option<&str>)->Result<Vec<KeyTotal>> {
  if let Some(path)=app_path {
   let mut stmt=self.conn.prepare("SELECT c.input_code,SUM(c.count) FROM daily_input_counts c JOIN applications a ON a.id=c.app_id WHERE c.local_date=?1 AND c.input_type='keyboard' AND a.executable_path=?2 GROUP BY c.input_code ORDER BY SUM(c.count) DESC")?;
   let rows=stmt.query_map(params![date,path],|r|Ok(KeyTotal{code:r.get(0)?,count:r.get(1)?}))?;
   return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
  }
  let mut stmt=self.conn.prepare("SELECT c.input_code,SUM(c.count) FROM daily_input_counts c WHERE c.local_date=?1 AND c.input_type='keyboard' GROUP BY c.input_code ORDER BY SUM(c.count) DESC")?;
  let rows=stmt.query_map([date],|r|Ok(KeyTotal{code:r.get(0)?,count:r.get(1)?}))?;
  Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
 }
 #[allow(dead_code)] pub fn set_policy(&self,path:&str,mode:&str)->Result<()> { self.conn.execute("INSERT INTO app_policies(app_id,capture_mode) SELECT id,?2 FROM applications WHERE executable_path=?1 ON CONFLICT(app_id) DO UPDATE SET capture_mode=excluded.capture_mode",params![path,mode])?; Ok(()) }
}
