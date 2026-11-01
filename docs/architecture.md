# Architecture

```
phone browser ──WS /ws (binary)──▶ server.rs ──▶ Sessions (session.rs) ──▶ Box<dyn Pad>
admin browser ──WS /admin/ws (JSON)─▶ admin.rs ──┘  slots: pad + owner      from a Backend
  web/ (embedded in the binary)      axum          ▲ watch<version> → admin snapshots
```

## Components

| File | Responsibility |
|------|----------------|
| `src/main.rs` | CLI (`clap`), picks the backend, binds `0.0.0.0:<port>` with `TCP_NODELAY`, builds the join URL, prints the QR code and the admin URL, and runs the reaper tick every 1 s. |
| `src/server.rs` | `AppState` (sessions, auth, join code, join URL), the router, embedded static files, and the phone socket. The phone loop `select!`s over incoming frames (1.5 s heartbeat timeout) and the session's control channel. |
| `src/admin.rs` | `AdminAuth` (optional password, in-memory session tokens, cookie check), login and logout, `/api/info`, the admin WebSocket, and the join QR as SVG. |
| `src/protocol.rs` | Frame encode/decode, button bit constants, `GamepadState`. The spec is [protocol.md](protocol.md). |
| `src/session.rs` | `Sessions`: one `Mutex<Inner>` holding `max_players` slots. Each slot is a **pad** plus an **owner** (token, name, last state, connection). Exposes `join`, `update`, `set_name`, `leave` and `reap`, plus the admin operations `swap`, `kick`, `unplug`, `identify`, `set_locked` and `snapshot`. |
| `src/backend/mod.rs` | The `Backend` and `Pad` traits, `BackendKind` (CLI enum) and `create()`. |
| `src/backend/*.rs` | One file per backend: `log` (any OS), `vigem` (Windows), `uinput` (Linux). See [roadmap.md](roadmap.md). |
| `web/index.html` | The landing page: name, join code if one is required, **Join** or **Manage**. |
| `web/pad.html`, `controller.js`, `pad.css` | The controller: touch handling, state encoding, connection and reconnect, fullscreen and wake lock, the name, and identify flashes. |
| `web/admin.html`, `admin.js`, `pages.css` | The admin page: login, join QR, lock toggle, and one card per slot with a live preview and actions. |
| `web/common.js`, `base.css` | Shared button bits, player colors, a safe `localStorage` wrapper, and color tokens and form styles. |

All pages are plain HTML, CSS and JS with no build step.

## HTTP routes

| Route | Auth | Purpose |
|---|---|---|
| `GET /`, `/pad`, `/admin`, `/<asset>` | none | Pages and static files |
| `GET /ws?code=` | join code (if set) | Phone socket, binary protocol |
| `GET /api/info` | none | `{codeRequired, adminPasswordRequired, admin}` |
| `POST /api/login` / `/api/logout` | none / cookie | Sets or clears the `webpad_admin` cookie (HttpOnly, SameSite=Strict) |
| `GET /admin/ws` | admin | Admin JSON socket. Also requires the Origin header to match Host. |
| `GET /admin/qr.svg` | admin | QR code of the join URL (it includes the code) |

"Admin" means a valid session cookie, or anyone at all when no `--admin-password` is set. See [ADR 0004](decisions/0004-auth-model.md).

## Admin

The admin socket carries JSON text frames.

**Server → admin**
- `{type: "state", slots: [null | {name, connected, graceLeft, state: {buttons, lx, ly, rx, ry, lt, rt}}], locked, joinUrl, joinCode, maxPlayers, backend}`. It is sent when the session version changes, at most every 66 ms, and at least once a second so grace countdowns tick.
- `{type: "error", message}` when an action fails.

**Admin → server**: `{action: …}`, one of:
- `swap {a, b}`
- `kick {slot}`
- `unplug {slot}` (disconnected slots only)
- `unplugStale`
- `identify {slot}`
- `lock {locked}`

## Backend contract

```rust
trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    fn connect(&self, slot: u8) -> anyhow::Result<Box<dyn Pad>>; // plug in a device
}
trait Pad: Send {
    fn update(&mut self, state: &GamepadState) -> anyhow::Result<()>;
}
// Dropping a Pad unplugs its device.
```

- **Pads never move between slots.** `connect` is called when a slot first gets a player, or when the host moves a player into an empty slot. The pad is reused across reconnects, takeovers and owner swaps, so games keep seeing the same controller at the same index.
- `update` receives the **full** state on every frame, up to about 120 per second per player, while holding the sessions mutex. It must be fast and non-blocking. A device write is fine; sleeping is not.
- Axis conventions are the protocol's: positive Y is up. Each backend converts to its OS convention.
- Errors from `update` are logged as warnings and the connection stays up. Errors from `connect` reject the player with reason 3, or fail the admin's swap with an error message.
- For OS-specific backends, use `#[cfg(target_os = "…")]` on both the module and its `BackendKind` variant so other platforms still compile.

## Connection lifecycle

1. The client opens `/ws?code=…` and sends Hello. The server checks the version and the join code.
2. `Sessions::join` returns a `Joined { lease, slot, control }`, and the server sends Welcome.
   - The `Lease { token, conn }` is resolved to a slot by connection id on every call, so it stays valid after the host reorders players.
   - `control` is an mpsc receiver that session-layer events use to reach this connection: `Send(msg)` (Slot, Identify) or `Close(msg)` (a kick).
   - The channel closes when a newer connection takes over the slot, which ends the old connection task.
3. The client sends State, Ping and Name frames. State and Name only take effect while the lease still owns a slot.
4. On close, a heartbeat timeout, an error or a kick, `Sessions::leave` sets neutral state and marks the slot disconnected. It does nothing if the slot is already gone or taken over.
5. The reaper unplugs slots that have been disconnected longer than `--grace`. The admin can unplug them sooner.

Every session change bumps a `tokio::sync::watch` version, including input updates for the live preview. Admin sockets wait on it and send throttled snapshots.

## Design choices

See [decisions/](decisions/).
