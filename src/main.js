const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = T.event.listen;
const $ = (id) => document.getElementById(id);
const set = (id, attrs) => { const el = $(id); if (el) for (const k in attrs) el.setAttribute(k, attrs[k]); };
const call = (cmd, args) => invoke(cmd, args).catch((e) => { console.error(cmd, e); });
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const lerp = (a, b, t) => a + (b - a) * t;
const rand = (a, b) => a + Math.random() * (b - a);
const pick = (list) => list[Math.floor(Math.random() * list.length)];

// ---- state fed by the backend -------------------------------------------------
const S = {
  mic: 0, out: 0,                       // audio levels
  walking: false, dir: 1, hop: false,   // roaming
  act: null,                            // pointer/keyboard action in progress
  look: { x: 0, y: 0 }, lookTarget: { x: 0, y: 0 }, lookUntil: 0,
  cursor: { x: 0, y: 0, t: -1e9 },      // pointer position relative to the window centre (CSS px)
  flags: { mic: true, screen: true, hands: false, roam: true, configured: false },
  connected: false,
  voice: 'idle', voiceAt: 0, earState: '',      // voice activity from the backend: idle | hearing
  thinking: false, thinkStart: 0, replying: false, asking: false, chatting: false,
  dragged: false, sleeping: false, gentleWake: false,
  lastActive: performance.now(), lastMove: performance.now(), activeSince: performance.now(),
  wave: -1e9, startle: -1e9, poke: -1e9, happyUntil: 0, hoverUntil: 0, nextHover: 0,
  // moods and reactions (each lasts until the given time)
  curiousUntil: 0, sleepyUntil: 0,       // moods the reply asked for ([curious], [sleepy])
  pettedUntil: 0, dizzyUntil: 0, grumpyUntil: 0, sadUntil: 0, squintUntil: 0, crossUntil: 0, surpriseUntil: 0,
  pokes: [],                            // recent poke times (ticklish / grumpy detection)
  quirk: null, nextQuirk: performance.now() + 6000,
  nextChatter: performance.now() + rand(4, 7) * 60000,
  calm: window.matchMedia('(prefers-reduced-motion: reduce)').matches,
};
const wake = () => { S.lastActive = performance.now(); };
const free = () => !S.thinking && !S.replying && !S.asking && !S.chatting;    // nothing important on screen
const bubbleFree = () => free() && $('bubble').classList.contains('hidden');

// ---- what I say on my own (all original lines) ------------------------------------------
const L = {
  zones: {
    head: ['Hehe, head pat!', 'Hey, my hair!', 'Ooh, scritches.'],
    eye: ['Ow, my eye!', 'Hey! I need that to see.', 'Eek, not the eye!'],
    nose: ['Boop!', 'Nose pressed. Hello!', 'Honk!'],
    cheek: ['Aww, you are making me blush.', 'Squish!', 'Hehe, my cheek.'],
    feet: ['Hey, my toes!', 'Hop!', 'Tickly feet!'],
    arm: ['That is my arm!', 'High five!', 'Hehe, handshake.'],
  },
  grumpy: ['Hmph. Enough poking for now.', 'Okay, okay! Personal space!', 'I am pretending to be annoyed.'],
  stillGrumpy: ['Not talking to you. ...okay, maybe a little.', 'Hmph.'],
  pickup: ['Wheee!', 'Up we go!', 'Careful, careful!', 'I can see everything from here!'],
  shake: ['Whoaaa, too much shaking!', 'Everything is spinning!'],
  circle: ['Whoa... the room is spinning.', 'Round and round... I feel dizzy.'],
  whoosh: ['Whoa, zoom!', 'So fast!', 'Where are you off to?'],
  cross: ['Boop?', 'That is very close.', 'I can see you!'],
  wakeNear: ['Huh?! I am up, I am up!', 'Wha-? Oh, it is you.'],
  wakeSoft: ['Mm... welcome back.', '*yawn* Oh, hi.', 'I was just resting my eyes.'],
  wakeLong: ['Welcome back! I missed you.', 'There you are!'],
  sneeze: ['Achoo!', 'Ah... ah... choo!'],
  // stepping out of your way: polite, a little funny, and said rarely
  dodge: {
    text: ['Oops, I was standing on your words. Scooting!', 'Sorry, that is your text, not mine.', 'Excuse me, writer at work.'],
    pointer: ['Oh, you need that spot. Hop!', 'Okay, okay, moving!', 'Pardon me, I will get out of your way.'],
  },
};
function timeGreeting() {
  const d = new Date(), h = d.getHours(), day = d.getDay();
  if (h >= 5 && h < 12) return day === 1 ? 'Good morning! A fresh week.' : 'Good morning!';
  if (h >= 12 && h < 17) return day === 5 ? 'Good afternoon! Almost the weekend.' : 'Good afternoon!';
  if (h >= 17 && h < 22) return 'Good evening!';
  return 'Up late? I will keep you company.';
}
function chatterLines() {
  const h = new Date().getHours();
  const extra = h >= 12 && h < 14 ? ['Lunch time? Good ideas need snacks.']
    : h >= 23 || h < 4 ? ['It is getting late. Sleep is a feature, not a bug.']
    : h >= 7 && h < 10 ? ['Morning plan: one thing at a time.'] : [];
  return [...(meta.chatter || ['Just saying hi.']), ...extra];
}
const isNight = () => { const h = new Date().getHours(); return h >= 23 || h < 5; };

// ---- speech bubble (streams while I reply) --------------------------------------
let bubbleTimer;
const readingTime = (t) => clamp(t.length * 60, 4500, 20000);
function hideBubble() { clearTimeout(bubbleTimer); $('bubble').classList.add('hidden'); }
function bubble(text, { notice = false, ms = 6000, typing = false } = {}) {
  const b = $('bubble');
  clearTimeout(bubbleTimer);
  if (!text) { hideBubble(); return; }
  $('bubble-text').textContent = text;
  b.classList.toggle('notice', notice);
  b.classList.toggle('typing', typing);
  b.classList.remove('hidden');
  b.scrollTop = b.scrollHeight;                 // keep the newest words in view
  if (ms && !typing) bubbleTimer = setTimeout(() => b.classList.add('hidden'), ms);
}
/** A short remark of my own; never talks over a reply, a question or the chat box. */
function quip(text, ms = 2200) { if (text && free()) bubble(text, { ms }); }
function endReply() {
  S.replying = false;
  const b = $('bubble');
  b.classList.remove('typing');
  if (!b.classList.contains('hidden')) {
    clearTimeout(bubbleTimer);
    bubbleTimer = setTimeout(() => b.classList.add('hidden'), readingTime($('bubble-text').textContent));
  }
}

// ---- thought bubble: what I'm doing while I think -----------------------------------
function showThought(label, text = '') {
  if (S.asking) return;
  $('thought-label').textContent = label;
  $('thought-text').textContent = text;
  $('thought').classList.remove('hidden');
}
function hideThought() { $('thought').classList.add('hidden'); $('thought-time').textContent = ''; }

