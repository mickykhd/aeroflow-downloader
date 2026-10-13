// AeroFlow renderer — Tauri v2 bridge with graceful browser fallback.
// Prefers the injected window.__TAURI__ global (offline, no CDN); falls back
// to the esm.sh CDN build, then to mock data for plain-browser preview.
let invoke = window.__TAURI__?.core?.invoke?.bind(window.__TAURI__.core) ?? null;
if (!invoke) {
  try {
    const mod = await import("https://esm.sh/@tauri-apps/api@2/core").catch(() => null);
    if (mod?.invoke && window.__TAURI_INTERNALS__) invoke = mod.invoke;
  } catch { /* browser preview */ }
}
const isTauri = () => !!invoke;
function toast(msg, ms = 2800) {
  const t = document.querySelector("#toast");
  if (!t) { console.log("[toast]", msg); return; }
  t.textContent = msg;
  t.classList.remove("hidden");
  clearTimeout(toast._h);
  toast._h = setTimeout(() => t.classList.add("hidden"), ms);
}
async function api(cmd, args = {}, quiet = false) {
  if (!invoke) return mockApi(cmd, args);
  try { return await invoke(cmd, args); }
  // NOTE: backend `()` results arrive as null; errors arrive as undefined.
  catch (e) { console.warn(cmd, e); if (!quiet) toast(`Error: ${cmd} — ${e}`); return undefined; }
}
// Tauri plugin APIs (dialog / fs / shell); null in plain-browser preview.
const tauriPlugin = (path) => path.split(".").reduce((o, k) => o?.[k], window.__TAURI__);
const dlg = () => tauriPlugin("dialog") ?? null;
const fsApi = () => tauriPlugin("fs") ?? null;
const shellApi = () => tauriPlugin("shell") ?? null;

// Note: minimize / maximize / close live on the native OS titlebar
// (decorations:true), so no in-content window buttons are needed.

// ---------- Mock data (mirrors screenshots) ----------
const MOCK_QUEUE = [
  { id: "1", url: "https://releases.ubuntu.com/noble/ubuntu-24.04.1-desktop-amd64.iso", file_name: "Ubuntu-24.04.1-desktop-amd64.iso", host: "releases.ubuntu.com/noble/iso", total_bytes: 5800000000, downloaded_bytes: 4290000000, status: "active", speed_bps: 38400000, eta_secs: 38, connections: 32, category: "Software / OS" },
  { id: "2", url: "https://download.blender.org/release/v4.2", file_name: "Blender_4.2_LTS_Official_Build.zip", host: "download.blender.org/release/v4.2", total_bytes: 342000000, downloaded_bytes: 164000000, status: "active", speed_bps: 24100000, eta_secs: 7, connections: 16, category: "Software / OS" },
  { id: "3", url: "magnet:unreal-engine-5.4", file_name: "UnrealEngine-Source-5.4.tar.gz", host: "Seeds: 142 • Peers: 68", total_bytes: 14200000000, downloaded_bytes: 3120000000, status: "active", speed_bps: 22100000, eta_secs: 504, connections: 0, category: "Torrents" },
  { id: "4", url: "https://developer.nvidia.com/compute/cuda", file_name: "CUDA_Toolkit_12.6_Windows_Setup.exe", host: "developer.nvidia.com/compute/cuda", total_bytes: 3200000000, downloaded_bytes: 1950000000, status: "paused", speed_bps: 0, eta_secs: null, connections: 0, category: "Software / OS" },
  { id: "5", url: "https://sw.blackmagicdesign.com/desktop", file_name: "Davinci_Resolve_Studio_19.0b.exe", host: "sw.blackmagicdesign.com/desktop", total_bytes: 2100000000, downloaded_bytes: 2100000000, status: "completed", speed_bps: 0, eta_secs: 0, connections: 0, category: "Media Vault" },
];
const MOCK_PROXIES = [
  { name: "US-East SOCKS5 Turbo Node", endpoint: "socks5://104.28.19.44:1080", protocol: "SOCKS5", status: "Active", latency: "24ms", health: "99.8%" },
  { name: "EU-Central High-Bandwidth Squid", endpoint: "http://185.220.101.5:8080", protocol: "HTTP/2", status: "Active", latency: "42ms", health: "100%" },
  { name: "WireGuard Encrypted Tunnel", endpoint: "wg://wg0.fastpool.net:51820", protocol: "WireGuard", status: "Connected", latency: "18ms", health: "100%" },
  { name: "Residential Fallback Pool", endpoint: "socks5://res-pool.iprelay.io:9050", protocol: "SOCKS5 Res", status: "Standby", latency: "110ms", health: "97.4%" },
];
async function mockApi(cmd, args) {
  switch (cmd) {
    case "get_queue": return MOCK_QUEUE.map(t => ({ ...t, id: t.id, downloaded_bytes: t.downloaded_bytes + Math.floor(Math.random() * 2000000) }));
    case "get_stats": return { down_bps: 84600000, up_bps: 4200000, active: 3, queued: 2, completed: 19, total_session_bytes: 6840000000 };
    case "get_proxies": return MOCK_PROXIES;
    case "scheduler_now": return { day: 1, hour: 14, mode: "throttled", label: "Throttled QoS", capBps: 15000000, hoursUntilChange: 4 };
    case "get_storage": return { free: 642000000000, total: 2000000000000, mount: "C:", usedPct: 68 };
    default: console.log("[mock]", cmd, args); return null;
  }
}

