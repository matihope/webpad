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
