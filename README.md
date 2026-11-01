# webpad

Use phones as game controllers, with no app to install. Run `webpad` on the computer with the game, scan the QR code with a phone and play. Each phone shows up as its own controller.

```
phone browser ──Wi-Fi──▶ webpad (this computer) ──▶ virtual Xbox controller ──▶ your game
```

> **Status:** early. The server, phone UI and debug `log` backend work. The real gamepad backends for Windows and Linux and the keyboard backend for macOS are in progress; see [docs/roadmap.md](docs/roadmap.md).

## Quick start

```sh
cargo run --release -- --backend log
```

1. A QR code and a URL such as `http://192.168.1.20:8080` appear in the terminal.
2. Connect the phone to the **same Wi-Fi** and scan the code or open the URL.
3. Tap to start. On Android the page goes fullscreen and locks to landscape.
4. Each phone gets a player number (P1–P4), shown in its player color.

### iPhone
iPhone browsers can't make a web page fullscreen. For a fullscreen controller, open the URL in **Safari** (Add to Home Screen isn't available in Chrome or other iPhone browsers), tap **Share → Add to Home Screen**, and open webpad from the home screen icon.

## Options

| Flag | Default | Meaning |
|------|---------|---------|
| `--port` | 8080 | HTTP and WebSocket port |
| `--backend` | `auto` | `auto` or `log` for now. `vigem`, `uinput` and `keyboard` are coming |
| `--max-players` | 4 | Number of player slots (1–16) |
| `--grace` | 30 | Seconds a disconnected phone keeps its slot |

Logging verbosity is set with `RUST_LOG`, for example `RUST_LOG=webpad=debug`.

## Platform setup (for the upcoming backends)

- **Windows:** install the [ViGEmBus](https://github.com/nefarius/ViGEmBus/releases) driver once. Up to 4 controllers.
- **Linux:** allow access to `/dev/uinput`:
  ```sh
  echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/60-webpad.rules
  sudo usermod -aG input $USER   # then log out and back in
  sudo udevadm control --reload && sudo udevadm trigger
  ```
- **macOS:** there is no virtual gamepad support ([why](docs/decisions/0003-macos-keyboard-fallback.md)). A keyboard-mapping backend is planned. It will need the Accessibility permission.

## Troubleshooting

- **The phone can't open the URL.** Make sure both devices are on the same network, guest Wi-Fi often isolates devices, and that the firewall allows the port. On macOS, allow incoming connections when asked.
- **The wrong IP is shown.** If the computer has several network interfaces, open the right address yourself: `http://<computer-ip>:8080`.
- **Lag.** The controller shows round-trip time under the player number. On a healthy home network it is under about 30 ms. 5 GHz Wi-Fi helps.

## Development

See [AGENTS.md](AGENTS.md) for repo layout and conventions, and [docs/](docs/) for architecture, the wire protocol, platform notes, decisions and the dev log.
