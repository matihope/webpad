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

### Linux: `uinput` (works; verified on kolpa 2026-11-01)
- Each pad is created with a few ioctls on `/dev/uinput` (through `libc`) and mirrors what the kernel's `xpad` driver exposes for a wired Xbox 360 pad:
  - name `Microsoft X-Box 360 pad`, ID `045e:028e` version `0110`, bus USB;
  - buttons BTN_SOUTH/EAST/NORTH/WEST, TL/TR, SELECT/START/MODE, THUMBL/R;
  - sticks ABS_X/Y/RX/RY (−32768…32767, fuzz 16, flat 128), triggers ABS_Z/RZ (0…255), D-pad ABS_HAT0X/Y.
  - SDL, Steam and Wine/Proton therefore map it like a real 360 pad.
- **Phys is `webpad/P<n>`**, so `/proc/bus/input/devices` and udev rules can tell webpad's pads apart.
  - The `evdev` crate (0.13.2) wasn't used: its `UI_SET_PHYS` is declared with the size of `char` instead of `char*`, so the kernel answers EINVAL.
- evdev's Y axis is **positive-down**. Y is flipped with bitwise NOT like xpad does, so a centered stick reads −1, as on real hardware.
- udev tags the pads `ID_INPUT_JOYSTICK=1` and `uaccess`. On a desktop, the logged-in user (and their games) can read them without extra setup, and the kernel adds a `js*` node too.
- **Permissions:** `/dev/uinput` is usually root-only. Steam's udev rule (in the `steam` package on Arch, `steam-devices` on Debian and Ubuntu) tags it `uaccess`, so on machines with Steam the logged-in user usually has access already.
  - **Started from a terminal** (stdin and stderr are TTYs), webpad explains the problem, shows the command and asks `[Y/n]`. It then runs the command and retries; each fix is offered once.
    - If access is missing: `sudo setfacl -m u:<uid>:rw /dev/uinput`. It covers only this user and only this node, and is gone after a reboot because the node is recreated at boot.
    - If `/dev/uinput` is missing: `sudo modprobe uinput`, also until reboot.
  - Following [ADR 0005](decisions/0005-minimal-host-changes.md), webpad **never installs anything permanent**. The error message prints the permanent option instead: Steam's rule, `KERNEL=="uinput", SUBSYSTEM=="misc", TAG+="uaccess", OPTIONS+="static_node=uinput"`, in `/etc/udev/rules.d/60-webpad.rules`. That rule doesn't cover SSH or headless sessions.
  - **Without a terminal**, or after the user declines, `--backend uinput` exits with both options, and `auto` warns and falls back to `log`.
- **Verified 2026-11-01 on kolpa** (Arch, kernel 7.2):
  - `tools/drive-pad.mjs` and `tools/read-pad.py` showed the right key, axis and hat values for each step.
  - The pad stays plugged in during the grace period and disappears after it.
  - Not tested yet: a real game or Steam on a Linux desktop.
- Test host `ssh kolpa`: access there is temporary until reboot.
  - An ACL on `/dev/uinput` for mateusz.
  - A runtime rule `/run/udev/rules.d/60-webpad-test.rules` that makes `webpad/*` event nodes owned by mateusz, so the pads can be read back. A `RUN+="setfacl …"` rule failed inside udev (exit 1); `OWNER=` works.

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
