//! Binary wire protocol between the phone and the server. See `docs/protocol.md`,
//! which is the authoritative spec — keep the two in sync.

use anyhow::{bail, ensure};

pub const PROTOCOL_VERSION: u8 = 2;

/// Longest player name in bytes (UTF-8).
pub const MAX_NAME_LEN: usize = 24;

/// Reconnect token handed out by the server. All zeroes means "no token".
pub type Token = [u8; 16];

/// Button bits. Values match XInput's `wButtons`, so the ViGEm backend can pass them through.
pub mod buttons {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LS: u16 = 0x0040;
    pub const RS: u16 = 0x0080;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;

    pub const NAMES: [(u16, &str); 15] = [
        (A, "A"),
        (B, "B"),
        (X, "X"),
        (Y, "Y"),
        (LB, "LB"),
        (RB, "RB"),
        (LS, "LS"),
        (RS, "RS"),
        (BACK, "Back"),
        (START, "Start"),
        (GUIDE, "Guide"),
        (DPAD_UP, "Up"),
        (DPAD_DOWN, "Down"),
        (DPAD_LEFT, "Left"),
        (DPAD_RIGHT, "Right"),
    ];
}

/// Full controller state. Axes follow XInput: positive Y is up.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct GamepadState {
    pub buttons: u16,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
    pub lt: u8,
    pub rt: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMsg {
    Hello { version: u8, token: Option<Token> },
    State(GamepadState),
    Ping(u32),
    Name(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectReason {
    Full = 1,
    BadVersion = 2,
    BackendError = 3,
    BadCode = 4,
    Kicked = 5,
    Locked = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMsg {
    Welcome { slot: u8, token: Token },
    Pong(u32),
    #[allow(dead_code)] // sent once backends report rumble (milestone 4)
    Rumble { low: u8, high: u8 },
    Reject(RejectReason),
    /// The host moved this player to another slot.
    Slot(u8),
    /// Flash and vibrate the phone so the host can tell who is who.
    Identify,
}

const HELLO: u8 = 0x01;
const STATE: u8 = 0x02;
const PING: u8 = 0x03;
const NAME: u8 = 0x04;
const WELCOME: u8 = 0x81;
const PONG: u8 = 0x82;
const RUMBLE: u8 = 0x83;
const REJECT: u8 = 0x84;
const SLOT: u8 = 0x85;
const IDENTIFY: u8 = 0x86;

impl ClientMsg {
    pub fn decode(buf: &[u8]) -> anyhow::Result<Self> {
        let Some((&kind, body)) = buf.split_first() else {
            bail!("empty frame");
        };
        if kind == NAME {
            let Some((&len, name)) = body.split_first() else {
                bail!("name frame without length");
            };
            ensure!(name.len() == len as usize, "name frame: length byte says {len}, got {}", name.len());
            ensure!(name.len() <= MAX_NAME_LEN, "name longer than {MAX_NAME_LEN} bytes");
            return Ok(Self::Name(std::str::from_utf8(name)?.trim().to_owned()));
        }
        let want = match kind {
            HELLO => 17,
            STATE => 12,
            PING => 4,
            _ => bail!("unknown client frame type {kind:#04x}"),
        };
        ensure!(body.len() == want, "frame {kind:#04x}: expected {want} bytes, got {}", body.len());
        let i16_at = |i: usize| i16::from_le_bytes([body[i], body[i + 1]]);
        Ok(match kind {
            HELLO => {
                let token: Token = body[1..17].try_into().unwrap();
                Self::Hello {
                    version: body[0],
                    token: (token != [0; 16]).then_some(token),
                }
            }
            STATE => Self::State(GamepadState {
                buttons: u16::from_le_bytes([body[0], body[1]]),
                lx: i16_at(2),
                ly: i16_at(4),
                rx: i16_at(6),
                ry: i16_at(8),
                lt: body[10],
                rt: body[11],
            }),
            _ => Self::Ping(u32::from_le_bytes(body.try_into().unwrap())),
        })
    }

    #[cfg(test)]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match *self {
            Self::Hello { version, token } => {
                out.extend([HELLO, version]);
                out.extend(token.unwrap_or_default());
            }
            Self::State(s) => {
                out.push(STATE);
                out.extend(s.buttons.to_le_bytes());
                for axis in [s.lx, s.ly, s.rx, s.ry] {
                    out.extend(axis.to_le_bytes());
                }
                out.extend([s.lt, s.rt]);
            }
            Self::Ping(t) => {
                out.push(PING);
                out.extend(t.to_le_bytes());
            }
            Self::Name(ref name) => {
                out.extend([NAME, name.len() as u8]);
                out.extend(name.as_bytes());
            }
        }
        out
    }
}

impl ServerMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match *self {
            Self::Welcome { slot, token } => {
                out.extend([WELCOME, PROTOCOL_VERSION, slot]);
                out.extend(token);
            }
            Self::Pong(t) => {
                out.push(PONG);
                out.extend(t.to_le_bytes());
            }
            Self::Rumble { low, high } => out.extend([RUMBLE, low, high]),
            Self::Reject(reason) => out.extend([REJECT, reason as u8]),
            Self::Slot(slot) => out.extend([SLOT, slot]),
            Self::Identify => out.push(IDENTIFY),
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_roundtrip() {
        let msgs = [
            ClientMsg::Hello { version: 1, token: None },
            ClientMsg::Hello { version: 1, token: Some([7; 16]) },
            ClientMsg::State(GamepadState {
                buttons: buttons::A | buttons::DPAD_LEFT,
                lx: -32767,
                ly: 32767,
                rx: 1,
                ry: -1,
                lt: 255,
                rt: 3,
            }),
            ClientMsg::Ping(0xDEAD_BEEF),
            ClientMsg::Name("Zoë".into()),
            ClientMsg::Name(String::new()),
        ];
        for msg in msgs {
            assert_eq!(ClientMsg::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn state_frame_layout() {
        let frame = ClientMsg::State(GamepadState { buttons: 0x1001, lx: -2, lt: 9, ..Default::default() }).encode();
        assert_eq!(frame, [0x02, 0x01, 0x10, 0xFE, 0xFF, 0, 0, 0, 0, 0, 0, 9, 0]);
    }

    #[test]
    fn rejects_bad_frames() {
        assert!(ClientMsg::decode(&[]).is_err());
        assert!(ClientMsg::decode(&[0x02, 0, 0]).is_err());
        assert!(ClientMsg::decode(&[0x7F]).is_err());
        assert!(ClientMsg::decode(&[0x04, 5, b'a']).is_err(), "length mismatch");
        assert!(ClientMsg::decode(&[0x04, 2, 0xFF, 0xFE]).is_err(), "invalid UTF-8");
        let long = [vec![0x04, 25], vec![b'a'; 25]].concat();
        assert!(ClientMsg::decode(&long).is_err(), "too long");
    }

    #[test]
    fn server_frames() {
        let welcome = ServerMsg::Welcome { slot: 2, token: [9; 16] }.encode();
        assert_eq!(&welcome[..3], &[0x81, PROTOCOL_VERSION, 2]);
        assert_eq!(welcome.len(), 19);
        assert_eq!(ServerMsg::Pong(5).encode(), [0x82, 5, 0, 0, 0]);
        assert_eq!(ServerMsg::Reject(RejectReason::Full).encode(), [0x84, 1]);
        assert_eq!(ServerMsg::Reject(RejectReason::Kicked).encode(), [0x84, 5]);
        assert_eq!(ServerMsg::Slot(3).encode(), [0x85, 3]);
        assert_eq!(ServerMsg::Identify.encode(), [0x86]);
    }
}
