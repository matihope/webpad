# Architecture

```
phone browser ──WebSocket (binary)──▶ server.rs ──▶ Sessions (session.rs) ──▶ Box<dyn Pad>
  web/ (embedded in the binary)        axum           slot · token · conn       from a Backend
```

## Components

| File | Responsibility |
|------|----------------|
| `src/main.rs` | CLI (`clap`), picks the backend, binds `0.0.0.0:<port>` with `TCP_NODELAY`, prints the LAN URL and a QR code, runs the reaper tick every 1 s. |
| `src/server.rs` | Serves `web/` (embedded with `rust-embed`) at `/` and handles each socket at `/ws`: Hello, join, then a loop that reads frames with a 1.5 s heartbeat timeout, then leave. |
| `src/protocol.rs` | Frame encode/decode, button bit constants, `GamepadState`. The spec is [protocol.md](protocol.md). |
| `src/session.rs` | `Sessions`: one `Mutex<Inner>` holding `max_players` slots. Each slot owns a `Pad`, a reconnect token and the id of the connection driving it. Exposes `join`, `update`, `leave` and `reap`. |
| `src/backend/mod.rs` | The `Backend` and `Pad` traits, `BackendKind` (CLI enum) and `create()`. |
| `src/backend/*.rs` | One file per backend. Only `log` exists so far; see [roadmap.md](roadmap.md). |
| `web/controller.js` | Touch handling, state encoding, connection and reconnect, fullscreen and wake lock. Plain JS with no build step. |

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

- `connect` is called once per slot, the first time a player takes it. The pad is reused across reconnects and takeovers, so games keep seeing the same controller.
- `update` receives the **full** state on every frame, up to about 120 per second per player, while holding the sessions mutex. It must be fast and non-blocking. A device write is fine; sleeping is not.
- Axis conventions are the protocol's: positive Y is up. Each backend converts to its OS convention.
- Errors from `update` are logged as warnings and the connection stays up. Errors from `connect` reject the player with reason 3.
- For OS-specific backends, use `#[cfg(target_os = "…")]` on both the module and its `BackendKind` variant so other platforms still compile.

## Connection lifecycle

1. The client opens the socket and sends Hello.
2. `Sessions::join` returns a `Lease { slot, token, conn }`. The server sends Welcome.
3. The client sends State and Ping frames. Each State calls `Sessions::update`, which only takes effect if `lease.conn` still owns the slot.
4. On close, a heartbeat timeout or an error, `Sessions::leave` sets neutral state and marks the slot disconnected.
5. The reaper unplugs slots that have been disconnected longer than `--grace`.

## Design choices

See [decisions/](decisions/).
