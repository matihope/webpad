# Wire protocol (v2)

This is the authoritative spec for messages between the phone (`web/controller.js`) and the server (`src/protocol.rs`). Any change here must update both implementations and bump `PROTOCOL_VERSION` if old clients would break. The admin page's JSON protocol is described in [architecture.md](architecture.md#admin).

**Changes since v1:**
- the join code in the URL;
- the Name message;
- the Slot and Identify messages;
- Reject reasons 4–6.

## Transport

- The client opens a WebSocket at `ws://<host>:<port>/ws` (`wss://` once HTTPS mode exists).
- If the server was started with `--join-code`, the client adds `?code=<code>` (percent-encoded). A missing or wrong code gets Reject 4 right after Hello.
- Every message is a **binary** frame. Text frames are ignored.
- All integers are **little-endian**.
- The first byte of each frame is the message type. Client types are below `0x80`. Server types are `0x80` and above.

## Client → server

| Type | Name  | Size (bytes) | Layout |
|------|-------|--------------|--------|
| 0x01 | Hello | 18 | `version u8`, `token [16]u8` (all zeroes means no token) |
| 0x02 | State | 13 | `buttons u16`, `lx i16`, `ly i16`, `rx i16`, `ry i16`, `lt u8`, `rt u8` |
| 0x03 | Ping  | 5  | `t u32`, a client timestamp in ms that the server echoes back |
| 0x04 | Name  | 2 + len | `len u8`, followed by `len` bytes of UTF-8 (at most 24) |

- **Hello** must be the first frame on a connection. The server answers with Welcome or Reject. A second Hello on the same connection is a protocol error.
- **State** always carries the full controller state, not a diff. Clients send it:
  - right away when buttons or triggers change;
  - at most once per animation frame for stick motion;
  - right after Welcome and after a Slot message, so a reconnect or a move restores whatever is held.
- **Ping** is sent every 500 ms and doubles as the heartbeat. If the server receives nothing for **1500 ms**, it closes the connection and releases the player's inputs.
- **Name** is the player's display name, shown on the admin page. It is sent after Welcome and whenever it changes. The server trims whitespace. An empty name means "Player".

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

| Type | Name     | Size (bytes) | Layout |
|------|----------|--------------|--------|
| 0x81 | Welcome  | 19 | `version u8`, `slot u8` (0-based), `token [16]u8` |
| 0x82 | Pong     | 5  | `t u32`, echoed from Ping |
| 0x83 | Rumble   | 3  | `low u8`, `high u8` (motor strengths, 0 means stop) |
| 0x84 | Reject   | 2  | `reason u8`, see the table below |
| 0x85 | Slot     | 2  | `slot u8`: the host moved this player to another slot |
| 0x86 | Identify | 1  | none: flash the screen and vibrate |

- **Welcome:** the client stores the token in `sessionStorage` and sends it in the Hello of every later reconnect.
- **Pong:** the client computes round-trip time from it. If no Pong arrives for 2500 ms, the client drops the socket and reconnects.
- **Rumble:** defined, but the server does not send it yet (milestone 4).
- **Slot:** the connection and token stay the same. The client updates its player number and color, then sends a full State.

| Reject reason | Meaning | Client behavior |
|---|---|---|
| 1 | Server full | retry every 3 s |
| 2 | Protocol version mismatch | stop, offer to reload |
| 3 | Backend could not create a pad | retry every 3 s |
| 4 | Missing or wrong join code | stop, ask for the code |
| 5 | Kicked by the host | stop, offer "Join again" |
| 6 | The host locked joining | retry every 3 s (the lock may be lifted) |

The server closes the connection after sending Reject.

## Slot and token rules

These are implemented in `src/session.rs`.

1. If the Hello token matches a slot, the client gets that slot back. If an older connection still holds it, the new connection replaces it and the server closes the old one. This works even when joining is locked.
2. Otherwise, if joining is locked, the server sends Reject 6.
3. Otherwise the client gets the lowest free slot and a fresh token.
4. Otherwise the client gets the slot that has been **disconnected** the longest. It receives a new token, and the virtual pad is reused.
5. Otherwise the server sends Reject with reason "full".

When a connection ends, the slot's inputs reset to neutral right away. The pad stays plugged in for `--grace` seconds (default 30) and is then unplugged. The host can unplug it sooner from the admin page.

**Host reordering:** each virtual pad stays at its slot index; the host moves players between pads. Swapping two players releases both pads and sends each phone a Slot message. Moving a player into an empty slot plugs in a pad there and unplugs the old one.
