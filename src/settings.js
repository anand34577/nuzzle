const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const MAC = /Mac/i.test(navigator.platform || navigator.userAgent);
const LINUX = !MAC && /Linux/i.test(navigator.userAgent);

// Key names in the help text follow the system: Ctrl/Alt here, ⌘/⌥ on a Mac (CommandOrControl is Cmd there).
if (MAC) {
  const MAC_KEYS = { Ctrl: '⌘', Alt: '⌥', Shift: '⇧', Win: '⌃' };
  for (const k of $$('kbd')) if (MAC_KEYS[k.textContent]) k.textContent = MAC_KEYS[k.textContent];
}
document.documentElement.dataset.os = MAC ? 'mac' : LINUX ? 'linux' : 'windows';

const TEXT = ['llama_server', 'model', 'mmproj', 'name', 'playfulness', 'tts_voice', 'tts_lang', 'asr_prompt', 'llama_args', 'coord_mode', 'chat_shortcut', 'ptt_shortcut', 'voice_ref_path',
  'telegram_token', 'agent_api_token',
  'llm_provider', 'llm_api_url', 'llm_api_model', 'llm_api_key', 'stt_engine', 'stt_api_url', 'stt_api_model', 'stt_api_key',
  'tts_engine', 'tts_api_url', 'tts_api_voice', 'tts_api_model', 'tts_api_key', 'speech_python', 'speech_asr_dir', 'speech_tts_dir', 'home_place'];
const NUM = ['gpu_layers', 'ctx', 'max_image_width', 'agent_api_port', 'speech_port'];
const CHECKS = ['telegram_enabled', 'agent_api_enabled', 'agent_api_lan', 'llm_api_vision', 'speech_server_auto', 'morning_hello'];
// Fields that only matter for one choice (data-when="llm_provider=api") show only while that choice is made.
function showEngineFields() {
  for (const el of document.querySelectorAll('[data-when]')) {
    const [id, want] = el.dataset.when.split('=');
    const sel = document.getElementById(id);
    el.hidden = !sel || (sel.type === 'checkbox' ? String(sel.checked) : sel.value) !== want;
  }
}
document.addEventListener('change', (e) => { if (e.target && ['llm_provider', 'stt_engine', 'tts_engine', 'speech_server_auto'].includes(e.target.id)) showEngineFields(); });
// The speech server Nuzzle runs itself: show whether it is up, and why not when it isn't.
async function loadSpeech() {
  let st = { state: 'off', detail: '' };
  try { st = await T.core.invoke('speech_status'); } catch { /* older backend */ }
  const pill = document.getElementById('speech-state'), detail = document.getElementById('speech-detail');
  if (!pill) return;
  pill.textContent = { off: 'Off', starting: 'Starting...', ready: 'Running', error: 'Not running' }[st.state] || st.state;
  pill.dataset.state = st.state;
  detail.textContent = st.state === 'error' || st.state === 'starting' ? st.detail : '';
  detail.hidden = !detail.textContent;
}
T.event.listen('speech', loadSpeech);
addEventListener('DOMContentLoaded', loadSpeech);
const PATHS = { llama_server: 'exe', model: 'gguf', mmproj: 'mmproj', voice_ref_path: 'wav' };
const BUILTIN_TOOLS = ['remember', 'forget', 'refresh_view', 'move_pointer', 'click', 'scroll', 'select_all', 'drag_select', 'type_text', 'press_key', 'open_url', 'read_text_file',
  'list_directory', 'write_text_file', 'run_command', 'clipboard_get', 'clipboard_set', 'web_fetch', 'get_datetime', 'system_info', 'set_reminder', 'list_reminders', 'cancel_reminder', 'convert_units', 'random',
  'web_search', 'calculate', 'list_windows', 'open_path', 'launch_app', 'find_files', 'come_here', 'email_draft', 'notes', 'weather', 'media_key'];
const MANAGED_FLAGS = ['--host', '--port', '--api-key', '-m', '--model', '--mmproj', '--device'];

let current = {};          // last settings known to the backend
let snap = {};             // form values at the last load/save (for "unsaved changes")
let chosen = new Set();    // selected GPUs
let hooks = [];            // connector editor rows
let character = 'pip';
let approvals = 'risky';
let listenMode = 'always';
let pathRes = {};          // field -> { value, ok, message }
let llamaFound = [];
let gpusListed = false;
let brain = { state: 'setup', detail: '' };
let models = [];
let mcps = [];             // MCP server editor rows
let mcpStatus = [];        // live status from the backend
let ackFull = false;       // the user has accepted the full-access warning
let llamaInfo = { bundled: false };

// ------------------------------------------------------------------ helpers
/** A thin moving bar under the title bar while something loads or saves. */
let topCount = 0;
function setTop(on) { topCount = Math.max(0, topCount + (on ? 1 : -1)); $('#toploader').hidden = topCount === 0; }
/** Grey placeholder rows while a list loads, so the page never looks empty or stuck. */
function skeleton(sel, n = 2) { const h = $(sel); if (h) h.innerHTML = Array.from({ length: n }, () => '<li class="skel-row"><span class="skel w3"></span><span class="skel w1"></span></li>').join(''); }
let toastTimer;
function toast(text, error = false) {
  const t = $('#toast');
  t.textContent = text; t.className = 'toast' + (error ? ' err' : ''); t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, error ? 6000 : 3200);
}
const debounce = (fn, ms) => { let id; return (...a) => { clearTimeout(id); id = setTimeout(() => fn(...a), ms); }; };
/** Run `fn` with the button showing a spinner and `label`, so slow actions never look frozen. */
async function busy(btn, label, fn) {
  const old = btn.textContent;
  btn.disabled = true; btn.classList.add('busy'); btn.textContent = label;
  try { return await fn(); } finally { btn.disabled = false; btn.classList.remove('busy'); btn.textContent = old; }
}
const dirname = (p) => p.replace(/[\\/][^\\/]*$/, '');

function parseHeaders(text) {
  const out = {};
  for (const [i, raw] of text.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line) continue;
    const at = line.indexOf(':');
    const name = at > 0 ? line.slice(0, at).trim() : '';
    if (!/^[A-Za-z0-9-]+$/.test(name)) return { error: `Line ${i + 1}: write headers as  Name: value` };
    out[name] = line.slice(at + 1).trim();
  }
  return { value: out };
}

function parseEnv(text) {
  const out = {};
  for (const [i, raw] of text.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line) continue;
    const at = line.indexOf('=');
    const key = at > 0 ? line.slice(0, at).trim() : '';
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) return { error: `Line ${i + 1}: write it as KEY=value` };
    out[key] = line.slice(at + 1);
  }
  return { value: out };
}
const lines = (text) => text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);

function readForm() {
  const s = { ...current };
  for (const k of TEXT) s[k] = $('#' + k).value.trim();
  for (const k of NUM) { const v = $('#' + k).value.trim(); s[k] = v === '' ? NaN : Number(v); }
  s.always_look = $('#always_look').checked;
  s.avoid_cursor = $('#avoid_cursor').checked;
  s.hear_tone = $('#hear_tone').checked;
  s.memory_enabled = $('#memory_enabled').checked;
  for (const k of CHECKS) s[k] = $('#' + k).checked;
  // Running the speech server here means using it: point hearing and speaking at it for the folders given.
  if (s.speech_server_auto) {
    const url = `http://127.0.0.1:${s.speech_port || 8765}/v1`;
    for (const [dir, engine, field] of [['speech_asr_dir', 'stt_engine', 'stt_api_url'], ['speech_tts_dir', 'tts_engine', 'tts_api_url']]) {
      if (!s[dir]) continue;
      s[engine] = 'api'; s[field] = url;
      $('#' + engine).value = 'api'; $('#' + field).value = url;
    }
    showEngineFields();
  }
  s.devices = [...chosen].sort();
  s.character = character;
  s.approvals = approvals;
  s.listen_mode = listenMode;
  s.full_access_ack = ackFull;
  s.mcp_servers = mcps.map(mcpCfg);
  s.webhooks = hooks.map((h) => ({
    name: h.name.trim(), description: h.description.trim(), url: h.url.trim(),
    headers: parseHeaders(h.headersText).value || {},
  }));
  return s;
}

// ------------------------------------------------------------------ validation
const intRange = (min, max) => (v) => (!Number.isInteger(v) ? 'Enter a whole number.' : v < min || v > max ? `Use a number from ${min} to ${max}.` : '');

