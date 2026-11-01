//! Player slots: which connection owns which virtual pad, reconnects, admin actions, cleanup.
//!
//! Pads stay at their slot index for as long as they live, because the OS and games see
//! controllers in the order they were plugged in. Reordering players moves the *owner*
//! (token, connection, name) between slots, never the pad.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{mpsc, watch};

use crate::backend::{Backend, Pad};
use crate::protocol::{GamepadState, RejectReason, ServerMsg, Token};

/// Proof that a connection owns a slot. The slot is looked up by `conn`, so a lease
/// follows its owner when the host reorders players. A newer connection with the same
/// token replaces it, after which the old lease is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    pub token: Token,
    conn: u64,
}

/// Messages from the session layer (usually admin actions) to a phone's connection task.
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    Send(ServerMsg),
    /// Send this, then close the connection.
    Close(ServerMsg),
}

pub struct Joined {
    pub lease: Lease,
    pub slot: u8,
    /// Closes when the slot is taken over by a newer connection.
    pub control: mpsc::UnboundedReceiver<Control>,
}

#[derive(Debug)]
pub enum JoinError {
    Full,
    Locked,
    Backend(anyhow::Error),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub slots: Vec<Option<SlotInfo>>,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotInfo {
    pub name: String,
    pub connected: bool,
    /// Seconds until a disconnected player's pad is unplugged.
    pub grace_left: Option<u64>,
    pub state: GamepadState,
}

pub struct Sessions {
    backend: Box<dyn Backend>,
    /// How long a disconnected slot keeps its pad before it is unplugged.
    grace: Duration,
    inner: Mutex<Inner>,
    /// Bumped on every change, so admin views know when to refresh.
    version: watch::Sender<u64>,
}

struct Inner {
    slots: Vec<Option<Slot>>,
    next_conn: u64,
    locked: bool,
}

struct Slot {
    pad: Box<dyn Pad>,
    owner: Owner,
}

struct Owner {
    token: Token,
    name: String,
    /// Last state applied to the pad, for the admin live preview.
    state: GamepadState,
    /// Connection currently driving this slot, if any.
    conn: Option<Conn>,
    left_at: Instant,
}

struct Conn {
    id: u64,
    tx: mpsc::UnboundedSender<Control>,
}

impl Owner {
    fn send(&self, msg: Control) {
        if let Some(conn) = &self.conn {
            let _ = conn.tx.send(msg);
        }
    }
}

impl Sessions {
    pub fn new(backend: Box<dyn Backend>, max_players: u8, grace: Duration) -> Self {
        Self {
            backend,
            grace,
            inner: Mutex::new(Inner {
                slots: (0..max_players).map(|_| None).collect(),
                next_conn: 0,
                locked: false,
            }),
            version: watch::Sender::new(0),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn max_players(&self) -> usize {
        self.inner.lock().unwrap().slots.len()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.version.subscribe()
    }

    fn changed(&self) {
        self.version.send_modify(|v| *v += 1);
    }

    /// Claim a slot: the one matching `token`, else a free one, else the one that
    /// has been disconnected longest. While locked, only `token` rejoins are allowed.
    pub fn join(&self, token: Option<Token>) -> Result<Joined, JoinError> {
        let mut inner = self.inner.lock().unwrap();
        inner.next_conn += 1;
        let (tx, control) = mpsc::unbounded_channel();
        let conn = Conn { id: inner.next_conn, tx };
        let lease_for = |token| Lease { token, conn: conn.id };

        if let Some(token) = token
            && let Some(i) = inner.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.owner.token == token))
        {
            let owner = &mut inner.slots[i].as_mut().unwrap().owner;
            let lease = lease_for(token);
            // Replacing the old Conn drops its sender, which ends the old connection task.
            if owner.conn.replace(conn).is_some() {
                tracing::info!("P{} reconnected, replacing its previous connection", i + 1);
            } else {
                tracing::info!("P{} reconnected", i + 1);
            }
            drop(inner);
            self.changed();
            return Ok(Joined { lease, slot: i as u8, control });
        }

        if inner.locked {
            return Err(JoinError::Locked);
        }
        let token: Token = rand::random();
        let lease = lease_for(token);
        let owner = Owner { token, name: String::new(), state: GamepadState::default(), conn: Some(conn), left_at: Instant::now() };

        let slot = if let Some(i) = inner.slots.iter().position(Option::is_none) {
            let pad = self.backend.connect(i as u8).map_err(JoinError::Backend)?;
            inner.slots[i] = Some(Slot { pad, owner });
            tracing::info!("P{} joined", i + 1);
            i
        } else {
            // Every slot is taken. Hand over the longest-idle disconnected one, keeping its
            // pad: it is already neutral, and games keep seeing the same controller.
            let idle = inner
                .slots
                .iter_mut()
                .enumerate()
                .filter_map(|(i, s)| s.as_mut().filter(|s| s.owner.conn.is_none()).map(|s| (i, s)))
                .min_by_key(|(_, s)| s.owner.left_at);
            let Some((i, slot)) = idle else {
                return Err(JoinError::Full);
            };
            slot.owner = owner;
            tracing::info!("P{} taken over by a new player", i + 1);
            i
        };
        drop(inner);
        self.changed();
        Ok(Joined { lease, slot: slot as u8, control })
    }

