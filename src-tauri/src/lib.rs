//! AeroFlow Tauri bridge — exposes the Tokio engine to the Glassmorphic UI.

mod core;

use core::{
    downloader::DownloadManager,
    hash,
    model::{DownloadTask, EngineSettings, EngineStats, NewDownloadRequest},
    proxy::{ProxyNode, ProxyPool},
    scheduler::{SlotMode, WeeklyMatrix},
    store,
};
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, State};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct AppState {
    pub engine: DownloadManager,
    pub data_dir: PathBuf,
    pub proxies: Mutex<Vec<ProxyNode>>,
    pub scheduler: Mutex<WeeklyMatrix>,
}

impl AppState {
    async fn persist(&self) -> Result<(), String> {
        let tasks = self.engine.list().await;
        let settings = self.engine.settings.read().await.clone();
        let scheduler = self.scheduler.lock().await.clone();
        let state = store::PersistedState { version: 1, tasks, settings, scheduler };
        // save_state is blocking file IO: run it on the blocking pool.
        let data_dir = self.data_dir.clone();
        tokio::task::spawn_blocking(move || store::save_state(&data_dir, &state))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
}

// ---------- Queue commands ----------

#[tauri::command]
async fn get_queue(state: State<'_, Arc<AppState>>) -> Result<Vec<DownloadTask>, String> {
    Ok(state.engine.list().await)
}

#[tauri::command]
async fn add_download(state: State<'_, Arc<AppState>>, req: NewDownloadRequest) -> Result<DownloadTask, String> {
    let task = state.engine.add(req.url, req.dest_dir, req.connections, req.sha256, req.start_paused.unwrap_or(false)).await?;
    state.persist().await?;
    Ok(task)
}

#[tauri::command]
async fn pause_download(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state.engine.pause(id).await?;
    state.persist().await?;
    Ok(())
}

#[tauri::command]
async fn resume_download(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state.engine.resume(id).await?;
    Ok(())
}

#[tauri::command]
async fn remove_download(state: State<'_, Arc<AppState>>, id: String, delete_file: bool) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state.engine.remove(id, delete_file).await?;
    store::remove_task_checkpoint(&state.data_dir, &id);
    state.persist().await?;
    Ok(())
}

#[tauri::command]
async fn pause_all(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    for t in state.engine.list().await {
        let _ = state.engine.pause(t.id).await;
    }
    state.persist().await?;
    Ok(())
}

#[tauri::command]
async fn resume_all(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    for t in state.engine.list().await {
        if t.status == core::DownloadStatus::Paused || t.status == core::DownloadStatus::Queued {
            let _ = state.engine.resume(t.id).await;
        }
    }
    Ok(())
}

// ---------- Stats / storage / hash ----------

#[tauri::command]
async fn get_stats(state: State<'_, Arc<AppState>>) -> Result<EngineStats, String> {
    let tasks = state.engine.list().await;
    let mut s = EngineStats::default();
    for t in &tasks {
        match t.status {
            core::DownloadStatus::Active | core::DownloadStatus::Verifying => {
                s.active += 1;
                s.down_bps += t.speed_bps;
            }
            core::DownloadStatus::Queued | core::DownloadStatus::Paused => s.queued += 1,
            core::DownloadStatus::Completed => s.completed += 1,
            core::DownloadStatus::Failed => {}
        }
    }
    s.total_session_bytes = state.engine.session_bytes.load(std::sync::atomic::Ordering::Relaxed);
    Ok(s)
}

#[tauri::command]
async fn verify_hash(path: String, expected: String) -> Result<bool, String> {
    let actual = hash::sha256_file(std::path::Path::new(&path)).await.map_err(|e| e.to_string())?;
    Ok(hash::verify(&expected, &actual))
}

#[tauri::command]
async fn hash_file(path: String) -> Result<String, String> {
    hash::sha256_file(std::path::Path::new(&path)).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn probe_url(url: String) -> Result<serde_json::Value, String> {
    let (total, ranges, ctype) = core::downloader::probe_url(&url).await?;
    Ok(serde_json::json!({ "totalBytes": total, "supportsRange": ranges, "contentType": ctype }))
}

#[tauri::command]
async fn get_storage(path: String) -> Result<serde_json::Value, String> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    // pick disk containing `path`, else first
    let target = path.to_lowercase();
    let mut best: Option<(u64, u64, String)> = None;
    for d in disks.list() {
        let mount = d.mount_point().to_string_lossy().to_lowercase();
        if target.starts_with(mount.as_str()) || best.is_none() {
            best = Some((d.available_space(), d.total_space(), d.mount_point().to_string_lossy().to_string()));
        }
    }
    let (free, total, mount) = best.unwrap_or((0, 0, String::new()));
    Ok(serde_json::json!({ "free": free, "total": total, "mount": mount, "usedPct": if total>0 { 100 - free*100/total } else {0} }))
}

// ---------- Settings / scheduler / proxies ----------

#[tauri::command]
async fn get_settings(state: State<'_, Arc<AppState>>) -> Result<EngineSettings, String> {
    Ok(state.engine.settings.read().await.clone())
}

#[tauri::command]
async fn update_settings(state: State<'_, Arc<AppState>>, settings: EngineSettings) -> Result<(), String> {
    state.engine.set_global_cap(settings.global_cap_bps).await;
    *state.engine.settings.write().await = settings;
    state.persist().await?;
    Ok(())
}

#[tauri::command]
async fn get_scheduler(state: State<'_, Arc<AppState>>) -> Result<WeeklyMatrix, String> {
    Ok(state.scheduler.lock().await.clone())
}

#[tauri::command]
async fn set_scheduler_slot(state: State<'_, Arc<AppState>>, day: usize, hour: usize, mode: SlotMode) -> Result<(), String> {
    if day > 6 || hour > 23 {
        return Err("day 0-6, hour 0-23".into());
    }
    state.scheduler.lock().await.slots[day][hour] = mode;
    state.persist().await?;
    Ok(())
}

#[tauri::command]
async fn scheduler_now(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let m = state.scheduler.lock().await;
    let (dow, hour, mode) = m.current();
    Ok(serde_json::json!({
        "day": dow, "hour": hour,
        "mode": mode, "label": mode.label(),
        "capBps": mode.cap_bps(),
        "hoursUntilChange": m.hours_until_change(),
    }))
}

#[tauri::command]
async fn get_proxies(state: State<'_, Arc<AppState>>) -> Result<Vec<ProxyNode>, String> {
    Ok(state.proxies.lock().await.clone())
}

#[tauri::command]
async fn add_proxy(state: State<'_, Arc<AppState>>, name: String, endpoint: String, protocol: String) -> Result<Vec<ProxyNode>, String> {
    let mut p = state.proxies.lock().await;
    p.push(ProxyNode::new(&name, &endpoint, &protocol));
    // round-robin sanity check via pool
    let _pool = ProxyPool::new(p.clone());
    Ok(p.clone())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("./data"));
            let _ = std::fs::create_dir_all(&data_dir);
            let persisted = store::load_state(&data_dir);
            let engine = DownloadManager::new(data_dir.clone(), persisted.settings.clone());
            let had_history = !persisted.tasks.is_empty();
            // restore non-terminal tasks as paused (safe restart semantics)
            for mut t in persisted.tasks {
                if t.status.is_active() || t.status == core::DownloadStatus::Queued {
                    t.status = core::DownloadStatus::Paused;
                    t.speed_bps = 0;
                }
                let rt = engine.clone();
                let task = t.clone();
                tauri::async_runtime::block_on(async { rt.restore(task).await });
            }
            let proxies = if !had_history {
                vec![
                    ProxyNode::new("US-East SOCKS5 Turbo", "socks5://104.28.19.44:1080", "SOCKS5"),
                    ProxyNode::new("EU-Central Squid", "http://185.220.101.5:8080", "HTTP/2"),
                    ProxyNode::new("WireGuard Tunnel", "wg://wg0.fastpool.net:51820", "WireGuard"),
                ]
            } else {
                vec![
                    ProxyNode::new("US-East SOCKS5 Turbo", "socks5://104.28.19.44:1080", "SOCKS5"),
                    ProxyNode::new("EU-Central Squid", "http://185.220.101.5:8080", "HTTP/2"),
                ]
            };
            app.manage(Arc::new(AppState {
                engine,
                data_dir,
                proxies: Mutex::new(proxies),
                scheduler: Mutex::new(persisted.scheduler),
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_queue, add_download, pause_download, resume_download, remove_download,
            pause_all, resume_all, get_stats, verify_hash, hash_file, get_storage,
            get_settings, update_settings, get_scheduler, set_scheduler_slot,
            scheduler_now, get_proxies, add_proxy, probe_url
        ]);
    builder.run(tauri::generate_context!()).expect("error while running AeroFlow");
}
