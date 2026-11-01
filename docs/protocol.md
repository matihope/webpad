# Wire protocol (v1)

This is the authoritative spec for messages between the phone (`web/controller.js`) and the server (`src/protocol.rs`). Any change here must update both implementations and bump `PROTOCOL_VERSION` if old clients would break.

## Transport

- The client opens a WebSocket at `ws://<host>:<port>/ws` (`wss://` once HTTPS mode exists).
- Every message is a **binary** frame. Text frames are ignored.
- All integers are **little-endian**.
- The first byte of each frame is the message type. Client types are below `0x80`. Server types are `0x80` and above.

## Client → server

| Type | Name  | Size (bytes) | Layout |
|------|-------|--------------|--------|
| 0x01 | Hello | 18 | `version u8`, `token [16]u8` (all zeroes means no token) |
| 0x02 | State | 13 | `buttons u16`, `lx i16`, `ly i16`, `rx i16`, `ry i16`, `lt u8`, `rt u8` |
| 0x03 | Ping  | 5  | `t u32`, a client timestamp in ms that the server echoes back |

- **Hello** must be the first frame on a connection. The server answers with Welcome or Reject. A second Hello on the same connection is a protocol error.
- **State** always carries the full controller state, not a diff. Clients send it:
  - right away when buttons or triggers change;
  - at most once per animation frame for stick motion;
  - right after Welcome, so a reconnect restores whatever is held.
- **Ping** is sent every 500 ms and doubles as the heartbeat. If the server receives nothing for **1500 ms**, it closes the connection and releases the player's inputs.

### Buttons bitmask

The values match XInput `wButtons`.

| Bit    | Button | Bit    | Button |
|--------|--------|--------|--------|
| 0x0001 | D-pad up    | 0x0100 | LB |
| 0x0002 | D-pad down  | 0x0200 | RB |
| 0x0004 | D-pad left  | 0x0400 | Guide |
| 0x0008 | D-pad right | 0x0800 | *(unused)* |
| 0x0010 | Start | 0x1000 | A |
| 0x0020 | Back  | 0x2000 | B |
| 0x0040 | LS (L3) | 0x4000 | X |
| 0x0080 | RS (R3) | 0x8000 | Y |

### Axes

- Stick axes range from −32767 to 32767.
- **Positive X is right. Positive Y is up**, as in XInput. Backends whose OS uses positive-down, such as Linux evdev, must flip Y.
- Triggers (`lt`, `rt`) range from 0 to 255. The current UI only sends 0 or 255.

## Server → client

| Type | Name    | Size (bytes) | Layout |
|------|---------|--------------|--------|
| 0x81 | Welcome | 19 | `version u8`, `slot u8` (0-based), `token [16]u8` |
| 0x82 | Pong    | 5  | `t u32`, echoed from Ping |
| 0x83 | Rumble  | 3  | `low u8`, `high u8` (motor strengths, 0 means stop) |
| 0x84 | Reject  | 2  | `reason u8`: 1 = full, 2 = version mismatch, 3 = backend error |

- **Welcome:** the client stores the token in `sessionStorage` and sends it in the Hello of every later reconnect.
- **Pong:** the client computes round-trip time from it. If no Pong arrives for 2500 ms, the client drops the socket and reconnects.
- **Rumble:** defined, but the server does not send it yet (milestone 4).
- **Reject:** the server closes the connection after sending it. Clients retry every 3 s for reasons 1 and 3, and stop retrying for reason 2.

## Slot and token rules

These are implemented in `src/session.rs`.

1. If the Hello token matches a slot, the client gets that slot back. If an older connection still holds it, the new connection replaces it, and frames from the old connection are ignored from then on.
2. Otherwise the client gets the lowest free slot and a fresh token.
3. Otherwise the client gets the slot that has been **disconnected** the longest. It receives a new token, and the virtual pad is reused.
4. Otherwise the server sends Reject with reason "full".

When a connection ends, the slot's inputs reset to neutral right away. The pad stays plugged in for `--grace` seconds (default 30) and is then unplugged.
