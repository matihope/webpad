# 0002: WebSocket with a small binary protocol

**Status:** accepted, 2026-11-01

## Context
Phones have to stream input with low latency and no app install. The browser options are WebSocket (TCP) and the WebRTC DataChannel (which can be unreliable and unordered).

## Decision
We use one WebSocket per phone. It carries little-endian binary frames, and each State frame holds the **full** controller state (13 bytes). The server sets `TCP_NODELAY`. See [protocol.md](../protocol.md).

## Consequences
- The implementation is simple everywhere. Sending the full state means a lost or reordered frame can't leave a button stuck. TCP already orders frames, and each frame is self-contained anyway.
- On a lossy network, TCP head-of-line blocking can add latency spikes. If that shows up on real Wi-Fi, a WebRTC DataChannel transport can be added behind the same frame format.
- Ping/Pong in the app protocol is the heartbeat in both directions, so dead connections are noticed within about 1.5–2.5 s.
