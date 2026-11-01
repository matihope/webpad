// Scripted fake phone: joins, sends a few known inputs (A + stick + LT, D-pad, stick down,
// neutral) with pings, then leaves. Pair it with tools/read-pad.py to check a backend.
//   node tools/drive-pad.mjs [ws://host:port/ws]
const ws = new WebSocket(process.argv[2] ?? 'ws://127.0.0.1:8099/ws');
ws.binaryType = 'arraybuffer';
const state = (buttons, lx, ly, lt) => { const b = new DataView(new ArrayBuffer(13)); b.setUint8(0, 2); b.setUint16(1, buttons, true);
  b.setInt16(3, lx, true); b.setInt16(5, ly, true); b.setUint8(11, lt); return b.buffer; };
const steps = [[0x1000, 32767, 32767, 255, 'A + left stick up-right + LT'], [0x0005, 0, 0, 0, 'D-pad up+left'], [0, 0, -32767, 0, 'left stick down'], [0, 0, 0, 0, 'neutral']];
ws.onopen = () => { const h = new Uint8Array(18); h[0] = 1; h[1] = 2; ws.send(h); };
ws.onmessage = async (e) => {
  const b = new Uint8Array(e.data);
  if (b[0] !== 0x81) { if (b[0] === 0x84) { console.log('rejected', b[1]); process.exit(1); } return; }
  console.log('joined as P' + (b[2] + 1));
  const ping = setInterval(() => { const p = new DataView(new ArrayBuffer(5)); p.setUint8(0, 3); ws.send(p.buffer); }, 300);
  for (const [buttons, lx, ly, lt, label] of steps) { await new Promise(r => setTimeout(r, 1500)); console.log('send:', label); ws.send(state(buttons, lx, ly, lt)); }
  await new Promise(r => setTimeout(r, 800)); clearInterval(ping); ws.close(); setTimeout(() => process.exit(0), 300);
};
