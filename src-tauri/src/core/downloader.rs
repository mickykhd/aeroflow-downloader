use crate::core::{
    hash,
    model::{split_segments, DownloadStatus, DownloadTask, EngineSettings},
    throttle::Throttler,
};
use futures_util::StreamExt;
use reqwest::header;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Shared, cloneable download engine.
#[derive(Debug, Clone)]
pub struct DownloadManager {
    tasks: Arc<RwLock<HashMap<Uuid, DownloadTask>>>,
    cancel: Arc<RwLock<HashMap<Uuid, Arc<AtomicBool>>>>,
    client: reqwest::Client,
    pub data_dir: PathBuf,
    pub settings: Arc<RwLock<EngineSettings>>,
    pub throttler: Arc<Throttler>,
    pub session_bytes: Arc<std::sync::atomic::AtomicU64>,
}

impl DownloadManager {
    pub fn new(data_dir: PathBuf, settings: EngineSettings) -> Self {
        let cap = settings.global_cap_bps;
        let client = reqwest::Client::builder()
            .user_agent("AeroFlow-Downloader/4.2 (Windows; Turbo-Engine)")
            .build()
            .expect("reqwest client");
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            cancel: Arc::new(RwLock::new(HashMap::new())),
            client,
            data_dir,
            settings: Arc::new(RwLock::new(settings)),
            throttler: Arc::new(Throttler::new(cap)),
            session_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub async fn set_global_cap(&self, cap: Option<u64>) {
        self.throttler.set_rate(cap).await;
        self.settings.write().await.global_cap_bps = cap;
    }

    pub async fn list(&self) -> Vec<DownloadTask> {
        let mut v: Vec<_> = self.tasks.read().await.values().cloned().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    pub async fn get(&self, id: Uuid) -> Option<DownloadTask> {
        self.tasks.read().await.get(&id).cloned()
    }

    pub async fn restore(&self, task: DownloadTask) {
        self.tasks.write().await.insert(task.id, task);
    }

    pub async fn add(
        &self,
        url: String,
        dest_dir: Option<String>,
        connections: Option<u32>,
        sha256: Option<String>,
        start_paused: bool,
    ) -> Result<DownloadTask, String> {
        url::Url::parse(&url).map_err(|e| format!("Invalid URL: {e}"))?;
        let settings = self.settings.read().await.clone();
        let dest = dest_dir.unwrap_or_else(|| settings.default_dest.clone());
        let mut task = DownloadTask::new(url, PathBuf::from(dest), connections.unwrap_or(settings.max_threads_per_task));
        task.sha256_expected = sha256.filter(|s| !s.trim().is_empty());
        if start_paused || !settings.start_immediately {
            task.status = DownloadStatus::Paused;
        }
        let id = task.id;
        std::fs::create_dir_all(&task.dest_dir).map_err(|e| format!("Cannot create folder: {e}"))?;
        self.tasks.write().await.insert(id, task.clone());
        self.cancel.write().await.insert(id, Arc::new(AtomicBool::new(false)));
        if task.status != DownloadStatus::Paused {
            self.spawn(id);
        }
        Ok(task)
    }

    pub async fn pause(&self, id: Uuid) -> Result<(), String> {
        let flag = self.cancel.read().await.get(&id).cloned().ok_or("Task not found")?;
        flag.store(true, Ordering::SeqCst);
        if let Some(t) = self.tasks.write().await.get_mut(&id) {
            if t.status.is_active() || t.status == DownloadStatus::Queued {
                t.status = DownloadStatus::Paused;
            }
        }
        Ok(())
    }

    pub async fn resume(&self, id: Uuid) -> Result<(), String> {
        let exists = self.tasks.read().await.contains_key(&id);
        if !exists {
            return Err("Task not found".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel.write().await.insert(id, flag);
        if let Some(t) = self.tasks.write().await.get_mut(&id) {
            t.error = None;
            if t.status != DownloadStatus::Completed {
                t.status = DownloadStatus::Queued;
            }
        }
        self.spawn(id);
        Ok(())
    }

    pub async fn remove(&self, id: Uuid, delete_file: bool) -> Result<(), String> {
        self.pause(id).await.ok();
        let task = self.tasks.write().await.remove(&id).ok_or("Task not found")?;
        self.cancel.write().await.remove(&id);
        // clean part files
        for i in 0..task.connections.max(1) {
            let _ = tokio::fs::remove_file(part_path(&task.dest_path(), i)).await;
        }
        if delete_file {
            let _ = tokio::fs::remove_file(task.dest_path()).await;
        }
        Ok(())
    }

    fn spawn(&self, id: Uuid) {
        let me = self.clone();
        tokio::spawn(async move {
            me.run(id).await;
        });
    }

    async fn set_status(&self, id: Uuid, s: DownloadStatus) {
        if let Some(t) = self.tasks.write().await.get_mut(&id) {
            t.status = s;
        }
    }

    async fn fail(&self, id: Uuid, msg: String) {
        if let Some(t) = self.tasks.write().await.get_mut(&id) {
            t.status = DownloadStatus::Failed;
            t.error = Some(msg);
        }
    }

    fn cancelled(&self, id: Uuid) -> bool {
        // blocking read via try_read to avoid async in hot loop; fallback false
        self.cancel.try_read().map(|m| m.get(&id).map(|f| f.load(Ordering::SeqCst)).unwrap_or(true)).unwrap_or(false)
    }

    async fn run(&self, id: Uuid) {
        let snapshot = match self.get(id).await {
            Some(t) => t,
            None => return,
        };
        if snapshot.status == DownloadStatus::Completed {
            return;
        }
        self.set_status(id, DownloadStatus::Active).await;

        // 1. Probe total size + range support
        let (total, range_ok) = self.probe(&snapshot.url).await;
        {
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.total_bytes = total;
                t.supports_range = range_ok;
            }
        }

        let task = match self.get(id).await {
            Some(t) => t,
            None => return,
        };

        let dest = task.dest_path();
        if let Some(parent) = dest.parent() {
            if tokio::fs::create_dir_all(parent).await.is_err() {
                self.fail(id, "Cannot create destination folder".into()).await;
                return;
            }
        }

        let use_segments = range_ok && total.map(|t| t > 1_048_576).unwrap_or(false) && task.connections > 1;
        let result = if use_segments {
            let total = total.unwrap_or(0);
            self.download_segmented(id, &task, total).await
        } else {
            self.download_single(id, &task).await
        };

        if let Err(e) = result {
            if self.cancelled(id) {
                self.set_status(id, DownloadStatus::Paused).await;
            } else {
                self.fail(id, e).await;
            }
            return;
        }
        if self.cancelled(id) {
            self.set_status(id, DownloadStatus::Paused).await;
            return;
        }

        // 2. Optional SHA-256 verification
        let expected = self.get(id).await.and_then(|t| t.sha256_expected.clone());
        let auto = self.settings.read().await.auto_verify_sha256;
        if let Some(exp) = expected.filter(|s| !s.is_empty()) {
            self.set_status(id, DownloadStatus::Verifying).await;
            match hash::sha256_file(&dest).await {
                Ok(actual) => {
                    let ok = hash::verify(&exp, &actual);
                    if let Some(t) = self.tasks.write().await.get_mut(&id) {
                        t.sha256_actual = Some(actual);
                        t.status = if ok { DownloadStatus::Completed } else { DownloadStatus::Failed };
                        if !ok {
                            t.error = Some("SHA-256 mismatch — file may be corrupt".into());
                        } else {
                            t.downloaded_bytes = t.total_bytes.unwrap_or(t.downloaded_bytes);
                        }
                    }
                }
                Err(e) => {
                    self.fail(id, format!("Hash read failed: {e}")).await;
                    return;
                }
            }
        } else if auto && total.map(|t| t > 0).unwrap_or(false) {
            // background hash for the Storage & Crypto panel (non-blocking UX)
            if let Ok(actual) = hash::sha256_file(&dest).await {
                if let Some(t) = self.tasks.write().await.get_mut(&id) {
                    t.sha256_actual = Some(actual);
                }
            }
            self.set_status(id, DownloadStatus::Completed).await;
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.downloaded_bytes = t.total_bytes.unwrap_or(t.downloaded_bytes);
            }
        } else {
            self.set_status(id, DownloadStatus::Completed).await;
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.downloaded_bytes = t.total_bytes.unwrap_or(t.downloaded_bytes);
            }
        }
        let _ = crate::core::store::write_task_checkpoint(&self.data_dir, &self.get(id).await.unwrap());
    }

    async fn probe(&self, url: &str) -> (Option<u64>, bool) {
        // HEAD first, fall back to ranged GET(0-0)
        if let Ok(resp) = self.client.head(url).send().await {
            let len = resp.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<u64>().ok());
            let ranges = resp.headers().get(header::ACCEPT_RANGES).map(|v| v.as_bytes() != b"none").unwrap_or(false);
            if len.is_some() {
                return (len, ranges);
            }
        }
        match self.client.get(url).header(header::RANGE, "bytes=0-0").send().await {
            Ok(resp) => {
                let cr = resp.headers().get(header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
                let total = cr.as_deref().and_then(parse_content_range_total);
                let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
                (total, partial)
            }
            Err(_) => (None, false),
        }
    }

    /// Single-connection download with resume via existing `.part` length.
    async fn download_single(&self, id: Uuid, task: &DownloadTask) -> Result<(), String> {
        let dest = task.dest_path();
        let tmp = dest.with_extension("aeroflow.part");
        let resume_from = tokio::fs::metadata(&tmp).await.map(|m| m.len()).unwrap_or(0);
        let mut req = self.client.get(&task.url);
        if resume_from > 0 {
            req = req.header(header::RANGE, format!("bytes={resume_from}-"));
        }
        let resp = req.send().await.map_err(|e| format!("Request failed: {e}"))?;
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(format!("HTTP {}", resp.status()));
        }
        let mut file = tokio::fs::OpenOptions::new().create(true).append(resume_from > 0).write(true).truncate(resume_from == 0).open(&tmp).await.map_err(|e| format!("File open: {e}"))?;
        let mut stream = resp.bytes_stream();
        let mut downloaded = resume_from;
        let started = Instant::now();
        let mut last_tick = Instant::now();
        let mut tick_bytes = 0u64;
        {
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.downloaded_bytes = downloaded;
            }
        }
        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            if self.cancelled(id) {
                break;
            }
            let bytes = chunk.map_err(|e| format!("Stream error: {e}"))?;
            if bytes.is_empty() {
                continue;
            }
            self.throttler.acquire(bytes.len()).await;
            file.write_all(&bytes).await.map_err(|e| format!("Write: {e}"))?;
            downloaded += bytes.len() as u64;
            tick_bytes += bytes.len() as u64;
            self.session_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            if last_tick.elapsed().as_millis() >= 250 {
                let elapsed = started.elapsed().as_secs_f64().max(0.1);
                let speed = (downloaded as f64 / elapsed) as u64;
                let eta = {
                    let t = self.tasks.read().await;
                    t.get(&id).and_then(|t| t.total_bytes).map(|total| total.saturating_sub(downloaded).saturating_div(speed.max(1)))
                };
                if let Some(t) = self.tasks.write().await.get_mut(&id) {
                    t.downloaded_bytes = downloaded;
                    t.speed_bps = tick_bytes * 1000 / last_tick.elapsed().as_millis().max(1) as u64;
                    t.eta_secs = eta;
                    let _ = speed;
                }
                last_tick = Instant::now();
                tick_bytes = 0;
            }
        }
        file.flush().await.map_err(|e| format!("Flush: {e}"))?;
        drop(file);
        if self.cancelled(id) {
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.downloaded_bytes = downloaded;
            }
            return Ok(());
        }
        tokio::fs::rename(&tmp, &dest).await.map_err(|e| format!("Finalize: {e}"))?;
        if let Some(t) = self.tasks.write().await.get_mut(&id) {
            t.downloaded_bytes = tokio::fs::metadata(&dest).await.map(|m| m.len()).unwrap_or(downloaded);
            t.speed_bps = 0;
            t.eta_secs = Some(0);
        }
        Ok(())
    }

    /// Turbo multi-segment: N ranged workers → `.part{i}` → concat.
    async fn download_segmented(&self, id: Uuid, task: &DownloadTask, total: u64) -> Result<(), String> {
        let n = (task.connections as u64).min(64).max(2) as u32;
        let ranges = split_segments(total, n);
        {
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.segments = ranges.iter().enumerate().map(|(i, (s, e))| {
                    let existing = std::fs::metadata(part_path(&t.dest_path(), i as u32)).map(|m| m.len()).unwrap_or(0);
                    crate::core::SegmentState { index: i as u32, start: *s, end: *e, downloaded: existing.min(e - s + 1) }
                }).collect();
                t.downloaded_bytes = t.segments.iter().map(|s| s.downloaded).sum();
            }
        }
        let mut handles = Vec::new();
        for (i, (start, end)) in ranges.into_iter().enumerate() {
            let me = self.clone();
            let url = task.url.clone();
            let part = part_path(&task.dest_path(), i as u32);
            let already: u64 = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
            let seg_len = end - start + 1;
            let from = start + already.min(seg_len);
            if from > end {
                continue;
            }
            handles.push(tokio::spawn(async move {
                me.fetch_range(id, &url, i as u32, from, end, &part).await
            }));
        }
        let mut first_err: Option<String> = None;
        for h in handles {
            match h.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(format!("Join: {e}"));
                    }
                }
            }
        }
        if let Some(e) = first_err {
            if !self.cancelled(id) {
                return Err(e);
            }
            return Ok(());
        }
        if self.cancelled(id) {
            return Ok(());
        }
        // concat parts
        let dest = task.dest_path();
        let tmp = dest.with_extension("aeroflow.part");
        let mut out = tokio::fs::File::create(&tmp).await.map_err(|e| format!("Concat create: {e}"))?;
        use tokio::io::AsyncWriteExt;
        for i in 0..n {
            let part = part_path(&dest, i);
            if !part.exists() {
                continue;
            }
            let mut f = tokio::fs::File::open(&part).await.map_err(|e| format!("Concat read: {e}"))?;
            tokio::io::copy(&mut f, &mut out).await.map_err(|e| format!("Concat copy: {e}"))?;
        }
        out.flush().await.map_err(|e| format!("Concat flush: {e}"))?;
        drop(out);
        tokio::fs::rename(&tmp, &dest).await.map_err(|e| format!("Finalize: {e}"))?;
        for i in 0..n {
            let _ = tokio::fs::remove_file(part_path(&dest, i)).await;
        }
        if let Some(t) = self.tasks.write().await.get_mut(&id) {
            t.downloaded_bytes = total;
            t.speed_bps = 0;
            t.eta_secs = Some(0);
        }
        Ok(())
    }

    async fn fetch_range(&self, id: Uuid, url: &str, index: u32, from: u64, end: u64, part: &Path) -> Result<(), String> {
        let resp = self.client.get(url).header(header::RANGE, format!("bytes={from}-{end}")).send().await.map_err(|e| format!("Seg {index}: {e}"))?;
        if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT && resp.status() != reqwest::StatusCode::OK {
            return Err(format!("Seg {index}: HTTP {}", resp.status()));
        }
        let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(part).await.map_err(|e| format!("Seg {index} open: {e}"))?;
        use tokio::io::AsyncWriteExt;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if self.cancelled(id) {
                break;
            }
            let bytes = chunk.map_err(|e| format!("Seg {index} stream: {e}"))?;
            self.throttler.acquire(bytes.len()).await;
            file.write_all(&bytes).await.map_err(|e| format!("Seg {index} write: {e}"))?;
            self.session_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            if let Some(t) = self.tasks.write().await.get_mut(&id) {
                t.downloaded_bytes += bytes.len() as u64;
                if let Some(s) = t.segments.iter_mut().find(|s| s.index == index) {
                    s.downloaded += bytes.len() as u64;
                }
            }
        }
        file.flush().await.map_err(|e| format!("Seg {index} flush: {e}"))?;
        Ok(())
    }
}

