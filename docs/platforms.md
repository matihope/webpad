# Platform notes

## Host backends

### Windows: `vigem` (works; verified over Steam Link 2026-11-01)
- It creates virtual Xbox 360 controllers through the ViGEmBus driver, using the `vigem-client` crate. XInput games see at most 4 of them.
- The user must install ViGEmBus once (`tools/windows-setup.bat` does it, along with the VS Build Tools). The project is archived upstream (2023), but the last release (1.22) still works on Windows 11 26H2.
- Button bits map directly onto `XUSB_REPORT.wButtons`. Axes use the same convention as XInput.
- Windows assigns the XInput index (the lowest free one), not webpad. The log shows which index each player got.
- `--backend auto` uses ViGEm when the driver is reachable. Otherwise it warns and falls back to `log`.
- ViGEm also works when webpad is started from a non-interactive SSH session.
- Windows Firewall blocks the port until you allow it (admin): `netsh advfirewall firewall add rule name="webpad" dir=in action=allow protocol=TCP localport=8080 profile=private`. The tunnel below doesn't need this.
- **Smart App Control** (Windows 11), when On, blocks the unsigned build-script executables cargo runs, so `cargo build` fails with `os error 4551` and offers no override.
  - Either turn it off (Windows Security → App & browser control), or cross-compile on macOS and copy the exe:
    - `brew install mingw-w64`
    - `cargo build --release --target x86_64-pc-windows-gnu`
    - `scp target/x86_64-pc-windows-gnu/release/webpad.exe kolpa2:C:/Users/23era/webpad/`
  - On kolpa2 the cross-built `webpad.exe` was *not* blocked.
- **Steam Link:** run webpad on the PC that runs the game, not on the Steam Link client. The client only forwards controllers its own OS sees, and macOS can't create virtual gamepads.
  - If the phones aren't on the PC's network (for example Steam Link over the internet), run webpad on the PC and tunnel it to a machine on the phones' network. One command on the Mac starts both:
    ```
    ssh -g -L 8080:127.0.0.1:8080 kolpa2 "C:\Users\23era\webpad\webpad.exe --backend vigem --public-url http://<mac-lan-ip>:8080"
    ```
    - `-g` lets phones on the Mac's Wi-Fi connect. `--public-url` makes the QR code point at the Mac.
    - Use `127.0.0.1`, not `localhost`. Windows resolves `localhost` to `::1`, webpad listens on IPv4 only, and the tunnel then returns empty replies.
    - Closing the ssh leaves `webpad.exe` running on Windows, so run `ssh kolpa2 "taskkill /F /IM webpad.exe"` before starting it again.
    - When the Mac's IP changes (a new Wi-Fi network or a hotspot), restart with the new `--public-url`.
    - Input then follows the same path Steam Link's own controller forwarding would (about 30–35 ms one way from the Mac to kolpa2, measured 2026-11-01).
- Test host: `ssh kolpa2` (dual-boot; it sometimes comes up in Linux on the same address). It has ViGEmBus, VS Build Tools and rustup (MSVC), but Smart App Control is On, so build on the Mac (above).

### Linux: `uinput` (planned, milestone 2)
- It creates a device through `/dev/uinput` with the `evdev` crate. To look like a standard pad to SDL and Steam, it should advertise BTN_SOUTH/EAST/NORTH/WEST, BTN_TL/TR, BTN_SELECT/START/MODE, BTN_THUMBL/R, ABS_X/Y/RX/RY, ABS_Z/RZ for the triggers and ABS_HAT0X/Y for the D-pad. Using the Xbox 360 VID/PID (045e:028e) helps.
- evdev's Y axis is **positive-down**, so Y must be flipped.
- Permissions: `/dev/uinput` is usually root-only. The fix is a udev rule such as `KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"` plus adding the user to `input`, or running with sudo.
- Test host: `ssh kolpa`. It runs Arch, is headless and has cargo installed. As of 2026-11-01 `/dev/uinput` was root:root 0600. Use `evtest` to check the device.

### macOS: `keyboard` (planned, milestone 3)
- There is no practical public API for a virtual gamepad. DriverKit virtual HID needs an entitlement that Apple must grant, and Karabiner's driver only does keyboard and pointer. See [ADR 0003](decisions/0003-macos-keyboard-fallback.md).
  - Tested 2026-11-01 on macOS 27: `IOHIDUserDeviceCreateWithProperties` with a gamepad descriptor returns NULL, **even as root**.
  - Commercial apps that do it (for example "Remote Gamepad" by SmartFusionLabs) ship with Apple's `com.apple.developer.hid.virtual.device` entitlement and a DriverKit extension that only their own app may open.
  - Apple's GameController framework reportedly ignores virtual HID devices anyway.
  - Keys are digital, so the keyboard fallback is useless for analog games (racing) and for games that need N controllers (Portal 2 co-op). For those, use Steam Link from a Windows host (above).
- The fallback maps buttons to key presses through `enigo`. It needs the Accessibility permission (System Settings → Privacy & Security → Accessibility).
- The `log` backend works on every OS and is the default here for now.

## Phone browsers

| | Android Chrome | iPhone Safari |
|---|---|---|
| Fullscreen API | ✅ (needs a user gesture; we use the start overlay's tap) | ❌. The workaround is Share → Add to Home Screen, which gives a fullscreen standalone view through the manifest and `apple-mobile-web-app-capable`. **This only works from Safari**: other iOS browsers (Chrome, Firefox, …) can't add it, so the start overlay tells their users to switch to Safari |
| Orientation lock | ✅ (fullscreen only) | ❌. We show a "rotate your phone" overlay in portrait |
| `navigator.vibrate` | ✅ | ❌ (no haptics) |
| Wake Lock | HTTPS only | HTTPS only |
| Gyro (DeviceMotion) | HTTPS only | HTTPS plus a permission prompt |
| Pointer Events, multi-touch | ✅ | ✅ |

Other notes:
- Locking the phone kills the socket. The client reconnects with its token when the page becomes visible again, and the server keeps the slot for `--grace` seconds.
- Wi-Fi power saving on phones can add latency spikes. The RTT readout on the controller shows them.
- The token lives in `sessionStorage`, not `localStorage`, so two tabs on one device become two players. That is useful for testing on a desktop browser.
