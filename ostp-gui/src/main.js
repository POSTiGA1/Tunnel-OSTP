import { t, toggleLang, applyTranslations } from './i18n.js';

// ── Tauri invoke shim ────────────────────────────────────────────────────────
let invoke = () => Promise.resolve(null);
if (window.__TAURI__?.core) {
  invoke = window.__TAURI__.core.invoke;
}

// ── State ────────────────────────────────────────────────────────────────────
let appState    = 'disconnected'; // 'disconnected' | 'connecting' | 'connected'
let pollTimer   = null;
let uptimeTimer = null;
let uptimeSecs  = 0;
let rawConfig   = null;           // parsed config.json object
let serverAddr  = '';             // current server address (for badge)

// ── DOM refs ─────────────────────────────────────────────────────────────────
const $ = id => document.getElementById(id);

const homeScreen     = $('home-screen');
const settingsScreen = $('settings-screen');
const btnConnect     = $('btn-connect');
const orbitWrap      = $('orbit-wrap');
const brandDot       = $('brand-dot');
const statusLabel    = $('status-text');
const statusSub      = $('uptime-text');
const connInfo       = $('connection-info');
const serverBadgeTxt = $('server-badge-text');
const metricDown     = $('metric-down');
const metricUp       = $('metric-up');
const pingValueTxt   = $('ping-text-value');
const btnTestPing    = $('btn-test-ping');
const toast          = $('toast');

const btnGoSettings  = $('btn-go-settings');
const btnAutoConnect = $('btn-auto-connect');
const btnBack        = $('btn-back');
const btnImport      = $('btn-import-url');
const btnPeekKey     = $('btn-peek-key');
const importInput    = $('in-import-url');
const inServer       = $('in-server');
const inKey          = $('in-key');
const inSocks        = $('in-socks');
const inDns          = $('in-dns');

const groupCustomDns = $('group-custom-dns');
const inTransport    = $('in-transport');
const inSni          = $('in-stealth-sni');
const inWss          = $('in-wss');
const inMtu          = $('in-mtu');
const inTun          = $('in-tun-mode');
const inKillSwitch   = $('in-kill-switch');
const inMux          = $('in-mux-mode');
const inMuxSessions  = $('in-mux-sessions');
const inDebug          = $('in-debug');
const inAutoconnect    = $('in-autoconnect');
const inLaunchStartup  = $('in-launch-startup');

const wintunModal        = $('wintun-modal');
const btnWintunCancel    = $('btn-wintun-cancel');
const btnWintunOpen      = $('btn-wintun-open');
const wintunInstallPath  = $('wintun-install-path');

// ── Tag-input state ───────────────────────────────────────────────────────────
// Map of tagId -> Set<string>
const tagState = {
  domains:   new Set(),
  ips:       new Set(),
  processes: new Set(),
};

function renderTagList(key) {
  const list = $('tag-list-' + key);
  if (!list) return;
  list.innerHTML = '';
  for (const val of tagState[key]) {
    const chip = document.createElement('span');
    chip.className = 'tag-chip';
    chip.innerHTML = `${val}<button class="tag-chip-remove" title="Remove" tabindex="-1"><svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg></button>`;
    chip.querySelector('.tag-chip-remove').addEventListener('click', () => {
      tagState[key].delete(val);
      renderTagList(key);
      scheduleAutoSave();
    });
    list.appendChild(chip);
  }
}

function addTag(key, raw) {
  const vals = raw.split(/[\s,;]+/).map(v => v.trim()).filter(Boolean);
  let added = false;
  for (const v of vals) {
    if (!tagState[key].has(v)) { tagState[key].add(v); added = true; }
  }
  if (added) { renderTagList(key); scheduleAutoSave(); }
}

function wireTagInput(key) {
  const input = $('tag-input-' + key);
  const wrap  = $('tag-wrap-' + key);
  if (!input) return;
  input.addEventListener('keydown', e => {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      const v = input.value.trim();
      if (v) { addTag(key, v); input.value = ''; }
    } else if (e.key === 'Backspace' && !input.value) {
      const arr = [...tagState[key]];
      if (arr.length) { tagState[key].delete(arr[arr.length - 1]); renderTagList(key); scheduleAutoSave(); }
    }
  });
  input.addEventListener('paste', e => {
    e.preventDefault();
    const text = (e.clipboardData || window.clipboardData).getData('text');
    addTag(key, text);
    input.value = '';
  });
  // click on wrap focuses input
  if (wrap) wrap.addEventListener('click', () => input.focus());
}

