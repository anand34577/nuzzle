const T = window.__TAURI__;
const invoke = (c, a) => T.core.invoke(c, a);
const $ = (id) => document.getElementById(id);
let origin = { mx: 0, my: 0, scale: 1 };
let rect = null;                 // {x, y, w, h} in CSS px
let dragFrom = null;

function draw() {
  const sel = $('sel');
  if (!rect) { sel.hidden = true; $('dim').hidden = false; $('ok').disabled = true; return; }
  sel.hidden = false; $('dim').hidden = true;
  Object.assign(sel.style, { left: rect.x + 'px', top: rect.y + 'px', width: rect.w + 'px', height: rect.h + 'px' });
  $('size').textContent = `${Math.round(rect.w * origin.scale)} x ${Math.round(rect.h * origin.scale)} px`;
  $('size').style.top = rect.y < 30 ? '4px' : '-26px';
  const big = rect.w * origin.scale >= 200 && rect.h * origin.scale >= 150;
  $('ok').disabled = !big;
  $('err').textContent = big ? '' : 'That is a bit small: make it at least 200 x 150 pixels.';
}

window.addEventListener('mousedown', (e) => {
  if (e.target.closest('#panel') || e.button !== 0) return;
  dragFrom = { x: e.clientX, y: e.clientY }; rect = { x: e.clientX, y: e.clientY, w: 0, h: 0 }; draw();
});
window.addEventListener('mousemove', (e) => {
  if (!dragFrom) return;
  rect = { x: Math.min(dragFrom.x, e.clientX), y: Math.min(dragFrom.y, e.clientY), w: Math.abs(e.clientX - dragFrom.x), h: Math.abs(e.clientY - dragFrom.y) };
  draw();
});
window.addEventListener('mouseup', () => { dragFrom = null; if (rect && (rect.w < 4 || rect.h < 4)) { rect = null; draw(); } });

async function confirmArea() {
  if (!rect || $('ok').disabled) return;
  try {
    await invoke('save_area', {
      x: origin.mx + Math.round(rect.x * origin.scale), y: origin.my + Math.round(rect.y * origin.scale),
      w: Math.round(rect.w * origin.scale), h: Math.round(rect.h * origin.scale),
    });
    rect = null; draw();
  } catch (e) { $('err').textContent = String(e); }
}
const cancel = () => { rect = null; draw(); $('err').textContent = ''; invoke('cancel_area'); };
$('ok').onclick = confirmArea;
$('cancel').onclick = cancel;
window.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); confirmArea(); }
  if (e.key === 'Escape') cancel();
});
T.event.listen('area-start', (e) => {
  origin = { mx: e.payload.mx, my: e.payload.my, scale: e.payload.scale || 1 };
  const c = e.payload.current;
  rect = c ? { x: (c.x - origin.mx) / origin.scale, y: (c.y - origin.my) / origin.scale, w: c.w / origin.scale, h: c.h / origin.scale } : null;
  $('err').textContent = ''; draw();
});
draw();