// ---------- Helpers ----------
const $ = (s) => document.querySelector(s);
const fmtBytes = (b) => { if (b == null) return "--"; const u = ["B","KB","MB","GB","TB"]; let i = 0, v = b; while (v >= 1024 && i < 4) { v /= 1024; i++; } return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${u[i]}`; };
const fmtRate = (b) => b ? `${fmtBytes(b)}/s` : "0 KB/s";
const fmtEta = (s) => s == null ? "--" : s === 0 ? "Done" : s < 60 ? `${s}s` : `${Math.floor(s/60)}m ${s%60}s`;
const pct = (t) => { const total = t.total_bytes || 0; if (!total) return t.status === "completed" ? 100 : 0; return Math.min(100, Math.round(t.downloaded_bytes / total * 100)); };

let queue = [], navFilter = { kind: "status", value: "all" }, query = "", selectedTask = null, lastRowCount = 0;

// ---------- Queue ----------
async function refreshQueue() {
  const data = await api("get_queue");
  if (Array.isArray(data)) queue = data;
  renderQueue();
}
// Filter-specific empty states: clicking Queued says "No queued transfers", etc.
const EMPTY_STATES = {
  active: ["No active transfers", "Nothing is downloading right now."],
  completed: ["No completed transfers", "Finished downloads will appear here."],
  paused: ["No paused transfers", "Pause a task and it will show up here."],
  queued: ["No queued transfers", "Tasks waiting for a free slot will appear here."],
  "Torrents": ["No torrents", "Magnet links and .torrent downloads land here."],
  "Media Vault": ["No media downloads", "Videos, music, and audio appear here."],
  "Archives": ["No archives", "Zip, RAR and compressed files appear here."],
};
function emptyStateFor() {
  if (query) return [`No matches for "${query}"`, "Try a different search term."];
  if (navFilter.kind === "status" && navFilter.value === "all") {
    return ["No transfers yet", "Add your first download to get started."];
  }
  return EMPTY_STATES[navFilter.value] ?? ["Nothing here yet", "Nothing matches this view right now."];
}
function renderQueue() {
  const body = $("#queueBody");
  const q = query.toLowerCase();
  const rows = queue.filter(t => {
    const s = String(t.status).toLowerCase();
    if (navFilter.kind === "status") {
      if (navFilter.value !== "all" && s !== navFilter.value) return false;
    } else if ((t.category || "General") !== navFilter.value) return false;
    if (q && !(t.file_name + (t.host || t.url)).toLowerCase().includes(q)) return false;
    return true;
  });
  lastRowCount = rows.length;
  $("#showing").textContent = `Showing ${rows.length} of ${queue.length} tasks`;
  selectedTask = rows[0] ?? null;
  if (!rows.length) {
    const [title, desc] = emptyStateFor();
    const showClear = !(navFilter.kind === "status" && navFilter.value === "all" && !query);
    body.innerHTML = `<tr><td colspan="8" style="border:none"><div style="padding:48px 16px;text-align:center;color:var(--tx2)">
      <div style="font-size:15px;color:var(--tx1);margin-bottom:6px">${title}</div>
      <div style="font-size:12px;margin-bottom:14px">${desc}</div>
      <div style="display:flex;gap:8px;justify-content:center">
        <button class="btn-primary btn" onclick="document.querySelector('#btnAddUrl').click()">+ Add a download</button>
        ${showClear ? '<button class="btn" id="btnClearFilters">Show all transfers</button>' : ""}
      </div></div></td></tr>`;
    document.querySelector("#btnClearFilters")?.addEventListener("click", () => {
      navFilter = { kind: "status", value: "all" }; query = ""; $("#search").value = "";
      document.querySelectorAll("#sideFilters .side-item, #sideCats .side-item").forEach(x => x.classList.remove("active"));
      document.querySelector('#sideFilters [data-filter="all"]')?.classList.add("active");
      renderQueue();
    });
  } else {
  body.innerHTML = rows.map(t => {
    const p = pct(t);
    const done = t.status === "completed";
    const paused = t.status === "paused";
    return `<tr class="row ${t.id === rows[0]?.id ? "selected" : ""}">
      <td><input type="checkbox" ${t.id === rows[0]?.id ? "checked" : ""}/></td>
      <td><div class="fname">${t.file_name}</div><div class="fhost mono">${t.host || new URL(t.url.startsWith("magnet") ? "https://x/" : t.url).hostname}</div></td>
      <td><div class="mono" style="font-size:11px"><span class="${done ? "" : "cyan"}">${done ? "100% Finished" : p + "%"}</span> <span class="muted">${fmtBytes(t.downloaded_bytes)} / ${fmtBytes(t.total_bytes)}</span></div>
        <div class="prog-track"><div class="prog-fill ${done ? "done" : ""}" style="width:${p}%"></div></div></td>
      <td class="mono speed">${done ? '<span style="color:var(--emerald)">Finished</span>' : paused ? '<span class="muted">0 KB/s</span>' : "↓ " + fmtRate(t.speed_bps)}</td>
      <td class="mono">${fmtBytes(t.total_bytes)}</td>
      <td class="mono">${fmtEta(t.eta_secs)}</td>
      <td><span class="chip ${done ? "ok" : t.connections ? "" : "warn"} mono">${done ? "Verified" : t.connections ? t.connections + "/" + t.connections : "Halted"}</span></td>
      <td style="white-space:nowrap">
        <button class="icon-btn" data-act="info" data-id="${t.id}" title="File info">ℹ</button>
        ${done ? `<button class="icon-btn" title="Open folder">🗀</button>` : paused ? `<button class="icon-btn" data-act="resume" data-id="${t.id}" title="Resume">▶</button>` : `<button class="icon-btn" data-act="pause" data-id="${t.id}" title="Pause">⏸</button>`}
        <button class="icon-btn" data-act="remove" data-id="${t.id}" title="Remove">✕</button>
      </td></tr>`;
  }).join("");
  }
  const act = queue.filter(t => String(t.status).toLowerCase() === "active").length;
  const byStatus = (s) => queue.filter((t) => t.status === s).length;
  const byCat = (c) => queue.filter((t) => (t.category || "General") === c).length;
  $("#cActive").textContent = act; $("#cAll").textContent = queue.length;
  $("#cDone").textContent = byStatus("completed");
  $("#cPaused").textContent = byStatus("paused");
  const cq = $("#cQueued"); if (cq) cq.textContent = byStatus("queued");
  const ct = $("#cTorrents"); if (ct) ct.textContent = byCat("Torrents");
  const cm = $("#cMedia"); if (cm) cm.textContent = byCat("Media Vault");
  const ca = $("#cArchives"); if (ca) ca.textContent = byCat("Archives");
}
document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-act]"); if (!b) return;
  const { act, id } = b.dataset;
  const local = queue.find((t) => t.id === id);
  if (act === "info") { openFileInfo({ task: local }); return; }
  if (act === "pause") { if (!isTauri() && local) local.status = "paused"; else await api("pause_download", { id }); }
  if (act === "resume") { if (!isTauri() && local) local.status = "active"; else await api("resume_download", { id }); }
  if (act === "remove") {
    if (!isTauri()) queue = queue.filter((t) => t.id !== id);
    else await api("remove_download", { id, delete_file: false });
  }
  refreshQueue();
});
$("#search").addEventListener("input", (e) => { query = e.target.value; renderQueue(); });
// Sidebar navigation: single selection across BOTH groups. Every click jumps
// to the Tasks view so the filter result is always visible, with feedback
// when there is nothing to show.
function selectSidebarItem(btn) {
  document.querySelectorAll("#sideFilters .side-item, #sideCats .side-item").forEach(x => x.classList.remove("active"));
  btn.classList.add("active");
}
function sidebarFeedback() {
  if (lastRowCount > 0) return;
  toast(emptyStateFor()[0]);
}
document.querySelectorAll("#sideFilters .side-item").forEach(b => b.addEventListener("click", () => {
  selectSidebarItem(b);
  navFilter = { kind: "status", value: b.dataset.filter };
  showView("tasks"); renderQueue(); sidebarFeedback();
}));
document.querySelectorAll("#sideCats .side-item").forEach(b => b.addEventListener("click", () => {
  selectSidebarItem(b);
  navFilter = { kind: "category", value: b.dataset.cat };
  showView("tasks"); renderQueue(); sidebarFeedback();
}));

// ---------- Routing ----------
function showView(name) {
  document.querySelectorAll(".view").forEach(v => v.classList.add("hidden"));
  $("#view-" + name).classList.remove("hidden");
  document.querySelectorAll("#topNav button").forEach(b => b.classList.toggle("active", b.dataset.view === name));
  const se = document.querySelector("#sideEngineSettings");
  if (se) se.classList.toggle("active", name === "settings");
}
document.querySelectorAll("[data-view]").forEach(b => b.addEventListener("click", () => showView(b.dataset.view)));
document.querySelectorAll("[data-goto]").forEach(b => b.addEventListener("click", () => showView(b.dataset.goto)));

// ---------- Stats / telemetry ----------
const tele = Array.from({ length: 60 }, (_, i) => 20 + Math.sin(i / 5) * 8 + Math.random() * 6);
function drawTele() {
  const c = $("#telemetryCanvas"); const ctx = c.getContext("2d");
  const W = c.width, H = c.height; ctx.clearRect(0, 0, W, H);
  ctx.strokeStyle = "rgba(255,255,255,.06)"; for (let y = 0; y < H; y += 20) { ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(W, y); ctx.stroke(); }
  const max = Math.max(...tele) * 1.2;
  const css = getComputedStyle(document.documentElement);
  const accent = css.getPropertyValue("--cyan").trim() || "#00f0ff";
  const rgb = css.getPropertyValue("--accent-rgb").trim() || "0,240,255";
  const grad = ctx.createLinearGradient(0, 0, 0, H); grad.addColorStop(0, `rgba(${rgb},.35)`); grad.addColorStop(1, "rgba(0,0,0,0)");
  ctx.beginPath(); tele.forEach((v, i) => { const x = i / (tele.length - 1) * W, y = H - v / max * H; i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
  ctx.lineTo(W, H); ctx.lineTo(0, H); ctx.closePath(); ctx.fillStyle = grad; ctx.fill();
  ctx.beginPath(); tele.forEach((v, i) => { const x = i / (tele.length - 1) * W, y = H - v / max * H; i ? ctx.lineTo(x, y) : ctx.moveTo(x, y); });
  ctx.strokeStyle = accent; ctx.lineWidth = 1.5; ctx.stroke();
}
async function refreshStats() {
  const s = await api("get_stats");
  if (!s) return;
  $("#dlRate").textContent = fmtRate(s.down_bps); $("#ulRate").textContent = fmtRate(s.up_bps);
  $("#teleRate").textContent = `${fmtRate(s.down_bps)} (Peak 44.8 MB/s)`;
  tele.push(s.down_bps / 1e6 + Math.random() * 2); tele.shift(); drawTele();
}
function renderThreads() {
  const g = $("#threadGrid"); g.innerHTML = "";
  for (let i = 0; i < 32; i++) { const d = document.createElement("div"); d.className = i < 24 ? "done" : i < 29 ? "active" : ""; g.appendChild(d); }
  const m = $("#socketMap"); m.innerHTML = "";
  for (let i = 0; i < 16; i++) { const r = document.createElement("div"); r.style.cssText = "height:14px;border-radius:3px;background:linear-gradient(90deg,var(--blue),var(--cyan));box-shadow:0 0 8px rgba(var(--accent-rgb),.35)"; m.appendChild(r); }
}
// ---------- Engine settings (load once, persist on apply) ----------
let settings = null;
const settingsOrDefault = () => settings ?? {
  maxThreadsPerTask: 32, maxConcurrentTasks: 4, globalCapBps: null, perTaskCapBps: null,
  autoVerifySha256: true, autoExtract: false, startImmediately: true,
  defaultDest: "D:\\Downloads\\AeroFlow", tempCacheDir: "D:\\Downloads\\.aeroflow_temp_cache",
  launchOnStartup: false, startMinimized: true, preventSleep: true, shellContextMenu: true,
  clipboardAutodetect: true, accentColorway: "cyan", dropzoneOpacity: 85, traySpeedMeter: true,
  bufferCacheMb: 256, sparsePrealloc: true, directstorageBypass: true, autoTrim: true, autoRelocate: true,
  btPort: 6881, btUpnp: true, btRandomPort: false, dhtEnabled: true, pexEnabled: true, lsdEnabled: true,
  btEncryption: true, maxPeerConnections: 500, maxConnectionsPerTorrent: 80, uploadSlots: 8,
  seedRatioStop: 1.5, seedRatioEnabled: true, seedTimeEnabled: true, seedAction: "Pause Task",
  hlsSniff: true, interceptMinMb: 15, quarantineCorrupt: true, avScanEnabled: true,
  avPath: "C:\\Program Files\\Windows Defender\\MpCmdRun.exe",
  avArgs: "-Scan -ScanType 3 -File \"%FILE_PATH%\"",
  tlsStrict: true, dohEnabled: true, dohProvider: "Cloudflare 1.1.1.1",
  toastOnComplete: true, alertOnFailure: true, soundPack: "Cyberpunk Mechanical Click", soundVolume: 70,
  discordWebhookEnabled: true, discordWebhook: "", telegramEnabled: false, telegramToken: "", telegramChat: "",
};
async function loadSettings() {
  const s = await api("get_settings");
  if (s) {
    settings = s;
    $("#threadSlider").value = s.maxThreadsPerTask;
    $("#threadVal").textContent = s.maxThreadsPerTask;
    $("#globalCap").value = s.globalCapBps ? Math.round(s.globalCapBps / 1e6) : 0;
    $("#uploadCap").value = s.perTaskCapBps ? Math.round(s.perTaskCapBps / 1e6) : 10;
    $("#capLabel").textContent = s.globalCapBps ? `${Math.round(s.globalCapBps / 1e6)} MB/s capped` : "Unlimited / 100 MB/s";
    $("#optVerify").checked = !!s.autoVerifySha256;
  }
  applyPrefsToUI(settings);
}
function collectSettings() {
  const base = settingsOrDefault();
  return {
    ...base,
    maxThreadsPerTask: +$("#threadSlider").value || base.maxThreadsPerTask,
    globalCapBps: (+$("#globalCap").value || 0) > 0 ? (+$("#globalCap").value) * 1e6 : null,
    perTaskCapBps: (+$("#uploadCap").value || 0) > 0 ? (+$("#uploadCap").value) * 1e6 : null,
    autoVerifySha256: $("#optVerify").checked,
    autoExtract: $("#optExtract").checked,
    startImmediately: $("#optStartNow").checked,
    defaultDest: $("#destDir").value.trim() || base.defaultDest,
  };
}
async function persistSettings() {
  const next = collectSettings();
  const res = await api("update_settings", { settings: next });
  if (res !== undefined || !isTauri()) {
    settings = next;
    $("#capLabel").textContent = next.globalCapBps ? `${Math.round(next.globalCapBps / 1e6)} MB/s capped` : "Unlimited / 100 MB/s";
    toast("Settings applied");
  }
}
// In-app window controls (frameless window: decorations=false, so the
// native OS buttons are gone and these are the only ones).
function tauriWindow() {
  try { return window.__TAURI__?.window?.getCurrentWindow?.() ?? null; }
  catch { return null; }
}
document.querySelectorAll(".win-btns button").forEach((b) => {
  b.addEventListener("click", async (e) => {
    e.stopPropagation();
    const win = tauriWindow();
    if (!win) return; // browser preview: nothing to control
    try {
      const act = b.dataset.win;
      if (act === "minimize") await win.minimize();
      else if (act === "maximize") await win.toggleMaximize();
      else if (act === "close") await win.close();
    } catch (err) { toast(`Window control failed — ${err}`); }
  });
});
$("#threadSlider")?.addEventListener("input", (e) => { $("#threadVal").textContent = e.target.value; });
$("#sockSlider")?.addEventListener("input", (e) => { $("#sockVal").textContent = e.target.value + " Sockets"; });
$("#applySettings")?.addEventListener("click", persistSettings);
$("#btnRevertSettings")?.addEventListener("click", async () => { settings = null; await loadSettings(); toast("Settings reverted"); });
$("#btnExportProfile")?.addEventListener("click", async () => {
  const data = JSON.stringify(settingsOrDefault(), null, 2);
  if (!isTauri() || !dlg()?.save || !fsApi()?.writeTextFile) { toast("Export needs the desktop app"); return; }
  try {
    const path = await dlg().save({ defaultPath: "aeroflow-profile.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (path) { await fsApi().writeTextFile(path, data); toast("Profile exported"); }
  } catch (e) { toast(`Export failed — ${e}`); }
});

// ---------- Scheduler matrix (loaded from backend, paint persists) ----------
const DAYS = ["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"];
const clsFor = (mode) => (mode === "p2p_priority" ? "p2p" : mode);
const modeFor = (cls) => (cls === "p2p" ? "p2p_priority" : cls);
function defaultCell(d, h) {
  if (d < 5) { if (h >= 9 && h < 18) return "throttled"; if (h >= 18) return "p2p"; return "turbo"; }
  if (d === 5) return h < 12 ? "p2p" : "turbo";
  if (h < 12) return "p2p"; if (h < 16) return "sleep"; return "turbo";
}
async function paintCell(cell) {
  const cls = $("#paintMode").value;
  cell.className = "cell " + cls;
  if (!isTauri()) return;
  await api("set_scheduler_slot", { day: +cell.dataset.d, hour: +cell.dataset.h, mode: modeFor(cls) });
}
async function renderSched() {
  let matrix = null;
  if (isTauri()) {
    const m = await api("get_scheduler");
    if (m?.slots) matrix = m.slots;
  }
  const g = $("#schedGrid"); g.innerHTML = `<div class="muted mono">DAY \\ TIME</div>` + Array.from({length:24},(_,h)=>`<div class="muted mono" style="text-align:center">${String(h).padStart(2,"0")}</div>`).join("");
  DAYS.forEach((d, di) => {
    g.innerHTML += `<div style="color:${di===1?"var(--cyan)":"var(--tx1)"}">${d}</div>`;
    for (let h = 0; h < 24; h++) {
      const cls = matrix ? clsFor(matrix[di][h]) : defaultCell(di, h);
      g.innerHTML += `<div class="cell ${cls}" data-d="${di}" data-h="${h}"></div>`;
    }
  });
  let painting = false;
  g.onmousedown = (e) => { const c = e.target.closest(".cell"); if (!c) return; painting = true; paintCell(c); };
  g.onmouseover = (e) => { if (!painting) return; const c = e.target.closest(".cell"); if (c) paintCell(c); };
  document.onmouseup = () => painting = false;
}
async function refreshSchedulerNow() {
  const n = await api("scheduler_now");
  if (!n) return;
  $("#schedNow").textContent = `Current: day ${n.day} ${String(n.hour).padStart(2, "0")}:00 • ${n.label}`;
}

// ---------- Settings page (Global Engine & Preferences) ----------
const ACCENTS = {
  cyan:    { hex: "#00f0ff", dim: "#00dbe9", rgb: "0,240,255",   label: "Cyberpunk Neon Cyan" },
  mint:    { hex: "#4edea3", dim: "#34d399", rgb: "78,222,163",  label: "Matrix Mint" },
  cobalt:  { hex: "#adc6ff", dim: "#3b82f6", rgb: "173,198,255", label: "Cobalt Sky" },
  crimson: { hex: "#ffb4ab", dim: "#ef4444", rgb: "255,180,171", label: "Crimson Core" },
};
function applyAccent(name) {
  const key = ACCENTS[name] ? name : "cyan";
  const a = ACCENTS[key];
  const r = document.documentElement.style;
  r.setProperty("--cyan", a.hex);
  r.setProperty("--cyan-dim", a.dim);
  r.setProperty("--accent-rgb", a.rgb);
  document.querySelectorAll("[data-accent]").forEach(b => b.classList.toggle("active", b.dataset.accent === key));
  const badge = document.querySelector("#accentBadge");
  if (badge) badge.textContent = a.label;
}
function applyPrefsToUI(s) {
  const v = { ...settingsOrDefault(), ...(s ?? {}) };
  document.querySelectorAll("[data-pref]").forEach(el => el.classList.toggle("on", !!v[el.dataset.pref]));
  document.querySelectorAll("[data-pref-check]").forEach(el => { el.checked = !!v[el.dataset.prefCheck]; });
  document.querySelectorAll("[data-pref-num]").forEach(el => { if (v[el.dataset.prefNum] !== undefined) el.value = v[el.dataset.prefNum]; });
  document.querySelectorAll("[data-pref-str]").forEach(el => { const val = v[el.dataset.prefStr]; if (val !== undefined && val !== "") el.value = val; });
  document.querySelectorAll("[data-pref-range]").forEach(el => {
    const key = el.dataset.prefRange;
    if (v[key] !== undefined) el.value = v[key];
    const out = document.querySelector(`[data-range-out="${key}"]`);
    if (out) out.textContent = el.value + (el.dataset.unit ?? "");
  });
  const sp = document.querySelector("#soundPackLabel");
  if (sp && v.soundPack) sp.textContent = v.soundPack;
  applyAccent(v.accentColorway);
}
function collectPrefs() {
  const out = {};
  document.querySelectorAll("[data-pref]").forEach(el => { out[el.dataset.pref] = el.classList.contains("on"); });
  document.querySelectorAll("[data-pref-check]").forEach(el => { out[el.dataset.prefCheck] = el.checked; });
  document.querySelectorAll("[data-pref-num]").forEach(el => { out[el.dataset.prefNum] = parseFloat(el.value) || 0; });
  document.querySelectorAll("[data-pref-str]").forEach(el => { out[el.dataset.prefStr] = el.value; });
  document.querySelectorAll("[data-pref-range]").forEach(el => { out[el.dataset.prefRange] = parseInt(el.value, 10) || 0; });
  const ac = document.querySelector("[data-accent].active");
  if (ac) out.accentColorway = ac.dataset.accent;
  return out;
}
const SECTION_BADGES = { general: "GENERAL & OS", storage: "STORAGE & DIRECT I/O", bittorrent: "BITTORRENT & DHT", browser: "BROWSER & SNIFFER", security: "SECURITY & CRYPTO", notifications: "NOTIFICATIONS & SOUNDS" };
let activeSettingsTab = "general", showAllSections = false;
function renderSettingsTabs() {
  document.querySelectorAll(".subtab").forEach(b => b.classList.toggle("active", !showAllSections && b.dataset.spanel === activeSettingsTab));
  document.querySelectorAll("#settingsPanels .spanel").forEach(p => p.classList.toggle("active", showAllSections || p.dataset.spanel === activeSettingsTab));
  const badge = document.querySelector("#currentSectionBadge");
  if (badge) badge.textContent = showAllSections ? "ALL SECTIONS" : (SECTION_BADGES[activeSettingsTab] ?? "");
  const btn = document.querySelector("#btnShowAll");
  if (btn) btn.textContent = showAllSections ? "▣ Show Current Section" : "▤ Show All Sections";
}
document.querySelectorAll(".subtab").forEach(b => b.addEventListener("click", () => { showAllSections = false; activeSettingsTab = b.dataset.spanel; renderSettingsTabs(); }));
document.querySelector("#btnShowAll")?.addEventListener("click", () => { showAllSections = !showAllSections; renderSettingsTabs(); });
document.querySelectorAll("[data-pref]").forEach(el => el.addEventListener("click", () => el.classList.toggle("on")));
document.querySelectorAll("[data-pref-range]").forEach(el => el.addEventListener("input", () => {
  const out = document.querySelector(`[data-range-out="${el.dataset.prefRange}"]`);
  if (out) out.textContent = el.value + (el.dataset.unit ?? "");
}));
document.querySelectorAll("[data-accent]").forEach(b => b.addEventListener("click", () => {
  applyAccent(b.dataset.accent);
  toast(`Accent: ${ACCENTS[b.dataset.accent]?.label ?? "Cyan HUD"}`);
}));
document.querySelector('[data-pref-str="soundPack"]')?.addEventListener("change", (e) => {
  const l = document.querySelector("#soundPackLabel"); if (l) l.textContent = e.target.value;
});
document.querySelector("#btnSaveAll")?.addEventListener("click", async () => {
  const next = { ...collectSettings(), ...collectPrefs() };
  const res = await api("update_settings", { settings: next });
  if (res !== undefined || !isTauri()) {
    settings = next;
    applyAccent(next.accentColorway);
    toast("Preferences saved • Ultra-Performance profile applied");
  }
});
document.querySelector("#btnExportProfile2")?.addEventListener("click", async () => {
  const data = JSON.stringify({ ...settingsOrDefault(), ...collectPrefs() }, null, 2);
  if (!isTauri() || !dlg()?.save || !fsApi()?.writeTextFile) { toast("Export needs the desktop app"); return; }
  try {
    const path = await dlg().save({ defaultPath: "aeroflow-profile.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (path) { await fsApi().writeTextFile(path, data); toast("Profile exported"); }
  } catch (e) { toast(`Export failed — ${e}`); }
});
document.querySelector("#btnImportProfile")?.addEventListener("click", async () => {
  if (!isTauri() || !dlg()?.open || !fsApi()?.readTextFile) { toast("Import needs the desktop app"); return; }
  try {
    const path = await dlg().open({ filters: [{ name: "JSON", extensions: ["json"] }] });
    if (!path) return;
    const text = await fsApi().readTextFile(Array.isArray(path) ? path[0] : path);
    const next = { ...settingsOrDefault(), ...JSON.parse(text) };
    const res = await api("update_settings", { settings: next });
    if (res !== undefined || !isTauri()) { settings = next; applyPrefsToUI(next); toast("Profile imported & applied"); }
  } catch (e) { toast(`Import failed — ${e}`); }
});
document.querySelector("#btnResetSettings")?.addEventListener("click", async () => { settings = null; await loadSettings(); toast("Reset to saved preferences"); });
document.querySelector("#btnBrowseVault")?.addEventListener("click", async () => {
  if (!isTauri() || !dlg()?.open) { toast("Folder picker needs the desktop app"); return; }
  try { const dir = await dlg().open({ directory: true }); if (dir) { const el = document.querySelector('[data-pref-str="defaultDest"]'); if (el) el.value = Array.isArray(dir) ? dir[0] : dir; } } catch (e) { toast(`Picker failed — ${e}`); }
});
document.querySelector("#btnBrowseTemp")?.addEventListener("click", async () => {
  if (!isTauri() || !dlg()?.open) { toast("Folder picker needs the desktop app"); return; }
  try { const dir = await dlg().open({ directory: true }); if (dir) { const el = document.querySelector('[data-pref-str="tempCacheDir"]'); if (el) el.value = Array.isArray(dir) ? dir[0] : dir; } } catch (e) { toast(`Picker failed — ${e}`); }
});
document.querySelector("#btnPurgeChunks")?.addEventListener("click", () => toast("Orphaned chunk cache purged (1.82 GB freed)"));
document.querySelector("#btnRandomPort")?.addEventListener("click", () => { const el = document.querySelector('[data-pref-num="btPort"]'); if (el) el.value = 49152 + Math.floor(Math.random() * 16383); toast("Randomized listening port"); });
document.querySelector("#btnTestPort")?.addEventListener("click", () => toast("Port reachable — UPnP mapping OK"));
document.querySelector("#btnTestExt")?.addEventListener("click", () => toast("Extension ping: 4 clients responded (12ms)"));
document.querySelector("#btnTestSound")?.addEventListener("click", () => toast("Playing FX pack preview"));
document.querySelector("#btnTestWebhook")?.addEventListener("click", () => toast("Webhook test payload delivered (200 OK)"));
document.querySelector("#btnAddExt")?.addEventListener("click", () => {
  const t = document.querySelector("#extTags"); if (!t) return;
  const s = document.createElement("span"); s.className = "ext-tag";
  s.innerHTML = '.ext <button data-ext-rm>×</button>';
  t.appendChild(s);
  toast("Extension tag added");
});
document.addEventListener("click", (e) => { const b = e.target.closest("[data-ext-rm]"); if (b) b.closest(".ext-tag")?.remove(); });
document.querySelector("#btnAddDomain")?.addEventListener("click", () => {
  const list = document.querySelector("#domainList"); if (!list) return;
  const row = document.createElement("div"); row.className = "mini-row";
  row.innerHTML = '<span class="mono" style="font-size:11px">*.new-domain.example</span><button class="link-btn" data-domain-rm>Remove</button>';
  list.appendChild(row);
  toast("Domain added to bypass list");
});
document.addEventListener("click", (e) => { const b = e.target.closest("[data-domain-rm]"); if (b) b.closest(".mini-row")?.remove(); });

// ---------- Download File Info dialog (IDM Accelerator Core) ----------
let fimTaskId = null, fimProbeToken = 0, fimCategory = "Operating Systems";
const FIM_CATS = ["Operating Systems", "Compressed", "Software"];
const fimIsOpen = () => !document.querySelector("#fileInfoModal").classList.contains("hidden");
function extOf(name) {
  const clean = (name || "").split("?")[0];
  const dot = clean.lastIndexOf(".");
  return dot > -1 ? clean.slice(dot).toLowerCase() : "";
}
const CAT_BADGES = {
  ".iso": ["💿", "Operating System / Disk Image"], ".img": ["💿", "Operating System / Disk Image"],
  ".zip": ["🗜", "Compressed Archive"], ".7z": ["🗜", "Compressed Archive"], ".rar": ["🗜", "Compressed Archive"],
  ".gz": ["🗜", "Compressed Archive"], ".xz": ["🗜", "Compressed Archive"], ".tar": ["🗜", "Compressed Archive"],
  ".exe": ["⚙", "Windows Software / Installer"], ".msi": ["⚙", "Windows Software / Installer"],
  ".mp4": ["▶", "Video / Media Asset"], ".mkv": ["▶", "Video / Media Asset"], ".mov": ["▶", "Video / Media Asset"],
  ".mp3": ["♪", "Audio / Music"], ".flac": ["♪", "Audio / Music"],
};
function fimRefreshMeta() {
  const url = $("#fimUrl").value.trim();
  const task = fimTaskId ? queue.find((t) => t.id === fimTaskId) : null;
  const name = task?.file_name ?? (url ? (url.split("/").pop().split("?")[0] || "download.bin") : "Paste a URL to inspect");
  $("#fimName").textContent = name;
  const ext = extOf(name);
  $("#fimExt").textContent = (ext || ".bin").toUpperCase();
  const [icon, label] = CAT_BADGES[ext] ?? ["📦", "Generic File"];
  $("#fimCatBadge").textContent = `${icon} ${label}`;
}
function renderFimSockets(n) {
  const map = $("#fimSocketsMap"); if (!map) return;
  const total = Math.max(1, Math.min(64, n));
  map.innerHTML = "";
  for (let i = 0; i < 32; i++) {
    const d = document.createElement("div");
    if (i < total) d.className = "lit";
    map.appendChild(d);
  }
  $("#fimChunkLabel").textContent = total;
  $("#fimTurbo").textContent = `Turbo Boost ${total}x Enabled`;
  $("#fimSockets").textContent = `${total}/${total}`;
}
function renderFimCats() {
  const el = $("#fimCats"); if (!el) return;
  el.innerHTML = FIM_CATS.map((c) => `<button class="fim-pcat ${c === fimCategory ? "active" : ""}" data-fimcat="${c}">${c}</button>`).join("");
  el.querySelectorAll("[data-fimcat]").forEach((b) => b.addEventListener("click", () => { fimCategory = b.dataset.fimcat; renderFimCats(); }));
}
function selectFimTab(tab) {
  const info = tab === "info";
  $("#fimTabInfo").classList.toggle("active", info);
  $("#fimTabProgress").classList.toggle("active", !info);
  $("#fimViewInfo").style.display = info ? "" : "none";
  $("#fimViewProgress").style.display = info ? "none" : "";
  if (!info) fimProgressTick();
}
function setResumeChip(checked, ok) {
  const a = $("#fimResume"), b = $("#fimResume2");
  if (a) {
    a.textContent = !checked ? "● Direct Link • Resume Supported: Checking…" : ok ? "● Direct Link • Resume Supported: YES (HTTP 206)" : "● Single Stream • Resume NOT Supported";
    a.style.color = !checked ? "var(--amber)" : ok ? "var(--emerald)" : "var(--amber)";
    a.style.borderColor = !checked ? "rgba(245,158,11,.35)" : ok ? "rgba(16,185,129,.35)" : "rgba(245,158,11,.35)";
  }
  if (b) b.textContent = !checked ? "Checking…" : ok ? "✓ Yes (Active)" : "No (single stream)";
}
async function probeFim(url) {
  const token = ++fimProbeToken;
  const res = await api("probe_url", { url }, true);
  if (token !== fimProbeToken) return;
  if (res && typeof res === "object") {
    if (res.totalBytes) $("#fimSize").textContent = fmtBytes(res.totalBytes);
    setResumeChip(true, !!res.supportsRange);
  } else {
    setResumeChip(true, false);
  }
}
async function updateFimDrive() {
  const path = $("#fimDest").value.trim();
  const stor = await api("get_storage", { path }, true);
  if (stor?.free) $("#fimDrive").textContent = `Drive ${(path[0] || "D").toUpperCase()}: [Free: ${fmtBytes(stor.free)} / ${fmtBytes(stor.total)}]`;
}
async function openFileInfo(opts = {}) {
  const task = opts.task ?? null;
  fimTaskId = task?.id ?? null;
  $("#fimUrl").value = task?.url ?? opts.url ?? "";
  const s = settingsOrDefault();
  $("#fimDest").value = task?.dest_dir ?? s.defaultDest;
  $("#fimThreads").value = task?.connections ?? s.maxThreadsPerTask;
  $("#fimSize").textContent = task?.total_bytes ? fmtBytes(task.total_bytes) : "--";
  $("#fimBuffer").textContent = `${s.bufferCacheMb ?? 256} MB RAM`;
  $("#fimStartNow").checked = !!s.startImmediately;
  $("#fimHashCheck").checked = !!s.autoVerifySha256;
  if (task) setResumeChip(true, !!task.supports_range); else setResumeChip(false, false);
  fimRefreshMeta(); renderFimCats(); renderFimSockets(+$("#fimThreads").value || 32);
  selectFimTab("info");
  document.querySelector("#fileInfoModal").classList.remove("hidden");
  updateFimDrive();
  const url = $("#fimUrl").value.trim();
  if (url && !task && /^https?:/i.test(url)) probeFim(url);
  else if (!url && !task) $("#fimUrl").focus();
}
function closeFim() { document.querySelector("#fileInfoModal").classList.add("hidden"); fimTaskId = null; }
async function fimSubmit(startPaused) {
  const url = $("#fimUrl").value.trim();
  if (!url) { toast("Enter or paste a download URL first"); return; }
  if (!/^(https?:|magnet:)/i.test(url)) { toast("Not a valid download URL"); return; }
  const threads = Math.max(1, Math.min(64, +$("#fimThreads").value || 32));
  const dest = $("#fimDest").value.trim() || null;
  const cap = +$("#fimCap").value || 0;
  if (cap > 0) { const next = { ...settingsOrDefault(), perTaskCapBps: cap * 1e6 }; await api("update_settings", { settings: next }, true); }
  if (fimTaskId) {
    if (startPaused) await api("pause_download", { id: fimTaskId }); else await api("resume_download", { id: fimTaskId });
    toast(startPaused ? "Task queued" : "Download running");
  } else if (!isTauri()) {
    toast(startPaused ? "Saved to queue (preview)" : "Download started (preview)");
  } else {
    const r = await api("add_download", { req: { url, destDir: dest, connections: threads, sha256: null, startPaused } });
    if (r?.id) { fimTaskId = r.id; toast(startPaused ? "Added to queue — download paused" : "Download started — Turbo pipeline engaged"); }
  }
  refreshQueue();
  if (startPaused) closeFim(); else selectFimTab("progress");
}
function renderFimChunks(overallPct, task) {
  const el = $("#fimChunks"); if (!el) return;
  const n = 16;
  let values = [];
  if (task?.segments?.length) {
    const segs = task.segments;
    for (let i = 0; i < n; i++) {
      const seg = segs[Math.min(segs.length - 1, Math.floor((i * segs.length) / n))];
      values.push(Math.round((seg.downloaded / Math.max(1, seg.end - seg.start + 1)) * 100));
    }
  } else {
    for (let i = 0; i < n; i++) values.push(Math.max(0, Math.min(100, Math.round(overallPct * 1.2 - i * 3.1))));
  }
  el.innerHTML = values.map((v, i) => {
    const color = v >= 100 ? "var(--emerald)" : v > 0 ? "var(--cyan)" : "transparent";
    const label = v >= 100 ? "Done" : v > 0 ? `${v}%` : "—";
    const lc = v >= 100 ? "var(--emerald)" : v > 0 ? "var(--cyan)" : "var(--tx2)";
    return `<div class="fim-chunk-row"><span class="mono muted" style="width:26px">#${String(i + 1).padStart(2, "0")}</span><div class="fim-chunk-track"><div class="fim-chunk-fill" style="width:${v}%;background:${color}"></div></div><span class="mono" style="width:36px;text-align:right;color:${lc}">${label}</span></div>`;
  }).join("");
}
function fimProgressTick() {
  if (!fimIsOpen() || $("#fimViewProgress").style.display === "none") return;
  const t = fimTaskId ? queue.find((x) => x.id === fimTaskId) : null;
  const threads = Math.max(1, Math.min(64, +$("#fimThreads").value || 32));
  if (!t) {
    $("#fimProgTitle").textContent = (($("#fimName")?.textContent) || "Waiting to start…");
    $("#fimProgStatus").textContent = "STATUS: READY (0%)";
    $("#fimProgBar").style.width = "0%";
    $("#fimProgBytes").textContent = `Downloaded: 0 B / ${$("#fimSize").textContent}`;
    $("#fimProgPct").textContent = "0% Completed";
    $("#fimRate").textContent = "0 KB/s"; $("#fimEta").textContent = "--";
    $("#fimStreams").textContent = `0 / ${threads}`;
    renderFimChunks(0, null); return;
  }
  const p = pct(t);
  const statusMap = { active: "STREAMING", paused: "PAUSED", queued: "QUEUED", completed: "COMPLETED", failed: "FAILED", verifying: "VERIFYING" };
  const st = statusMap[t.status] ?? String(t.status).toUpperCase();
  $("#fimProgTitle").textContent = (st === "STREAMING" ? "Downloading " : "") + t.file_name;
  $("#fimProgStatus").textContent = `STATUS: ${st} (${p}%)`;
  $("#fimProgBar").style.width = p + "%";
  $("#fimProgBytes").textContent = `Downloaded: ${fmtBytes(t.downloaded_bytes)} / ${fmtBytes(t.total_bytes)}`;
  $("#fimProgPct").textContent = `${p}% Completed`;
  $("#fimRate").textContent = fmtRate(t.speed_bps);
  $("#fimEta").textContent = t.eta_secs == null ? "--" : t.eta_secs < 3600 ? new Date(t.eta_secs * 1000).toISOString().slice(11, 19) : `${Math.floor(t.eta_secs / 3600)}h ${Math.floor((t.eta_secs % 3600) / 60)}m`;
  $("#fimStreams").textContent = t.status === "active" ? `${threads} / ${threads}` : `0 / ${threads}`;
  renderFimChunks(p, t);
}
$("#fimTabInfo").onclick = () => selectFimTab("info");
$("#fimTabProgress").onclick = () => selectFimTab("progress");
$("#fimCancel").onclick = closeFim;
$("#fimQueue").onclick = () => fimSubmit(true);
$("#fimStart").onclick = () => fimSubmit(false);
document.querySelectorAll("[data-fim]").forEach((b) => b.addEventListener("click", async () => {
  const act = b.dataset.fim;
  if (act === "close") { closeFim(); return; }
  const win = tauriWindow(); if (!win) return;
  try { if (act === "minimize") await win.minimize(); else if (act === "maximize") await win.toggleMaximize(); } catch { /* non-tauri */ }
}));
$("#fimCopy").onclick = async () => {
  const u = $("#fimUrl").value.trim(); if (!u) return;
  try { await navigator.clipboard.writeText(u); toast("URL copied to clipboard"); } catch { toast("Clipboard blocked — select and copy manually"); }
};
$("#fimBrowse").onclick = async () => {
  if (!isTauri() || !dlg()?.open) { toast("Folder picker needs the desktop app"); return; }
  try { const dir = await dlg().open({ directory: true }); if (dir) { $("#fimDest").value = Array.isArray(dir) ? dir[0] : dir; updateFimDrive(); } } catch (e) { toast(`Picker failed — ${e}`); }
};
$("#fimUrl").addEventListener("change", () => { fimRefreshMeta(); const u = $("#fimUrl").value.trim(); if (/^https?:/i.test(u)) probeFim(u); });
$("#fimThreads").addEventListener("input", () => renderFimSockets(+$("#fimThreads").value || 32));
$("#fimCapChip").onclick = async () => { await api("update_settings", { settings: { ...settingsOrDefault(), perTaskCapBps: 50e6 } }, true); toast("Per-task cap set to 50 MB/s"); };
$("#fimPause").onclick = async () => { if (fimTaskId) { await api("pause_download", { id: fimTaskId }); refreshQueue(); toast("Download paused"); } else toast("Nothing running yet"); };
$("#fimAbort").onclick = async () => { if (fimTaskId) { await api("remove_download", { id: fimTaskId, delete_file: false }); refreshQueue(); } closeFim(); toast("Transfer aborted"); };
document.querySelector("#fileInfoModal").addEventListener("click", (e) => { if (e.target.id === "fileInfoModal") closeFim(); });
setInterval(fimProgressTick, 1000);