// ---- background tasks: waking up, downloading, connecting tools -----------------------
// Anything slow shows a pill with a spinner (and a bar when the amount is known), so waiting is never silent.
const tasks = new Map();                         // key -> { text, pct, since }
const TASK_ORDER = ['dl', 'brain', 'mcp'];
function setTask(key, v) {
  if (v) tasks.set(key, { since: (tasks.get(key) || {}).since || Date.now(), ...v });
  else tasks.delete(key);
  renderTask();
}
function renderTask() {
  const key = TASK_ORDER.find((k) => tasks.has(k));
  const el = $('task');
  el.classList.toggle('hidden', !key);
  if (!key) return;
  const t = tasks.get(key);
  const secs = Math.round((Date.now() - t.since) / 1000);
  const pct = typeof t.pct === 'number';
  el.classList.toggle('pct', pct);
  $('task-text').textContent = pct ? `${t.text} ${t.pct}%` : secs >= 3 ? `${t.text}... ${secs}s` : `${t.text}...`;
  if (pct) $('task-fill').style.width = t.pct + '%';
}
setInterval(() => {
  renderTask();
  if (S.thinking && !$('thought').classList.contains('hidden')) {
    const s = Math.round((performance.now() - S.thinkStart) / 1000);
    $('thought-time').textContent = s >= 3 ? s + 's' : '';
  }
}, 1000);

// ---- little effects -------------------------------------------------------------------
let fxCount = 0;
function burst(kind, n = 1, { spread = 0.4, y = 0.22 } = {}) {
  if (S.calm || fxCount > 24) return;
  const r = $('petbox').getBoundingClientRect();
  const colors = ['#ffcf5a', '#7cd6a0', '#8ab4ff', '#ff8aa0', '#c39bff'];
  for (let i = 0; i < n; i++) {
    const el = document.createElement('i');
    el.className = 'fx ' + kind;
    if (kind === 'note') el.textContent = pick(['♪', '♫']);
    if (kind === 'bang') el.textContent = '!';
    if (kind === 'spark') { el.style.background = pick(colors); el.classList.add('star'); }
    el.style.left = (r.left + r.width * (0.5 + (Math.random() - 0.5) * spread)) + 'px';
    el.style.top = (r.top + r.height * y + rand(-6, 6)) + 'px';
    el.style.setProperty('--dx', rand(-34, 34).toFixed(0) + 'px');
    el.style.animationDelay = (i * 90) + 'ms';
    document.body.appendChild(el);
    fxCount++;
    setTimeout(() => { el.remove(); fxCount--; }, 1700 + i * 90);
  }
}
function celebrate(text) {
  const n = performance.now();
  wake();
  S.happyUntil = n + 2600; S.wave = n;
  S.quirk = { name: 'hop', start: n, dur: QUIRK_MS.hop };
  burst('spark', 8, { spread: 0.8, y: 0.3 });
  if (text) quip(text, 5000);
}

// ---- approval card -----------------------------------------------------------------
// A countdown shows how long I wait (no answer counts as "no"). Enter / Esc only work while the card has focus,
// so a stray keypress meant for another app can never approve anything.
let askTimer;
function ask(p) {
  const text = p && (typeof p === 'string' ? p : p.text);
  S.asking = !!text;
  $('ask').classList.toggle('hidden', !text);
  clearInterval(askTimer);
  if (text) {
    $('ask-text').textContent = text; hideBubble(); hideThought(); flashBar(0); wake();
    const secs = (p && p.secs) || 30, end = Date.now() + secs * 1000, fill = $('ask-fill');
    fill.style.transition = 'none'; fill.style.width = '100%';
    requestAnimationFrame(() => requestAnimationFrame(() => { fill.style.transition = `width ${secs}s linear`; fill.style.width = '0%'; }));
    askTimer = setInterval(() => { if (Date.now() > end + 500) clearInterval(askTimer); }, 1000);
  } else if (S.thinking) showThought($('thought-label').textContent);
}
$('ask-yes').onclick = () => call('answer_approval', { approve: true });
$('ask-no').onclick = () => call('answer_approval', { approve: false });
$('ask').addEventListener('keydown', (e) => {
  if (e.key === 'Escape') { e.preventDefault(); call('answer_approval', { approve: false }); }
  else if (e.key === 'Enter' && e.target.tagName !== 'BUTTON') { e.preventDefault(); call('answer_approval', { approve: true }); }
});
$('ask').tabIndex = 0;

// ---- typed chat ---------------------------------------------------------------------
function openChat() {
  S.chatting = true; wake();
  call('set_chatting', { on: true });                 // so I don't wander off while you type
  document.body.classList.add('chatting');
  $('chat').classList.remove('hidden');
  call('focus_main').then(() => $('chat-input').focus());
  $('chat-input').focus();
}
function closeChat() {
  S.chatting = false;
  call('set_chatting', { on: false });
  document.body.classList.remove('chatting');
  $('chat').classList.add('hidden');
  $('chat-input').blur();
}
$('chat').addEventListener('submit', (e) => {
  e.preventDefault();
  const text = $('chat-input').value.trim();
  if (!text) return;
  invoke('send_text', { text })
    .then(() => { $('chat-input').value = ''; closeChat(); })
    .catch((err) => bubble(String(err), { notice: true, ms: 4000 }));
});
$('chat-input').addEventListener('keydown', (e) => { if (e.key === 'Escape') closeChat(); });
// An empty typing box that has lost focus closes itself, so I'm not frozen in place for nothing.
let idleChat;
addEventListener('blur', () => {
  clearTimeout(idleChat);
  idleChat = setTimeout(() => { if (S.chatting && !document.hasFocus() && !$('chat-input').value.trim()) closeChat(); }, 15000);
});
addEventListener('focus', () => clearTimeout(idleChat));
$('b-chat').onclick = openChat;

// ---- controls -----------------------------------------------------------------------
let barTimer;
function flashBar(ms) {                          // show the control bar for a while (or until the pointer leaves)
  document.body.classList.add('hot');
  clearTimeout(barTimer);
  if (ms) barTimer = setTimeout(() => document.body.classList.remove('hot'), ms);
}
for (const el of [$('petbox'), $('bar')]) el.addEventListener('mouseenter', () => flashBar(0));
// Glad to be noticed: the pointer coming over me gets a happy squish, and now and then a heart.
$('petbox').addEventListener('mouseenter', () => {
  const n = performance.now();
  S.hoverUntil = n + 1600;
  if (n < S.nextHover || S.sleeping || S.dragged) return;
  S.nextHover = n + 6000;
  wake(); sp.squash.kick(-4);
  if (Math.random() < 0.5) burst('heart', 1);
});
document.documentElement.addEventListener('mouseleave', () => flashBar(1500));

function renderFlags() {
  const f = S.flags;
  if (f.size_pct) document.documentElement.style.setProperty('--pet', String(f.size_pct / 100));   // character only
  for (const [id, on] of [['b-mic', f.mic], ['b-screen', f.screen], ['b-hands', f.hands]]) {
    $(id).classList.toggle('off', !on);
    $(id).setAttribute('aria-pressed', String(!!on));
  }
  $('b-hands').classList.toggle('on-hands', f.hands);
  const push = f.listen_mode === 'push';
  $('b-mic').title = push ? `Microphone (push to talk: hold ${prettyKeys(f.ptt_shortcut)})` : 'Microphone';
  $('b-chat').title = `Type to me${f.chat_shortcut ? ` (${prettyKeys(f.chat_shortcut)})` : ''}`;
  document.body.classList.toggle('ptt', push && !!f.ptt_held);
}
const prettyKeys = (a) => (a || '').replace(/CommandOrControl/g, 'Ctrl').replace(/Super/g, 'Win').replace(/\+/g, '+');
$('b-mic').onclick = () => call('set_flag', { name: 'mic', value: !S.flags.mic });
$('b-screen').onclick = () => call('set_flag', { name: 'screen', value: !S.flags.screen });
$('b-hands').onclick = () => call('set_flag', { name: 'hands', value: !S.flags.hands });
$('b-stop').onclick = () => call('stop_all');
$('b-hide').onclick = () => call('hide_main');
$('b-key').onclick = () => call('open_settings');

