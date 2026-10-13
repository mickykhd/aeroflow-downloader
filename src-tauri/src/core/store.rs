use crate::core::{model::{DownloadTask, EngineSettings}, scheduler::WeeklyMatrix};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Everything persisted to `queue.json` (versioned for forward migration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub version: u32,
    pub tasks: Vec<DownloadTask>,
    pub settings: EngineSettings,
    pub scheduler: WeeklyMatrix,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            version: 1,
            tasks: vec![],
            settings: EngineSettings::default(),
            scheduler: WeeklyMatrix::default(),
        }
    }
}

pub fn state_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("queue.json")
}

pub fn load_state(data_dir: &Path) -> PersistedState {
    let p = state_file_path(data_dir);
    if !p.exists() {
        return PersistedState::default();
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => PersistedState::default(),
    }
}

pub fn save_state(data_dir: &Path, state: &PersistedState) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let dest = state_file_path(data_dir);
    let tmp = data_dir.join("queue.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(state).unwrap())?;
    // Windows rename() fails when the destination exists — remove first.
    let _ = std::fs::remove_file(&dest);
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

pub fn task_state_path(data_dir: &Path, id: &Uuid) -> PathBuf {
    data_dir.join(format!("{id}.task.json"))
}

pub fn write_task_checkpoint(data_dir: &Path, task: &DownloadTask) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let p = task_state_path(data_dir, &task.id);
    std::fs::write(p, serde_json::to_string_pretty(task).unwrap())?;
    Ok(())
}

pub fn remove_task_checkpoint(data_dir: &Path, id: &Uuid) {
    let _ = std::fs::remove_file(task_state_path(data_dir, id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = PersistedState::default();
        s.settings.max_threads_per_task = 16;
        save_state(dir.path(), &s).unwrap();
        let loaded = load_state(dir.path());
        assert_eq!(loaded.settings.max_threads_per_task, 16);
        let missing = load_state(Path::new("definitely/not/here"));
        assert!(missing.tasks.is_empty());
    }

    #[test]
    fn overwrite_twice_windows_safe() {
        // Regression: Windows rename() refuses to overwrite — save must
        // succeed on every write, not just the first.
        let dir = tempfile::tempdir().unwrap();
        let mut s = PersistedState::default();
        save_state(dir.path(), &s).unwrap();
        s.settings.max_threads_per_task = 8;
        save_state(dir.path(), &s).unwrap();
        assert_eq!(load_state(dir.path()).settings.max_threads_per_task, 8);
    }
}
