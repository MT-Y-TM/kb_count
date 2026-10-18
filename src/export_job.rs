//! At most one user-initiated CSV job. Both result channels have capacity one.
use crate::{
    MainWindow, format_count, panel_is_presented,
    storage::export::{self, Outcome},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Job {
    cancel: Arc<AtomicBool>,
    progress: Receiver<u64>,
    result: Receiver<Result<Outcome, String>>,
    thread: Option<JoinHandle<()>>,
    target: PathBuf,
}

impl Job {
    fn spawn(
        database: PathBuf,
        from: String,
        to: String,
        target: PathBuf,
    ) -> std::io::Result<Self> {
        let target_worker = target.clone();
        Self::spawn_operation(target, move |cancel, progress| {
            let mut updated = Instant::now();
            export::export_snapshot(&database, &from, &to, &target_worker, &cancel, |rows| {
                if rows == 0 || updated.elapsed() >= Duration::from_millis(250) {
                    let _ = progress.try_send(rows);
                    updated = Instant::now();
                }
            })
            .map_err(|error| format!("{error:#}"))
        })
    }

    fn spawn_operation(
        target: PathBuf,
        operation: impl FnOnce(Arc<AtomicBool>, mpsc::SyncSender<u64>) -> Result<Outcome, String>
        + Send
        + 'static,
    ) -> std::io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (progress_tx, progress) = mpsc::sync_channel(1);
        let (result_tx, result) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("kb-csv-export".into())
            .spawn(move || {
                let result = operation(worker_cancel, progress_tx);
                let _ = result_tx.send(result);
            })?;
        Ok(Self {
            cancel,
            progress,
            result,
            thread: Some(thread),
            target,
        })
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
pub struct Controller {
    job: Option<Job>,
    pending: Option<String>,
}

impl Controller {
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    pub fn start(
        &mut self,
        ui: &MainWindow,
        database: PathBuf,
        from: String,
        to: String,
        target: PathBuf,
    ) {
        if self.is_running() {
            return;
        }
        match Job::spawn(database, from, to, target) {
            Ok(job) => {
                self.job = Some(job);
                self.pending = Some("正在后台导出，请稍候…".into());
            }
            Err(error) => self.pending = Some(format!("无法开始导出：{error}")),
        }
        self.poll(ui);
    }

    pub fn cancel(&mut self, ui: &MainWindow) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Release);
            self.pending = Some("正在取消导出…".into());
        }
        self.poll(ui);
    }

    pub fn poll(&mut self, ui: &MainWindow) {
        if let Some(job) = &self.job {
            if let Some(rows) = job.progress.try_iter().last()
                && !job.cancel.load(Ordering::Acquire)
            {
                self.pending = Some(format!(
                    "正在后台导出 · 已写入 {} 行",
                    format_count(rows as i64)
                ));
            }
            let finished = match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("导出任务意外结束，请重试。".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = finished {
                self.pending = Some(match result {
                    Ok(Outcome::Completed(rows)) => format!(
                        "已导出 {} 行：{}",
                        format_count(rows as i64),
                        job.target.display()
                    ),
                    Ok(Outcome::Cancelled) => "已取消导出，原目标文件保持不变。".into(),
                    Err(error) => format!("导出失败：{error}"),
                });
                self.job.take();
            }
        }
        if panel_is_presented(ui)
            && let Some(text) = self.pending.take()
        {
            ui.set_export_busy(self.is_running());
            ui.set_export_cancelling(
                self.job
                    .as_ref()
                    .is_some_and(|job| job.cancel.load(Ordering::Acquire)),
            );
            ui.set_export_text(text.into());
        }
    }

    pub fn shutdown(&mut self) {
        self.job.take();
    }
}

#[cfg(test)]
pub fn controlled_test_job(
    controller: &mut Controller,
) -> (mpsc::Receiver<()>, mpsc::SyncSender<()>) {
    let (started_tx, started) = mpsc::sync_channel(1);
    let (resume, resume_rx) = mpsc::sync_channel(1);
    controller.job = Some(
        Job::spawn_operation("preview.csv".into(), move |cancel, progress| {
            let _ = progress.try_send(128);
            started_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            Ok(if cancel.load(Ordering::Acquire) {
                Outcome::Cancelled
            } else {
                Outcome::Completed(128)
            })
        })
        .unwrap(),
    );
    controller.pending = Some("正在后台导出，请稍候…".into());
    (started, resume)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropping_a_running_job_cancels_and_joins_its_worker() {
        let (started_tx, started) = mpsc::sync_channel(1);
        let (observed_tx, observed) = mpsc::sync_channel(1);
        let job = Job::spawn_operation("not-written.csv".into(), move |cancel, _| {
            started_tx.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !cancel.load(Ordering::Acquire) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(2));
            }
            observed_tx.send(cancel.load(Ordering::Acquire)).unwrap();
            Ok(Outcome::Cancelled)
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(job);
        assert!(observed.try_recv().unwrap());
    }
}