// ---- geometry: where my eyes are in this window ---------------------------------------
// The backend sends the pointer relative to the window centre (CSS px); the eyes sit lower than that.
let G = { ex: 0, ey: 30, k: 1, left: 30, top: 60 };
function measure() {
  const r = $('petbox').getBoundingClientRect();
  const k = r.width / 200;
  G = { ex: r.left + 100 * k - innerWidth / 2, ey: r.top + 140 * k - innerHeight / 2, k, left: r.left, top: r.top };
}
measure();
addEventListener('resize', measure);
/** Pointer (relative to the window centre) -> my own drawing coordinates (0..200 x 0..240). */
const toPet = (x, y) => ({ x: (x + innerWidth / 2 - G.left) / G.k, y: (y + innerHeight / 2 - G.top) / G.k });

// ---- the pointer: eyes follow it, and it can pet, dizzy, startle or wake me ------------
const H = { prev: null, turns: [], petRevs: [], petDir: 0, nextPetLine: 0, shakeRevs: [], shakeDir: 0, nextWhoosh: 0, crossSince: 0, nextCross: 0, lastAngle: null };
function onCursor(p) {
  const now = performance.now();
  S.cursor = { x: p.x, y: p.y, t: now };
  const abs = { x: p.sx ?? p.x, y: p.sy ?? p.y };
  const prev = H.prev;
  H.prev = { ...abs, t: now };
  if (!prev) return;
  const dx = abs.x - prev.x, dy = abs.y - prev.y, dist = Math.hypot(dx, dy);
  if (dist < 3) return;
  const away = now - S.lastMove;
  S.lastMove = now;
  if (away > 5 * 60000) S.activeSince = now;

  if (S.dragged) { shakeCheck(dx, now); return; }
  const ex = p.x - G.ex, ey = p.y - G.ey, fromEyes = Math.hypot(ex, ey);

  if (S.sleeping) {                                           // the pointer wakes me
    if (fromEyes < 180) { S.gentleWake = false; quip(pick(L.wakeNear), 2600); }
    else { S.gentleWake = true; if (Math.random() < 0.6 || away > 10 * 60000) setTimeout(() => quip(pick(away > 10 * 60000 ? L.wakeLong : L.wakeSoft), 3200), 900); }
    return;
  }

  // a fast swipe right past me
  const speed = dist / Math.max(0.016, (now - prev.t) / 1000);
  if (speed > 3200 && fromEyes < 240 && now > H.nextWhoosh) {
    H.nextWhoosh = now + 15000;
    S.surpriseUntil = now + 650;
    if (Math.random() < 0.4) quip(pick(L.whoosh), 1600);
  }

  // circling around me makes me dizzy
  if (fromEyes > 40 && fromEyes < 300) {
    const a = Math.atan2(ey, ex);
    if (H.lastAngle !== null) {
      let d = a - H.lastAngle;
      if (d > Math.PI) d -= 2 * Math.PI; else if (d < -Math.PI) d += 2 * Math.PI;
      H.turns.push({ t: now, d });
    }
    H.lastAngle = a;
    H.turns = H.turns.filter((x) => now - x.t < 2600);
    if (Math.abs(H.turns.reduce((s, x) => s + x.d, 0)) > Math.PI * 3.5 && now > S.dizzyUntil) {
      H.turns = [];
      getDizzy(L.circle);
    }
  } else H.lastAngle = null;

  // stroking back and forth over my head is petting
  const pp = toPet(p.x, p.y);
  if (pp.x > 36 && pp.x < 164 && pp.y > 20 && pp.y < 128 && Math.abs(dx) > 2) {
    const dir = Math.sign(dx);
    if (dir !== H.petDir) { H.petRevs.push(now); H.petDir = dir; }
    H.petRevs = H.petRevs.filter((t) => now - t < 1600);
    if (H.petRevs.length >= 3) petted(now);
  }
}
function petted(now) {
  wake();
  const first = now > S.pettedUntil;
  S.pettedUntil = now + 1400;
  if (first || Math.random() < 0.25) burst('heart', 1);
  if (now > H.nextPetLine) { H.nextPetLine = now + 9000; quip(pick(meta.petted || ['That is nice.']), 2400); }
}
function shakeCheck(dx, now) {
  if (Math.abs(dx) < 8) return;
  const dir = Math.sign(dx);
  if (dir !== H.shakeDir) { H.shakeRevs.push(now); H.shakeDir = dir; }
  H.shakeRevs = H.shakeRevs.filter((t) => now - t < 1200);
  if (H.shakeRevs.length >= 5 && now > S.dizzyUntil) { H.shakeRevs = []; getDizzy(L.shake); }
}
function getDizzy(lines) {
  const now = performance.now();
  wake();
  S.dizzyUntil = now + 2800;
  quip(pick(lines), 2400);
}

// ---- backend events -----------------------------------------------------------------
listen('flags', (e) => { S.flags = e.payload; S.connected = e.payload.ready; renderFlags(); loadCharacter(e.payload.character); });
listen('levels', (e) => {
  S.mic = e.payload.mic; S.out = e.payload.out;
  if (S.mic > 0.03 || S.out > 0.01) wake();
});
listen('voice', (e) => { S.voice = e.payload.state; S.voiceAt = performance.now(); if (S.voice === 'hearing') wake(); });
/** The reply starts with a mood ([happy], [excited], ...): show it on my face and body before I say a word. */
function applyMood(m) {
  const n = performance.now();
  wake();
  switch (m) {
    case 'happy': S.happyUntil = Math.max(S.happyUntil, n + 3500); break;
    case 'excited': S.happyUntil = n + 4000; S.wave = n; S.quirk = { name: 'hop', start: n, dur: QUIRK_MS.hop }; burst('spark', 4, { spread: 0.7, y: 0.3 }); break;
    case 'curious': S.curiousUntil = n + 3500; break;
    case 'sad': S.sadUntil = n + 4500; break;
    case 'surprised': S.surpriseUntil = n + 900; break;
    case 'sleepy': S.sleepyUntil = n + 4000; break;
    default: break;                                                      // calm / neutral: just be myself
  }
}
listen('mood', (e) => applyMood(e.payload));
listen('dodge', (e) => {
  if (S.flags.playfulness === 'quiet' || Math.random() > 0.6) return;
  quip(pick(L.dodge[e.payload.kind] || L.dodge.pointer), 2400);
});
listen('move', (e) => { Object.assign(S, { walking: e.payload.walking, dir: e.payload.dir, hop: e.payload.hop }); });
listen('look', (e) => { S.lookTarget = { x: e.payload.dx, y: e.payload.dy }; S.lookUntil = performance.now() + 3500; });
listen('act', (e) => { S.act = e.payload; wake(); });
listen('cursor', (e) => onCursor(e.payload));
listen('connected', (e) => { S.connected = e.payload; });
listen('drag', (e) => {
  const was = S.dragged;
  S.dragged = e.payload; wake();
  if (e.payload && !was) { H.shakeRevs = []; if (Math.random() < 0.45) quip(pick(L.pickup), 1600); }
  if (!e.payload) { sp.squash.kick(-7); S.happyUntil = performance.now() + 700; }   // set down: little bounce
});
listen('poke', (e) => poke(false, e.payload));
listen('dpoke', (e) => poke(true, e.payload));
listen('open-chat', openChat);
listen('say', (e) => {
  if (S.asking) return;
  S.replying = true; wake();
  hideThought();
  bubble(e.payload, { typing: true });
});
listen('heard', (e) => {
  const t = e.payload.length > 90 ? e.payload.slice(0, 89) + '…' : e.payload;
  if (S.thinking || !$('thought').classList.contains('hidden')) $('thought-text').textContent = '“' + t + '”';
  else bubble('You: ' + t, { ms: 3500 });
});
listen('status', (e) => {
  bubble(e.payload, { notice: true, ms: e.payload ? Math.max(9000, e.payload.length * 80) : 0 });
  if (e.payload) flashBar(6000);
});
listen('approve', (e) => ask(e.payload));
listen('remembered', () => { if (!S.asking && !S.replying) { bubble('Noted. I will remember that.', { notice: true, ms: 2500 }); burst('spark', 3); } });
listen('reminder', (e) => { wake(); S.wave = performance.now(); S.happyUntil = performance.now() + 2500; burst('bang', 1); bubble('Reminder: ' + e.payload, { notice: true, ms: 12000 }); });
listen('thinking', (e) => {
  const d = e.payload || {};
  if (d.on && !S.thinking) S.thinkStart = performance.now();
  S.thinking = !!d.on; wake();
  if (d.on) {
    S.replying = false;
    $('bubble').classList.remove('typing');
    hideBubble();
    showThought(d.label || 'Thinking');
  } else {
    hideThought();
    endReply();
  }
});
listen('activity', (e) => { if (S.thinking) { $('thought-label').textContent = e.payload; $('thought-text').textContent = ''; } });
listen('thought', (e) => { if (S.thinking) $('thought-text').textContent = e.payload; });

