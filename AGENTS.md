# AGENTS.md

Instructions for coding agents working in this repo. The user-facing overview is in [README.md](README.md).

## What this is
webpad is a Rust server that serves a phone controller web page and turns each connected phone into a virtual gamepad on the host. It uses ViGEm on Windows and uinput on Linux (planned). macOS can't create virtual gamepads ([ADR 0003](docs/decisions/0003-macos-keyboard-fallback.md)). Read [docs/architecture.md](docs/architecture.md) first.

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
src/server.rs        AppState, router, static files, phone /ws handler
src/admin.rs         admin auth (password/cookie), /api/*, admin JSON socket, join QR
src/protocol.rs      binary frames (spec: docs/protocol.md)
src/session.rs       player slots (pad + owner), tokens, reconnect, release, reap, admin ops
src/backend/         Backend/Pad traits + one file per backend
web/                 pages, embedded into the binary, plain HTML/CSS/JS with no build step:
                     index.html (landing), pad.html + controller.js (controller), admin.html + admin.js
tools/               smoke-test scripts (Node + headless Chrome), windows-setup.bat (ViGEmBus + Build Tools)
docs/                architecture, protocol, platforms, roadmap, decisions/, log/
```

## Commands
```sh
cargo build
cargo test                                   # protocol, session and admin unit tests
cargo clippy --all-targets                   # keep it warning-free
cargo run -- --backend log --port 8099       # run locally; the log backend prints input
node tools/fake-phones.mjs                   # protocol smoke test (server: --max-players 2 --join-code 1234)
node tools/admin-smoke.mjs                   # login + every admin action (server: --max-players 3 --admin-password test --join-code 1234)
node tools/ui-smoke.mjs <outdir>             # controller touch test via headless Chrome (see file header)
node tools/pages-smoke.mjs <outdir>          # screenshots of landing + admin pages (see file header)
```
In debug builds `rust-embed` serves `web/` from disk, so a browser reload picks up UI changes without rebuilding.

## Test machines
- **This machine:** macOS. Use it for development and the `log` backend.
- **`ssh kolpa`:** headless Arch Linux with cargo. Use it for the uinput backend and check with `evtest`. `/dev/uinput` is root-only there, so ask the user before changing udev rules or groups.
- **`ssh kolpa2`:** Windows (cmd shell; dual-boot, so it may answer from Linux instead). Use it for the ViGEm backend. ViGEmBus is installed. The SSH session isn't elevated, so admin installs go through a script the user double-clicks (see `tools/windows-setup.bat`).

The repo is on GitHub (`origin` = https://github.com/matihope/webpad). Test machines build from a clone: push first, then `git pull` there.
- kolpa2: **Smart App Control blocks `cargo build` there** (build scripts can't run), so cross-compile on macOS and copy the exe:
  - `brew install mingw-w64` (once)
  - `cargo build --release --target x86_64-pc-windows-gnu`
  - `scp target/x86_64-pc-windows-gnu/release/webpad.exe kolpa2:C:/Users/23era/webpad/`
  - To run it with the Steam Link tunnel, see `docs/platforms.md`.
  - There is also a clone at `%USERPROFILE%\code\webpad`, with rustup (MSVC) and VS Build Tools.
- From macOS, `cargo check --target x86_64-pc-windows-gnu` type-checks the Windows-only code.

## Conventions
- **Protocol changes:** update `docs/protocol.md`, `src/protocol.rs` and `web/controller.js` together, and bump `PROTOCOL_VERSION` if the change breaks old clients.
- **Backends:** follow the contract in `docs/architecture.md`.
  - `update` gets the full state on every frame and runs under the sessions mutex, so it must not block.
  - Flip Y wherever the OS uses positive-down.
  - Put OS-only code behind `#[cfg(target_os = "…")]` on both the module and its `BackendKind` variant.
- **Stuck inputs:** every disconnect path must release inputs. `Sessions::leave` does this, so don't bypass it.
- **Pads never move between slots.** Games see controllers in plug-in order, so admin reordering moves owners, not pads (see `docs/architecture.md`).
- **Auth:** admin-only routes check `AdminAuth::is_admin`. Secrets are compared with `admin::ct_eq`. See ADR 0004.
- **Rust style:** `anyhow` for errors, `tracing` for logs ("P{n} …" for player-scoped messages, 1-based). Comments explain *why*, not what.
- **Web:** no frameworks and no build step. Touch handling uses Pointer Events. All buttons go through `updateButtons()`, which hit-tests every active pointer so thumbs can slide between buttons.
- **Dependencies:** add them with `cargo add`. Keep the dependency list short.
