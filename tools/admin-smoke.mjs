// Admin smoke test: login, admin WebSocket snapshots and every admin action, end to end.
// Usage:
//   cargo run -- --backend log --port 8099 --max-players 3 --admin-password test --join-code 1234 &
//   node tools/admin-smoke.mjs [http://127.0.0.1:8099] [password] [join-code]
const BASE = process.argv[2] ?? 'http://127.0.0.1:8099';
const PASSWORD = process.argv[3] ?? 'test';
const CODE = process.argv[4] ?? '1234';
const WS_BASE = BASE.replace(/^http/, 'ws');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failed = true; };

function phone(token = new Uint8Array(16), name = '') {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`${WS_BASE}/ws?code=${CODE}`);
    ws.binaryType = 'arraybuffer';
    const inbox = [];
    const p = { ws, inbox, last: () => inbox.at(-1), ping: null };
    ws.onmessage = (e) => {
      const m = new Uint8Array(e.data);
      inbox.push(m);
      if (m[0] === 0x81 && name) ws.send(new Uint8Array([4, name.length, ...new TextEncoder().encode(name)]));
    };
    ws.onopen = () => {
      const h = new Uint8Array(18); h[0] = 1; h[1] = 2; h.set(token, 2); ws.send(h);
      // Keep the heartbeat alive.
      p.ping = setInterval(() => ws.readyState === 1 && ws.send(new Uint8Array([3, 0, 0, 0, 0])), 400);
      resolve(p);
    };
    ws.onclose = () => clearInterval(p.ping);
    ws.onerror = reject;
  });
}
const lastOf = (p, type) => [...p.inbox].reverse().find((m) => m[0] === type);

// --- Auth ---
const info0 = await (await fetch(`${BASE}/api/info`)).json();
check(info0.adminPasswordRequired && info0.codeRequired && !info0.admin, `/api/info ${JSON.stringify(info0)}`);
check((await fetch(`${BASE}/admin/qr.svg`)).status === 401, 'QR needs login');
const t0 = Date.now();
const bad = await fetch(`${BASE}/api/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: 'wrong' }) });
check(bad.status === 401 && Date.now() - t0 >= 900, 'wrong password → 401 after ~1 s');
const good = await fetch(`${BASE}/api/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: PASSWORD }) });
const cookie = good.headers.get('set-cookie')?.split(';')[0];
check(good.status === 204 && cookie?.startsWith('webpad_admin='), 'right password sets cookie');
const qr = await fetch(`${BASE}/admin/qr.svg`, { headers: { cookie } });
check(qr.status === 200 && (await qr.text()).includes('<svg'), 'QR served after login');

// Node's WebSocket accepts headers via the second argument (undici extension).
const anon = new WebSocket(`${WS_BASE}/admin/ws`);
const anonResult = await new Promise((r) => { anon.onopen = () => r('open'); anon.onerror = () => r('error'); });
check(anonResult === 'error', 'admin WS refused without cookie');

const admin = new WebSocket(`${WS_BASE}/admin/ws`, { headers: { cookie } });
let snap = null;
admin.onmessage = (e) => { const m = JSON.parse(e.data); if (m.type === 'state') snap = m; else console.log('admin error:', m.message); };
await new Promise((r) => (admin.onopen = r));
const act = (a) => admin.send(JSON.stringify(a));
await sleep(200);
check(snap?.slots.length === 3 && snap.joinUrl.includes('/pad?code=1234'), 'admin snapshot arrives');

// --- Players ---
const p1 = await phone(undefined, 'Ala');
const p2 = await phone(undefined, 'Bob');
await sleep(300);
check(snap.slots[0]?.name === 'Ala' && snap.slots[1]?.name === 'Bob' && snap.slots[0].connected, 'names and live status in snapshot');

p1.ws.send(new Uint8Array([2, 0x00, 0x10, 0, 0x40, 0, 0, 0, 0, 0, 0, 0, 0])); // A pressed, lx = 0x4000
await sleep(200);
check(snap.slots[0].state.buttons === 0x1000 && snap.slots[0].state.lx === 0x4000, 'live input preview data');

act({ action: 'swap', a: 0, b: 1 });
await sleep(200);
check(lastOf(p1, 0x85)?.[1] === 1 && lastOf(p2, 0x85)?.[1] === 0, 'swap: both phones told their new slot');
check(snap.slots[0].name === 'Bob' && snap.slots[1].name === 'Ala', 'swap reflected in snapshot');

act({ action: 'swap', a: 1, b: 2 }); // Ala into the empty P3
await sleep(200);
check(lastOf(p1, 0x85)?.[1] === 2 && snap.slots[1] === null && snap.slots[2]?.name === 'Ala', 'move into empty slot');

act({ action: 'identify', slot: 2 });
await sleep(150);
check(p1.last()?.[0] === 0x86, 'identify reaches the phone');

act({ action: 'lock', locked: true });
await sleep(150);
const p3 = await phone();
await sleep(150);
check(p3.inbox[0]?.[0] === 0x84 && p3.inbox[0]?.[1] === 6 && snap.locked, 'locked: new phone gets Reject 6');
const bobToken = p2.inbox[0].slice(3, 19);
p2.ws.close();
await sleep(200);
const p2b = await phone(bobToken);
await sleep(150);
check(p2b.inbox[0]?.[0] === 0x81 && p2b.inbox[0]?.[2] === 0, 'locked: token rejoin still allowed');
act({ action: 'lock', locked: false });

act({ action: 'kick', slot: 0 });
await sleep(200);
check(lastOf(p2b, 0x84)?.[1] === 5 && snap.slots[0] === null, 'kick: Reject 5 and slot freed');

p1.ws.close();
await sleep(300);
check(snap.slots[2] && !snap.slots[2].connected && snap.slots[2].graceLeft > 0, 'disconnected player shows grace countdown');
act({ action: 'unplugStale' });
await sleep(200);
check(snap.slots.every((s) => s === null), 'remove all disconnected');

act({ action: 'swap', a: 0, b: 9 });
await sleep(150);
admin.close();
process.exit(failed ? 1 : 0);