// The model server: show it waking up, and be glad when it is ready.
let brainState = null;
function onBrain(b) {
  if (!b) return;
  const prev = brainState;
  brainState = b.state;
  setTask('brain', b.state === 'loading' ? { text: 'Waking up' } : null);
  if (b.state === 'ready' && prev && prev !== 'ready') celebrate("I'm awake! Talk to me, or press Ctrl+Alt+C to type.");
  if (b.state === 'error' && prev !== 'error') S.sadUntil = performance.now() + 6000;
}
listen('brain', (e) => onBrain(e.payload));
call('get_brain').then(onBrain);

listen('download', (e) => {
  const d = e.payload;
  if (d.state === 'progress') setTask('dl', { text: d.file === 'mmproj' ? 'Getting my eyes & ears' : 'Downloading my brain', pct: Math.min(100, Math.floor((d.done / d.total) * 100)) });
  else if (d.state === 'verifying') setTask('dl', { text: 'Checking the download' });
  else {
    setTask('dl', null);
    if (d.state === 'done') celebrate('Got it! Give me a moment to wake up...');
  }
});
async function checkMcp() {
  try {
    const st = await invoke('mcp_status');
    const n = (st.servers || []).filter((s) => s.state === 'starting').length;
    setTask('mcp', n ? { text: n > 1 ? `Connecting ${n} tools` : 'Connecting tools' } : null);
  } catch { setTask('mcp', null); }
}
listen('mcp', checkMcp);
checkMcp();

// ---- touching me: where you poke matters ------------------------------------------------
function zoneAt(p) {
  if (!p || typeof p.x !== 'number') return 'belly';
  const x = (p.x - G.left) / G.k, y = (p.y - G.top) / G.k;
  if (y > 208) return 'feet';
  if ((x < 46 || x > 154) && y > 128) return 'arm';
  if (y < 112) return 'head';
  if (Math.abs(y - 138) < 16 && (Math.abs(x - 74) < 13 || Math.abs(x - 126) < 13)) return 'eye';
  if (y >= 148 && y < 176 && Math.abs(x - 100) < 20) return 'nose';
  if (y >= 146 && y < 174 && (Math.abs(x - 62) < 15 || Math.abs(x - 138) < 15)) return 'cheek';
  return 'belly';
}
function poke(double, pos) {
  const now = performance.now();
  wake();
  if (double) { openChat(); return; }
  S.poke = now;
  S.pokes = S.pokes.filter((t) => now - t < 6000).concat(now);
  const zone = zoneAt(pos);

  if (now < S.grumpyUntil) {                                 // still sulking
    S.grumpyUntil = now + 2500; sp.squash.kick(-5);
    quip(pick(L.stillGrumpy), 1600);
    return;
  }
  if (S.pokes.length >= 6) {                                 // too many: a little huff
    S.pokes = []; S.grumpyUntil = now + 4500; sp.squash.kick(-9);
    burst('bang', 1);
    quip(pick(L.grumpy), 2600);
    return;
  }

  S.happyUntil = now + 900;
  sp.squash.kick(-9);
  const own = (meta.zones || {})[zone];
  let line;
  switch (zone) {
    case 'eye': S.squintUntil = now + 800; S.happyUntil = 0; line = pick(own || L.zones.eye); break;
    case 'nose': S.crossUntil = now + 900; sp.squash.kick(-6); line = pick(own || L.zones.nose); break;
    case 'cheek': S.happyUntil = now + 1800; burst('heart', 1); line = pick(own || L.zones.cheek); break;
    case 'head': S.pettedUntil = now + 1000; S.quirk = { name: 'earTwitch', start: now, dur: QUIRK_MS.earTwitch }; line = pick(own || L.zones.head); break;
    case 'feet': S.quirk = { name: 'hop', start: now, dur: QUIRK_MS.hop }; line = pick(own || L.zones.feet); break;
    case 'arm': S.wave = now; line = pick(own || L.zones.arm); break;
    default: {
      const ticklish = S.pokes.length >= 3;
      if (ticklish) S.quirk = { name: 'wiggle', start: now, dur: 1400 };
      line = ticklish ? 'Hehehe, stop, that tickles!' : pick(own || meta.pokes || ['Hehe!']);
    }
  }
  quip(line, 1800);
}

// ---- hello ---------------------------------------------------------------------------
call('get_state').then((f) => {
  if (!f) return;
  S.flags = f; S.connected = !!f.ready; renderFlags();
  loadCharacter(f.character);
  flashBar(6000);
  if (!f.configured) call('open_settings');
  else setTimeout(() => {
    const n = performance.now();
    S.wave = n; S.happyUntil = n + 1800; sp.squash.kick(-6);
    if (!S.thinking) bubble(`${timeGreeting()} ${meta.greet || ''}`.trim(), { ms: 4500 });
  }, 900);
});
// Calm mode: respect the OS "reduce motion" setting by not wandering (the tray menu turns it back on).
if (S.calm) call('set_flag', { name: 'roam', value: false });

