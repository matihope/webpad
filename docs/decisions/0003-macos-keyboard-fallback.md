# 0003: macOS uses a keyboard backend for now

**Status:** accepted, 2026-11-01

## Context
macOS has no public, unentitled API for creating a virtual game controller. The options looked at:
- **DriverKit virtual HID:** needs `com.apple.developer.hid.virtual.device`, which Apple must grant, plus a signed system extension.
- **`IOHIDUserDevice`:** needs the same entitlement on current macOS.
- **Karabiner-DriverKit-VirtualHIDDevice:** keyboard and pointing devices only.

## Decision
On macOS we provide a `keyboard` backend (`enigo`) that maps controller buttons to keys, configurable per slot. A real macOS gamepad stays a later research spike.

## Consequences
- macOS works for games that accept keyboard input, and the user must grant the Accessibility permission.
- Games that only read controllers through the GameController framework won't work with webpad on macOS.

## Update 2026-11-01
- Measured: `IOHIDUserDeviceCreateWithProperties` returns NULL on macOS 27, even as root. Shipping apps that create gamepads (for example Remote Gamepad) hold Apple's `com.apple.developer.hid.virtual.device` entitlement. There is no unentitled route, short of disabling SIP and AMFI.
- The keyboard backend is **deprioritized**. The user's games need analog input (racing) or several real controllers (Portal 2 co-op), which keys can't provide.
- The supported way to play "on a Mac" is Steam Link from a Windows host running webpad with `vigem`, tunneled to the Mac's network. See platforms.md.
- Revisit if a developer account becomes available. The entitlement plus a small signed helper would allow a real macOS backend.
