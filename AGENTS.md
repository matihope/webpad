# AGENTS.md

Instructions for coding agents working in this repo. The user-facing overview is in [README.md](README.md).

## What this is
webpad is a Rust server that serves a phone controller web page and turns each connected phone into a virtual gamepad on the host. It uses ViGEm on Windows and uinput on Linux, with a keyboard fallback on macOS. Read [docs/architecture.md](docs/architecture.md) first.

## Start-of-session checklist
1. Read the **latest entry in `docs/log/`** and [docs/roadmap.md](docs/roadmap.md) to see where things stand.
2. Before touching the wire format, read [docs/protocol.md](docs/protocol.md).

## End-of-session checklist
1. Add or extend `docs/log/YYYY-MM-DD.md` (today's date) with what changed, how it was verified (commands and results), open questions and next steps. Be honest about what wasn't tested.
2. Tick boxes in `docs/roadmap.md`.
3. If you made a design decision with trade-offs, add `docs/decisions/NNNN-title.md` (Context / Decision / Consequences).
4. Update `docs/platforms.md` when you learn something OS- or browser-specific.

## Repo map
```
src/main.rs          CLI, startup, QR code
src/server.rs        axum: static files + /ws handler
src/protocol.rs      binary frames (spec: docs/protocol.md)
src/session.rs       player slots, tokens, reconnect, release, reap
src/backend/         Backend/Pad traits + one file per backend
web/                 phone UI (plain HTML/CSS/JS, no build step, embedded into the binary)
tools/               smoke-test scripts (Node + headless Chrome)
docs/                architecture, protocol, platforms, roadmap, decisions/, log/
```

## Commands
```sh
cargo build
cargo test                                   # protocol + session unit tests
cargo clippy --all-targets                   # keep it warning-free
cargo run -- --backend log --port 8099       # run locally; the log backend prints input
node tools/fake-phones.mjs                   # protocol smoke test (server must run with --max-players 2)
node tools/ui-smoke.mjs <outdir>             # touch/UI smoke test via headless Chrome (see file header)
```
In debug builds `rust-embed` serves `web/` from disk, so a browser reload picks up UI changes without rebuilding.

## Test machines
- **This machine:** macOS. Use it for development and the `log` backend.
- **`ssh kolpa`:** headless Arch Linux with cargo. Use it for the uinput backend and check with `evtest`. `/dev/uinput` is root-only there, so ask the user before changing udev rules or groups.
- **`ssh kolpa2`:** Windows (cmd shell). Use it for the ViGEm backend. It has no Rust and no ViGEmBus yet, so ask before installing.

Copy code to the test machines with `rsync`/`scp` (excluding `target/`), or have them `git pull` once a remote exists.

## Conventions
- **Protocol changes:** update `docs/protocol.md`, `src/protocol.rs` and `web/controller.js` together, and bump `PROTOCOL_VERSION` if the change breaks old clients.
- **Backends:** follow the contract in `docs/architecture.md`.
  - `update` gets the full state on every frame and runs under the sessions mutex, so it must not block.
  - Flip Y wherever the OS uses positive-down.
  - Put OS-only code behind `#[cfg(target_os = "…")]` on both the module and its `BackendKind` variant.
- **Stuck inputs:** every disconnect path must release inputs. `Sessions::leave` does this, so don't bypass it.
- **Rust style:** `anyhow` for errors, `tracing` for logs ("P{n} …" for player-scoped messages, 1-based). Comments explain *why*, not what.
- **Web:** no frameworks and no build step. Touch handling uses Pointer Events. All buttons go through `updateButtons()`, which hit-tests every active pointer so thumbs can slide between buttons.
- **Dependencies:** add them with `cargo add`. Keep the dependency list short.