    pub fn update(&self, lease: &Lease, state: &GamepadState) {
        let mut inner = self.inner.lock().unwrap();
        let Some((i, slot)) = inner.owned(lease) else {
            return;
        };
        if let Err(e) = slot.pad.update(state) {
            tracing::warn!("P{}: backend update failed: {e:#}", i + 1);
        }
        slot.owner.state = *state;
        drop(inner);
        self.changed();
    }

    pub fn set_name(&self, lease: &Lease, name: String) {
        let mut inner = self.inner.lock().unwrap();
        if let Some((i, slot)) = inner.owned(lease)
            && slot.owner.name != name
        {
            tracing::info!("P{} is now called {name:?}", i + 1);
            slot.owner.name = name;
            drop(inner);
            self.changed();
        }
    }

    /// The connection ended: release every input right away, keep the pad for `grace`.
    pub fn leave(&self, lease: &Lease) {
        let mut inner = self.inner.lock().unwrap();
        let Some((i, slot)) = inner.owned(lease) else {
            return;
        };
        release(i, slot);
        slot.owner.conn = None;
        slot.owner.left_at = Instant::now();
        tracing::info!("P{} disconnected", i + 1);
        drop(inner);
        self.changed();
    }

    /// Unplug pads whose owner has been gone longer than the grace period.
    pub fn reap(&self, now: Instant) {
        let mut inner = self.inner.lock().unwrap();
        let mut removed = false;
        for slot in &mut inner.slots {
            if slot.as_ref().is_some_and(|s| s.owner.conn.is_none() && now - s.owner.left_at >= self.grace) {
                *slot = None;
                removed = true;
            }
        }
        drop(inner);
        if removed {
            self.changed();
        }
    }

    /// Swap the players in slots `a` and `b` (either may be empty). Pads stay put.
    pub fn swap(&self, a: usize, b: usize) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let len = inner.slots.len();
        anyhow::ensure!(a < len && b < len, "slot out of range");
        if a == b {
            return Ok(());
        }
        match (inner.slots[a].is_some(), inner.slots[b].is_some()) {
            (false, false) => return Ok(()),
            (true, true) => {
                let (lo, hi) = (a.min(b), a.max(b));
                let (left, right) = inner.slots.split_at_mut(hi);
                let (x, y) = (left[lo].as_mut().unwrap(), right[0].as_mut().unwrap());
                release(lo, x);
                release(hi, y);
                std::mem::swap(&mut x.owner, &mut y.owner);
                x.owner.send(Control::Send(ServerMsg::Slot(lo as u8)));
                y.owner.send(Control::Send(ServerMsg::Slot(hi as u8)));
            }
            (from_a, _) => {
                let (from, to) = if from_a { (a, b) } else { (b, a) };
                let pad = self.backend.connect(to as u8)?;
                let mut old = inner.slots[from].take().unwrap();
                release(from, &mut old);
                old.owner.send(Control::Send(ServerMsg::Slot(to as u8)));
                inner.slots[to] = Some(Slot { pad, owner: old.owner });
                // `old.pad` drops here, unplugging the device at `from`.
            }
        }
        tracing::info!("host swapped P{} and P{}", a + 1, b + 1);
        drop(inner);
        self.changed();
        Ok(())
    }