pub fn part_path(dest: &Path, index: u32) -> PathBuf {
    let mut name = dest.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    name.push(format!(".part{index}"));
    dest.with_file_name(name)
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    // "bytes 0-0/12345"
    value.rsplit('/').next()?.trim().parse::<u64>().ok()
}

/// Standalone URL probe for the Download File Info dialog:
/// returns (total bytes, range support, content type).
pub async fn probe_url(url: &str) -> Result<(Option<u64>, bool, Option<String>), String> {
    let client = reqwest::Client::builder()
        .user_agent("AeroFlow-Downloader/4.2 (Windows; Turbo-Engine)")
        .build()
        .map_err(|e| e.to_string())?;
    if let Ok(resp) = client.head(url).send().await {
        let len = resp
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        let ranges = resp
            .headers()
            .get(header::ACCEPT_RANGES)
            .map(|v| v.as_bytes() != b"none")
            .unwrap_or(false);
        let ctype = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if len.is_some() {
            return Ok((len, ranges, ctype));
        }
    }
    match client.get(url).header(header::RANGE, "bytes=0-0").send().await {
        Ok(resp) => {
            let cr = resp
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let total = cr
                .as_deref()
                .and_then(|v| v.rsplit('/').next())
                .and_then(|p| p.trim().parse::<u64>().ok());
            let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
            let ctype = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            Ok((total, partial, ctype))
        }
        Err(e) => Err(format!("Probe failed: {e}")),
    }
}