// ── Utilities ────────────────────────────────────────────────────────────────
function fmtBytes(b) {
  if (!b || b === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log2(b) / 10), 4);
  return (b / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + ' ' + units[i];
}

function fmtTime(s) {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = n => String(n).padStart(2, '0');
  return h > 0
    ? `${h}:${pad(m)}:${pad(sec)}`
    : `${pad(m)}:${pad(sec)}`;
}

function splitLines(val) {
  return val.split('\n').map(l => l.trim()).filter(Boolean);
}

// ── Theme ────────────────────────────────────────────────────────────────────
function applyTheme(theme) {
  document.documentElement.setAttribute('data-theme', theme);
  localStorage.setItem('ostp-theme', theme);
}

function toggleTheme() {
  const current = document.documentElement.getAttribute('data-theme');
  applyTheme(current === 'light' ? 'dark' : 'light');
}

// Apply saved theme immediately (before any paint)
(function() {
  const saved = localStorage.getItem('ostp-theme') || 'dark';
  document.documentElement.setAttribute('data-theme', saved);
})();
// ── Toast ────────────────────────────────────────────────────────────────────
let toastTimer = null;
function showToast(msg, variant = '') {
  toast.textContent = msg;
  toast.className = 'toast show' + (variant ? ' is-' + variant : '');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.classList.remove('show');
  }, 2400);
}

// ── DNS & Kill Switch visibility ──────────────────────────────────────────────


function updateKillSwitchVisibility() {
  const group = $('group-kill-switch');
  if (!group || !inTun) return;
  group.style.display = inTun.checked ? 'flex' : 'none';
}


// ── State machine ────────────────────────────────────────────────────────────
function setState(next) {
  if (appState === next) return;
  appState = next;

  // Reset all dynamic classes
  btnConnect.className = 'power-btn';
  orbitWrap.className  = 'orbit-wrap';
  brandDot.className   = 'brand-dot';
  statusLabel.className = 'status-label';

  if (next === 'disconnected') {
    statusLabel.textContent = t('status_disconnected');
    statusSub.textContent   = t('hint_tap');
    connInfo.classList.add('hidden');
    metricDown.textContent  = '0 B';
    metricUp.textContent    = '0 B';
    pingValueTxt.textContent = 'Target Ping: -- ms';
    pingValueTxt.className = 'ping-test-value';
    clearInterval(pollTimer);
    clearInterval(uptimeTimer);
    pollTimer = uptimeTimer = null;
    uptimeSecs = 0;

  } else if (next === 'connecting') {
    btnConnect.classList.add('connecting');
    orbitWrap.classList.add('connecting');
    brandDot.classList.add('connecting');
    statusLabel.classList.add('is-connecting');
    statusLabel.textContent = t('status_connecting');
    statusSub.textContent   = t('hint_connecting');
    connInfo.classList.add('hidden');
    clearInterval(uptimeTimer);
    uptimeTimer = null;
    uptimeSecs = 0;

  } else if (next === 'connected') {
    btnConnect.classList.add('connected');
    orbitWrap.classList.add('connected');
    brandDot.classList.add('connected');
    statusLabel.classList.add('is-connected');
    statusLabel.textContent = t('status_connected');

    // Show connection info
    if (serverAddr) {
      serverBadgeTxt.textContent = serverAddr;
      connInfo.classList.remove('hidden');
    }

    // Start uptime counter
    if (!uptimeTimer) {
      uptimeSecs = 0;
      statusSub.textContent = fmtTime(uptimeSecs);
      uptimeTimer = setInterval(() => {
        uptimeSecs++;
        statusSub.textContent = fmtTime(uptimeSecs);
      }, 1000);
    }
  }
}