    /// Disconnect the player in `slot`, tell their phone why, and unplug the pad.
    pub fn kick(&self, slot: usize) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(s) = inner.slots.get_mut(slot).and_then(Option::take) {
            s.owner.send(Control::Close(ServerMsg::Reject(RejectReason::Kicked)));
            tracing::info!("host kicked P{}", slot + 1);
            drop(inner);
            self.changed();
        }
    }

    /// Unplug a disconnected player's pad now instead of waiting for the grace period.
    pub fn unplug(&self, slot: usize) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(s) = inner.slots.get_mut(slot)
            && s.as_ref().is_some_and(|s| s.owner.conn.is_none())
        {
            *s = None;
            drop(inner);
            self.changed();
        }
    }

    pub fn identify(&self, slot: usize) {
        let inner = self.inner.lock().unwrap();
        if let Some(Some(s)) = inner.slots.get(slot) {
            s.owner.send(Control::Send(ServerMsg::Identify));
        }
    }

    pub fn set_locked(&self, locked: bool) {
        self.inner.lock().unwrap().locked = locked;
        tracing::info!("joining {}", if locked { "locked" } else { "unlocked" });
        self.changed();
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let slots = inner
            .slots
            .iter()
            .map(|s| {
                s.as_ref().map(|s| SlotInfo {
                    name: s.owner.name.clone(),
                    connected: s.owner.conn.is_some(),
                    grace_left: s
                        .owner
                        .conn
                        .is_none()
                        .then(|| self.grace.saturating_sub(now - s.owner.left_at).as_secs()),
                    state: s.owner.state,
                })
            })
            .collect();
        Snapshot { slots, locked: inner.locked }
    }
}

/// Set a slot's pad to neutral.
fn release(i: usize, slot: &mut Slot) {
    slot.owner.state = GamepadState::default();
    if let Err(e) = slot.pad.update(&slot.owner.state) {
        tracing::warn!("P{}: failed to release inputs: {e:#}", i + 1);
    }
}