const RULES = {
  name: (s) => (!s.name ? 'Give me a name.' : [...s.name].length > 40 ? 'Keep it to 40 characters or fewer.' : ''),
  ctx: (s) => intRange(512, 1048576)(s.ctx),
  agent_api_port: (s) => intRange(1024, 65535)(s.agent_api_port),
  telegram_token: (s) => (!s.telegram_enabled || /^\d+:[A-Za-z0-9_-]{20,}$/.test(s.telegram_token) ? '' : 'Paste the token from @BotFather (it looks like 123456:ABC-...).'),
  agent_api_token: (s) => (!s.agent_api_enabled || s.agent_api_token.length >= 16 ? '' : 'Press New token.'),
  gpu_layers: (s) => intRange(0, 999)(s.gpu_layers),
  max_image_width: (s) => intRange(256, 4096)(s.max_image_width),
  tts_voice: (s) => (s.tts_voice ? '' : 'Choose a voice.'),
  tts_lang: (s) => (/^[a-z]{2,3}(-[A-Za-z]{2,4})?$/.test(s.tts_lang) ? '' : 'Use a language code such as en or en-US.'),
  asr_prompt: (s) => (s.asr_prompt.length > 400 ? 'That is too long (400 characters max).' : ''),
  llama_args: (s) => {
    const bad = s.llama_args.split(/\s+/).find((a) => MANAGED_FLAGS.includes(a));
    return bad ? `${bad} is managed by Nuzzle; please remove it.` : '';
  },
  ptt_shortcut: (s) => (s.listen_mode === 'push' && !s.ptt_shortcut ? 'Push to talk needs a shortcut.'
    : s.ptt_shortcut && s.ptt_shortcut.toLowerCase() === s.chat_shortcut.toLowerCase() ? 'Use a different shortcut from the typing box.' : ''),
  chat_shortcut: (s) => (/^(commandorcontrol|ctrl|control|cmd|command)\+alt\+space$/i.test(s.chat_shortcut) ? `${MAC ? '⌘⌥Space' : 'Ctrl+Alt+Space'} is reserved for stopping me.` : ''),
};

function pathMessage(id, s) {
  const v = s[id];
  const r = pathRes[id];
  if (!v) {
    if (id === 'mmproj') return { text: "Optional, but without it I can't see or hear.", kind: 'warn' };
    return null;
  }
  if (!r || r.value !== v) return { text: 'Checking the file...', kind: 'pending', pending: true };
  return { text: r.message, kind: r.ok ? 'ok' : 'err' };
}

function hookErrors() {
  const out = [];
  const seen = new Set();
  hooks.forEach((h, i) => {
    const name = h.name.trim();
    const e = {};
    if (!/^[A-Za-z0-9_-]{1,40}$/.test(name)) e.name = 'Use 1-40 letters, digits, _ or -.';
    else if (BUILTIN_TOOLS.includes(name)) e.name = 'That name is already used by one of my own tools.';
    else if (seen.has(name)) e.name = 'Another connector already uses this name.';
    seen.add(name);
    try { const u = new URL(h.url.trim()); if (!['http:', 'https:'].includes(u.protocol)) throw 0; }
    catch { e.url = 'Enter a full web address starting with http:// or https://'; }
    const ph = parseHeaders(h.headersText);
    if (ph.error) e.headersText = ph.error;
    if (!h.description.trim()) e.description = 'Describe it so I know when to use it.';
    out[i] = e;
  });
  return out;
}

function showMsg(fieldEl, m) {
  if (!fieldEl) return;
  fieldEl.classList.toggle('invalid', !!m && m.kind === 'err');
  const el = $('.msg', fieldEl);
  if (!el) return;
  el.textContent = m ? m.text : '';
  el.className = 'msg ' + (m && m.kind ? m.kind : '');
}

function refresh() {
  const s = readForm();
  let errors = 0, pending = 0;
  for (const [id, rule] of Object.entries(RULES)) {
    const text = rule(s);
    showMsg($(`.field[data-field="${id}"]`), text ? { text, kind: 'err' } : null);
    if (text) errors++;
  }
  for (const id of Object.keys(PATHS)) {
    const m = pathMessage(id, s);
    showMsg($(`.field[data-field="${id}"]`), m);
    if (m && m.kind === 'err') errors++;
    if (m && m.pending) pending++;
  }
  const he = hookErrors();
  $$('.hook').forEach((el, i) => {
    for (const f of ['name', 'url', 'description', 'headersText']) {
      const text = he[i] && he[i][f];
      showMsg($(`[data-hf="${f}"]`, el), text ? { text, kind: 'err' } : null);
      if (text) errors++;
    }
  });

  const me = mcpErrors();
  $$('.mcp').forEach((el, i) => {
    for (const f of ['name', 'cmdText', 'extraText']) {
      const text = me[i] && me[i][f];
      showMsg($(`[data-mf="${f}"]`, el), text ? { text, kind: 'err' } : null);
      if (text) errors++;
    }
  });
  $('#mcp-total').textContent = mcps.length ? `(${mcps.length})` : '';
  $('#mcp-empty').hidden = mcps.length > 0;
  $('#full-banner').hidden = approvals !== 'full';

  const dirty = JSON.stringify(s) !== JSON.stringify(snap);
  const info = $('#save-info');
  info.className = 'save-info' + (errors ? ' invalid' : dirty ? ' dirty' : '');
  info.textContent = errors ? `Fix ${errors} problem${errors > 1 ? 's' : ''} to save`
    : pending ? 'Checking your files...' : dirty ? 'You have unsaved changes' : 'All changes saved';
  $('#btn-save').disabled = !!errors || !!pending || !dirty;
  $('#btn-revert').disabled = !dirty;
  $('#savebar').classList.toggle('show', !!(errors || pending || dirty));

  // setup progress
  const done1 = (s.llama_server && pathRes.llama_server && pathRes.llama_server.ok && pathRes.llama_server.value === s.llama_server)
    || (!s.llama_server && (llamaInfo.bundled || llamaFound.length > 0));
  const done2 = !!(s.model && pathRes.model && pathRes.model.ok && pathRes.model.value === s.model);
  const done3 = chosen.size > 0 || gpusListed;
  const done4 = brain.state === 'ready';
  $('#mem-off').hidden = $('#memory_enabled').checked;
  [done1, done2, done3, done4].forEach((d, i) => $('#step-' + (i + 1)).classList.toggle('done', !!d));
  const doneN = [done1, done2, done3, done4].filter(Boolean).length;
  const NEXT = ['Get the engine', 'Choose a model', 'Pick your graphics card', 'Wake me up'];
  const nextIdx = [done1, done2, done3, done4].findIndex((d) => !d);
  $('#progress-label').textContent = doneN === 4 ? 'All set' : `${doneN} of 4 done`;
  $('#progress-next').textContent = nextIdx < 0 ? '' : `Next: ${NEXT[nextIdx]}`;
  $('#progress-fill').style.width = (doneN * 25) + '%';
  $('#progress-bar').setAttribute('aria-valuenow', String(doneN));
  $('#setup-lead').textContent = doneN === 4 ? "I'm awake and living on your desktop. Say hi, or try something from the Chat tab." : 'Four quick steps and your new buddy comes to life. Everything runs on this computer.';
  $$('#suggest button').forEach((b) => { b.disabled = !done4; });
  $('#dot-setup').classList.toggle('on', !done2);
  return { s, errors, pending, dirty };
}

// One debounce per field: a shared one let a later field cancel an earlier field's check, leaving it on
// "Checking..." forever (and Save disabled).
const pathCheckers = {};
function checkPath(id) {
  pathCheckers[id] ||= debounce(async () => {
    const value = $('#' + id).value.trim();
    if (!value) { delete pathRes[id]; refresh(); return; }
    try {
      const r = await (id === 'voice_ref_path' ? invoke('check_voice', { path: value }) : invoke('check_path', { path: value, kind: PATHS[id] }));
      pathRes[id] = { value, ok: r.ok, message: r.message };
    } catch (e) { pathRes[id] = { value, ok: false, message: String(e) }; }
    refresh();
  }, 250);
  pathCheckers[id]();
}

// ------------------------------------------------------------------ rendering
function renderChars() {
  const host = $('#characters');
  host.innerHTML = '';
  for (const c of CHARACTERS) {
    const b = document.createElement('button');
    b.className = 'char'; b.type = 'button'; b.setAttribute('role', 'radio');
    b.setAttribute('aria-checked', String(c.id === character));
    b.dataset.id = c.id;
    b.innerHTML = `<svg viewBox="0 0 200 240" aria-hidden="true">${characterSvg(c.id, { forPreview: true })}</svg><strong></strong><small></small>`;
    $('strong', b).textContent = c.name; $('small', b).textContent = c.tagline;
    b.onclick = () => chooseCharacter(c.id);
    host.appendChild(b);
  }
}

async function syncFromBackend() {
  const s = await invoke('get_settings');
  const nameEdited = $('#name').value.trim() !== current.name;
  current = s;
  character = s.character;
  if (!nameEdited) { $('#name').value = s.name; snap.name = s.name; }
  snap.character = s.character;
  renderChars();
  refresh();
}

async function chooseCharacter(id) {
  if (id === character) return;
  try { await invoke('set_character', { id }); await syncFromBackend(); toast(`${CHARACTERS.find((c) => c.id === id).name} is on your screen now.`); }
  catch (e) { toast(String(e), true); }
}

function renderGpus(list) {
  const host = $('#gpus');
  host.innerHTML = '';
  for (const [id, desc] of list) {
    const row = document.createElement('label');
    row.className = 'gpu';
    row.innerHTML = '<input type="checkbox"><span><b></b><small></small></span>';
    $('b', row).textContent = id; $('small', row).textContent = desc;
    const cb = $('input', row);
    cb.checked = chosen.has(id);
    cb.onchange = () => { cb.checked ? chosen.add(id) : chosen.delete(id); refresh(); };
    host.appendChild(row);
  }
}