// ── Polling ──────────────────────────────────────────────────────────────────
async function poll() {
  if (!pollTimer) return;
  try {
    const code = await invoke('get_tunnel_status');
    if (!pollTimer) return; // Prevent race condition if disconnected during await
    console.log('[OSTP] poll status code:', code);
    
    if      (code === 0) { setState('disconnected'); return; }
    else if (code === 1)   setState('connecting');
    else if (code === 2)   setState('connected');

    const metrics = await invoke('get_metrics');
    if (metrics && pollTimer) {
      metricDown.textContent = fmtBytes(metrics.bytes_recv);
      metricUp.textContent   = fmtBytes(metrics.bytes_sent);
    }
  } catch (err) {
    console.error('[OSTP] poll threw:', err);
    if (pollTimer) {
      setState('disconnected');
      showToast(String(err), 'error');
      alert('[OSTP POLL ERROR] ' + String(err));
    }
  }
}

function startPolling() {
  clearInterval(pollTimer);
  poll();
  pollTimer = setInterval(poll, 1000);
}

// ── Connect / Disconnect ─────────────────────────────────────────────────────
async function handleToggle() {
  if (appState === 'disconnected') {
    try {
      const raw = await invoke('get_config');
      const cfg = JSON.parse(raw);
      serverAddr = cfg.server || '';
    } catch { serverAddr = ''; }

    setState('connecting');

    try {
      console.log('[OSTP] invoking start_tunnel...');
      const ok = await invoke('start_tunnel');
      console.log('[OSTP] start_tunnel returned:', ok);
      if (ok) {
        startPolling();
      } else {
        setState('disconnected');
        showToast(t('toast_error') || 'Failed to connect', 'error');
        alert('[OSTP] start_tunnel returned false');
      }
    } catch (err) {
      console.error('[OSTP] start_tunnel threw:', err);
      setState('disconnected');
      if (String(err).includes("WINTUN_MISSING")) {
        if (wintunModal) wintunModal.classList.remove('hidden');
      } else {
        showToast(String(err), 'error');
        alert('[OSTP ERROR] ' + String(err));
      }
    }
  } else {
    setState('disconnected');
    try { await invoke('stop_tunnel'); } catch { /* ignore */ }
    showToast(t('toast_disconnected') || 'Disconnected');
  }
}

// ── Screen navigation ────────────────────────────────────────────────────────
function showScreen(name) {
  if (name === 'settings') {
    loadConfigIntoForm();
    homeScreen.classList.remove('active');
    settingsScreen.classList.add('active');
  } else {
    settingsScreen.classList.remove('active');
    homeScreen.classList.add('active');
  }
}

// ── Config — load ─────────────────────────────────────────────────────────────
async function loadConfigIntoForm() {
  try {
    const raw = await invoke('get_config');
    rawConfig = JSON.parse(raw);
    const c = rawConfig.mode === 'client' ? rawConfig : null;
    if (!c) return;

    inServer.value  = c.server        || '';
    inKey.value     = c.access_key    || '';
    inSocks.value   = c.socks5_bind   || '127.0.0.1:1088';
    inTransport.value = c.transport?.mode || 'udp';
    inSni.value     = c.transport?.stealth_sni || '';
    inWss.checked   = !!c.transport?.wss;

    inMtu.value     = c.mtu           || '';
    inTun.checked   = !!c.tun?.enable;
    if (inKillSwitch) inKillSwitch.checked = !!c.tun?.kill_switch;
    inMux.checked   = !!c.mux?.enabled;
    inMuxSessions.value = c.mux?.sessions || '';
    
    inDns.value = c.tun?.dns || '';
    updateKillSwitchVisibility();

    inDebug.checked = !!c.debug;
    if (inAutoconnect) inAutoconnect.checked = !!c.gui?.autoconnect;
    if (inLaunchStartup) inLaunchStartup.checked = !!c.gui?.launch_startup;


    const ex = c.exclude || {};
    tagState.domains   = new Set(ex.domains   || []);
    tagState.ips       = new Set(ex.ips       || []);
    tagState.processes = new Set(ex.processes || []);
    renderTagList('domains');
    renderTagList('ips');
    renderTagList('processes');
  } catch (err) {
    showToast(String(err), 'error');
  }
}

// ── Config — save ─────────────────────────────────────────────────────────────
let autoSaveTimer = null;
function scheduleAutoSave() {
  clearTimeout(autoSaveTimer);
  autoSaveTimer = setTimeout(() => handleSave(true), 600);
}