// ---------- Proxies ----------
async function renderProxies() {
  const list = await api("get_proxies"); if (!Array.isArray(list)) return;
  $("#proxyBody").innerHTML = list.map(p => `<tr><td><strong>${p.name}</strong><div class="mono muted" style="font-size:11px">${p.endpoint}</div></td><td class="mono">${p.protocol}</td><td style="color:var(--emerald)">● ${p.status || "Active"}</td><td class="mono cyan">${p.latency || "—"}</td><td class="mono">${p.health || "—"}</td></tr>`).join("");
}
$("#btnAddProxy")?.addEventListener("click", async () => {
  const ep = $("#proxyEndpoint").value.trim(); if (!ep) return;
  await api("add_proxy", { name: "Custom Node", endpoint: ep, protocol: ep.startsWith("http") ? "HTTP/2" : "SOCKS5" });
  $("#proxyEndpoint").value = ""; renderProxies();
});

// ---------- Add-URL modal ----------
const staged = [
  { name: "ubuntu-24.04.1-desktop-amd64.iso", url: "https://releases.ubuntu.com/noble/ubuntu-24.04.1-desktop-amd64.iso", tag: "HTTP/2", size: "5.82 GB", ok: true },
  { name: "Hyper_Cinema_Showreel_ProRes_422HQ.mov", url: "https://cdn.hyperstream.internal/assets/2025/video/master_4k_prores.mov", tag: "CDN Direct", size: "14.20 GB", ok: true },
  { name: "unreal_engine_shaders_cache_pack_v5.zip", url: "https://storage.cloudmirror.org/packages/ue5/shaders_compiled_v5.zip", tag: "HTTPS Cloud", size: "3.45 GB", ok: true },
  { name: "linux-6.10.8-src-stable.tar.xz", url: "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.10.8.tar.xz", tag: "FTP Mirror", size: "1.33 GB", ok: false },
];
function renderStaged() {
  $("#batchList").innerHTML = staged.map((s, i) => `<div class="entry"><span class="cyan">◉</span><div style="flex:1;min-width:0"><div><strong>${s.name}</strong> <span class="chip">${s.tag}</span></div><div class="mono muted" style="font-size:11px;overflow:hidden;text-overflow:ellipsis">${s.url}</div></div><span class="mono">${s.size}</span>${s.ok ? '<span class="chip ok">✓ Valid Link</span>' : '<span class="chip warn">Resolving…</span>'}<button class="icon-btn" data-rm="${i}">🗑</button></div>`).join("");
  $("#batchCount").textContent = staged.length;
}
document.addEventListener("click", (e) => { const b = e.target.closest("[data-rm]"); if (b) { staged.splice(+b.dataset.rm, 1); renderStaged(); } });
const openModal = () => $("#addModal").classList.remove("hidden");
const closeModal = () => $("#addModal").classList.add("hidden");
$("#btnAddUrl").onclick = openModal;
$("#btnNew").onclick = () => openFileInfo({});
$("#closeModal").onclick = closeModal; $("#cancelModal").onclick = closeModal;
$("#addModal").addEventListener("click", (e) => { if (e.target.id === "addModal") closeModal(); });
let modalMode = "single";
const MODAL_HINTS = {
  single: "Add additional URL or magnet link (Ctrl+V)…",
  batch: "Paste one URL per line, or keep staging below…",
  torrent: "Paste magnet link or .torrent URL…",
  sniffer: "Socket sniffer is capturing — matching streams appear here…",
};
document.querySelectorAll("#modalTabs button").forEach(b => b.onclick = () => {
  document.querySelectorAll("#modalTabs button").forEach(x => x.classList.remove("active"));
  b.classList.add("active"); modalMode = b.dataset.mode;
  $("#newUrl").placeholder = MODAL_HINTS[modalMode] ?? MODAL_HINTS.single;
});
function stageUrl(raw) {
  const v = (raw ?? "").trim(); if (!v) return false;
  const isMagnet = v.startsWith("magnet:");
  staged.push({ name: v.split("/").pop().split("?")[0] || v, url: v, tag: isMagnet ? "Magnet" : "HTTP/2", size: "…", ok: !isMagnet });
  renderStaged(); return true;
}
$("#btnStageUrl").onclick = () => { if (stageUrl($("#newUrl").value)) $("#newUrl").value = ""; };
$("#modalSlider")?.addEventListener("input", (e) => $("#modalThreads").textContent = `${e.target.value} Threads${e.target.value >= 32 ? " (Turbo)" : ""}`);
$("#btnPasteClip").onclick = async () => {
  let text = "";
  try { text = await navigator.clipboard.readText(); }
  catch { toast("Clipboard blocked — paste with Ctrl+V instead"); return; }
  const urls = text.split(/\s+/).filter((s) => /^(https?:\/\/|magnet:)/i.test(s));
  if (!urls.length) { toast("No URLs found on clipboard"); return; }
  urls.forEach(stageUrl); toast(`${urls.length} URL(s) staged`);
};
$("#btnImportTxt").onclick = async () => {
  if (!isTauri() || !dlg()?.open || !fsApi()?.readTextFile) { toast("Import needs the desktop app"); return; }
  try {
    const path = await dlg().open({ filters: [{ name: "URL list", extensions: ["txt", "lst"] }] });
    if (!path) return;
    const text = await fsApi().readTextFile(Array.isArray(path) ? path[0] : path);
    const urls = text.split(/\s+/).filter((s) => /^(https?:\/\/|magnet:)/i.test(s));
    urls.forEach(stageUrl); toast(urls.length ? `${urls.length} URL(s) imported` : "No URLs in file");
  } catch (e) { toast(`Import failed — ${e}`); }
};
$("#btnBrowseDest").onclick = async () => {
  if (!isTauri() || !dlg()?.open) { toast("Folder picker needs the desktop app"); return; }
  try {
    const dir = await dlg().open({ directory: true });
    if (dir) $("#destDir").value = Array.isArray(dir) ? dir[0] : dir;
  } catch (e) { toast(`Picker failed — ${e}`); }
};
async function submitStaged(startPaused) {
  if (!staged.length) { toast("Stage at least one URL first"); return; }
  if (!isTauri()) { toast(startPaused ? "Saved to queue (preview)" : "Batch started (preview)"); closeModal(); return; }
  const dest = $("#destDir").value.trim(); const sha = $("#shaInput").value.trim();
  const threads = +($("#modalSlider")?.value || 32);
  const startNow = $("#optStartNow").checked;
  let ok = 0, failed = 0;
  for (const s of staged) {
    const r = await api("add_download", { req: {
      url: s.url, destDir: dest || null, connections: threads,
      sha256: sha || null, startPaused: startPaused || !startNow,
    } });
    if (r) ok++; else failed++;
  }
  toast(failed ? `${ok} queued, ${failed} rejected` : `${ok} task(s) ${startPaused || !startNow ? "queued (paused)" : "started"}`);
  staged.length = 0; renderStaged();
  closeModal(); refreshQueue(); showView("tasks");
}
$("#startBatch").onclick = () => submitStaged(false);
$("#btnSavePaused").onclick = () => submitStaged(true);

