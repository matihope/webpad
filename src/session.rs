//! Player slots: which connection owns which virtual pad, reconnects, and cleanup.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::backend::{Backend, Pad};
use crate::protocol::{GamepadState, Token};

/// Proof that a connection owns a slot. A newer connection with the same token
/// replaces it, after which the old lease's updates are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    pub slot: u8,
    pub token: Token,
    conn: u64,
}

#[derive(Debug)]
pub enum JoinError {
    Full,
    Backend(anyhow::Error),
}

pub struct Sessions {
    backend: Box<dyn Backend>,
    /// How long a disconnected slot keeps its pad before it is unplugged.
    grace: Duration,
    inner: Mutex<Inner>,
}

struct Inner {
    slots: Vec<Option<Slot>>,
    next_conn: u64,
}

struct Slot {
    token: Token,
    pad: Box<dyn Pad>,
    /// Connection currently driving this slot, if any.
    conn: Option<u64>,
    left_at: Instant,
}

impl Sessions {
    pub fn new(backend: Box<dyn Backend>, max_players: u8, grace: Duration) -> Self {
        Self {
            backend,
            grace,
            inner: Mutex::new(Inner {
                slots: (0..max_players).map(|_| None).collect(),
                next_conn: 0,
            }),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// Claim a slot: the one matching `token`, else a free one, else the one that
    /// has been disconnected longest.
    pub fn join(&self, token: Option<Token>) -> Result<Lease, JoinError> {
        let mut inner = self.inner.lock().unwrap();
        inner.next_conn += 1;
        let conn = inner.next_conn;

        if let Some(token) = token
            && let Some(i) = inner.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.token == token))
        {
            let slot = inner.slots[i].as_mut().unwrap();
            if slot.conn.is_some() {
                tracing::info!("P{} reconnected, replacing its previous connection", i + 1);
            } else {
                tracing::info!("P{} reconnected", i + 1);
            }
            slot.conn = Some(conn);
            return Ok(Lease { slot: i as u8, token, conn });
        }

        let token: Token = rand::random();
        if let Some(i) = inner.slots.iter().position(Option::is_none) {
            let pad = self.backend.connect(i as u8).map_err(JoinError::Backend)?;
            inner.slots[i] = Some(Slot { token, pad, conn: Some(conn), left_at: Instant::now() });
            tracing::info!("P{} joined", i + 1);
            return Ok(Lease { slot: i as u8, token, conn });
        }

        // Every slot is taken. Hand over the longest-idle disconnected one, keeping its
        // pad: it is already neutral, and games keep seeing the same controller.
        let idle = inner
            .slots
            .iter_mut()
            .enumerate()
            .filter_map(|(i, s)| s.as_mut().filter(|s| s.conn.is_none()).map(|s| (i, s)))
            .min_by_key(|(_, s)| s.left_at);
        let Some((i, slot)) = idle else {
            return Err(JoinError::Full);
        };
        slot.token = token;
        slot.conn = Some(conn);
        tracing::info!("P{} taken over by a new player", i + 1);
        Ok(Lease { slot: i as u8, token, conn })
    }

    pub fn update(&self, lease: &Lease, state: &GamepadState) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(slot) = inner.owned(lease)
            && let Err(e) = slot.pad.update(state)
        {
            tracing::warn!("P{}: backend update failed: {e:#}", lease.slot + 1);
        }
    }

    /// The connection ended: release every input right away, keep the pad for `grace`.
    pub fn leave(&self, lease: &Lease) {
        let mut inner = self.inner.lock().unwrap();
        let Some(slot) = inner.owned(lease) else {
            return;
        };
        if let Err(e) = slot.pad.update(&GamepadState::default()) {
            tracing::warn!("P{}: failed to release inputs: {e:#}", lease.slot + 1);
        }
        slot.conn = None;
        slot.left_at = Instant::now();
        tracing::info!("P{} disconnected", lease.slot + 1);
    }

    /// Unplug pads whose owner has been gone longer than the grace period.
    pub fn reap(&self, now: Instant) {
        let mut inner = self.inner.lock().unwrap();
        for slot in &mut inner.slots {
            if slot.as_ref().is_some_and(|s| s.conn.is_none() && now - s.left_at >= self.grace) {
                *slot = None;
            }
        }
    }
}