async function handleSave(silent = false) {
  if (!rawConfig) rawConfig = { mode: 'client', log_level: 'info' };

  const server = inServer.value.trim();
  const key    = inKey.value.trim();

  if (!server) { if (!silent) showToast(t('err_server_req') || 'Server address required', 'error'); return; }
  if (!key)    { if (!silent) showToast(t('err_key_req')    || 'Access key required',     'error'); return; }

  rawConfig.mode       = 'client';
  rawConfig.server     = server;
  rawConfig.access_key = key;
  rawConfig.socks5_bind = inSocks.value.trim() || null;
  rawConfig.debug      = inDebug.checked;
  if (inAutoconnect || inLaunchStartup) {
    rawConfig.gui = rawConfig.gui || {};
    if (inAutoconnect) rawConfig.gui.autoconnect = inAutoconnect.checked;
    if (inLaunchStartup) rawConfig.gui.launch_startup = inLaunchStartup.checked;
  }

  if (inLaunchStartup) {
    try { await invoke('set_autostart', { enable: inLaunchStartup.checked }); } catch (err) { console.error('autostart error', err); }
  }



  rawConfig.transport = rawConfig.transport || {};
  rawConfig.transport.mode = inTransport.value;
  rawConfig.transport.stealth_sni = inSni.value.trim() || undefined;
  rawConfig.transport.wss = inWss.checked;

  const mtuStr = inMtu.value.trim();
  if (mtuStr) rawConfig.mtu = parseInt(mtuStr, 10);
  else delete rawConfig.mtu;

  if (inMux.checked) {
    const s = parseInt(inMuxSessions.value.trim(), 10);
    rawConfig.mux = { enabled: true, sessions: isNaN(s) ? 1 : s };
  } else {
    delete rawConfig.mux;
  }

  rawConfig.tun = rawConfig.tun || {};
  rawConfig.tun.enable = inTun.checked;
  rawConfig.tun.kill_switch = inKillSwitch ? inKillSwitch.checked : false;
  rawConfig.tun.wintun_path = rawConfig.tun.wintun_path || './wintun.dll';
  rawConfig.tun.ipv4_address = rawConfig.tun.ipv4_address || '10.1.0.2/24';
  rawConfig.tun.stack = 'ostp';
  rawConfig.tun.dns    = inDns.value.trim() || null;

  rawConfig.exclude = {
    domains:   [...tagState.domains],
    ips:       [...tagState.ips],
    processes: [...tagState.processes],
  };

  try {
    const ok = await invoke('save_config', { jsonContent: JSON.stringify(rawConfig, null, 2) });
    if (!ok && !silent) {
      showToast(t('toast_error'), 'error');
    }
  } catch (err) {
    if (!silent) showToast(String(err), 'error');
  }
}

// ── Import share link ─────────────────────────────────────────────────────────
function handleImport() {
  const raw = importInput.value.trim();
  if (!raw) return;
  try {
    if (!raw.startsWith('ostp://')) throw new Error('Link must start with ostp://');
    const url = new URL(raw);
    const key  = decodeURIComponent(url.username);
    const host = url.host;
    if (!key || !host) throw new Error('Incomplete link parameters');
    inServer.value = host;
    inKey.value    = key;
    inSni.value    = url.searchParams.get('sni') || '';
    
    const type = url.searchParams.get('type');
    if (type === 'tcp' || type === 'http') inTransport.value = 'uot';
    else inTransport.value = 'udp';
    
    importInput.value = '';
    showToast(t('toast_imported'), 'ok');
    handleSave(false);
  } catch (err) {
    showToast(err.message, 'error');
  }
}

// ── Peek key ──────────────────────────────────────────────────────────────────
let peeking = false;
function togglePeek() {
  peeking = !peeking;
  inKey.type = peeking ? 'text' : 'password';
  btnPeekKey.style.color = peeking
    ? 'var(--c-accent)'
    : 'var(--c-txt-3)';
}