function renderModels() {
  const host = $('#models');
  host.innerHTML = '';
  const cur = { model: $('#model').value.trim(), mmproj: $('#mmproj').value.trim() };
  for (const m of models) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'pick';
    const field = m.projector ? 'mmproj' : 'model';
    b.setAttribute('aria-pressed', String(cur[field] === m.path));
    b.title = m.path;
    b.innerHTML = '<span class="name"></span><span class="tag"></span><span class="sub"></span>';
    $('.name', b).textContent = m.name;
    $('.tag', b).textContent = m.projector ? 'sight & hearing' : 'model';
    $('.sub', b).textContent = `${m.size_mb >= 1024 ? (m.size_mb / 1024).toFixed(1) + ' GB' : m.size_mb + ' MB'}`;
    b.onclick = () => {
      setValue(field, m.path);
      if (!m.projector && !$('#mmproj').value.trim()) {                       // suggest the projector sitting next to it
        const mate = models.find((x) => x.projector && dirname(x.path) === dirname(m.path));
        if (mate) setValue('mmproj', mate.path);
      }
      renderModels();
    };
    host.appendChild(b);
  }
}

function setValue(id, v) { $('#' + id).value = v; checkPath(id); refresh(); }

function renderHooks() {
  const host = $('#hooks');
  host.innerHTML = '';
  hooks.forEach((h, i) => {
    const el = document.createElement('div');
    el.className = 'hook';
    el.innerHTML = `<div class="grid2">
      <div class="field" data-hf="name"><label>Name (letters, digits, _ -)</label><input type="text" maxlength="40" autocomplete="off" spellcheck="false"><div class="msg"></div></div>
      <div class="field" data-hf="url"><label>Web address</label><input type="text" autocomplete="off" spellcheck="false" placeholder="https://"><div class="msg"></div></div>
      <div class="field full" data-hf="description"><label>What it does (I read this to know when to use it)</label><input type="text" autocomplete="off"><div class="msg"></div></div>
      <div class="field full" data-hf="headersText"><label>Headers, one per line (optional)</label><textarea spellcheck="false" placeholder="Authorization: Bearer ..."></textarea><div class="msg"></div></div>
    </div><div class="hook-foot"><button type="button" class="btn danger">Remove</button></div>`;
    for (const f of ['name', 'url', 'description', 'headersText']) {
      const input = $(`[data-hf="${f}"] ${f === 'headersText' ? 'textarea' : 'input'}`, el);
      input.value = h[f];
      input.oninput = () => { h[f] = input.value; refresh(); };
    }
    $('.btn.danger', el).onclick = () => { hooks.splice(i, 1); renderHooks(); refresh(); };
    host.appendChild(el);
  });
}

function fill(s) {
  for (const k of TEXT) {
    if (k === 'tts_voice' && ![...$('#tts_voice').options].some((o) => o.value === s[k])) {
      const o = document.createElement('option'); o.value = o.textContent = s[k]; $('#tts_voice').appendChild(o);
    }
    $('#' + k).value = s[k] ?? '';
  }
  for (const k of NUM) $('#' + k).value = s[k] ?? '';
  $('#always_look').checked = !!s.always_look;
  $('#avoid_cursor').checked = s.avoid_cursor !== false;
  $('#hear_tone').checked = s.hear_tone !== false;
  $('#own-voice').open = !!s.voice_ref_path;
  $('#memory_enabled').checked = s.memory_enabled !== false;
  for (const k of CHECKS) $('#' + k).checked = !!s[k];
  showEngineFields();
  chosen = new Set(s.devices || []);
  character = s.character || 'pip';
  approvals = s.approvals || 'risky';
  listenMode = s.listen_mode || 'always';
  ackFull = !!s.full_access_ack;
  $$('input[name="approvals"]').forEach((r) => { r.checked = r.value === approvals; });
  $('#voice-clear').onclick = () => setValue('voice_ref_path', '');
$('#voice-hear').onclick = () => invoke('test_voice').then(() => toast('Listen...')).catch((e) => toast(String(e), true));
$('#voice-next').onclick = async () => {
  try {
    const n = await invoke('voice_next');
    current.voice_variant = snap.voice_variant = n;
    toast('Made a new voice. Listen...');
    setTimeout(() => invoke('test_voice').catch((e) => toast(String(e), true)), 400);
  } catch (e) { toast(String(e), true); }
};
$$('input[name="listen_mode"]').forEach((r) => { r.checked = r.value === listenMode; });
  $('#speak_aloud').checked = s.speak_aloud !== false;
  setVolumeUi(s.volume ?? 100);
  syncAsrPreset();
  hooks = (s.webhooks || []).map((w) => ({
    name: w.name, description: w.description, url: w.url,
    headersText: Object.entries(w.headers || {}).map(([k, v]) => `${k}: ${v}`).join('\n'),
  }));
  mcps = (s.mcp_servers || []).map(mcpRow);
  $('#own-llama').open = !!s.llama_server || !llamaInfo.bundled;
  setSizeUi(s.size_pct || 80);
  renderArea();
  renderChars(); renderHooks(); renderMcps();
  renderGpus([...chosen].map((d) => [d, 'saved choice - press the button above to refresh']));
  pathRes = {};
  for (const id of Object.keys(PATHS)) checkPath(id);
}

// ------------------------------------------------------------------ model download
const fmtBytes = (n) => (n >= 1073741824 ? (n / 1073741824).toFixed(2) + ' GB' : Math.round(n / 1048576) + ' MB');
let dl = { state: 'idle', paths: null };   // idle | running | ready

function setDl(state, msg = '', kind = '') {
  dl.state = state;
  if (state !== 'running') { dl.last = null; dl.speed = 0; }
  $('#dl-start').hidden = state === 'running';
  $('#dl-cancel').hidden = state !== 'running';
  $('#dl-start').textContent = state === 'ready' ? 'Use this model' : msg && kind === 'err' ? 'Try again (resumes)' : 'Download (1.4 GB)';
  if (state !== 'running') $('#dl-bar').hidden = true;
  const m = $('#dl-msg'); m.textContent = msg; m.className = 'hint dl-msg ' + kind;
}
function useDownloaded() {
  if (!dl.paths) return;
  setValue('model', dl.paths.model); setValue('mmproj', dl.paths.mmproj);
  toast('Model selected. Press "Save & wake me up" when you are ready.');
  $('#btn-wake').scrollIntoView({ block: 'center', behavior: 'smooth' });
}
async function initDownload() {
  try {
    const st = await invoke('download_status');
    const get = (k) => st.items.find((i) => i.key === k);
    if (st.items.every((i) => i.ready)) { dl.paths = { model: get('model').path, mmproj: get('mmproj').path }; setDl('ready', 'Already downloaded on this PC.', 'ok'); }
    else if (st.running) { setDl('running'); $('#dl-bar').hidden = false; }
    else setDl('idle');
  } catch { setDl('idle'); }
}
$('#dl-start').onclick = async () => {
  if (dl.state === 'ready') return useDownloaded();
  try {
    await invoke('start_download');
    setDl('running', 'Connecting to Hugging Face...'); $('#dl-bar').hidden = false; $('#dl-fill').style.width = '0%';
  } catch (e) { setDl('idle', String(e), 'err'); }
};
$('#dl-cancel').onclick = () => { invoke('cancel_download'); $('#dl-msg').textContent = 'Stopping...'; };
T.event.listen('download', (e) => {
  const d = e.payload;
  if (d.state === 'progress') {
    const pct = Math.min(100, Math.floor((d.done / d.total) * 100));
    dl.state = 'running'; $('#dl-start').hidden = true; $('#dl-cancel').hidden = false; $('#dl-bar').hidden = false;
    $('#dl-fill').style.width = pct + '%'; $('#dl-bar').setAttribute('aria-valuenow', pct);
    // speed: smoothed over the last few seconds, so the estimate doesn't jump around
    const now = Date.now();
    if (dl.last && now > dl.last.t && d.done >= dl.last.done) {
      const inst = (d.done - dl.last.done) / ((now - dl.last.t) / 1000);
      dl.speed = dl.speed ? dl.speed * 0.85 + inst * 0.15 : inst;
    }
    dl.last = { t: now, done: d.done };
    let tail = '';
    if (dl.speed > 1024) {
      const left = (d.total - d.done) / dl.speed;
      tail = ` · ${(dl.speed / 1048576).toFixed(1)} MB/s · ${left < 60 ? 'under a minute' : `about ${Math.ceil(left / 60)} min`} left`;
    }
    $('#dl-msg').className = 'hint dl-msg';
    $('#dl-msg').textContent = `Downloading ${d.file === 'mmproj' ? 'sight & hearing file' : 'model'}: ${fmtBytes(d.done)} of ${fmtBytes(d.total)} (${pct}%)${tail}`;
  } else if (d.state === 'verifying') {
    $('#dl-msg').className = 'hint dl-msg';
    $('#dl-msg').textContent = 'Checking the files are exactly the official ones...';
  } else if (d.state === 'done') {
    dl.paths = { model: d.model_path, mmproj: d.mmproj_path };
    if (d.adopted) {
      setDl('ready', 'Downloaded, checked and in use. Waking up...', 'ok');
      toast('Model ready. Waking up now...');
    } else {
      setDl('ready', 'Downloaded and checked.', 'ok');
      useDownloaded();
    }
  } else if (d.state === 'cancelled') setDl('idle', 'Paused. Press Download to resume where it stopped.');
  else setDl('idle', d.message || 'The download failed.', 'err');
});