impl Inner {
    fn owned(&mut self, lease: &Lease) -> Option<&mut Slot> {
        self.slots[lease.slot as usize].as_mut().filter(|s| s.conn == Some(lease.conn))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Records what each pad saw, so tests can inspect backend calls.
    #[derive(Default, Clone)]
    struct Fake(Arc<Mutex<Vec<String>>>);

    struct FakePad(u8, Fake);

    impl Backend for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn connect(&self, slot: u8) -> anyhow::Result<Box<dyn Pad>> {
            self.0.lock().unwrap().push(format!("connect {slot}"));
            Ok(Box::new(FakePad(slot, self.clone())))
        }
    }

    impl Pad for FakePad {
        fn update(&mut self, s: &GamepadState) -> anyhow::Result<()> {
            self.1.0.lock().unwrap().push(format!("update {} {}", self.0, s.buttons));
            Ok(())
        }
    }

    impl Drop for FakePad {
        fn drop(&mut self) {
            self.1.0.lock().unwrap().push(format!("drop {}", self.0));
        }
    }

    fn setup(max: u8) -> (Sessions, Fake) {
        let fake = Fake::default();
        (Sessions::new(Box::new(fake.clone()), max, Duration::from_secs(30)), fake)
    }

    fn pressed(b: u16) -> GamepadState {
        GamepadState { buttons: b, ..Default::default() }
    }

    fn take(fake: &Fake) -> Vec<String> {
        std::mem::take(&mut *fake.0.lock().unwrap())
    }

    #[test]
    fn assigns_slots_in_order_and_rejects_when_full() {
        let (s, _) = setup(2);
        assert_eq!(s.join(None).unwrap().slot, 0);
        assert_eq!(s.join(None).unwrap().slot, 1);
        assert!(matches!(s.join(None), Err(JoinError::Full)));
    }

    #[test]
    fn token_reclaims_slot_and_replaces_old_connection() {
        let (s, fake) = setup(2);
        let old = s.join(None).unwrap();
        let new = s.join(Some(old.token)).unwrap();
        assert_eq!(new.slot, old.slot);
        take(&fake);

        s.update(&old, &pressed(1));
        s.leave(&old);
        assert!(take(&fake).is_empty(), "stale connection must not touch the pad");

        s.update(&new, &pressed(2));
        assert_eq!(take(&fake), ["update 0 2"]);
    }

    #[test]
    fn leave_releases_inputs_and_reap_unplugs_after_grace() {
        let (s, fake) = setup(1);
        let lease = s.join(None).unwrap();
        s.update(&lease, &pressed(1));
        s.leave(&lease);
        assert_eq!(take(&fake), ["connect 0", "update 0 1", "update 0 0"]);

        let now = Instant::now();
        s.reap(now);
        assert!(take(&fake).is_empty(), "still in grace period");
        s.reap(now + Duration::from_secs(31));
        assert_eq!(take(&fake), ["drop 0"]);
        assert_eq!(s.join(None).unwrap().slot, 0);
    }

    #[test]
    fn full_server_hands_over_longest_idle_slot() {
        let (s, fake) = setup(2);
        let a = s.join(None).unwrap();
        let b = s.join(None).unwrap();
        s.leave(&b);
        s.leave(&a);
        take(&fake);

        let c = s.join(None).unwrap();
        assert_eq!(c.slot, 1);
        assert_ne!(c.token, b.token);
        assert!(take(&fake).is_empty(), "pad is reused, not reconnected");
        assert!(s.join(Some(b.token)).is_ok_and(|l| l.slot == 0), "unknown token falls back to a free or idle slot");
    }
}