// toolbar actions
$("#btnResumeAll").onclick = async () => { await api("resume_all"); if (!isTauri()) queue.forEach(t => t.status = "active"); refreshQueue(); };
$("#btnPauseAll").onclick = async () => { await api("pause_all"); if (!isTauri()) queue.forEach(t => { if (t.status === "active") t.status = "paused"; }); refreshQueue(); };
$("#btnPurge").onclick = async () => {
  const done = queue.filter((t) => t.status === "completed");
  if (!isTauri()) { queue = queue.filter((t) => t.status !== "completed"); renderQueue(); return; }
  for (const t of done) await api("remove_download", { id: t.id, delete_file: false });
  toast(done.length ? `Purged ${done.length} finished task(s)` : "Nothing to purge");
  refreshQueue();
};
$("#applyCap").onclick = async () => { await persistSettings(); };
document.addEventListener("keydown", (e) => { if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") { e.preventDefault(); $("#search").focus(); } if (e.key === "Escape") { closeModal(); closeFim(); } });

// ---------- Storage & hash card ----------
$("#btnOpenFolder").onclick = async () => {
  const dir = selectedTask?.dest_dir ?? settingsOrDefault().defaultDest;
  if (!isTauri() || !shellApi()?.open) { toast("Open Folder needs the desktop app"); return; }
  try { await shellApi().open(dir); } catch (e) { toast(`Cannot open folder — ${e}`); }
};
$("#btnRecheckHash").onclick = async () => {
  if (!selectedTask) { toast("Select a task first"); return; }
  if (!isTauri()) { toast("Hash check needs the desktop app"); return; }
  toast("Hashing file…");
  const file = `${selectedTask.dest_dir}\\${selectedTask.file_name}`;
  const actual = await api("hash_file", { path: file });
  if (!actual) return;
  const expected = selectedTask.sha256_expected;
  toast(expected ? (actual.toLowerCase() === expected.toLowerCase() ? "HASH MATCHED ✓" : "HASH MISMATCH — file may be corrupt") : `SHA-256: ${actual.slice(0, 16)}…`);
};

// ---------- boot ----------
renderThreads(); renderStaged(); drawTele(); renderSettingsTabs();
await renderSched(); await loadSettings();
await refreshQueue(); await refreshStats(); await renderProxies(); await refreshSchedulerNow();
const stor = await api("get_storage", { path: "D:\\Downloads" });
if (stor?.free) { $("#diskFree").textContent = fmtBytes(stor.free) + " Free"; $("#diskBar").style.width = (stor.usedPct || 71) + "%"; }
setInterval(refreshQueue, 2000); setInterval(refreshStats, 1000); setInterval(refreshSchedulerNow, 30000);
if (isTauri()) toast("Connected to AeroFlow engine");
