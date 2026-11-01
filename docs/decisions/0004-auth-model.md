# 0004: Separate join code and admin password, both optional

**Status:** accepted, 2026-11-01

## Context
Players and the host need different access:
- Players need a frictionless way in, ideally a QR code that takes them straight to the controller.
- The host needs to manage players: reorder, kick, lock joining.

webpad runs on a home LAN, usually for one evening, so heavy auth would only get in the way.

## Decision
There are two independent, optional secrets.
- **`--join-code`**
  - Players must send it as `/ws?code=`. It is embedded in the join QR and URL, so scanning gets you straight in.
  - A wrong or missing code gets Reject 4.
- **`--admin-password`**
  - Protects `/admin/ws` and `/admin/qr.svg`. Login sets a random 128-bit session cookie (`HttpOnly; SameSite=Strict`).
  - Sessions live in memory, so a restart logs admins out.
  - A failed login waits 1 s.
  - The admin socket also rejects a mismatched `Origin`, to prevent cross-site WebSocket hijacking.
- **Without an admin password, Manage is open to everyone** on the network. The user chose this: the password is opt-in.

Both secrets come from flags or env vars (`WEBPAD_JOIN_CODE`, `WEBPAD_ADMIN_PASSWORD`) and are compared in constant time.

## Consequences
- Scanning the QR never grants admin rights, and the admin password is never put in a URL.
- The server speaks plain HTTP, so on the LAN both secrets travel unencrypted. That is fine against casual guests, not against someone sniffing the network. HTTPS mode (milestone 4) would fix it.
- If the server is started without an admin password, any guest can kick players. The startup message says so and suggests `--admin-password`.
- The join code lives in the phone's `localStorage` (and is removed from the address bar), so reconnects and home-screen launches keep working.
