# Roadmap

Update the checkboxes as work lands and record details in the [dev log](log/).

## Milestone 0: Scaffolding ✅
- [x] git repo, Cargo crate, `README.md`, `AGENTS.md`, `CLAUDE.md`
- [x] `docs/`: architecture, protocol, platforms, ADRs, roadmap, log

## Milestone 1: Server, web UI and `log` backend ✅
- [x] axum server with embedded `web/`, WebSocket at `/ws`, `TCP_NODELAY`
- [x] binary protocol v1 with unit tests
- [x] sessions: slots, reconnect tokens, takeover, release on disconnect, grace and reap, with unit tests
- [x] `log` backend
- [x] controller UI: floating sticks, D-pad with diagonals, ABXY, bumpers and triggers, Back/Guide/Start, L3/R3, multi-touch with slide-between-buttons, haptics, RTT, fullscreen, rotate overlay, iOS home-screen hint
- [x] terminal QR code with the LAN URL
- [x] try on a real iPhone (Safari + home screen), 2026-11-01
- [ ] try on a real Android phone *(needs a human)*

## Milestone 1b: Admin and auth ✅
- [x] landing page (Join or Manage), `/pad` for the controller, `/admin` for managing
- [x] optional `--join-code` (in the QR) and `--admin-password` (cookie session), ADR 0004
- [x] protocol v2: player names, Slot (reordered), Identify, Reject 4–6
- [x] admin page: join QR (fullscreen on click), lock joining, per-player cards with live input preview, reorder, identify, kick, remove disconnected
- [x] unit tests (session ops, protocol, auth) and `tools/admin-smoke.mjs`
- [ ] try on real phones: reorder, identify, kick *(needs a human)*

## Milestone 2: OS gamepad backends
- [ ] `uinput` backend on Linux. Test on `ssh kolpa` with `evtest`. Needs uinput permissions; see platforms.md.
- [x] `vigem` backend on Windows (code; type-checks from macOS via `--target x86_64-pc-windows-gnu`)
- [ ] verify `vigem` on kolpa2: phones show up in `joy.cpl` and in Steam (over Steam Link). Needs ViGEmBus installed by the user.
- [x] `--backend auto` picks the OS backend, falling back to `log`, and says why
- [ ] CI cross-compile check for each OS (`cargo check --target …`)

## Milestone 3: Keyboard backend (macOS-usable)
- [ ] `keyboard` backend via `enigo`, with a per-slot key map and a config file
- [ ] Detect missing Accessibility permission and show a clear error

## Milestone 4: Polish
- [ ] Rumble passthrough (ViGEm notification and uinput FF) → `0x83 Rumble`
- [ ] Analog triggers (slide on the trigger to set its value)
- [ ] Alternative layouts (SNES-style, 2-button), chosen via `?layout=`
- [ ] Optional HTTPS with a self-signed certificate for wake lock and gyro
- [ ] `/host` status page listing players, RTT, and a kick button
- [ ] Release binaries per OS