// ------------------------------------------------------------------ memory
let memConfirm;
function fmtDate(sec) { return new Date(sec * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }); }
async function loadMemory() {
  let list = [];
  try { list = await invoke('list_memory'); } catch (e) { toast(String(e), true); }
  const host = $('#mem-list');
  host.innerHTML = '';
  for (const m of [...list].reverse()) {
    const li = document.createElement('li');
    li.className = 'mem-item';
    li.innerHTML = '<div class="text"><span class="t"></span><span class="meta"><span class="by"></span> · <span class="d"></span></span></div><button type="button"><svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg></button>';
    $('.t', li).textContent = m.text;
    $('.by', li).textContent = m.by === 'user' ? 'You' : current.name || 'Nuzzle';
    $('.d', li).textContent = fmtDate(m.created);
    const b = $('button', li);
    b.setAttribute('aria-label', 'Forget: ' + m.text); b.title = 'Forget this';
    b.onclick = () => invoke('delete_memory', { id: m.id }).catch((e) => toast(String(e), true));
    host.appendChild(li);
  }
  $('#mem-empty').hidden = list.length > 0;
  $('#mem-total').textContent = list.length ? `(${list.length})` : '';
  $('#mem-clear').disabled = !list.length;
}
async function addMemory() {
  const input = $('#mem-new'), text = input.value.trim();
  if (!text) return;
  $('#mem-add').disabled = true;
  try {
    await invoke('add_memory', { text });
    input.value = ''; showMsg($('.field[data-field="mem_new"]'), null);
    toast('Saved to memory.');
  } catch (e) { showMsg($('.field[data-field="mem_new"]'), { text: String(e), kind: 'err' }); }
  memInput();
}
function memInput() {
  const n = [...$('#mem-new').value].length;
  $('#mem-count').textContent = n;
  $('#mem-add').disabled = !$('#mem-new').value.trim();
  if (n > 0 && n <= 300) showMsg($('.field[data-field="mem_new"]'), null);
}
$('#mem-new').addEventListener('input', memInput);
$('#mem-new').addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); addMemory(); } });
$('#mem-add').onclick = addMemory;
$('#mem-clear').onclick = async () => {
  const b = $('#mem-clear');
  if (!memConfirm) {                                    // two taps so nothing is lost by accident
    memConfirm = setTimeout(() => { memConfirm = null; b.textContent = 'Clear all'; }, 4000);
    b.textContent = 'Tap again to clear all';
    return;
  }
  clearTimeout(memConfirm); memConfirm = null; b.textContent = 'Clear all';
  try { await invoke('clear_memory'); toast('Memory cleared.'); } catch (e) { toast(String(e), true); }
};
T.event.listen('memory', () => loadMemory());

// ------------------------------------------------------------------ MCP servers
// Each server is one line: the command to run (exactly as MCP guides print it) or a web address. Keys and tokens
// go in the optional box under it. Paste a whole "mcpServers" JSON block to add several at once.
const MCP_TEMPLATES = [
  { id: 'files', label: 'Files', desc: 'Read and edit files in one folder', cmd: (docs) => `npx -y @modelcontextprotocol/server-filesystem ${quoteArg(docs)}`, note: 'Change the folder at the end if you like. Needs Node.js.' },
  { id: 'memory', label: 'Knowledge graph', desc: 'Long-term notes as a graph', cmd: () => 'npx -y @modelcontextprotocol/server-memory', note: 'Needs Node.js.' },
  { id: 'fetch', label: 'Web fetch', desc: 'Read pages as clean text', cmd: () => 'uvx mcp-server-fetch', note: 'Needs uv (uvx).' },
  { id: 'time', label: 'Time zones', desc: 'Time anywhere, conversions', cmd: () => 'uvx mcp-server-time', note: 'Needs uv (uvx).' },
  { id: 'git', label: 'Git', desc: 'History and diffs of a repo', cmd: () => 'uvx mcp-server-git', note: 'Needs uv (uvx).' },
  { id: 'github', label: 'GitHub', desc: 'Issues, PRs, code search', cmd: () => 'https://api.githubcopilot.com/mcp/', env: 'Authorization: Bearer YOUR_GITHUB_TOKEN', note: 'Put your GitHub token in the Keys box.' },
  { id: 'playwright', label: 'Browser', desc: 'Drive a web browser', cmd: () => 'npx -y @playwright/mcp@latest', note: 'Needs Node.js.' },
];
const blankMcp = () => ({ name: '', cmdText: '', extraText: '', enabled: true, showExtra: false });

/** Split a command line into words, keeping "quoted parts" together. */
function splitCmd(text) {
  const out = []; let cur = '', q = null, any = false;
  for (const ch of text.trim()) {
    if (q) { if (ch === q) q = null; else cur += ch; }
    else if (ch === '"' || ch === "'") { q = ch; any = true; }
    else if (/\s/.test(ch)) { if (cur || any) out.push(cur); cur = ''; any = false; }
    else cur += ch;
  }
  if (cur || any) out.push(cur);
  return out;
}
const quoteArg = (a) => (a !== '' && !/[\s"']/.test(a) ? a : a.includes('"') ? `'${a}'` : `"${a}"`);
const isUrl = (t) => /^https?:\/\//i.test(t.trim());

/** Editor row -> the settings shape the backend keeps. */
function mcpCfg(m) {
  const http = isUrl(m.cmdText);
  const words = http ? [] : splitCmd(m.cmdText);
  return {
    name: m.name.trim(), kind: http ? 'http' : 'stdio', command: words[0] || '', args: words.slice(1),
    env: http ? {} : parseEnv(m.extraText).value || {}, url: http ? m.cmdText.trim() : '',
    headers: http ? parseHeaders(m.extraText).value || {} : {}, enabled: !!m.enabled,
  };
}
/** Settings -> editor row. */
function mcpRow(c) {
  const http = c.kind === 'http';
  const extra = http ? Object.entries(c.headers || {}).map(([k, v]) => `${k}: ${v}`) : Object.entries(c.env || {}).map(([k, v]) => `${k}=${v}`);
  return { name: c.name || '', cmdText: http ? c.url || '' : [c.command || '', ...(c.args || [])].filter((x, i) => i > 0 || x).map(quoteArg).join(' '),
    extraText: extra.join('\n'), enabled: c.enabled !== false, showExtra: extra.length > 0 };
}
function mcpErrors() {
  const seen = new Set();
  return mcps.map((m) => {
    const e = {};
    const name = m.name.trim();
    if (!/^[a-z0-9_-]{1,24}$/.test(name)) e.name = 'Use 1-24 lowercase letters, digits, - or _.';
    else if (seen.has(name)) e.name = 'Another server already uses this name.';
    seen.add(name);
    if (!m.cmdText.trim()) e.cmdText = 'Enter the command (like npx -y some-server) or a web address.';
    else if (isUrl(m.cmdText)) {
      try { new URL(m.cmdText.trim()); } catch { e.cmdText = 'That web address looks incomplete.'; }
      const ph = parseHeaders(m.extraText); if (ph.error) e.extraText = `${ph.error} (for a web address, write Name: value)`;
    } else {
      if (splitCmd(m.cmdText).length > 51) e.cmdText = 'That is too many arguments (50 max).';
      const pe = parseEnv(m.extraText); if (pe.error) e.extraText = pe.error;
    }
    return e;
  });
}
function uniqueName(base) {
  const clean = (base || 'server').toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 20) || 'server';
  let name = clean, n = 2;
  while (mcps.some((m) => m.name === name)) name = `${clean}${n++}`;
  return name;
}
function addMcp(row, note) {
  mcps.push({ ...blankMcp(), ...row, name: uniqueName(row.name) });
  renderMcps(); refresh();
  if (note) toast(note);
  const last = $$('.mcp').pop(); if (last) last.scrollIntoView({ block: 'center' });
}

async function renderGallery() {
  let docs = '~/Documents';
  try { docs = (await T.path.documentDir()).replace(/[\\/]+$/, ''); } catch { /* keep the placeholder */ }
  const g = $('#mcp-gallery');
  g.innerHTML = '';
  for (const t of MCP_TEMPLATES) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'mcp-tile';
    b.innerHTML = `<span class="mcp-ico">${iconSvg(t.id)}</span><strong></strong><small></small>`;
    $('strong', b).textContent = t.label; $('small', b).textContent = t.desc;
    b.onclick = () => addMcp({ name: t.id, cmdText: t.cmd(docs), extraText: t.env || '', showExtra: !!t.env }, t.note);
    g.appendChild(b);
  }
}

