// Protocol smoke test: simulates phones against a running server.
// Usage:
//   cargo run -- --backend log --port 8099 --max-players 2 --join-code 1234 &
//   node tools/fake-phones.mjs [ws://127.0.0.1:8099/ws] [join-code]
// Check the server log for: P1 [A Up], P1 [X] (not [B]), P2 disconnected after the silence.
const URL = process.argv[2] ?? 'ws://127.0.0.1:8099/ws';
const CODE = process.argv[3] ?? '1234';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failed = true; };

function open(token = new Uint8Array(16), code = CODE) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(code === null ? URL : `${URL}?code=${encodeURIComponent(code)}`);
    ws.binaryType = 'arraybuffer';
    const inbox = [];
    ws.onmessage = (e) => inbox.push(new Uint8Array(e.data));
    ws.onopen = () => { const h = new Uint8Array(18); h[0] = 1; h[1] = 2; h.set(token, 2); ws.send(h); resolve({ ws, inbox }); };
    ws.onerror = reject;
  });
}
function state(buttons, lx = 0) { const dv = new DataView(new ArrayBuffer(13)); dv.setUint8(0, 2); dv.setUint16(1, buttons, true); dv.setInt16(3, lx, true); return dv.buffer; }

const wrong = await open(undefined, 'nope'); await sleep(100);
check(wrong.inbox[0]?.[0] === 0x84 && wrong.inbox[0]?.[1] === 4, 'wrong join code → Reject 4');
const none = await open(undefined, null); await sleep(100);
check(none.inbox[0]?.[0] === 0x84 && none.inbox[0]?.[1] === 4, 'missing join code → Reject 4');

const a = await open(); await sleep(100);
check(a.inbox[0]?.[0] === 0x81 && a.inbox[0]?.[2] === 0, 'A welcomed as P1');
const b = await open(); await sleep(100);
check(b.inbox[0]?.[0] === 0x81 && b.inbox[0]?.[2] === 1, 'B welcomed as P2');
const c = await open(); await sleep(100);
check(c.inbox[0]?.[0] === 0x84 && c.inbox[0]?.[1] === 1, 'C rejected: full');

a.ws.send(state(0x1000 | 0x0001, 16000)); await sleep(50);
a.ws.send(new Uint8Array([3, 42, 0, 0, 0])); await sleep(100);
check(a.inbox.at(-1)?.[0] === 0x82 && a.inbox.at(-1)?.[1] === 42, 'pong echoes ping');
a.ws.send(new Uint8Array([4, 3, ...new TextEncoder().encode('Ala')])); await sleep(50);

// Reconnect A with its token while the old socket is still open → same slot, old socket closed.
const tokenA = a.inbox[0].slice(3, 19);
const a2 = await open(tokenA); await sleep(150);
check(a2.inbox[0]?.[2] === 0, 'A2 reclaims P1 with its token');
check(a.ws.readyState >= 2, 'old A socket closed by server');
a2.ws.send(state(0x4000)); await sleep(50);

// B goes silent (no pings) → heartbeat timeout releases it.
await sleep(1800);
check(b.ws.readyState === 3, 'silent B dropped after heartbeat timeout');
a2.ws.close(); a.ws.close(); await sleep(200);
process.exit(failed ? 1 : 0);
