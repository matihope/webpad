'use strict';

// Wire format: see docs/protocol.md. Keep in sync with src/protocol.rs.
// BTN, playerColor and store come from common.js.
const PROTOCOL_VERSION = 2;
const MSG = {
  HELLO: 0x01, STATE: 0x02, PING: 0x03, NAME: 0x04,
  WELCOME: 0x81, PONG: 0x82, RUMBLE: 0x83, REJECT: 0x84, SLOT: 0x85, IDENTIFY: 0x86,
};
const REJECT = { FULL: 1, BAD_VERSION: 2, BACKEND: 3, BAD_CODE: 4, KICKED: 5, LOCKED: 6 };
const REJECT_TEXT = {
  [REJECT.FULL]: 'Server is full',
  [REJECT.BAD_VERSION]: 'Server was updated — reload the page',
  [REJECT.BACKEND]: 'Server could not create a controller',
  [REJECT.BAD_CODE]: 'Wrong join code',
  [REJECT.KICKED]: 'You were removed by the host',
  [REJECT.LOCKED]: 'The host has locked joining',
};
const MAX_NAME_BYTES = 24;
const PING_MS = 500;
const PONG_TIMEOUT_MS = 2500;

const $ = (sel) => document.querySelector(sel);
const pad = $('#pad');
const buttonEls = [...document.querySelectorAll('[data-btn]')];
const dpadEl = $('.dpad');

// ---- Controller state -------------------------------------------------------

const state = { buttons: 0, lx: 0, ly: 0, rx: 0, ry: 0, lt: 0, rt: 0 };
let lastSent = null;

/** Active pointers: id -> { stick } for stick drags, { x, y } for button presses. */
const pointers = new Map();
const sticks = {};
for (const zone of document.querySelectorAll('.stick-zone')) {
  const id = zone.dataset.stick;
  sticks[id] = {
    zone,
    base: zone.querySelector('.stick-base'),
    knob: zone.querySelector('.stick-knob'),
    xKey: id + 'x',
    yKey: id + 'y',
    pointer: null,
    ox: 0,
    oy: 0,
  };
}

function onPointerDown(e) {
  if (!e.target.closest('#pad') || e.target.closest('.ui')) return;
  e.preventDefault();
  const zone = e.target.closest('.stick-zone');
  const stick = zone && sticks[zone.dataset.stick];
  if (stick && stick.pointer === null) {
    const r = zone.getBoundingClientRect();
    stick.pointer = e.pointerId;
    stick.ox = e.clientX - r.left;
    stick.oy = e.clientY - r.top;
    zone.classList.add('active');
    pointers.set(e.pointerId, { stick });
    moveStick(stick, e);
  } else {
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    updateButtons();
  }
}

function onPointerMove(e) {
  const p = pointers.get(e.pointerId);
  if (!p) return;
  if (p.stick) {
    moveStick(p.stick, e);
  } else {
    p.x = e.clientX;
    p.y = e.clientY;
    updateButtons();
  }
}

function onPointerUp(e) {
  const p = pointers.get(e.pointerId);
  if (!p) return;
  pointers.delete(e.pointerId);
  if (p.stick) resetStick(p.stick);
  else updateButtons();
}

function moveStick(stick, e) {
  const r = stick.zone.getBoundingClientRect();
  const radius = stick.base.offsetWidth / 2;
  let dx = e.clientX - r.left - stick.ox;
  let dy = e.clientY - r.top - stick.oy;
  const len = Math.hypot(dx, dy);
  if (len > radius) {
    dx *= radius / len;
    dy *= radius / len;
  }
  stick.base.style.left = stick.ox + 'px';
  stick.base.style.top = stick.oy + 'px';
  stick.knob.style.transform = `translate(${dx}px, ${dy}px)`;
  state[stick.xKey] = Math.round((dx / radius) * 32767);
  state[stick.yKey] = Math.round((-dy / radius) * 32767); // protocol: positive Y is up
  stateChanged(false);
}

function resetStick(stick) {
  stick.pointer = null;
  stick.zone.classList.remove('active');
  stick.base.style.left = stick.base.style.top = '';
  stick.knob.style.transform = '';
  state[stick.xKey] = state[stick.yKey] = 0;
  stateChanged(true);
}