function renderMcps() {
  const host = $('#mcps');
  host.innerHTML = '';
  mcps.forEach((m, i) => {
    const el = document.createElement('div');
    el.className = 'mcp';
    el.innerHTML = `<div class="mcp-top">
        <span class="mcp-ico" data-mico></span>
        <div class="field mcp-name" data-mf="name"><label class="sr">Server name</label><input type="text" maxlength="24" autocomplete="off" spellcheck="false" placeholder="Name, e.g. files"><div class="msg"></div></div>
        <span class="status" data-mstatus data-state="off"><i></i><span>Not saved</span></span>
        <label class="switch sm"><input type="checkbox" data-men aria-label="Turn this server on"><span class="track"></span></label>
      </div>
      <div class="bar indet mcp-load" data-mload hidden role="progressbar" aria-label="Starting server"><i></i></div>
      <div class="field full" data-mf="cmdText"><label>Command or web address</label><input type="text" autocomplete="off" spellcheck="false" placeholder="npx -y @modelcontextprotocol/server-memory   or   https://example.com/mcp"><div class="msg"></div></div>
      <div class="field full" data-mf="extraText" data-mextrabox><label data-mextralabel></label><textarea spellcheck="false"></textarea><div class="msg"></div></div>
      <div class="mcp-tools" data-mtools></div>
      <div class="mcp-err" data-merr hidden><span data-merr-text></span><button type="button" class="btn sm" data-mretry>Try again</button></div>
      <div class="mcp-foot"><button type="button" class="btn ghost sm" data-mextra>Keys &amp; settings</button><span class="spacer"></span><button type="button" class="btn danger sm" data-mdel>Remove</button></div>`;
    const bind = (f, sel) => {
      const input = $(`[data-mf="${f}"] ${sel}`, el);
      input.value = m[f];
      input.addEventListener('input', () => { m[f] = input.value; extraLabel(); refresh(); applyMcpStatus(); });
    };
    const extraLabel = () => {
      const http = isUrl(m.cmdText);
      $('[data-mextralabel]', el).textContent = http ? 'Headers, one per line (Authorization: Bearer ...)' : 'Keys and settings, one per line (API_KEY=...)';
      $('[data-mextrabox] textarea', el).placeholder = http ? 'Authorization: Bearer ...' : 'API_KEY=...';
      $('[data-mextrabox]', el).hidden = !m.showExtra;
    };
    bind('name', 'input'); bind('cmdText', 'input'); bind('extraText', 'textarea');
    extraLabel();
    const en = $('[data-men]', el);
    en.checked = m.enabled;
    en.onchange = () => { m.enabled = en.checked; refresh(); applyMcpStatus(); };
    $('[data-mextra]', el).onclick = () => { m.showExtra = !m.showExtra; extraLabel(); };
    $('[data-mdel]', el).onclick = () => { mcps.splice(i, 1); renderMcps(); refresh(); };
    $('[data-mretry]', el).onclick = (e) => busy(e.currentTarget, 'Trying...', () => invoke('mcp_reload').catch((err) => toast(String(err), true)));
    host.appendChild(el);
  });
  applyMcpStatus();
}

/** Import Claude Desktop / Cursor / VS Code style JSON: {"mcpServers": {...}}, {"servers": {...}} or one server. */
function importMcpJson(text) {
  let v;
  try { v = JSON.parse(text); } catch { return { error: "That isn't valid JSON. Copy the whole block, braces included." }; }
  let map = v.mcpServers || v.servers || (v.mcp && v.mcp.servers) || null;
  if (!map && (v.command || v.url)) map = { [v.name || 'server']: v };
  if (!map || typeof map !== 'object') return { error: 'I found no servers in it (expected "mcpServers").' };
  let n = 0;
  for (const [name, c] of Object.entries(map)) {
    if (!c || typeof c !== 'object') continue;
    const url = c.url || c.serverUrl || '';
    const cfg = url ? { kind: 'http', url, headers: c.headers || {} } : { kind: 'stdio', command: c.command || '', args: c.args || [], env: c.env || {} };
    if (!cfg.url && !cfg.command) continue;
    mcps.push({ ...mcpRow({ name, ...cfg, enabled: c.disabled !== true }), name: uniqueName(name) });
    n++;
  }
  return n ? { added: n } : { error: 'Those entries have no command or url.' };
}
$('#mcp-custom').onclick = () => addMcp({ name: 'server', cmdText: '' });
$('#mcp-paste').onclick = () => { $('#mcp-paste-box').hidden = false; $('#mcp-paste-text').focus(); };
$('#mcp-paste-cancel').onclick = () => { $('#mcp-paste-box').hidden = true; $('#mcp-paste-msg').textContent = ''; };
$('#mcp-paste-add').onclick = () => {
  const r = importMcpJson($('#mcp-paste-text').value);
  if (r.error) { $('#mcp-paste-msg').textContent = r.error; return; }
  $('#mcp-paste-text').value = ''; $('#mcp-paste-box').hidden = true; $('#mcp-paste-msg').textContent = '';
  renderMcps(); refresh();
  toast(`Added ${r.added} server${r.added === 1 ? '' : 's'}. Press Save to start ${r.added === 1 ? 'it' : 'them'}.`);
};

/* Live status of each saved server next to its editor row. */
const ICONS = {
  files: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  memory: '<circle cx="6" cy="7" r="2.5"/><circle cx="18" cy="7" r="2.5"/><circle cx="12" cy="18" r="2.5"/><path d="M8 8.5l3 7M16 8.5l-3 7M8.5 7h7"/>',
  fetch: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"/>',
  time: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  git: '<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="9" r="2.5"/><path d="M6 8.5v7M18 11.5c0 4-6 3-11 5"/>',
  github: '<path d="M8 8l-5 4 5 4M16 8l5 4-5 4M14 5l-4 14"/>',
  playwright: '<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M3 9h18M7 6.5h.01M10 6.5h.01"/>',
  web: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  plug: '<path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4"/>',
};
function mcpIconFor(cmd) {
  const c = (cmd || '').toLowerCase();
  const hit = [['filesystem', 'files'], ['server-memory', 'memory'], ['mcp-server-fetch', 'fetch'], ['mcp-server-time', 'time'], ['mcp-server-git', 'git'], ['githubcopilot', 'github'], ['playwright', 'playwright']].find(([k]) => c.includes(k));
  return hit ? hit[1] : isUrl(cmd) ? 'web' : 'plug';
}
const iconSvg = (k) => `<svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[k] || ICONS.plug}</svg>`;

function applyMcpStatus() {
  $$('.mcp').forEach((el, i) => {
    const m = mcps[i];
    if (!m) return;
    const st = mcpStatus.find((s) => s.name === m.name.trim());
    const saved = (current.mcp_servers || []).some((c) => c.name === m.name.trim());
    let state = 'off', text = 'Not saved';
    if (st && saved) {
      state = st.state;
      text = { ready: `Ready · ${st.tools.length} tool${st.tools.length === 1 ? '' : 's'}`, starting: 'Starting...', error: 'Problem', off: 'Off' }[st.state] || st.state;
    }
    const chip = $('[data-mstatus]', el);
    chip.dataset.state = state; $('span', chip).textContent = text;
    el.dataset.state = state;
    $('[data-mload]', el).hidden = state !== 'starting';
    $('[data-mico]', el).innerHTML = iconSvg(mcpIconFor(m.cmdText));
    const tools = $('[data-mtools]', el);
    tools.innerHTML = '';
    if (state === 'starting') tools.innerHTML = '<span class="skel w1"></span><span class="skel w2"></span><span class="skel w3"></span>';
    else if (st && saved && st.state === 'ready') {
      const head = document.createElement('b'); head.textContent = `${st.tools.length} tool${st.tools.length === 1 ? '' : 's'}`; tools.appendChild(head);
      for (const t of st.tools.slice(0, 30)) { const s = document.createElement('span'); s.textContent = t; tools.appendChild(s); }
    }
    const err = $('[data-merr]', el), bad = st && saved && st.state === 'error';
    err.hidden = !bad; $('[data-merr-text]', err).textContent = bad ? st.message : '';
  });
}
async function loadMcpStatus() {
  try { mcpStatus = (await invoke('mcp_status')).servers || []; } catch { mcpStatus = []; }
  applyMcpStatus();
}
$('#mcp-reload').onclick = async () => {
  if (JSON.stringify(readForm().mcp_servers) !== JSON.stringify(current.mcp_servers || [])) { toast('Save your changes first, then I will restart the servers.'); return; }
  await busy($('#mcp-reload'), 'Restarting...', () => invoke('mcp_reload').catch((e) => toast(String(e), true)));
};
renderGallery();
T.event.listen('mcp', () => loadMcpStatus());

// ------------------------------------------------------------------ roaming area
function renderArea() {
  const a = current.roam_area;
  $('#area-summary').textContent = a ? `Inside a ${a.w} x ${a.h} px area at (${a.x}, ${a.y}).` : 'Anywhere on your screen.';
  $('#area-clear').disabled = !a;
}
$('#area-pick').onclick = () => invoke('pick_area').catch((e) => toast(String(e), true));
$('#area-clear').onclick = () => invoke('clear_area').then(() => toast('I will roam anywhere again.')).catch((e) => toast(String(e), true));
T.event.listen('area', (e) => {
  current.roam_area = e.payload; snap.roam_area = e.payload;
  renderArea(); refresh();
  toast(e.payload ? 'Roaming area saved.' : 'Roaming anywhere.');
});

