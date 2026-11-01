// UI smoke test: drives headless Chrome over CDP with a phone-sized viewport and multi-touch.
// Usage:
//   cargo run -- --backend log --port 8099 &
//   "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --remote-debugging-port=9333 --user-data-dir=/tmp/webpad-chrome &
//   node tools/ui-smoke.mjs <screenshot-dir> [http://127.0.0.1:8099/]
// Then check the server log (A + stick, A→B slide, Down Left, LT+RT) and the screenshots.
import { writeFileSync } from 'node:fs';
const SP = process.argv[2];
const PAGE = process.argv[3] ?? 'http://127.0.0.1:8099/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const target = await (await fetch('http://127.0.0.1:9333/json/new?about:blank', { method: 'PUT' })).json();
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0; const pending = new Map();
ws.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } };
const cdp = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const evalJs = async (expr) => (await cdp('Runtime.evaluate', { expression: expr, returnByValue: true })).result.result.value;
const shot = async (name) => writeFileSync(`${SP}/${name}.png`, Buffer.from((await cdp('Page.captureScreenshot')).result.data, 'base64'));
const touch = (type, points) => cdp('Input.dispatchTouchEvent', { type, touchPoints: points });
const center = async (sel) => evalJs(`(() => { const r = document.querySelector('${sel}').getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);

await cdp('Emulation.setDeviceMetricsOverride', { width: 932, height: 430, deviceScaleFactor: 2, mobile: true });
await cdp('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
await cdp('Page.navigate', { url: PAGE });
await sleep(1200);
await shot('1-start');
await touch('touchStart', [{ x: 400, y: 200, id: 0 }]); await touch('touchEnd', []);
await sleep(400);
console.log('status:', await evalJs(`document.querySelector('#status').innerText.replace(/\\n/g, ' | ')`));
await shot('2-idle');

const a = await center('[data-btn=A]');
const ls = await center('[data-stick=l]');
const dp = await center('.dpad');
// Hold A, drag the left stick up-right, then slide A thumb onto B.
await touch('touchStart', [{ ...a, id: 1 }]);
await touch('touchStart', [{ ...a, id: 1 }, { ...ls, id: 2 }]);
for (let i = 1; i <= 6; i++) { await touch('touchMove', [{ ...a, id: 1 }, { x: ls.x + i * 12, y: ls.y - i * 12, id: 2 }]); await sleep(20); }
await sleep(150);
await shot('3-held');
const b = await center('[data-btn=B]');
await touch('touchMove', [{ ...b, id: 1 }, { x: ls.x + 72, y: ls.y - 72, id: 2 }]);
await sleep(150);
await touch('touchEnd', []);
await sleep(150);
// D-pad diagonal (down-left)
const s = 0.3 * (await evalJs(`document.querySelector('.dpad').offsetWidth`));
await touch('touchStart', [{ x: dp.x - s, y: dp.y + s, id: 3 }]); await sleep(150);
await shot('3b-dpad');
await touch('touchEnd', []); await sleep(150);
// Two-finger LT + RT
await touch('touchStart', [{ ...(await center('[data-btn=LT]')), id: 4 }, { ...(await center('[data-btn=RT]')), id: 5 }]); await sleep(150);
await touch('touchEnd', []); await sleep(300);
console.log('rtt:', await evalJs(`document.querySelector('#rtt').textContent`));

await cdp('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
await sleep(300); await shot('4-portrait');
ws.close();