impl Inner {
    fn owned(&mut self, lease: &Lease) -> Option<(usize, &mut Slot)> {
        self.slots
            .iter_mut()
            .enumerate()
            .find_map(|(i, s)| s.as_mut().filter(|s| s.owner.conn.as_ref().is_some_and(|c| c.id == lease.conn)).map(|s| (i, s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc::error::TryRecvError;

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
        let mut old = s.join(None).unwrap();
        let new = s.join(Some(old.lease.token)).unwrap();
        assert_eq!(new.slot, old.slot);
        assert_eq!(old.control.try_recv(), Err(TryRecvError::Disconnected), "old connection is told to stop");
        take(&fake);

        s.update(&old.lease, &pressed(1));
        s.leave(&old.lease);
        assert!(take(&fake).is_empty(), "stale connection must not touch the pad");

        s.update(&new.lease, &pressed(2));
        assert_eq!(take(&fake), ["update 0 2"]);
    }

    #[test]
    fn leave_releases_inputs_and_reap_unplugs_after_grace() {
        let (s, fake) = setup(1);
        let p = s.join(None).unwrap();
        s.update(&p.lease, &pressed(1));
        s.leave(&p.lease);
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
        s.leave(&b.lease);
        s.leave(&a.lease);
        take(&fake);

        let c = s.join(None).unwrap();
        assert_eq!(c.slot, 1);
        assert_ne!(c.lease.token, b.lease.token);
        assert!(take(&fake).is_empty(), "pad is reused, not reconnected");
        assert!(s.join(Some(b.lease.token)).is_ok_and(|j| j.slot == 0), "unknown token falls back to a free or idle slot");
    }

    #[test]
    fn swap_moves_owners_not_pads() {
        let (s, fake) = setup(2);
        let mut a = s.join(None).unwrap();
        let mut b = s.join(None).unwrap();
        s.update(&a.lease, &pressed(1));
        take(&fake);

        s.swap(0, 1).unwrap();
        assert_eq!(take(&fake), ["update 0 0", "update 1 0"], "both pads released, none reconnected");
        assert_eq!(a.control.try_recv(), Ok(Control::Send(ServerMsg::Slot(1))));
        assert_eq!(b.control.try_recv(), Ok(Control::Send(ServerMsg::Slot(0))));

        s.update(&a.lease, &pressed(4));
        assert_eq!(take(&fake), ["update 1 4"], "lease follows its owner to the new slot");
        assert!(s.join(Some(a.lease.token)).is_ok_and(|j| j.slot == 1));
    }

    #[test]
    fn swap_into_empty_slot_plugs_target_and_unplugs_source() {
        let (s, fake) = setup(3);
        let mut a = s.join(None).unwrap();
        take(&fake);

        s.swap(0, 2).unwrap();
        assert_eq!(take(&fake), ["connect 2", "update 0 0", "drop 0"]);
        assert_eq!(a.control.try_recv(), Ok(Control::Send(ServerMsg::Slot(2))));
        let snap = s.snapshot();
        assert!(snap.slots[0].is_none() && snap.slots[2].is_some());
        assert!(s.swap(0, 3).is_err());
    }

    #[test]
    fn kick_closes_connection_and_unplugs() {
        let (s, fake) = setup(1);
        let mut a = s.join(None).unwrap();
        take(&fake);
        s.kick(0);
        assert_eq!(a.control.try_recv(), Ok(Control::Close(ServerMsg::Reject(RejectReason::Kicked))));
        assert_eq!(take(&fake), ["drop 0"]);
        s.leave(&a.lease); // the connection task still calls leave; it must be a no-op
        assert!(take(&fake).is_empty());
        assert!(s.join(Some(a.lease.token)).is_ok_and(|j| j.slot == 0 && j.lease.token != a.lease.token), "token no longer valid");
    }

    #[test]
    fn unplug_only_removes_disconnected_slots() {
        let (s, fake) = setup(2);
        let a = s.join(None).unwrap();
        let b = s.join(None).unwrap();
        s.leave(&b.lease);
        take(&fake);
        s.unplug(0);
        s.unplug(1);
        assert_eq!(take(&fake), ["drop 1"]);
        assert!(s.snapshot().slots[0].is_some());
        drop(a);
    }

    #[test]
    fn locked_refuses_new_players_but_allows_rejoin() {
        let (s, _) = setup(3);
        let a = s.join(None).unwrap();
        s.leave(&a.lease);
        s.set_locked(true);
        assert!(matches!(s.join(None), Err(JoinError::Locked)));
        assert!(s.join(Some(a.lease.token)).is_ok_and(|j| j.slot == 0));
        s.set_locked(false);
        assert!(s.join(None).is_ok());
    }

    #[test]
    fn snapshot_reports_names_state_and_grace() {
        let (s, _) = setup(2);
        let a = s.join(None).unwrap();
        let b = s.join(None).unwrap();
        s.set_name(&a.lease, "Ala".into());
        s.update(&a.lease, &pressed(8));
        s.leave(&b.lease);
        let mut version = s.subscribe();
        version.mark_unchanged();
        s.set_locked(true);
        assert!(version.has_changed().unwrap());

        let snap = s.snapshot();
        let p1 = snap.slots[0].as_ref().unwrap();
        assert_eq!((p1.name.as_str(), p1.connected, p1.grace_left, p1.state.buttons), ("Ala", true, None, 8));
        let p2 = snap.slots[1].as_ref().unwrap();
        assert!(!p2.connected && p2.grace_left.is_some_and(|g| g <= 30));
        assert!(snap.locked);
    }
}