// ------------------------------------------------------------------ reminders
async function loadReminders() {
  let list = [];
  try { list = await invoke('list_reminders'); } catch { /* none */ }
  const now = Date.now() / 1000;
  const host = $('#rem-list');
  host.innerHTML = '';
  for (const r of [...list].sort((a, b) => a.due - b.due)) {
    const li = document.createElement('li');
    li.className = 'mem-item';
    li.innerHTML = '<div class="text"><span class="t"></span><span class="meta"></span></div><button type="button"><svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg></button>';
    $('.t', li).textContent = r.text;
    const mins = Math.max(0, Math.round((r.due - now) / 60));
    $('.meta', li).textContent = mins < 1 ? 'in under a minute' : mins < 90 ? `in ${mins} min` : `at ${new Date(r.due * 1000).toLocaleString(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' })}`;
    if (r.every) {                                            // a repeating reminder says how often
      const m = Math.round(r.every / 60);
      $('.meta', li).textContent += ` · repeats every ${m % 1440 === 0 ? (m === 1440 ? 'day' : m / 1440 + ' days') : m % 60 === 0 ? (m === 60 ? 'hour' : m / 60 + ' hours') : m + ' min'}`;
    }
    const b = $('button', li);
    b.setAttribute('aria-label', 'Cancel reminder: ' + r.text); b.title = 'Cancel this reminder';
    b.onclick = () => invoke('delete_reminder', { id: r.id }).catch((e) => toast(String(e), true));
    host.appendChild(li);
  }
  $('#rem-empty').hidden = list.length > 0;
  $('#rem-total').textContent = list.length ? `(${list.length})` : '';
}
T.event.listen('reminders', () => loadReminders());
setInterval(loadReminders, 30000);

// ------------------------------------------------------------------ appearance (this window only)
const UI_DEFAULT = { theme: 'system', density: 'compact', font: 'system', scale: 100 };
let ui = { ...UI_DEFAULT };
try { ui = { ...UI_DEFAULT, ...JSON.parse(localStorage.getItem('nuzzle-ui') || '{}') }; } catch { /* defaults */ }
const sendZoom = debounce((pct) => invoke('ui_zoom', { label: 'settings', pct }).catch(() => {}), 80);
function applyUi(persist = true) {
  const r = document.documentElement;
  r.dataset.theme = ui.theme; r.dataset.density = ui.density; r.dataset.font = ui.font;
  $('#ui-theme').value = ui.theme; $('#ui-density').value = ui.density; $('#ui-font').value = ui.font;
  $('#ui-scale').value = ui.scale; $('#ui-scale-out').textContent = ui.scale + '%';
  sendZoom(ui.scale);
  if (persist) { try { localStorage.setItem('nuzzle-ui', JSON.stringify(ui)); } catch { /* private mode */ } }
}
for (const [id, key] of [['ui-theme', 'theme'], ['ui-density', 'density'], ['ui-font', 'font']]) {
  $('#' + id).addEventListener('change', () => { ui[key] = $('#' + id).value; applyUi(); });
}
$('#ui-scale').addEventListener('input', () => { ui.scale = Number($('#ui-scale').value); applyUi(); });
$$('[data-scale]').forEach((b) => { b.onclick = () => { ui.scale = Number(b.dataset.scale); applyUi(); }; });
$('#ui-reset').onclick = () => { ui = { ...UI_DEFAULT }; applyUi(); toast('Appearance reset.'); };
window.addEventListener('keydown', (e) => {
  if (!(e.ctrlKey || e.metaKey)) return;
  const step = { '=': 5, '+': 5, '-': -5, '_': -5 }[e.key];
  if (step) { e.preventDefault(); ui.scale = Math.min(150, Math.max(70, ui.scale + step)); applyUi(); }
  else if (e.key === '0') { e.preventDefault(); ui.scale = 100; applyUi(); }
});

// ------------------------------------------------------------------ built-in llama.cpp
async function initLlama() {
  try { llamaInfo = await invoke('llama_info'); } catch { llamaInfo = { bundled: false }; }
  $('#llama-built').hidden = !llamaInfo.bundled;
  $('#llama-missing').hidden = llamaInfo.bundled;
  if (llamaInfo.bundled) $('#llama-built-title').textContent = `llama.cpp ${llamaInfo.version || ''} is built in`.replace('  ', ' ');
  $('#own-llama').open = !!$('#llama_server').value.trim() || !llamaInfo.bundled;
  refresh();
}

// ------------------------------------------------------------------ chat history (this session only)
const fmtClock = (sec) => new Date(sec * 1000).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
let chatLines = [];
async function loadChat() {
  try { chatLines = await invoke('list_transcript'); } catch { chatLines = []; }
  const host = $('#chat-log');
  const stick = host.scrollHeight - host.scrollTop - host.clientHeight < 40;
  host.innerHTML = '';
  for (const m of chatLines) {
    const li = document.createElement('li');
    li.className = 'msg-' + m.role;
    li.innerHTML = '<span class="who"></span><span class="said"></span><time></time>';
    $('.who', li).textContent = m.role === 'user' ? 'You' : current.name || 'Nuzzle';
    $('.said', li).textContent = m.text;
    $('time', li).textContent = fmtClock(m.t);
    host.appendChild(li);
  }
  if (stick) host.scrollTop = host.scrollHeight;
  $('#chat-empty').hidden = chatLines.length > 0;
  $('#chat-total').textContent = chatLines.length ? `(${chatLines.length})` : '';
  $('#chat-copy').disabled = $('#chat-clear').disabled = !chatLines.length;
}
$('#chat-copy').onclick = () => {
  const text = chatLines.map((m) => `${m.role === 'user' ? 'You' : current.name || 'Nuzzle'}: ${m.text}`).join('\n');
  navigator.clipboard.writeText(text).then(() => toast('Copied.'), () => toast("I couldn't copy that.", true));
};
$('#chat-clear').onclick = () => invoke('clear_transcript');
$('#chat-say').addEventListener('input', () => { $('#chat-send-btn').disabled = !$('#chat-say').value.trim(); });
$('#chat-form').addEventListener('submit', (e) => {
  e.preventDefault();
  const text = $('#chat-say').value.trim();
  if (!text) return;
  invoke('send_text', { text }).then(() => { $('#chat-say').value = ''; $('#chat-send-btn').disabled = true; }).catch((err) => toast(String(err), true));
});
T.event.listen('transcript', () => loadChat());

// ------------------------------------------------------------------ microphone & speaker (apply at once)
function fillDevices(sel, names, chosen, fallbackName) {
  sel.innerHTML = '';
  const add = (value, text) => { const o = document.createElement('option'); o.value = value; o.textContent = text; sel.appendChild(o); };
  add('', fallbackName ? `System default (${fallbackName})` : 'System default');
  for (const n of names) add(n, n);
  if (chosen && !names.includes(chosen)) add(chosen, `${chosen} (not connected)`);
  sel.value = chosen || '';
}
async function loadDevices() {
  let d;
  try { d = await invoke('audio_devices'); } catch (e) { $('#mic-using').textContent = String(e); return; }
  fillDevices($('#mic_device'), d.mics, current.mic_device, d.default_mic);
  fillDevices($('#speaker_device'), d.speakers, current.speaker_device, d.default_speaker);
  $('#mic-using').textContent = d.using_mic ? `Listening with: ${d.using_mic}` : 'No microphone is working right now.';
  $('#spk-using').textContent = d.using_speaker ? `Playing on: ${d.using_speaker}` : 'No speaker is working right now.';
}
function setVolumeUi(pct) { $('#volume').value = pct; $('#volume-out').textContent = pct + '%'; }
async function applyAudio() {
  const a = { mic: $('#mic_device').value, speaker: $('#speaker_device').value, volume: Number($('#volume').value), speakAloud: $('#speak_aloud').checked };
  try {
    await invoke('set_audio', a);
    Object.assign(current, { mic_device: a.mic, speaker_device: a.speaker, volume: a.volume, speak_aloud: a.speakAloud });
    Object.assign(snap, { mic_device: a.mic, speaker_device: a.speaker, volume: a.volume, speak_aloud: a.speakAloud });
    refresh();
    setTimeout(loadDevices, 700);                     // the audio thread switches within moments
  } catch (e) { toast(String(e), true); }
}
const applyAudioSoon = debounce(applyAudio, 150);
$('#mic_device').addEventListener('change', applyAudio);
$('#speaker_device').addEventListener('change', applyAudio);
$('#speak_aloud').addEventListener('change', applyAudio);
$('#volume').addEventListener('input', () => { setVolumeUi(Number($('#volume').value)); applyAudioSoon(); });
$('#test-chime').onclick = () => invoke('test_speaker').catch((e) => toast(String(e), true));
$('#test-voice').onclick = () => invoke('test_voice').then(() => toast('Listen...')).catch((e) => toast(String(e), true));
T.event.listen('levels', (e) => { const v = Math.min(100, Math.round(e.payload.mic * 400)); $('#mic-meter').style.width = v + '%'; $('#mic-meter-box').setAttribute('aria-valuenow', v); });
T.event.listen('audio-devices', () => loadDevices());
T.event.listen('settings-shown', () => loadDevices());