// Now and then, say something of my own, or suggest a stretch after a long stretch of work.
setInterval(() => {
  const now = performance.now();
  if (S.sleeping || S.dragged || !bubbleFree() || now - S.lastMove > 60000) return;
  const mood = S.flags.playfulness || 'normal';
  if (mood === 'quiet') return;                                           // no unprompted talking at all
  if (now - S.activeSince > 55 * 60000) {
    S.activeSince = now;
    S.quirk = { name: 'stretch', start: now, dur: QUIRK_MS.stretch };
    quip('We have been at it for an hour. Stretch with me?', 6000);
    return;
  }
  if (now > S.nextChatter) {
    S.nextChatter = now + (mood === 'lively' ? rand(3, 6) : rand(6, 12)) * 60000;
    S.happyUntil = now + 1200;
    quip(pick(chatterLines()), 5000);
  }
}, 5000);

// ---- click-through ------------------------------------------------------------------
// The transparent window ignores the mouse except over these rectangles (CSS px). The first one is the character.
let lastRects = '';
function reportHitRegions() {
  measure();
  const rects = [];
  const p = $('petbox').getBoundingClientRect(), kx = p.width / 200, ky = p.height / 240;
  rects.push([p.left + 28 * kx, p.top + 12 * ky, p.left + 172 * kx, p.top + 232 * ky]);
  for (const id of ['bubble', 'thought', 'ask', 'chat', 'bar']) {
    const el = $(id);
    const hidden = id === 'bar' ? (!document.body.classList.contains('hot') && !el.matches(':focus-within')) || S.chatting
                                : el.classList.contains('hidden');
    if (hidden) continue;
    const r = el.getBoundingClientRect();
    rects.push([r.left, r.top, r.right, r.bottom]);
  }
  const key = JSON.stringify(rects.map((r) => r.map(Math.round)));
  if (key !== lastRects) { lastRects = key; call('set_hit_regions', { rects }); }
}
setInterval(reportHitRegions, 120);

// ---- character: parts ---------------------------------------------------------------
let meta = characterMeta(DEFAULT_CHARACTER);
let currentCharacter = null;
let parts = {};                                  // optional rig parts and their pivots
function indexParts() {
  parts = {};
  for (const id of ['antenna', 'antenna2', 'earL', 'earR', 'tail', 'puff', 'flame', 'moss', 'drop']) {
    const el = $(id);
    if (!el) continue;
    const [px, py] = (el.getAttribute('data-pivot') || '0 0').split(' ').map(Number);
    parts[id] = { px, py };
  }
}
const rot = (id, deg) => { const p = parts[id]; if (p) set(id, { transform: `rotate(${deg.toFixed(2)} ${p.px} ${p.py})` }); };
const grow = (id, sx, sy = sx) => { const p = parts[id]; if (p) set(id, { transform: `translate(${p.px} ${p.py}) scale(${sx.toFixed(3)} ${sy.toFixed(3)}) translate(${-p.px} ${-p.py})` }); };

function loadCharacter(id) {
  if (!id || id === currentCharacter) return;
  const switching = currentCharacter !== null;
  currentCharacter = id;
  meta = characterMeta(id);
  $('pet').innerHTML = characterSvg(id);
  indexParts();
  const n = performance.now();
  S.startle = n; S.quirk = null; S.nextQuirk = n + 5000;
  sp.squash.kick(-5);                            // a little hello hop for the new friend
  if (switching) { S.wave = n + 600; burst('spark', 5); setTimeout(() => quip(meta.greet, 3500), 500); }
}

// ---- character: motion ----------------------------------------------------------------
// Springs give weight and overshoot: things settle instead of snapping.
class Spring {
  constructor(k, d) { this.k = k; this.d = d; this.x = 0; this.v = 0; }
  step(target, dt) { this.v += (this.k * (target - this.x) - this.d * this.v) * dt; this.x += this.v * dt; return this.x; }
  kick(v) { this.v += v; }
}
const sp = { tilt: new Spring(120, 11), squash: new Spring(240, 13), lean: new Spring(90, 10), sit: new Spring(70, 12) };
loadCharacter(DEFAULT_CHARACTER);

const QUIRK_MS = { lookaround: 2800, stretch: 2300, yawn: 2300, hum: 3400, antenna: 1800, wiggle: 1200, hop: 950, earTwitch: 800, tailWag: 1700, drip: 2000, flicker: 1700,
  sneeze: 1900, twirl: 1100, peekaboo: 2600 };
const COMMON_QUIRKS = ['sneeze', 'twirl', 'peekaboo'];            // everyone does these, just less often
function maybeQuirk(now, idle) {
  if (S.quirk && now - S.quirk.start > S.quirk.dur) { S.quirk = null; S.nextQuirk = now + rand(7000, 16000) / (0.6 + 0.4 * (meta.traits.energy || 1)); }
  if (!S.quirk && idle && !S.calm && now > S.nextQuirk) {
    const name = Math.random() < 0.22 ? pick(COMMON_QUIRKS) : pick(meta.traits.quirks || ['lookaround']);
    S.quirk = { name, start: now, dur: QUIRK_MS[name] || 1600 };
    if (name === 'hum') burst('note', 3, { y: 0.15 });
  }
}

/** The listening badge: off (mic muted / not ready / talking), ready (waiting for you), hearing (you're speaking). */
function updateEar(speaking, hearing, pttOpen) {
  const f = S.flags;
  let st = 'off';
  if (f.mic && S.connected && !speaking && !S.thinking && (f.listen_mode !== 'push' || f.ptt_held)) st = hearing ? 'hearing' : 'ready';
  const ear = $('ear');
  if (st === 'hearing') ear.style.setProperty('--lv', clamp(S.mic * 8, 0.12, 1).toFixed(2));
  if (st !== S.earState) {
    S.earState = st;
    ear.dataset.state = st;
    ear.title = st === 'hearing' ? 'I hear you' : st === 'ready' ? 'Listening' : '';
    $('b-mic').classList.toggle('live', st === 'hearing');
  }
}

let t0 = performance.now(), last = t0, lastDraw = 0;
let blinkAt = 2000, blinkStart = -1, glanceAt = 1500, waveAt = 15000, mouth = 0;
let wasSleeping = false, prevHop = false, prevWalking = false, yawnedForSleep = false;
const idleGlance = { x: 0, y: 0 };