/** D-pad direction bits for a point; 8 equal sectors around the center, small dead zone. */
function dpadBits(x, y) {
  const r = dpadEl.getBoundingClientRect();
  const dx = x - (r.left + r.width / 2);
  const dy = y - (r.top + r.height / 2);
  const len = Math.hypot(dx, dy);
  if (len < r.width * 0.1) return 0;
  const nx = dx / len;
  const ny = dy / len;
  const t = Math.sin(Math.PI / 8); // 22.5°
  return (ny < -t ? BTN.DPAD_UP : 0) | (ny > t ? BTN.DPAD_DOWN : 0)
       | (nx < -t ? BTN.DPAD_LEFT : 0) | (nx > t ? BTN.DPAD_RIGHT : 0);
}

/** Recompute buttons from every non-stick pointer, so thumbs can slide between buttons. */
function updateButtons() {
  let buttons = 0, lt = 0, rt = 0;
  const lit = new Set();
  for (const p of pointers.values()) {
    if (p.stick) continue;
    const el = document.elementFromPoint(p.x, p.y)?.closest('[data-btn], .dpad');
    if (!el) continue;
    if (el === dpadEl) {
      buttons |= dpadBits(p.x, p.y);
      continue;
    }
    const name = el.dataset.btn;
    if (name === 'LT') lt = 255;
    else if (name === 'RT') rt = 255;
    else buttons |= BTN[name];
    lit.add(el);
  }
  for (const el of buttonEls) el.classList.toggle('on', lit.has(el));
  dpadEl.classList.toggle('up', !!(buttons & BTN.DPAD_UP));
  dpadEl.classList.toggle('down', !!(buttons & BTN.DPAD_DOWN));
  dpadEl.classList.toggle('left', !!(buttons & BTN.DPAD_LEFT));
  dpadEl.classList.toggle('right', !!(buttons & BTN.DPAD_RIGHT));

  const newlyPressed = (buttons & ~state.buttons) || (lt && !state.lt) || (rt && !state.rt);
  if (newlyPressed) navigator.vibrate?.(12);
  state.buttons = buttons;
  state.lt = lt;
  state.rt = rt;
  stateChanged(true);
}

function releaseAll() {
  pointers.clear();
  for (const stick of Object.values(sticks)) if (stick.pointer !== null) resetStick(stick);
  updateButtons();
}

// ---- Sending ----------------------------------------------------------------

let flushQueued = false;

/** Button changes go out immediately; stick motion is batched to one frame per display refresh. */
function stateChanged(urgent) {
  if (urgent) {
    flush();
  } else if (!flushQueued) {
    flushQueued = true;
    requestAnimationFrame(() => {
      flushQueued = false;
      flush();
    });
  }
}

function flush(force = false) {
  if (!ws || ws.readyState !== WebSocket.OPEN || slot === null) return;
  if (!force && lastSent && Object.keys(state).every((k) => state[k] === lastSent[k])) return;
  const dv = new DataView(new ArrayBuffer(13));
  dv.setUint8(0, MSG.STATE);
  dv.setUint16(1, state.buttons, true);
  dv.setInt16(3, state.lx, true);
  dv.setInt16(5, state.ly, true);
  dv.setInt16(7, state.rx, true);
  dv.setInt16(9, state.ry, true);
  dv.setUint8(11, state.lt);
  dv.setUint8(12, state.rt);
  ws.send(dv.buffer);
  lastSent = { ...state };
}

// ---- Connection -------------------------------------------------------------

let ws = null;
let slot = null;
let retryMs = 500;
let lastPong = 0;
let stopped = false;
let rejectText = null; // shown instead of "reconnecting…" while the server keeps refusing us
let rttAvg = null;

const now32 = () => Math.floor(performance.now()) >>> 0;

function loadToken() {
  // sessionStorage, not localStorage: survives reloads, but two tabs stay two players.
  try {
    const hex = sessionStorage.getItem('webpad-token') || '';
    if (hex.length === 32) return Uint8Array.from(hex.match(/../g), (b) => parseInt(b, 16));
  } catch {}
  return new Uint8Array(16);
}

function saveToken(bytes) {
  try {
    sessionStorage.setItem('webpad-token', [...bytes].map((b) => b.toString(16).padStart(2, '0')).join(''));
  } catch {}
}