$$('input[name="listen_mode"]').forEach((r) => { r.onchange = () => { listenMode = r.value; refresh(); }; });

// Shortcut boxes: press the keys instead of typing their names.
const KEY_NAMES = { Space: 'Space', Enter: 'Enter', Tab: 'Tab', ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown', Insert: 'Insert' };
function accelerator(e) {
  let key = KEY_NAMES[e.code];
  if (!key && /^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (!key && /^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (!key && /^F\d{1,2}$/.test(e.code)) key = e.code;
  if (!key) return null;
  // CommandOrControl is Cmd on a Mac and Ctrl elsewhere, so record whichever of those was really pressed.
  const mods = MAC
    ? [e.metaKey && 'CommandOrControl', e.ctrlKey && 'Control', e.altKey && 'Alt', e.shiftKey && 'Shift'].filter(Boolean)
    : [e.ctrlKey && 'CommandOrControl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean);
  if (!mods.length && !/^F\d/.test(key)) return null;   // a bare letter would block typing everywhere
  return [...mods, key].join('+');
}
for (const id of ['ptt_shortcut', 'chat_shortcut']) {
  $('#' + id).addEventListener('keydown', (e) => {
    if (e.key === 'Tab') return;
    e.preventDefault();
    if ((e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.altKey) { $('#' + id).value = ''; refresh(); return; }
    if (e.key === 'Escape') { $('#' + id).blur(); return; }
    const acc = accelerator(e);
    if (acc) { $('#' + id).value = acc; refresh(); }
  });
}

function syncAsrPreset() {
  const v = $('#asr_prompt').value.trim();
  const sel = $('#asr-preset');
  sel.value = [...sel.options].some((o) => o.value === v) ? v : '';
}
$('#asr-preset').addEventListener('change', () => { if ($('#asr-preset').value) { $('#asr_prompt').value = $('#asr-preset').value; refresh(); } });
$('#asr_prompt').addEventListener('input', syncAsrPreset);

// ------------------------------------------------------------------ size
const sendSize = debounce(async (pct) => {
  try { await invoke('set_size', { pct }); current.size_pct = pct; snap.size_pct = pct; refresh(); }
  catch (e) { toast(String(e), true); }
}, 120);
function setSizeUi(pct) { $('#size').value = pct; $('#size-out').textContent = pct + '%'; }
$('#size').addEventListener('input', () => { const p = Number($('#size').value); setSizeUi(p); sendSize(p); });
$$('[data-size]').forEach((b) => { b.onclick = () => { const p = Number(b.dataset.size); setSizeUi(p); sendSize(p); }; });

// ------------------------------------------------------------------ actions
async function save() {
  const { s, errors, pending } = refresh();
  if (errors || pending) return false;
  const btn = $('#btn-save');
  btn.disabled = true; btn.classList.add('busy'); btn.textContent = 'Saving...'; setTop(true);
  try {
    const restarting = await invoke('save_settings', { settings: s });
    current = s; snap = readForm();
    toast(restarting ? 'Saved. Reloading my brain...' : 'Saved. Changes apply straight away.');
    return true;
  } catch (e) { toast(String(e), true); return false; }
  finally { btn.classList.remove('busy'); btn.textContent = 'Save'; setTop(false); refresh(); }
}

async function wake() {
  const s = readForm();
  const r = pathRes.model;
  if (!s.model || !r || !r.ok || r.value !== s.model) {
    toast('Please choose a valid model file first (step 2).', true);
    $('#step-2').scrollIntoView({ block: 'center' });
    $('#model').focus();
    return;
  }
  await busy($('#btn-wake'), 'Saving...', save);
}

async function pick(id) {
  const filters = id === 'llama_server' ? [] : id === 'voice_ref_path' ? [{ name: 'WAV audio', extensions: ['wav'] }] : [{ name: 'GGUF model', extensions: ['gguf'] }];
  try {
    const path = await T.dialog.open({ multiple: false, filters });
    if (path) setValue(id, path);
  } catch (e) { toast("I couldn't open the file picker: " + e, true); }
}

async function detectLlama() {
  llamaFound = await busy($('#btn-detect-llama'), 'Searching...', () => invoke('detect_llama').catch(() => []));
  const host = $('#llama-found');
  host.innerHTML = '';
  if (!llamaFound.length) {
    host.innerHTML = '<span class="hint">I could not find it automatically. Download the Vulkan build, unzip it, then press Browse.</span>';
  }
  for (const p of llamaFound) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'chip-pick'; b.textContent = p; b.title = 'Use this one';
    b.onclick = () => setValue('llama_server', p);
    host.appendChild(b);
  }
  if (llamaFound.length && !$('#llama_server').value.trim()) setValue('llama_server', llamaFound[0]);
  refresh();
}

async function scanModels() {
  models = await busy($('#btn-scan'), 'Searching your folders...', () => invoke('scan_models').catch(() => []));
  if (!models.length) toast('No .gguf files found in your usual folders. Use Browse, or download a model first.');
  renderModels();
}

async function detectGpus() {
  $('#gpumsg').textContent = '';
  try {
    const list = await busy($('#detect'), 'Asking llama.cpp...', () => invoke('list_gpus', { exe: $('#llama_server').value.trim() }));
    gpusListed = true;
    renderGpus(list);
    $('#gpumsg').textContent = list.length ? '' : 'No graphics cards reported. Is this a Vulkan build?';
  } catch (e) { $('#gpumsg').textContent = String(e); }
  refresh();
}

const BRAIN_TEXT = {
  setup: ['Waiting for setup', 'Choose a model above, then press the button.', 'Needs setup'],
  loading: ['Waking up...', 'Loading the model. The first time can take a minute.', 'Waking up'],
  ready: ["I'm awake!", 'Say something, or ask me about your screen.', 'Awake'],
  error: ["I couldn't wake up", '', 'Problem'],
};
let brainSince = 0;
function brainDetail() {
  const t = BRAIN_TEXT[brain.state] || BRAIN_TEXT.setup;
  let d = brain.state === 'error' || brain.state === 'loading' ? (brain.detail || t[1]) : t[1];
  const secs = Math.round((Date.now() - brainSince) / 1000);
  if (brain.state === 'loading' && secs >= 2) d += ` (${secs}s)`;
  $('#brain-detail').textContent = d;
}
setInterval(() => { if (brain.state === 'loading') brainDetail(); }, 1000);
function setBrain(b) {
  if (b.state === 'loading' && brain.state !== 'loading') brainSince = Date.now();
  brain = b;
  const t = BRAIN_TEXT[b.state] || BRAIN_TEXT.setup;
  $('#brainbox').dataset.state = b.state;
  $('#brain-title').textContent = t[0];
  brainDetail();
  $('#chip').dataset.state = b.state;
  $('#chip-text').textContent = t[2];
  refresh();
}

// ------------------------------------------------------------------ wiring
function showTab(name, focus = false) {
  $$('.nav [role=tab]').forEach((b) => {
    const on = b.dataset.tab === name;
    b.setAttribute('aria-selected', String(on)); b.tabIndex = on ? 0 : -1;
    if (on && focus) b.focus();
  });
  $$('.panel').forEach((p) => { p.hidden = p.dataset.panel !== name; });
  $('#content').scrollTop = 0;
}
$$('.nav [role=tab]').forEach((b) => {
  b.onclick = () => showTab(b.dataset.tab);
  b.onkeydown = (e) => {
    const tabs = $$('.nav [role=tab]'), i = tabs.indexOf(b);
    const move = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
    if (move) { e.preventDefault(); showTab(tabs[(i + move + tabs.length) % tabs.length].dataset.tab, true); }
  };
});

for (const id of [...TEXT, ...NUM, 'always_look', 'memory_enabled', 'avoid_cursor', 'hear_tone']) {
  const el = $('#' + id);
  el.addEventListener('input', () => { if (id in PATHS) checkPath(id); if (id === 'model' || id === 'mmproj') renderModels(); refresh(); });
  el.addEventListener('change', refresh);
}
const setApprovalsRadio = (v) => $$('input[name="approvals"]').forEach((x) => { x.checked = x.value === v; });
$$('input[name="approvals"]').forEach((r) => {
  r.onchange = () => {
    if (r.value === 'full' && !ackFull) {          // first time only: explain the risk before turning it on
      setApprovalsRadio(approvals);
      $('#full-dialog').showModal();
      return;
    }
    approvals = r.value; refresh();
  };
});
$('#full-dialog').addEventListener('close', () => {
  if ($('#full-dialog').returnValue === 'ok') { ackFull = true; approvals = 'full'; setApprovalsRadio('full'); refresh(); }
  $('#full-dialog').returnValue = '';
});
$$('[data-pick]').forEach((b) => { b.onclick = () => pick(b.dataset.pick); });
$$('[data-link]').forEach((b) => { b.onclick = () => invoke('open_link', { url: b.dataset.link }).catch((e) => toast(String(e), true)); });
$('#btn-detect-llama').onclick = detectLlama;
$('#btn-scan').onclick = scanModels;
$('#detect').onclick = detectGpus;
$('#btn-save').onclick = save;
$('#btn-wake').onclick = wake;
$('#btn-revert').onclick = () => { fill(snap); refresh(); toast('Changes reverted.'); };
$('#add-hook').onclick = () => { hooks.push({ name: '', description: '', url: '', headersText: '' }); renderHooks(); refresh(); $$('.hook').pop().querySelector('input').focus(); };
for (const id of ['btn-logs', 'btn-logs-2']) $('#' + id).onclick = () => invoke('open_logs').catch((e) => toast(String(e), true));
$('#btn-quit').onclick = () => invoke('quit');

// ------------------------------------------------------------------ suggestions, quick search, shortcuts
$$('#suggest button').forEach((b) => {
  b.onclick = () => busy(b, 'Sending...', () => invoke('send_text', { text: b.dataset.say }).then(() => toast('Sent. Watch your desktop.')).catch((e) => toast(String(e), true)));
});

const palette = $('#palette'), pInput = $('#palette-input'), pList = $('#palette-list');
let pItems = [], pSel = 0, pIndex = null;
/** Every setting label, switch title and section heading, with the tab it lives on. */
function buildIndex() {
  const out = [];
  for (const panel of $$('.panel')) {
    const tab = panel.dataset.panel, tabName = $(`.nav [data-tab="${tab}"] span`).textContent;
    out.push({ text: tabName, tab, el: null, where: 'Section' });
    for (const el of $$('h2, label[for], .switch-text strong, .seg strong, summary', panel)) {
      const text = el.textContent.replace(/\s+/g, ' ').trim();
      if (text.length > 2 && text.length < 70) out.push({ text, tab, el: el.closest('.card, .step, .field, details') || el, where: tabName });
    }
  }
  return out;
}
function paletteRender() {
  const q = pInput.value.trim().toLowerCase();
  pIndex ||= buildIndex();
  pItems = (q ? pIndex.filter((i) => i.text.toLowerCase().includes(q) || i.where.toLowerCase().includes(q)) : pIndex.filter((i) => !i.el)).slice(0, 30);
  pSel = Math.min(pSel, Math.max(0, pItems.length - 1));
  pList.innerHTML = '';
  if (!pItems.length) { pList.innerHTML = '<li class="palette-empty">Nothing matches. Try a shorter word.</li>'; return; }
  pItems.forEach((it, i) => {
    const li = document.createElement('li');
    li.setAttribute('role', 'option'); li.setAttribute('aria-selected', String(i === pSel));
    li.innerHTML = '<span class="what"></span><span class="where"></span>';
    $('.what', li).textContent = it.text; $('.where', li).textContent = it.where;
    li.onclick = () => paletteGo(it);
    pList.appendChild(li);
  });
}
function paletteGo(it) {
  palette.close();
  showTab(it.tab);
  if (it.el) setTimeout(() => { it.el.scrollIntoView({ block: 'center' }); it.el.classList.add('flash'); setTimeout(() => it.el.classList.remove('flash'), 1500); }, 60);
}
function paletteOpen() { pInput.value = ''; pSel = 0; paletteRender(); palette.showModal(); pInput.focus(); }
$('#search-btn').onclick = paletteOpen;
pInput.addEventListener('input', () => { pSel = 0; paletteRender(); });
pInput.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    pSel = (pSel + (e.key === 'ArrowDown' ? 1 : -1) + pItems.length) % Math.max(1, pItems.length);
    paletteRender(); pList.children[pSel]?.scrollIntoView({ block: 'nearest' });
  } else if (e.key === 'Enter' && pItems[pSel]) { e.preventDefault(); paletteGo(pItems[pSel]); }
});
palette.addEventListener('click', (e) => { if (e.target === palette) palette.close(); });
window.addEventListener('keydown', (e) => {
  if (!(e.ctrlKey || e.metaKey)) return;
  if (e.key.toLowerCase() === 'k') { e.preventDefault(); paletteOpen(); }
  else if (e.key.toLowerCase() === 's') { e.preventDefault(); if (!$('#btn-save').disabled) save(); }
});

// ------------------------------------------------------------------ remote: Telegram and the agent API
function setStatus(el, state, text) { el.dataset.state = state; $('span', el).textContent = text; }
async function loadRemote() {
  let r;
  try { r = await invoke('remote_status'); } catch { return; }
  const tg = r.telegram;
  setStatus($('#tg-status'), !tg.enabled ? 'off' : tg.bot ? 'ready' : 'starting', !tg.enabled ? 'Off' : tg.bot ? `@${tg.bot}` : 'Connecting...');
  const host = $('#tg-chats');
  host.innerHTML = '';
  for (const c of tg.chats || []) {
    const li = document.createElement('li');
    li.className = 'mem-item';
    li.innerHTML = '<div class="text"><span class="t"></span><span class="meta">Paired chat</span></div><button type="button" title="Unpair"><svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg></button>';
    $('.t', li).textContent = c.name || 'Chat';
    $('button', li).setAttribute('aria-label', `Unpair ${c.name || 'chat'}`);
    $('button', li).onclick = () => invoke('telegram_unpair', { id: c.id }).then(loadRemote).catch((e) => toast(String(e), true));
    host.appendChild(li);
  }
  const api = r.api;
  setStatus($('#api-status'), api.enabled && current.agent_api_enabled ? 'ready' : 'off', api.enabled && current.agent_api_enabled ? `Port ${api.port}` : 'Off');
  renderApiExample();
}
function renderApiExample() {
  const port = $('#agent_api_port').value || 7457, token = $('#agent_api_token').value || '<token>';
  $('#api-example').textContent = `curl http://127.0.0.1:${port}/message \\\n  -H "Authorization: Bearer ${token}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"text": "What is on my calendar today?"}'`;
}
$('#tg-pair').onclick = async () => {
  if (!current.telegram_enabled || !current.telegram_token) { toast('Turn Telegram on, paste the bot token and press Save first.', true); return; }
  const code = await invoke('telegram_pair_code');
  const el = $('#tg-code');
  el.hidden = false;
  el.textContent = `Send this to your bot: /pair ${code}  (valid 10 minutes)`;
};
$('#api-new').onclick = async () => { $('#agent_api_token').value = await invoke('new_api_token'); renderApiExample(); refresh(); };
$('#api-copy').onclick = () => {
  const t = $('#agent_api_token').value;
  if (!t) return;
  navigator.clipboard.writeText(t).then(() => toast('Token copied.'), () => toast("I couldn't copy that.", true));
};
$('#agent_api_enabled').addEventListener('change', async () => {
  if ($('#agent_api_enabled').checked && !$('#agent_api_token').value) { $('#agent_api_token').value = await invoke('new_api_token'); refresh(); }
  renderApiExample();
});
$('#agent_api_port').addEventListener('input', renderApiExample);
T.event.listen('remote', () => loadRemote());
$$('[data-privacy]').forEach((b) => { b.onclick = () => invoke('open_privacy', { kind: b.dataset.privacy }).catch((e) => toast(String(e), true)); });
$('#wc-min').onclick = () => invoke('window_action', { label: 'settings', action: 'minimize' });
$('#wc-max').onclick = () => invoke('window_action', { label: 'settings', action: 'maximize' });
$('#wc-close').onclick = () => invoke('window_action', { label: 'settings', action: 'close' });
window.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') { e.preventDefault(); save(); }
});