function frame(now) {
  requestAnimationFrame(frame);
  // Drowsy, then asleep, when nothing happens and the pointer stays still (sooner late at night).
  const idleFor = now - Math.max(S.lastActive, S.lastMove);
  const sleepAfter = isNight() ? 45000 : 75000;
  const sleeping = idleFor > sleepAfter && !S.walking && !S.dragged && !S.thinking && !S.asking && !S.chatting;
  S.sleeping = sleeping;
  if (sleeping && wasSleeping && now - lastDraw < 66) return;       // asleep: ~15 fps is plenty
  const dt = Math.min(0.05, (now - last) / 1000 || 0.016);
  last = now; lastDraw = now;
  const t = (now - t0) / 1000;
  const tr = meta.traits;

  const speaking = S.out > 0.012;
  const pttOpen = S.flags.listen_mode !== 'push' || S.flags.ptt_held;
  if (S.voice === 'hearing' && now - S.voiceAt > 30000) S.voice = 'idle';                 // never stay "hearing" by mistake
  const hearing = S.voice === 'hearing' && !speaking && S.flags.mic && pttOpen;
  const listening = !speaking && S.flags.mic && pttOpen && !S.thinking && (hearing || S.mic > 0.03);
  updateEar(speaking, hearing, pttOpen);
  const busy = speaking || listening || S.thinking || !!S.act || S.asking || S.chatting;
  const poked = now - S.poke < 1000;
  const dizzy = now < S.dizzyUntil;
  const grumpy = now < S.grumpyUntil;
  const sad = now < S.sadUntil && !grumpy;
  const pettedNow = now < S.pettedUntil;
  const hovered = now < S.hoverUntil;
  const happy = now < S.happyUntil || pettedNow || (hovered && !busy);
  const startled = now - S.startle < 700;
  const curious = now < S.curiousUntil, sleepyMood = now < S.sleepyUntil;
  const surprised = now < S.surpriseUntil;
  const pointing = S.act && now < S.lookUntil;
  const sitting = idleFor > sleepAfter * 0.45 && !busy && !S.walking && !S.dragged && !sleeping;

  if (wasSleeping && !sleeping) {
    if (S.gentleWake) S.quirk = { name: 'stretch', start: now, dur: QUIRK_MS.stretch };
    else { S.startle = now; sp.squash.kick(-8); }
    S.gentleWake = false; yawnedForSleep = false;
  }
  wasSleeping = sleeping;
  if (!yawnedForSleep && idleFor > sleepAfter - 14000 && !sleeping && !S.quirk && !busy) { S.quirk = { name: 'yawn', start: now, dur: 2300 }; yawnedForSleep = true; }
  if (S.hop && !prevHop) sp.squash.kick(6);                         // spring off the ground
  if (!S.hop && prevHop) { sp.squash.kick(-8); burst('puff', 2, { spread: 0.5, y: 0.93 }); }   // land with a squash and a puff of dust
  prevHop = S.hop;
  if (S.walking !== prevWalking) { sp.lean.kick(S.walking ? S.dir * 40 : -S.dir * 30); prevWalking = S.walking; }

  maybeQuirk(now, !busy && !S.walking && !S.dragged && !sleeping && !sitting && !dizzy && !grumpy);
  const q = S.quirk;
  const qp = q ? clamp((now - q.start) / q.dur, 0, 1) : 0;
  const qe = Math.sin(qp * Math.PI);                                // 0 -> 1 -> 0 over a quirk
  const qa = { aL: 0, aR: 0, tilt: 0, sy: 0, lid: null, mouth: 0, lookX: null, hop: 0, turn: 1, happy: false };
  if (q) {
    switch (q.name) {
      case 'lookaround': qa.lookX = Math.sin(qp * Math.PI * 2) * 1.1; break;
      case 'stretch': qa.aL = 150 * qe; qa.aR = -150 * qe; qa.sy = 0.07 * qe; qa.lid = 1 - 0.55 * qe; break;
      case 'yawn': qa.mouth = qe; qa.lid = 1 - 0.9 * qe; qa.aR = -45 * qe; qa.sy = 0.03 * qe; break;
      case 'hum': qa.tilt = Math.sin(t * 4) * 4 * qe; qa.mouth = 0.22 * (0.5 + 0.5 * Math.sin(t * 8)) * qe; qa.lid = 1 - 0.2 * qe; break;
      case 'wiggle': qa.tilt = Math.sin(t * 22) * 6 * qe; qa.aL = Math.sin(t * 22) * 12 * qe; qa.aR = Math.sin(t * 22) * 12 * qe; break;
      case 'hop': qa.hop = -26 * Math.sin(qp * Math.PI); qa.sy = qp < 0.2 ? -0.1 * (1 - qp / 0.2) : 0; break;
      case 'twirl': qa.turn = Math.cos(qp * Math.PI * 2); qa.hop = -8 * qe; break;   // turns all the way round
      case 'sneeze': {                                                               // ah... ah... choo!
        const build = clamp(qp / 0.55, 0, 1), after = clamp((qp - 0.6) / 0.4, 0, 1);
        if (qp < 0.6) { qa.tilt = -7 * build; qa.lid = 1 - 0.7 * build; qa.mouth = 0.35 * build; qa.sy = 0.04 * build; }
        else { qa.tilt = 9 * (1 - after); qa.lid = 0.15 + 0.85 * after; }
        if (qp >= 0.6 && !q.done) {
          q.done = true; sp.squash.kick(-10);
          if (Math.random() < 0.5) quip(pick(L.sneeze), 1300);
          burst('spark', 3, { y: 0.55, spread: 0.3 });
        }
        break;
      }
      case 'peekaboo': {                                                             // hands over eyes, then surprise
        const cover = qp < 0.25 ? qp / 0.25 : qp < 0.72 ? 1 : 1 - (qp - 0.72) / 0.28;
        qa.aL = 158 * cover; qa.aR = -158 * cover;
        if (cover > 0.8) qa.lid = 0.1;
        if (qp > 0.72) qa.happy = true;
        break;
      }
      default: break;
    }
    if (q.name === 'hop' && qp > 0.96 && !q.landed) { q.landed = true; sp.squash.kick(-8); }
  }

  // ---- where the eyes go ----
  // Follow the pointer anywhere on screen for a while after it moves; otherwise glance around.
  if (now > glanceAt) { glanceAt = now + rand(1500, 5000); idleGlance.x = (Math.random() - 0.5) * 1.4; idleGlance.y = (Math.random() - 0.5) * 0.8; }
  const ex = S.cursor.x - G.ex, ey = S.cursor.y - G.ey, ed = Math.hypot(ex, ey) || 1;
  const tracking = now - S.cursor.t < 20000 && !sleeping;
  const close = ed < 34;                                            // right on my nose: go cross-eyed
  let want = idleGlance;
  if (tracking && !busy) {
    const s = Math.min(1, ed / 110);
    want = { x: (ex / ed) * s, y: clamp((ey / ed) * s, -1, 1) };
  }
  if (qa.lookX !== null) want = { x: qa.lookX, y: -0.1 };
  if (S.thinking && !speaking) want = { x: -0.55 + Math.sin(t * 1.3) * 0.2, y: -0.9 };
  else if (listening || speaking || S.chatting) want = { x: 0, y: 0.05 };
  if (S.dragged) want = { x: Math.sin(t * 3) * 0.3, y: -0.4 };
  if (now < S.lookUntil) want = S.lookTarget;
  const ease = surprised ? 0.4 : 0.16;
  S.look.x = lerp(S.look.x, want.x, ease);
  S.look.y = lerp(S.look.y, want.y, ease);
  const crossed = now < S.crossUntil || (tracking && close && !busy);
  if (tracking && close && !busy) {
    if (!H.crossSince) H.crossSince = now;
    if (now - H.crossSince > 1500 && now > H.nextCross) { H.nextCross = now + 25000; quip(pick(L.cross), 1600); }
  } else H.crossSince = 0;

  // ---- eyes: blink, sleepy, happy ----
  if (blinkStart < 0 && now > blinkAt) blinkStart = now;
  let lid = tr.lid || 1;
  if (blinkStart >= 0) {
    const p = (now - blinkStart) / 150;
    lid = p >= 1 ? lid : Math.min(lid, Math.abs(1 - 2 * p));
    if (p >= 1) { blinkStart = -1; blinkAt = now + rand(2000, 5500) / (tr.blink || 1); }
  }
  if (qa.lid !== null) lid = Math.min(lid, qa.lid);
  if (S.thinking && !speaking) lid = Math.min(lid, 0.85);
  if (sleeping) lid = 0.04 + 0.02 * Math.sin(t * 0.8);
  else if (sitting) lid = Math.min(lid, 0.8 - clamp((idleFor - sleepAfter * 0.7) / (sleepAfter * 0.3), 0, 1) * 0.45);   // getting heavy
  if (sleepyMood) lid = Math.min(lid, 0.55);
  if (curious) lid = Math.max(lid, 1.12);                           // eyes wide
  if (grumpy) lid = Math.min(lid, 0.62);
  if (sad) lid = Math.min(lid, 0.78);
  if (now < S.squintUntil) lid = 0.08;
  if (startled || surprised || S.dragged || dizzy) lid = 1.2;

  // ---- mouth ----
  const mTarget = speaking ? Math.min(1, S.out * 9) : qa.mouth || (startled || S.dragged || surprised ? 0.5 : dizzy ? 0.25 : 0);
  mouth = lerp(mouth, mTarget, 0.35);

  // ---- body ----
  const energy = tr.energy || 1;
  const breath = Math.sin(t * (sleeping ? 1.2 : 2.2)) * (sleeping ? 0.025 : 0.015);
  let bob = 0, tilt = qa.tilt, hopY = qa.hop;
  const sit = sp.sit.step(sitting || sleeping ? 1 : 0, dt);
  if (S.walking) {
    const ph = t * 9 * (tr.speed || 1);
    if (tr.float) { bob = Math.sin(t * 3.2) * 5 - 6; tilt += Math.sin(t * 2.4) * 3 * (tr.sway || 1); }
    else {
      bob = -Math.abs(Math.sin(ph)) * 5 * (tr.bounce || 1);
      tilt += Math.sin(ph) * 3 * (tr.sway || 1) * (tr.waddle || 1);
    }
    if (S.hop) hopY += -Math.abs(Math.sin(t * 5)) * 20 * (tr.bounce || 1);
    set('footL', { cy: 220 + (tr.float ? 0 : Math.sin(ph) * 5) });
    set('footR', { cy: 220 - (tr.float ? 0 : Math.sin(ph) * 5) });
  } else if (S.dragged) {
    set('footL', { cy: 228 + Math.sin(t * 6) * 6 }); set('footR', { cy: 228 + Math.sin(t * 6 + 1.6) * 6 });
    hopY = -10; tilt += Math.sin(t * 5) * 6;
  } else {
    set('footL', { cy: 220 }); set('footR', { cy: 220 });
    if (tr.float) bob += Math.sin(t * 1.8) * 3 - 3;
  }
  if (!S.walking && !S.dragged && !sleeping && !sitting && !tr.float) bob -= (Math.sin(t * 3.1) * 0.5 + 0.5) * 1.6 * (tr.bounce || 1);   // alive even when idle
  if (happy && !S.walking && !S.dragged) { tilt += Math.sin(t * 9) * 2.5; bob -= Math.abs(Math.sin(t * 6)) * 3; }               // happy little wiggle-bounce
  if (hovered && !busy && !S.walking && !sleeping) { tilt += Math.sin(t * 7) * 2; bob -= Math.abs(Math.sin(t * 5)) * 2; }       // glad to see you
  if (listening) { tilt += Math.sin(t * 5) * 3; bob += Math.sin(t * 10) * 1.5; }                // nodding along
  if (speaking) { bob -= Math.abs(Math.sin(t * 8)) * 3 * Math.min(1, S.out * 9); tilt += Math.sin(t * 3.1) * 2 * (tr.sway || 1); }
  if (S.thinking && !speaking) tilt += Math.sin(t * 2) * 4 - 3;                                  // head-tilt ponder
  if (tracking && !busy && !S.walking) tilt += clamp(ex / 260, -1, 1) * 4;                       // lean toward the pointer
  if (pettedNow) tilt += Math.sin(t * 3) * 5;                                                    // leaning into the pats
  if (dizzy) { tilt += Math.sin(t * 7) * 9; bob += Math.sin(t * 3.5) * 3; }
  if (curious) tilt += 7 + Math.sin(t * 2) * 1.5;                                                // head tilt
  if (sleepyMood) tilt += 4 + Math.sin(t * 0.9) * 2;
  if (grumpy) tilt += Math.sin(t * 1.5) * 1.5 - 4;
  if (sad) { tilt += 3; bob += 3; }
  if (sleeping) tilt = Math.sin(t * 0.6) * 4 + 5;
  tilt += sp.lean.step(0, dt) * 0.1 + sp.tilt.step(0, dt);
  if (startled) hopY -= Math.sin(((now - S.startle) / 700) * Math.PI) * 24;
  if (surprised) hopY -= Math.sin(((S.surpriseUntil - now) / 650) * Math.PI) * 8;
  bob += sit * 6;

  const q2 = sp.squash.step(0, dt);
  const sy = 1 - breath + q2 + qa.sy - sit * 0.1 + (S.hop ? Math.abs(Math.sin(t * 5)) * 0.08 : 0);
  const sx = 1 + breath - q2 * 0.6 + sit * 0.05;
  const flip = (S.walking ? S.dir : 1) * (Math.abs(qa.turn) < 0.08 ? Math.sign(qa.turn || 1) * 0.08 : qa.turn);
  set('root', { transform: `translate(100 0) scale(${flip.toFixed(3)} 1) translate(-100 0)` });
  set('body', { transform: `translate(0 ${(bob + hopY).toFixed(2)}) translate(100 222) rotate(${tilt.toFixed(2)}) scale(${sx.toFixed(3)} ${sy.toFixed(3)}) translate(-100 -222)` });
  set('shadow', { rx: Math.max(30, 52 - -hopY * 0.4) });

  // ---- face ----
  set('face', { transform: `translate(${(S.look.x * 7).toFixed(2)} ${(S.look.y * 4.5).toFixed(2)})` });
  const wideEyes = (happy || qa.happy) && !startled && !dizzy && !grumpy && !surprised;
  for (const [id, hid, cx] of [['eyeL', 'happyL', 74], ['eyeR', 'happyR', 126]]) {
    set(id, { transform: `translate(${cx} 140) scale(1 ${Math.max(0.05, lid).toFixed(3)})`, display: wideEyes ? 'none' : 'inline' });
    set(hid, { display: wideEyes ? 'inline' : 'none' });
  }
  for (const [p, h, side] of [['pupL', 'hiL', 1], ['pupR', 'hiR', -1]]) {
    let px = S.look.x * 6, py = S.look.y * 7.5;
    if (crossed) { px = side * 4.5; py = 2; }
    const r = surprised || startled ? 6.5 : happy ? 8.6 : 8;                                    // pupils shrink in surprise
    set(p, { cx: px.toFixed(2), cy: py.toFixed(2), r, opacity: dizzy ? 0 : 1 });
    set(h, { cx: (px + 3).toFixed(2), cy: (py - 3.5).toFixed(2), opacity: dizzy ? 0 : 1 });
    set(side === 1 ? 'loL' : 'loR', { cx: (px - 3).toFixed(2), cy: (py + 3.5).toFixed(2), opacity: dizzy ? 0 : 0.85 });
  }
  for (const id of ['dizzyL', 'dizzyR']) {
    const cx = id === 'dizzyL' ? 74 : 126;
    set(id, { display: dizzy ? 'inline' : 'none', transform: `translate(${cx} 140) rotate(${((t * 420) % 360).toFixed(1)})` });
  }
  // eyebrows only when there is a feeling to show
  let brow = null;
  if (grumpy) brow = { a: 16, y: 6 };
  else if (sad) brow = { a: -14, y: 2 };
  else if (startled || surprised || dizzy) brow = { a: -4, y: -6 };
  else if (curious) brow = { a: -6, y: -4 };
  else if (S.thinking && !speaking) brow = { a: 0, y: -2, raise: true };
  set('browL', { display: brow ? 'inline' : 'none', transform: brow ? `translate(0 ${brow.raise ? -5 : brow.y}) rotate(${brow.a} 74 114)` : '' });
  set('browR', { display: brow ? 'inline' : 'none', transform: brow ? `translate(0 ${brow.y}) rotate(${-brow.a} 126 114)` : '' });

  const glow = happy || poked || grumpy ? 0.95 : 0.55;
  const puff = grumpy ? 1.35 : 1;
  set('cheekL', { opacity: glow, rx: 9 * puff }); set('cheekR', { opacity: glow, rx: 9 * puff });
  const curve = grumpy ? -5 : sad ? -4 : happy ? 11 : dizzy ? 1 : (listening || speaking || S.walking) ? 7 : sleeping ? 2 : 4;
  if (mouth < 0.07) {
    set('mouth', { display: 'none' });
    const y0 = curve < 0 ? 170 : 166;
    set('smile', { display: 'inline', d: dizzy ? `M88 167 Q94 ${163 + Math.sin(t * 9) * 2} 100 167 Q106 ${171 - Math.sin(t * 9) * 2} 112 167` : `M88 ${y0} Q100 ${y0 + curve} 112 ${y0}` });
  } else {
    set('smile', { display: 'none' });
    set('mouth', { display: 'inline', cx: 100, cy: 170, rx: 9 - mouth * 2, ry: 2.5 + mouth * 10 });
  }

  // ---- arms ----
  let aL = 14 + qa.aL, aR = -14 + qa.aR;
  if (S.walking) { const w = Math.sin(t * 9 * (tr.speed || 1)) * 22 * energy; aL += w; aR += w; }
  if (speaking) { aL += Math.sin(t * 4.3) * 10 * energy; aR -= Math.sin(t * 3.7) * 10 * energy; }
  if (S.thinking && !speaking) aR = -125 + Math.sin(t * 3) * 4;                                   // hand to chin
  if (startled) { aL = 40; aR = -40; }
  if (poked) { aL = 14 + 35 * Math.sin(t * 30); aR = -14 - 35 * Math.sin(t * 30); }
  if (grumpy) { aL = 62; aR = -62; }                                                              // arms folded-ish
  if (dizzy) { aL = 40 + Math.sin(t * 6) * 25; aR = -40 + Math.sin(t * 6 + 2) * 25; }
  if (S.dragged) { aL = 130 + Math.sin(t * 7) * 18; aR = -130 - Math.sin(t * 7) * 18; }
  if (now > waveAt && !S.walking && !busy && !sleeping && !q) { S.wave = now; waveAt = now + rand(25000, 55000); }
  if (now - S.wave < 1800 && now >= S.wave) aR = -150 + Math.sin(t * 14) * 18;
  if (pointing) aR = Math.atan2(-S.lookTarget.x * flip, S.lookTarget.y) * 180 / Math.PI;           // swings toward the target
  set('armL', { transform: `translate(52 150) rotate(${aL.toFixed(2)})` });
  set('armR', { transform: `translate(148 150) rotate(${aR.toFixed(2)})` });

  // ---- optional parts: each character's own bits (ears, tail, antenna, flame...) ----
  const sway = Math.sin(t * 2.2);
  const qn = q ? q.name : '';
  if (parts.antenna) {
    rot('antenna', Math.sin(t * 2 + 1) * 4 + (qn === 'antenna' ? Math.sin(t * 18) * 10 * qe : 0) + (sleeping ? 14 : 0) + (S.thinking ? Math.sin(t * 7) * 5 : 0) + (dizzy ? Math.sin(t * 9) * 14 : 0));
    const live = S.connected;
    const g = listening ? 0.7 + 0.3 * Math.sin(t * 12) : speaking ? 0.9 : S.thinking ? 0.6 + 0.4 * Math.sin(t * 6)
      : qn === 'antenna' ? 0.5 + 0.5 * Math.sin(t * 20) : sleeping ? 0.2 : live ? 0.6 : 0.2;
    const bulbBase = meta.antenna ? meta.antenna.bulb : '#9be3b0';
    set('bulb', { fill: !live ? '#b8b8c0' : S.thinking ? '#ffd27a' : grumpy ? '#ff8a8a' : bulbBase, opacity: g, r: 8 + (listening ? 2 : 0) });
  }
  if (parts.antenna2) rot('antenna2', -Math.sin(t * 2.3) * 5 + (qn === 'antenna' ? -Math.sin(t * 16) * 10 * qe : 0) + (sleeping ? -14 : 0));
  if (parts.tail) rot('tail', sway * 4 * (tr.sway || 1) + (qn === 'tailWag' ? Math.sin(t * 14) * 20 * qe : 0) + (listening ? Math.sin(t * 8) * 6 : 0) + (happy ? Math.sin(t * 16) * 14 : 0) + (grumpy ? Math.sin(t * 20) * 6 : 0));
  const perk = hearing ? -7 : listening || surprised ? -4 : 0;               // ears up when I hear you
  const flick = qn === 'earTwitch' ? Math.sin(qp * Math.PI * 4) * 16 * qe : 0;
  const droop = sleeping ? 10 : sad ? 12 : sitting ? 4 : pettedNow ? 8 : 0;
  if (parts.earL) rot('earL', -(Math.sin(t * 1.7 + 1) * 1.5 + perk + droop + flick + (speaking ? Math.sin(t * 6) * 2 : 0)));
  if (parts.earR) rot('earR', Math.sin(t * 1.9) * 1.5 + perk + droop + (qn === 'earTwitch' ? Math.sin(qp * Math.PI * 3 + 1) * 10 * qe : 0) + (speaking ? Math.sin(t * 6 + 1) * 2 : 0));
  if (parts.puff) grow('puff', 1 + Math.sin(t * 1.6) * 0.03 + (speaking ? 0.02 * Math.sin(t * 9) : 0) + (grumpy ? 0.06 : 0), 1 + Math.cos(t * 1.6) * 0.03);
  if (parts.flame) { rot('flame', Math.sin(t * 3) * 3); grow('flame', 1, 1 + Math.sin(t * 9) * 0.05 + (qn === 'flicker' ? Math.sin(t * 30) * 0.14 * qe : 0) + (sleeping ? -0.25 : 0) + (happy ? 0.12 : 0) + (grumpy ? Math.sin(t * 25) * 0.1 : 0)); }
  if (parts.moss) rot('moss', Math.sin(t * 1.2) * 2.5);
  if (parts.drop) {
    const f = qn === 'drip' ? qp : sad ? (t * 0.6) % 1 : 0;
    set('drop', { opacity: f > 0 && f < 0.95 ? 1 - f * 0.6 : 0, transform: `translate(0 ${(f * 70).toFixed(1)})` });
  }

  // ---- Zzz ----
  set('zzz', { opacity: sleeping ? 1 : 0 });
  if (sleeping) {
    for (const [i, id] of ['z1', 'z2', 'z3'].entries()) {
      const ph = (t * 0.5 + i * 0.33) % 1;
      set(id, { opacity: Math.sin(ph * Math.PI).toFixed(2), transform: `translate(${(ph * 6).toFixed(1)} ${(-ph * 14).toFixed(1)})` });
    }
  }
}
requestAnimationFrame(frame);
