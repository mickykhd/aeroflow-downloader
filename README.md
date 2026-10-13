# AeroFlow Downloader v4.2 PRO — Rust + Tauri v2 (Windows)

Glassmorphic Precision desktop download manager, rebuilt from the `one-download-manager` design mocks
(`DESIGN.md` + 5 `code.html` screens) as a native Windows app.

## Design fidelity

| Mock | Title | Implemented as |
|---|---|---|
| `New folder (3)` | HyperStream Pro (queue) | **Tasks** view — 48px rows, cyan gradient progress, JetBrains Mono telemetry |
| `New folder (4)` | Speed Engine & Scheduler | **Speed Engine** view — 32-thread slider, socket map, QoS caps |
| `New folder (2)` | Scheduler & Bandwidth Cron | **Scheduler** view — paintable 7×24 matrix, post-task triggers |
| `New folder` | Connections & Proxy Matrix | **Connections** view — proxy pools, TCP tuning, DoH |
| `New folder (5)` | Add URL / Batch Importer | **+ Add URL modal** — clipboard batch, storage target, SHA-256 |

Theme tokens: `bg #0C0E14`, panels `#141824/#1C2234/#262F47`, cyan `#00F0FF`,
blue `#3B82F6`, emerald `#10B981`, amber `#F59E0B`, red `#EF4444`.
Fonts: Geist (UI) + JetBrains Mono (telemetry). Shell: 40px topbar, 220px sidebar, 320px inspector.

## Architecture

```text
aeroflow/
├── src/                    # Web UI (no build step — Tauri frontendDist)
│   ├── index.html          # SPA: 4 views + batch modal
│   ├── styles.css          # DESIGN.md tokens
│   └── main.js             # @tauri-apps/api bridge + mock fallback
└── src-tauri/
    ├── Cargo.toml / tauri.conf.json / build.rs
    └── src/
        ├── main.rs         # windows_subsystem entry
        ├── lib.rs          # Tauri commands + AppState
        └── core/
            ├── model.rs        # task model, split_segments, categories
            ├── downloader.rs   # Tokio multi-segment engine (Range, .part, concat)
            ├── throttle.rs     # token-bucket global cap
            ├── scheduler.rs    # 7×24 SlotMode matrix
            ├── hash.rs         # streaming SHA-256
            ├── proxy.rs        # round-robin pool
            └── store.rs        # queue.json persistence
```

Engine highlights:
- **Turbo multi-segment**: HEAD probe → `split_segments(total, 32)` → N `Range` workers → `.part{i}` → concat. Single-stream fallback with resume.
- **Pause/resume**: per-task `AtomicBool` cancel flag, checkpoint JSON in `%APPDATA%/com.aeroflow.downloader`.
- **Throttle**: token-bucket `Throttler` (`None` = Turbo unlimited), 100ms pacing.
- **Scheduler**: `WeeklyMatrix` defaults mirror the mock (work-hours 15 MB/s, nights Turbo, Sun PM sleep).
- **Integrity**: background SHA-256 + `HASH MATCHED` panel.
- **Windows**: NSIS bundle, WebView2, `sysinfo` disk telemetry, UPnP-mapped status bar.

## Prerequisites (Windows 11)

- Rust stable MSVC (`rustup show` → `x86_64-pc-windows-msvc`)
- Node 20+ (only for `tauri dev/build`; UI itself needs no bundler)
- WebView2 (preinstalled on Win11)

## Run

```powershell
# 1. Backend check + unit tests (no Node needed)
cargo test -p aeroflow-downloader
cargo check -p aeroflow-downloader

# 2. Desktop app (from aeroflow/)
npm install
npm run tauri dev

# 3. Installer
npm run tauri build   # → src-tauri/target/release/bundle/nsis/*.exe
```

Preview UI without Rust: open `src/index.html` directly — `main.js` falls back to mock telemetry.

## Tauri commands (frontend ↔ Rust)

`get_queue`, `add_download`, `pause_download`, `resume_download`, `remove_download`,
`pause_all`, `resume_all`, `get_stats`, `verify_hash`, `hash_file`, `get_storage`,
`get_settings`, `update_settings`, `get_scheduler`, `set_scheduler_slot`,
`scheduler_now`, `get_proxies`, `add_proxy`.

## Roadmap

- [ ] SQLite (rusqlite) history + search index
- [ ] Browser extension (native-messaging host on 127.0.0.1:6800)
- [ ] BitTorrent (librqbit) + HTTP/3 (quiche) workers
- [ ] Auto-extract (zip/tar/7z) + AV scan hook
- [ ] Winget manifest + auto-update (tauri-plugin-updater)