T.event.listen('brain', (e) => setBrain(e.payload));
T.event.listen('character', () => syncFromBackend().catch(() => {}));
T.event.listen('settings-changed', async () => {
  if (JSON.stringify(readForm()) === JSON.stringify(snap)) return load(false).catch(() => {});
  const s = await invoke('get_settings');            // keep the user's other edits; just take the new model paths
  current.model = snap.model = s.model; current.mmproj = snap.mmproj = s.mmproj;
  if (!$('#model').value) setValue('model', s.model);
  if (!$('#mmproj').value) setValue('mmproj', s.mmproj);
  refresh();
});
T.event.listen('settings-shown', () => { if (JSON.stringify(readForm()) === JSON.stringify(snap)) load(false).catch(() => {}); });

async function load(first) {
  setTop(true);
  try { await load1(first); } finally { setTop(false); }
}
async function load1(first) {
  current = await invoke('get_settings');
  fill(current);
  snap = readForm();
  setBrain(await invoke('get_brain'));
  refresh();
  if (first && !current.model) { showTab('setup'); detectLlama(); scanModels(); }
}
applyUi(false);
skeleton('#mem-list'); skeleton('#rem-list'); skeleton('#chat-log', 1);
renderChars();
initDownload();
initLlama();
loadMemory();
loadReminders();
loadMcpStatus();
loadChat();
loadRemote();
load(true).then(loadDevices).catch((e) => toast("I couldn't load your settings: " + e, true));
