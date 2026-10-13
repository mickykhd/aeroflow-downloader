use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Lifecycle of a single download task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    #[default]
    Queued,
    Active,
    Paused,
    Completed,
    Failed,
    Verifying,
}

impl DownloadStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active | Self::Verifying)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub index: u32,
    pub start: u64,
    pub end: u64, // inclusive
    pub downloaded: u64,
}

impl SegmentState {
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }
    pub fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.downloaded)
    }
    pub fn progress(&self) -> f64 {
        if self.len() == 0 {
            1.0
        } else {
            self.downloaded as f64 / self.len() as f64
        }
    }
}

/// Split `total` bytes into `n` inclusive ranges.
pub fn split_segments(total: u64, n: u32) -> Vec<(u64, u64)> {
    let n = n.max(1) as u64;
    if total == 0 {
        return vec![];
    }
    let base = total / n;
    let rem = total % n;
    let mut out = Vec::with_capacity(n as usize);
    let mut cursor = 0u64;
    for i in 0..n {
        let extra = if i < rem { 1 } else { 0 };
        let len = base + extra;
        if len == 0 {
            break;
        }
        out.push((cursor, cursor + len - 1));
        cursor += len;
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: Uuid,
    pub url: String,
    pub file_name: String,
    pub dest_dir: PathBuf,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
    pub connections: u32,
    pub segments: Vec<SegmentState>,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub error: Option<String>,
    pub sha256_expected: Option<String>,
    pub sha256_actual: Option<String>,
    pub category: String,
    pub created_at: DateTime<Utc>,
    pub supports_range: bool,
}

impl DownloadTask {
    pub fn new(url: String, dest_dir: PathBuf, connections: u32) -> Self {
        let file_name = file_name_from_url(&url);
        Self {
            id: Uuid::new_v4(),
            url,
            file_name: file_name.clone(),
            dest_dir,
            total_bytes: None,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            connections: connections.clamp(1, 64),
            segments: vec![],
            speed_bps: 0,
            eta_secs: None,
            error: None,
            sha256_expected: None,
            sha256_actual: None,
            category: auto_category(&file_name),
            created_at: Utc::now(),
            supports_range: false,
        }
    }

    pub fn dest_path(&self) -> PathBuf {
        self.dest_dir.join(&self.file_name)
    }

    pub fn progress(&self) -> f64 {
        match self.total_bytes {
            Some(0) | None => {
                if self.status == DownloadStatus::Completed {
                    1.0
                } else {
                    0.0
                }
            }
            Some(total) => (self.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0),
        }
    }
}

pub fn file_name_from_url(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download.bin")
        .to_string()
}

pub fn auto_category(file_name: &str) -> String {
    let lower = file_name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "iso" | "img" | "exe" | "msi" | "dmg" => "Software / OS".into(),
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" => "Archives".into(),
        "mp4" | "mkv" | "mov" | "webm" | "mp3" | "flac" | "wav" => "Media Vault".into(),
        "torrent" | "magnet" => "Torrents".into(),
        _ => "General".into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDownloadRequest {
    pub url: String,
    pub dest_dir: Option<String>,
    pub connections: Option<u32>,
    pub sha256: Option<String>,
    pub start_paused: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub down_bps: u64,
    pub up_bps: u64,
    pub active: usize,
    pub queued: usize,
    pub completed: usize,
    pub total_session_bytes: u64,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            down_bps: 0,
            up_bps: 0,
            active: 0,
            queued: 0,
            completed: 0,
            total_session_bytes: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSettings {
    pub max_threads_per_task: u32,
    pub max_concurrent_tasks: u32,
    pub global_cap_bps: Option<u64>, // None = unlimited (Turbo)
    pub per_task_cap_bps: Option<u64>,
    pub auto_verify_sha256: bool,
    pub auto_extract: bool,
    pub start_immediately: bool,
    pub default_dest: String,
    // ---- Global Engine & Preferences (settings page) ----
    #[serde(default)] pub launch_on_startup: bool,
    #[serde(default = "d_true")] pub start_minimized: bool,
    #[serde(default = "d_true")] pub prevent_sleep: bool,
    #[serde(default = "d_true")] pub shell_context_menu: bool,
    #[serde(default = "d_true")] pub clipboard_autodetect: bool,
    #[serde(default = "d_cyan")] pub accent_colorway: String,
    #[serde(default = "d_85")] pub dropzone_opacity: u8,
    #[serde(default = "d_true")] pub tray_speed_meter: bool,
    #[serde(default = "d_temp_cache")] pub temp_cache_dir: String,
    #[serde(default = "d_256")] pub buffer_cache_mb: u32,
    #[serde(default = "d_true")] pub sparse_prealloc: bool,
    #[serde(default = "d_true")] pub directstorage_bypass: bool,
    #[serde(default = "d_true")] pub auto_trim: bool,
    #[serde(default = "d_true")] pub auto_relocate: bool,
    #[serde(default = "d_6881")] pub bt_port: u16,
    #[serde(default = "d_true")] pub bt_upnp: bool,
    #[serde(default)] pub bt_random_port: bool,
    #[serde(default = "d_true")] pub dht_enabled: bool,
    #[serde(default = "d_true")] pub pex_enabled: bool,
    #[serde(default = "d_true")] pub lsd_enabled: bool,
    #[serde(default = "d_true")] pub bt_encryption: bool,
    #[serde(default = "d_500")] pub max_peer_connections: u32,
    #[serde(default = "d_80")] pub max_connections_per_torrent: u32,
    #[serde(default = "d_8")] pub upload_slots: u32,
    #[serde(default = "d_ratio")] pub seed_ratio_stop: f64,
    #[serde(default = "d_true")] pub seed_ratio_enabled: bool,
    #[serde(default = "d_true")] pub seed_time_enabled: bool,
    #[serde(default = "d_pause")] pub seed_action: String,
    #[serde(default = "d_true")] pub hls_sniff: bool,
    #[serde(default = "d_15")] pub intercept_min_mb: u32,
    #[serde(default = "d_true")] pub quarantine_corrupt: bool,
    #[serde(default = "d_true")] pub av_scan_enabled: bool,
    #[serde(default = "d_av_path")] pub av_path: String,
    #[serde(default = "d_av_args")] pub av_args: String,
    #[serde(default = "d_true")] pub tls_strict: bool,
    #[serde(default = "d_true")] pub doh_enabled: bool,
    #[serde(default = "d_cloudflare")] pub doh_provider: String,
    #[serde(default = "d_true")] pub toast_on_complete: bool,
    #[serde(default = "d_true")] pub alert_on_failure: bool,
    #[serde(default = "d_click")] pub sound_pack: String,
    #[serde(default = "d_70")] pub sound_volume: u8,
    #[serde(default = "d_true")] pub discord_webhook_enabled: bool,
    #[serde(default)] pub discord_webhook: String,
    #[serde(default)] pub telegram_enabled: bool,
    #[serde(default)] pub telegram_token: String,
    #[serde(default)] pub telegram_chat: String,
}

fn d_true() -> bool { true }
fn d_cyan() -> String { "cyan".into() }
fn d_85() -> u8 { 85 }
fn d_70() -> u8 { 70 }
fn d_256() -> u32 { 256 }
fn d_6881() -> u16 { 6881 }
fn d_500() -> u32 { 500 }
fn d_80() -> u32 { 80 }
fn d_8() -> u32 { 8 }
fn d_15() -> u32 { 15 }
fn d_ratio() -> f64 { 1.5 }
fn d_pause() -> String { "Pause Task".into() }
fn d_click() -> String { "Cyberpunk Mechanical Click".into() }
fn d_cloudflare() -> String { "Cloudflare 1.1.1.1".into() }
fn d_av_path() -> String { "C:\\Program Files\\Windows Defender\\MpCmdRun.exe".into() }
fn d_av_args() -> String { "-Scan -ScanType 3 -File \"%FILE_PATH%\"".into() }
fn d_temp_cache() -> String { "D:\\Downloads\\.aeroflow_temp_cache".into() }

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            max_threads_per_task: 32,
            max_concurrent_tasks: 4,
            global_cap_bps: None,
            per_task_cap_bps: None,
            auto_verify_sha256: true,
            auto_extract: false,
            start_immediately: true,
            default_dest: default_download_dir(),
            temp_cache_dir: d_temp_cache(),
            launch_on_startup: false,
            start_minimized: true,
            prevent_sleep: true,
            shell_context_menu: true,
            clipboard_autodetect: true,
            accent_colorway: d_cyan(),
            dropzone_opacity: 85,
            tray_speed_meter: true,
            buffer_cache_mb: 256,
            sparse_prealloc: true,
            directstorage_bypass: true,
            auto_trim: true,
            auto_relocate: true,
            bt_port: 6881,
            bt_upnp: true,
            bt_random_port: false,
            dht_enabled: true,
            pex_enabled: true,
            lsd_enabled: true,
            bt_encryption: true,
            max_peer_connections: 500,
            max_connections_per_torrent: 80,
            upload_slots: 8,
            seed_ratio_stop: 1.5,
            seed_ratio_enabled: true,
            seed_time_enabled: true,
            seed_action: d_pause(),
            hls_sniff: true,
            intercept_min_mb: 15,
            quarantine_corrupt: true,
            av_scan_enabled: true,
            av_path: d_av_path(),
            av_args: d_av_args(),
            tls_strict: true,
            doh_enabled: true,
            doh_provider: d_cloudflare(),
            toast_on_complete: true,
            alert_on_failure: true,
            sound_pack: d_click(),
            sound_volume: 70,
            discord_webhook_enabled: true,
            discord_webhook: String::new(),
            telegram_enabled: false,
            telegram_token: String::new(),
            telegram_chat: String::new(),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn default_download_dir() -> String {
    std::env::var("USERPROFILE")
        .map(|h| format!("{h}\\Downloads\\AeroFlow"))
        .unwrap_or_else(|_| "D:\\Downloads\\AeroFlow".into())
}

#[cfg(not(target_os = "windows"))]
pub fn default_download_dir() -> String {
    std::env::var("HOME")
        .map(|h| format!("{h}/Downloads/AeroFlow"))
        .unwrap_or_else(|_| "./downloads".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cover_full_range() {
        for (total, n) in [(100, 4), (1024, 32), (5, 8), (1, 1), (1000, 3)] {
            let segs = split_segments(total, n);
            let covered: u64 = segs.iter().map(|(s, e)| e - s + 1).sum();
            assert_eq!(covered, total, "total={total} n={n}");
            assert_eq!(segs[0].0, 0);
            assert_eq!(segs.last().unwrap().1, total - 1);
        }
    }

    #[test]
    fn category_detection() {
        assert_eq!(auto_category("ubuntu.iso"), "Software / OS");
        assert_eq!(auto_category("movie.mkv"), "Media Vault");
        assert_eq!(auto_category("a.zip"), "Archives");
    }

    #[test]
    fn camel_case_request_contract() {
        // What the web UI sends via invoke("add_download", { req: {...} }).
        let req: NewDownloadRequest = serde_json::from_str(
            r#"{"url":"https://example.com/f.iso","destDir":"D:\\DL","connections":16,"sha256":null,"startPaused":true}"#,
        )
        .expect("frontend camelCase request must deserialize");
        assert_eq!(req.dest_dir.as_deref(), Some("D:\\DL"));
        assert_eq!(req.start_paused, Some(true));
    }

    #[test]
    fn camel_case_settings_contract() {
        let s: EngineSettings = serde_json::from_str(
            r#"{"maxThreadsPerTask":16,"maxConcurrentTasks":2,"globalCapBps":15000000,"perTaskCapBps":null,"autoVerifySha256":false,"autoExtract":true,"startImmediately":true,"defaultDest":"D:\\DL"}"#,
        )
        .expect("frontend camelCase settings must deserialize");
        assert_eq!(s.max_threads_per_task, 16);
        assert_eq!(s.global_cap_bps, Some(15_000_000));
        assert!(!s.auto_verify_sha256);
    }

    #[test]
    fn legacy_settings_deserialize_with_defaults() {
        // Old persisted queue.json lacks the settings-page fields — every
        // new field must fall back to its default, not fail the load.
        let s: EngineSettings = serde_json::from_str(
            r#"{"maxThreadsPerTask":32,"maxConcurrentTasks":4,"globalCapBps":null,"perTaskCapBps":null,"autoVerifySha256":true,"autoExtract":false,"startImmediately":true,"defaultDest":"D:\\DL"}"#,
        )
        .expect("legacy settings must deserialize");
        assert!(s.prevent_sleep);
        assert!(s.clipboard_autodetect);
        assert_eq!(s.accent_colorway, "cyan");
        assert_eq!(s.dropzone_opacity, 85);
        assert_eq!(s.bt_port, 6881);
        assert_eq!(s.sound_volume, 70);
        assert!(!s.telegram_enabled);
        assert_eq!(s.seed_ratio_stop, 1.5);
    }
}
