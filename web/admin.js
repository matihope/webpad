'use strict';
// Admin page: talks JSON over /admin/ws (see docs/architecture.md, "Admin").

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

let ws = null;
let cards = [];
let last = null; // latest state message

// ---- Auth -------------------------------------------------------------------

async function start() {
  let info;
  try {
    info = await (await fetch('/api/info')).json();
  } catch {
    return setTimeout(start, 2000);
  }
  $('#logout').hidden = !info.adminPasswordRequired;
  if (info.adminPasswordRequired && !info.admin) {
    $('#app').hidden = true;
    $('#login').hidden = false;
    $('#password').focus();
  } else {
    $('#login').hidden = true;
    $('#app').hidden = false;
    connect();
  }
}

$('#login').addEventListener('submit', async (e) => {
  e.preventDefault();
  $('#login-error').hidden = true;
  const res = await fetch('/api/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password: $('#password').value }),
  });
  if (res.ok) {
    $('#password').value = '';
    start();
  } else {
    $('#login-error').hidden = false;
    $('#password').select();
  }
});

$('#logout').addEventListener('click', async () => {
  await fetch('/api/logout', { method: 'POST' });
  location.reload();
});

// ---- Connection -------------------------------------------------------------

function connect() {
  const sock = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/admin/ws`);
  let opened = false;
  ws = sock;
  sock.onopen = () => {
    opened = true;
    banner(null);
    // The QR endpoint needs the login cookie, so load it only once we know we're in.
    $('#qr').src = $('#qr-big').src = `/admin/qr.svg?t=${Date.now()}`;
  };
  sock.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.type === 'state') render(msg);
    else if (msg.type === 'error') banner(msg.message, 4000);
  };
  sock.onclose = () => {
    if (ws !== sock) return;
    ws = null;
    if (opened) {
      banner('Connection to webpad lost — reconnecting…');
      setTimeout(connect, 1000);
    } else {
      // Refused before opening: probably logged out or the server is down. Re-check.
      setTimeout(start, 1000);
    }
  };
}

function send(action) {
  if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(action));
}

let bannerTimer = null;
function banner(text, ms) {
  clearTimeout(bannerTimer);
  $('#banner').hidden = !text;
  $('#banner').textContent = text || '';
  if (text && ms) bannerTimer = setTimeout(() => banner(null), ms);
}

// ---- Rendering --------------------------------------------------------------

function buildCards(count) {
  const container = $('#slots');
  container.replaceChildren();
  cards = [];
  for (let i = 0; i < count; i++) {
    const card = $('#slot-template').content.firstElementChild.cloneNode(true);
    card.style.setProperty('--player', playerColor(i));
    $('.badge', card).textContent = `P${i + 1}`;
    card.addEventListener('click', (e) => {
      const act = e.target.closest('button')?.dataset.act;
      if (act) onAction(i, act);
    });
    container.append(card);
    cards.push(card);
  }
}

function onAction(i, act) {
  const p = last?.slots[i];
  switch (act) {
    case 'up': return send({ action: 'swap', a: i, b: i - 1 });
    case 'down': return send({ action: 'swap', a: i, b: i + 1 });
    case 'identify': return send({ action: 'identify', slot: i });
    case 'unplug': return send({ action: 'unplug', slot: i });
    case 'kick':
      if (confirm(`Kick ${p?.name || `P${i + 1}`}? Their controller is unplugged.`)) send({ action: 'kick', slot: i });
  }
}

const axis = (v) => v / 32767;

function render(msg) {
  last = msg;
  if (cards.length !== msg.slots.length) buildCards(msg.slots.length);

  const players = msg.slots.filter(Boolean);
  const live = players.filter((p) => p.connected).length;
  $('#meta').textContent = `${live} connected · ${players.length}/${msg.maxPlayers} slots used · backend: ${msg.backend}`;
  $('#join-url').textContent = $('#qr-big-url').textContent = msg.joinUrl;
  $('#join-code').hidden = !msg.joinCode;
  $('#join-code').textContent = msg.joinCode ? `Code: ${msg.joinCode}` : '';
  $('#locked').checked = msg.locked;
  $('#unplug-stale').disabled = !players.some((p) => !p.connected);

  msg.slots.forEach((p, i) => {
    const card = cards[i];
    card.classList.toggle('is-empty', !p);
    card.classList.toggle('is-live', !!p?.connected);
    card.classList.toggle('is-stale', !!p && !p.connected);
    $('.name', card).textContent = p ? p.name || 'Player' : '';
    $('.status', card).textContent = !p ? '' : p.connected ? 'connected' : `disconnected · removed in ${p.graceLeft}s`;

    const show = (act, visible) => { $(`[data-act=${act}]`, card).hidden = !visible; };
    show('up', !!p);
    show('down', !!p);
    show('identify', !!p?.connected);
    show('kick', !!p?.connected);
    show('unplug', !!p && !p.connected);
    $('[data-act=up]', card).disabled = i === 0;
    $('[data-act=down]', card).disabled = i === msg.slots.length - 1;

    const s = p?.state ?? { buttons: 0, lx: 0, ly: 0, rx: 0, ry: 0, lt: 0, rt: 0 };
    for (const el of $$('[data-b]', card)) el.classList.toggle('on', !!(s.buttons & BTN[el.dataset.b]));
    $('[data-t=lt] i', card).style.height = `${(s.lt / 255) * 100}%`;
    $('[data-t=rt] i', card).style.height = `${(s.rt / 255) * 100}%`;
    for (const [id, x, y, click] of [['l', s.lx, s.ly, BTN.LS], ['r', s.rx, s.ry, BTN.RS]]) {
      const stick = $(`[data-s=${id}]`, card);
      stick.classList.toggle('on', !!(s.buttons & click));
      // Protocol Y is up-positive; screen Y is down-positive.
      $('i', stick).style.transform = `translate(${axis(x) * 60}%, ${-axis(y) * 60}%)`;
    }
  });
}

// ---- Header controls --------------------------------------------------------

$('#locked').addEventListener('change', (e) => send({ action: 'lock', locked: e.target.checked }));
$('#unplug-stale').addEventListener('click', () => send({ action: 'unplugStale' }));
$('#qr-button').addEventListener('click', () => { $('#qr-overlay').hidden = false; });
$('#qr-overlay').addEventListener('click', () => { $('#qr-overlay').hidden = true; });
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') $('#qr-overlay').hidden = true;
});

start();
