# Platform notes

## Host backends

### Windows: `vigem` (planned, milestone 2)
- It creates virtual Xbox 360 controllers through the ViGEmBus driver, using the `vigem-client` crate. XInput games see at most 4 of them.
- The user must install ViGEmBus once. The project is archived upstream (2023), but the last release still works on Windows 10 and 11.
- Button bits map directly onto `XUSB_REPORT.wButtons`. Axes use the same convention as XInput.
- Test host: `ssh kolpa2`. As of 2026-11-01 it had no Rust toolchain and no ViGEmBus installed.

### Linux: `uinput` (planned, milestone 2)
- It creates a device through `/dev/uinput` with the `evdev` crate. To look like a standard pad to SDL and Steam, it should advertise BTN_SOUTH/EAST/NORTH/WEST, BTN_TL/TR, BTN_SELECT/START/MODE, BTN_THUMBL/R, ABS_X/Y/RX/RY, ABS_Z/RZ for the triggers and ABS_HAT0X/Y for the D-pad. Using the Xbox 360 VID/PID (045e:028e) helps.
- evdev's Y axis is **positive-down**, so Y must be flipped.
- Permissions: `/dev/uinput` is usually root-only. The fix is a udev rule such as `KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"` plus adding the user to `input`, or running with sudo.
- Test host: `ssh kolpa`. It runs Arch, is headless and has cargo installed. As of 2026-11-01 `/dev/uinput` was root:root 0600. Use `evtest` to check the device.

### macOS: `keyboard` (planned, milestone 3)
- There is no practical public API for a virtual gamepad. DriverKit virtual HID needs an entitlement that Apple must grant, and Karabiner's driver only does keyboard and pointer. See [ADR 0003](decisions/0003-macos-keyboard-fallback.md).
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
