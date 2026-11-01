// Screenshots the landing and admin pages via headless Chrome (CDP), with fake phones connected.
// Usage:
//   cargo run -- --backend log --port 8099 --admin-password test --join-code 1234 &
//   "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --remote-debugging-port=9333 --user-data-dir=/tmp/webpad-chrome &
//   node tools/pages-smoke.mjs <screenshot-dir> [http://127.0.0.1:8099]
import { writeFileSync } from 'node:fs';
const OUT = process.argv[2];
const BASE = process.argv[3] ?? 'http://127.0.0.1:8099';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const target = await (await fetch('http://127.0.0.1:9333/json/new?about:blank', { method: 'PUT' })).json();
const cdpWs = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => (cdpWs.onopen = r));
let id = 0; const pending = new Map();
cdpWs.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
const cdp = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); cdpWs.send(JSON.stringify({ id: i, method, params })); });
const evalJs = async (expr) => (await cdp('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true })).result.result?.value;
const shot = async (name) => writeFileSync(`${OUT}/${name}.png`, Buffer.from((await cdp('Page.captureScreenshot')).result.data, 'base64'));
const viewport = (width, height, mobile) => cdp('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: mobile ? 2 : 1, mobile });
const go = async (path) => { await cdp('Page.navigate', { url: BASE + path }); await sleep(800); };

// Two fake phones so the admin page has something to show: one holding A + left stick.
function phone(name, buttons, lx, ly) {
  const ws = new WebSocket(`${BASE.replace(/^http/, 'ws')}/ws?code=1234`);
  ws.binaryType = 'arraybuffer';
  ws.onopen = () => { const h = new Uint8Array(18); h[0] = 1; h[1] = 2; ws.send(h); };
  ws.onmessage = (e) => {
    if (new Uint8Array(e.data)[0] !== 0x81) return;
    ws.send(new Uint8Array([4, name.length, ...new TextEncoder().encode(name)]));
    const dv = new DataView(new ArrayBuffer(13));
    dv.setUint8(0, 2); dv.setUint16(1, buttons, true); dv.setInt16(3, lx, true); dv.setInt16(5, ly, true); dv.setUint8(11, 255);
    ws.send(dv.buffer);
  };
  setInterval(() => ws.readyState === 1 && ws.send(new Uint8Array([3, 0, 0, 0, 0])), 400);
  return ws;
}

await viewport(390, 844, true);
await go('/');
await shot('landing');

await viewport(1280, 800, false);
await go('/admin');
await shot('admin-login');
await evalJs(`(async () => { document.querySelector('#password').value = 'test'; document.querySelector('#login').requestSubmit(); })()`);
await sleep(400);
phone('Ala', 0x1000 | 0x0100 | 0x0001, 20000, 20000);
phone('Bob', 0, 0, 0);
await sleep(800);
await shot('admin');
await viewport(390, 844, true);
await sleep(400);
await shot('admin-phone');
process.exit(0);