// ── Init ──────────────────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
  applyTranslations();
  setState('disconnected');
  updateKillSwitchVisibility();

  // Event wiring
  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen('tunnel-error', (evt) => {
      setState('disconnected');
      const errStr = String(evt.payload);
      showToast(errStr, 'error');
      alert(errStr);
    });
  }

  // Load wintun install path for modal instruction
  if (wintunInstallPath) {
    try {
      const p = await invoke('get_wintun_install_path');
      if (p) wintunInstallPath.textContent = p;
    } catch { /* ignore */ }
  }

  // Auto-connect on startup
  try {
    const raw = await invoke('get_config');
    rawConfig = JSON.parse(raw);
    if (rawConfig?.gui?.autoconnect) {
      setTimeout(() => {
        if (appState === 'disconnected') handleToggle();
      }, 800);
    }
  } catch (err) {
    console.error('Failed to load config on startup', err);
  }

  btnConnect.addEventListener('click',       handleToggle);
  
  if (btnAutoConnect) {
    btnAutoConnect.addEventListener('click', async () => {
      if (appState !== 'disconnected') {
        showToast('Disconnect first to run Auto mode', 'error');
        return;
      }
      
      try {
        const raw = await invoke('get_config');
        rawConfig = JSON.parse(raw);
      } catch {
        showToast('Please save your config first', 'error');
        return;
      }

      showToast('Starting Auto search...', 'ok');
      try {
        const mtus = [1500, 1350, 1280];
        const modes = [
          { t: 'udp', w: false, r: false },
          { t: 'uot', w: false, r: false },
          { t: 'uot', w: true, r: false },
          { t: 'uot', w: false, r: true }
        ];

        for (let mode of modes) {
          for (let mtu of mtus) {
            showToast(`Testing: ${mode.t} | WSS: ${mode.w} | XTLS: ${mode.r} | MTU: ${mtu}`);
            
            rawConfig.ostp = rawConfig.ostp || {};
            rawConfig.ostp.mtu = mtu;
            rawConfig.transport = rawConfig.transport || {};
            rawConfig.transport.mode = mode.t;
            rawConfig.transport.wss = mode.w;
            


            await invoke('save_config', { jsonContent: JSON.stringify(rawConfig, null, 2) });
            
            setState('connecting');
            const ok = await invoke('start_tunnel');
            if (ok) {
              startPolling();
              // Wait a bit to see if it stays connected and ping works
              await new Promise(r => setTimeout(r, 3000));
              try {
                const metrics = await invoke('get_metrics');
                if (metrics && metrics.rtt_ms > 0) {
                  showToast(`Success! Found working config: ${mode.t} (MTU ${mtu})`, 'ok');
                  return; // Stop on first working
                }
              } catch {}
              
              // If we are here, ping failed, so stop and try next
              await invoke('stop_tunnel');
              setState('disconnected');
            }
          }
        }
        showToast('Auto search finished. No working config found.', 'error');
      } catch (err) {
        showToast('Error during auto-connect: ' + String(err), 'error');
      }
    });
  }

  btnGoSettings.addEventListener('click',    () => showScreen('settings'));
  btnBack.addEventListener('click',          () => showScreen('home'));
  btnImport.addEventListener('click',        handleImport);
  btnPeekKey.addEventListener('click',       togglePeek);

  // Theme toggle
  const btnThemeToggle = $('btn-theme-toggle');
  if (btnThemeToggle) {
    btnThemeToggle.addEventListener('click', toggleTheme);
  }
  scheduleAutoSave();
  inTun.addEventListener('change', () => {
    updateKillSwitchVisibility();
    scheduleAutoSave();
  });
  importInput.addEventListener('keydown', e => { if (e.key === 'Enter') handleImport(); });

  // Auto-save wiring for standard form elements (excluding tag-inputs which wire themselves)
  const formInputs = document.querySelectorAll('#settings-screen input:not(#in-import-url):not(.tag-input-field), #settings-screen select');
  formInputs.forEach(el => {
    el.addEventListener('input', () => {
      scheduleAutoSave();
      if (appState === 'connected') {
        if (window.__TAURI__ && window.__TAURI__.invoke) {
          window.__TAURI__.invoke('reload_tunnel');
        }
      }
    });
    el.addEventListener('change', scheduleAutoSave);
  });

  // Wire tag inputs
  wireTagInput('domains');
  wireTagInput('ips');
  wireTagInput('processes');

  // Process picker
  const procPickerModal  = $('proc-picker-modal');
  const btnPickProcess   = $('btn-pick-process');
  const btnProcCancel    = $('btn-proc-cancel');
  const btnProcAdd       = $('btn-proc-add');
  const procList         = $('proc-list');
  const procSearch       = $('proc-search');
  let allProcs = [];
  let selectedProcs = new Set();

  function renderProcList(filter) {
    if (!procList) return;
    const q = (filter || '').toLowerCase();
    const filtered = q ? allProcs.filter(p => p.toLowerCase().includes(q)) : allProcs;
    if (!filtered.length) {
      procList.innerHTML = `<div class="proc-empty">${q ? 'No matches' : 'No processes found'}</div>`;
      return;
    }
    procList.innerHTML = filtered.map(p => {
      const sel = selectedProcs.has(p) ? 'selected' : '';
      return `<div class="proc-item ${sel}" data-name="${p}"><div class="proc-item-check"></div><span class="proc-item-name">${p}</span></div>`;
    }).join('');
    procList.querySelectorAll('.proc-item').forEach(el => {
      el.addEventListener('click', () => {
        const name = el.dataset.name;
        if (selectedProcs.has(name)) selectedProcs.delete(name);
        else selectedProcs.add(name);
        el.classList.toggle('selected');
        el.querySelector('.proc-item-check') // rerender
        renderProcList(procSearch ? procSearch.value : '');
      });
    });
  }

  if (btnPickProcess && procPickerModal) {
    btnPickProcess.addEventListener('click', async () => {
      selectedProcs = new Set([...tagState.processes]);
      procPickerModal.classList.remove('hidden');
      procList.innerHTML = '<div class="proc-loading"><span>Loading...</span></div>';
      if (procSearch) procSearch.value = '';
      try {
        allProcs = await invoke('list_running_processes');
      } catch {
        allProcs = [];
      }
      renderProcList('');
      if (procSearch) procSearch.focus();
    });
  }

  if (procSearch) {
    procSearch.addEventListener('input', () => renderProcList(procSearch.value));
  }

  if (btnProcCancel) {
    btnProcCancel.addEventListener('click', () => procPickerModal.classList.add('hidden'));
  }

  if (btnProcAdd) {
    btnProcAdd.addEventListener('click', () => {
      for (const p of selectedProcs) tagState.processes.add(p);
      renderTagList('processes');
      scheduleAutoSave();
      procPickerModal.classList.add('hidden');
    });
  }

  // Close picker on backdrop click
  if (procPickerModal) {
    procPickerModal.addEventListener('click', e => {
      if (e.target === procPickerModal) procPickerModal.classList.add('hidden');
    });
  }

  btnTestPing.addEventListener('click', runPingTest);

  btnWintunCancel.addEventListener('click', () => {
    wintunModal.classList.add('hidden');
  });

  // Open wintun.net link — handled natively by <a target="_blank">, but also wire as fallback
  if (btnWintunOpen && window.__TAURI__) {
    btnWintunOpen.addEventListener('click', (e) => {
      e.preventDefault();
      const opener = window.__TAURI__?.opener || window.__TAURI__?.shell;
      if (opener && opener.open) {
        opener.open('https://www.wintun.net');
      } else {
        window.open('https://www.wintun.net', '_blank');
      }
    });
  }

  async function runPingTest() {
    pingValueTxt.textContent = 'Testing...';
    pingValueTxt.className = 'ping-test-value';
    try {
      const metrics = await invoke('get_metrics');
      if (metrics && metrics.rtt_ms > 0) {
        const rtt = metrics.rtt_ms;
        pingValueTxt.textContent = `Target Ping: ${rtt} ms`;
        if (rtt < 80) pingValueTxt.classList.add('good');
        else if (rtt < 200) pingValueTxt.classList.add('warn');
        else pingValueTxt.classList.add('bad');
      } else {
        pingValueTxt.textContent = 'Target Ping: -- ms';
      }
    } catch {
      pingValueTxt.textContent = 'Target Ping: Error';
    }
  }

  // Restore status on app open
  try {
    const code = await invoke('get_tunnel_status');
    if (code > 0) startPolling();
  } catch { /* not in Tauri context */ }

  if (window.__TAURI__?.event) {
    const { listen } = window.__TAURI__.event;
    listen('tray_connect', () => {
      if (appState === 'disconnected') handleToggle();
    });
    listen('tray_disconnect', () => {
      if (appState !== 'disconnected') handleToggle();
    });
  }
});
