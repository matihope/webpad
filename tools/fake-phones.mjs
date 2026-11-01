// Protocol smoke test: simulates phones against a running server.
// Usage: cargo run -- --backend log --port 8099 --max-players 2 &  node tools/fake-phones.mjs [ws://127.0.0.1:8099/ws]
// Check the server log for: P1 [A Up], P1 [X] (not [B]), P2 disconnected after the silence.
const URL = process.argv[2] ?? 'ws://127.0.0.1:8099/ws';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function open(token = new Uint8Array(16)) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(URL); ws.binaryType = 'arraybuffer';
    const inbox = [];
    ws.onmessage = (e) => inbox.push(new Uint8Array(e.data));
    ws.onopen = () => { const h = new Uint8Array(18); h[0] = 1; h[1] = 1; h.set(token, 2); ws.send(h); resolve({ ws, inbox }); };
    ws.onerror = reject;
  });
}
function state(buttons, lx = 0) { const dv = new DataView(new ArrayBuffer(13)); dv.setUint8(0, 2); dv.setUint16(1, buttons, true); dv.setInt16(3, lx, true); return dv.buffer; }
const a = await open(); await sleep(100);
console.log('A welcome', [...a.inbox[0].slice(0, 3)]);
const b = await open(); await sleep(100);
console.log('B welcome', [...b.inbox[0].slice(0, 3)]);
const c = await open(); await sleep(100);
console.log('C (3rd, full) got', [...(c.inbox[0] ?? [])], 'state', c.ws.readyState);
a.ws.send(state(0x1000 | 0x0001, 16000)); await sleep(50);
a.ws.send(new Uint8Array([3, 42, 0, 0, 0])); await sleep(100);
console.log('A pong', [...a.inbox.at(-1)]);
// reconnect A with its token while old socket still open -> same slot
const tokenA = a.inbox[0].slice(3, 19);
const a2 = await open(tokenA); await sleep(100);
console.log('A2 welcome slot', a2.inbox[0][2]);
a.ws.send(state(0x2000)); await sleep(50); // stale: should be ignored
a2.ws.send(state(0x4000)); await sleep(50);
// B goes silent (no pings) -> heartbeat timeout should release it
await sleep(1800);
console.log('B readyState after silence', b.ws.readyState);
a2.ws.close(); a.ws.close(); await sleep(200);
process.exit(0);
