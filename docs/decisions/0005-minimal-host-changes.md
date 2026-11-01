# 0005: Minimal permanent changes on the host

**Status:** accepted, 2026-11-01

## Context
The OS backends need privileges that a normal user may not have. Linux needs `/dev/uinput`, and Windows needs the ViGEmBus driver. The first version of the Linux startup prompt offered to install a udev rule in `/etc` and add the user to `input`. The user objected: running webpad shouldn't leave permanent changes on a machine, and Steam doesn't need sudo either. In fact Steam does need root, but only once, through its package (`steam` / `steam-devices` ships the udev rule).

## Decision
- webpad never makes a permanent system change by itself.
- When it can fix a missing privilege, it offers a fix that lasts **until reboot** and touches as little as possible. It shows the exact command, asks `[Y/n]`, and only asks when someone is at a terminal.
  - Linux: `sudo setfacl -m u:<uid>:rw /dev/uinput`, for this user and this node only.
  - Linux: `sudo modprobe uinput`.
- Permanent options are **printed, not run**, so installing them stays the user's decision. On Linux that's Steam's udev rule (`uaccess`).
- Drivers the user installs on purpose (ViGEmBus via `tools/windows-setup.bat`) are separate, explicit setup steps, not something the server does.

## Consequences
- After every reboot, a machine without Steam's rule asks again (one sudo prompt).
- Nothing in `/etc` is touched, groups don't change, and there's nothing to uninstall.
- Headless and SSH use works through the same temporary ACL, which a `uaccess` rule wouldn't cover.