// A join code from the QR link is remembered, then removed from the address bar.
const urlCode = new URLSearchParams(location.search).get('code');
if (urlCode !== null) {
  store.set(CODE_KEY, urlCode);
  history.replaceState(null, '', location.pathname);
}

function connect() {
  const code = store.get(CODE_KEY);
  const query = code ? `?code=${encodeURIComponent(code)}` : '';
  const sock = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws${query}`);
  sock.binaryType = 'arraybuffer';
  ws = sock;
  setConn(rejectText || (retryMs === 500 ? 'connecting…' : 'reconnecting…'));

  sock.onopen = () => {
    const hello = new Uint8Array(18);
    hello[0] = MSG.HELLO;
    hello[1] = PROTOCOL_VERSION;
    hello.set(loadToken(), 2);
    sock.send(hello);
    lastPong = performance.now();
  };
  sock.onmessage = (e) => {
    if (ws === sock) onMessage(new DataView(e.data));
  };
  sock.onclose = () => {
    if (ws === sock) dropConnection();
  };
}

/** Forget the current socket and schedule a reconnect. */
function dropConnection() {
  const sock = ws;
  ws = null;
  slot = null;
  lastSent = null;
  rttAvg = null;
  showPlayer();
  try { sock?.close(); } catch {}
  if (stopped) return;
  setTimeout(connect, retryMs);
  retryMs = Math.min(retryMs * 2, 4000);
}

function onMessage(dv) {
  switch (dv.getUint8(0)) {
    case MSG.WELCOME:
      slot = dv.getUint8(2);
      saveToken(new Uint8Array(dv.buffer, 3, 16));
      retryMs = 500;
      rejectText = null;
      showPlayer();
      setConn('connected');
      flush(true);
      sendName();
      break;
    case MSG.SLOT:
      // The host reordered players: same connection, new number.
      slot = dv.getUint8(1);
      showPlayer();
      flash();
      flush(true);
      break;
    case MSG.IDENTIFY:
      flash();
      break;
    case MSG.PONG: {
      lastPong = performance.now();
      const rtt = (now32() - dv.getUint32(1, true)) >>> 0;
      rttAvg = rttAvg === null ? rtt : rttAvg * 0.8 + rtt * 0.2;
      $('#rtt').textContent = `${Math.round(rttAvg)} ms`;
      break;
    }
    case MSG.RUMBLE: {
      const strength = Math.max(dv.getUint8(1), dv.getUint8(2));
      navigator.vibrate?.(strength ? 1000 : 0);
      break;
    }
    case MSG.REJECT: {
      const reason = dv.getUint8(1);
      rejectText = REJECT_TEXT[reason] || 'Rejected by server';
      setConn(rejectText);
      retryMs = 3000;
      // These won't fix themselves by retrying; ask the player what to do.
      if (reason === REJECT.BAD_VERSION || reason === REJECT.BAD_CODE || reason === REJECT.KICKED) {
        stopped = true;
        showStopped(reason);
      }
      break;
    }
  }
}

setInterval(() => {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  if (performance.now() - lastPong > PONG_TIMEOUT_MS) {
    setConn('connection lost');
    dropConnection();
    return;
  }
  const dv = new DataView(new ArrayBuffer(5));
  dv.setUint8(0, MSG.PING);
  dv.setUint32(1, now32(), true);
  ws.send(dv.buffer);
}, PING_MS);

function setConn(text) {
  $('#conn').textContent = text;
  if (text !== 'connected') $('#rtt').textContent = '';
}

function showPlayer() {
  $('#player').textContent = slot === null ? 'P–' : `P${slot + 1}`;
  $('#name-label').textContent = store.get(NAME_KEY);
  document.documentElement.style.setProperty('--player', slot === null ? 'var(--muted)' : playerColor(slot));
}

/** Full-screen blink in the player color, so the host can see who is who. */
function flash() {
  const el = $('#flash');
  el.classList.remove('on');
  void el.offsetWidth; // restart the animation
  el.classList.add('on');
  navigator.vibrate?.([120, 80, 120]);
}

/** Trim to MAX_NAME_BYTES of UTF-8 without splitting a character. */
function clampName(name) {
  const enc = new TextEncoder();
  let out = name.trim();
  while (enc.encode(out).length > MAX_NAME_BYTES) out = [...out].slice(0, -1).join('');
  return out;
}

function sendName() {
  if (!ws || ws.readyState !== WebSocket.OPEN || slot === null) return;
  const bytes = new TextEncoder().encode(clampName(store.get(NAME_KEY)));
  const frame = new Uint8Array(2 + bytes.length);
  frame[0] = MSG.NAME;
  frame[1] = bytes.length;
  frame.set(bytes, 2);
  ws.send(frame);
}

/** Overlay for rejections that need the player: wrong code, kicked, outdated page. */
let stoppedReason = null;
function showStopped(reason) {
  stoppedReason = reason;
  $('#stopped-text').textContent = REJECT_TEXT[reason];
  $('#stopped-code').hidden = reason !== REJECT.BAD_CODE;
  $('#stopped-code input').value = store.get(CODE_KEY);
  $('#rejoin').textContent = reason === REJECT.BAD_VERSION ? 'Reload' : 'Join again';
  $('#stopped').hidden = false;
}

$('#stopped-form').addEventListener('submit', (e) => {
  e.preventDefault();
  if (stoppedReason === REJECT.BAD_VERSION) return location.reload();
  if (!$('#stopped-code').hidden) store.set(CODE_KEY, $('#stopped-code input').value.trim());
  $('#stopped').hidden = true;
  stopped = false;
  rejectText = null;
  retryMs = 500;
  connect();
});

// ---- Fullscreen, orientation, wake lock -------------------------------------

const root = document.documentElement;
const canFullscreen = !!(root.requestFullscreen || root.webkitRequestFullscreen);
let wakeLock = null;

async function enterFullscreen() {
  try {
    if (root.requestFullscreen) await root.requestFullscreen({ navigationUI: 'hide' });
    else root.webkitRequestFullscreen?.();
  } catch {}
  try { await screen.orientation?.lock?.('landscape'); } catch {}
}

async function keepAwake() {
  try { wakeLock = await navigator.wakeLock?.request('screen'); } catch {} // HTTPS only
}

function updateFullscreenButton() {
  const inFullscreen = document.fullscreenElement || document.webkitFullscreenElement;
  $('#fs').hidden = !canFullscreen || !!inFullscreen;
}

$('#name').value = store.get(NAME_KEY);
$('#name').addEventListener('input', () => {
  const clamped = clampName($('#name').value);
  if (new TextEncoder().encode($('#name').value.trim()).length > MAX_NAME_BYTES) $('#name').value = clamped;
});
$('#start').addEventListener('click', (e) => {
  if (e.target.closest('input, label')) return; // typing a name, not starting yet
  store.set(NAME_KEY, clampName($('#name').value));
  showPlayer();
  sendName();
  $('#start').hidden = true;
  enterFullscreen();
  keepAwake();
});
$('#fs').addEventListener('click', enterFullscreen);
document.addEventListener('fullscreenchange', updateFullscreenButton);
document.addEventListener('webkitfullscreenchange', updateFullscreenButton);
updateFullscreenButton();

if (!canFullscreen && !navigator.standalone && !matchMedia('(display-mode: fullscreen), (display-mode: standalone)').matches) {
  $('#ios-hint').hidden = false;
  // Chrome, Firefox, etc. on iOS can't add web apps to the home screen; only Safari can.
  if (/CriOS|FxiOS|EdgiOS|OPiOS|GSA\//.test(navigator.userAgent)) {
    $('#ios-hint').innerHTML = 'This browser can’t add webpad to the home screen. For fullscreen, open this address in <b>Safari</b>, then tap Share → <b>Add to Home Screen</b>.';
  }
}

// ---- Wiring -----------------------------------------------------------------

pad.addEventListener('pointerdown', onPointerDown, { passive: false });
window.addEventListener('pointermove', onPointerMove, { passive: false });
window.addEventListener('pointerup', onPointerUp);
window.addEventListener('pointercancel', onPointerUp);
window.addEventListener('blur', releaseAll);
document.addEventListener('visibilitychange', () => {
  if (document.hidden) releaseAll();
  else if (!wakeLock || wakeLock.released) keepAwake();
});
// iOS Safari pinch-zoom still fires gesture events despite the viewport meta.
document.addEventListener('gesturestart', (e) => e.preventDefault());
document.addEventListener('contextmenu', (e) => e.preventDefault());

showPlayer();
connect();
