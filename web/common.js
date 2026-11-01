'use strict';
// Shared by the controller, landing and admin pages (plain script, so these are globals).

/** Button bits; must match docs/protocol.md and src/protocol.rs. */
const BTN = {
  DPAD_UP: 0x0001, DPAD_DOWN: 0x0002, DPAD_LEFT: 0x0004, DPAD_RIGHT: 0x0008,
  START: 0x0010, BACK: 0x0020, LS: 0x0040, RS: 0x0080, LB: 0x0100, RB: 0x0200,
  GUIDE: 0x0400, A: 0x1000, B: 0x2000, X: 0x4000, Y: 0x8000,
};
const PLAYER_COLORS = ['#4ade80', '#f87171', '#60a5fa', '#facc15', '#c084fc', '#fb923c', '#22d3ee', '#f472b6'];
const playerColor = (slot) => PLAYER_COLORS[slot % PLAYER_COLORS.length];

/** localStorage that never throws (private mode, blocked storage). */
const store = {
  get(key) {
    try { return localStorage.getItem(key) || ''; } catch { return ''; }
  },
  set(key, value) {
    try { value ? localStorage.setItem(key, value) : localStorage.removeItem(key); } catch {}
  },
};
const NAME_KEY = 'webpad-name';
const CODE_KEY = 'webpad-code';
