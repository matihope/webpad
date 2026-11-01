//! Debug backend: prints input to the terminal instead of creating a device.

use super::{Backend, Pad};
use crate::protocol::{GamepadState, buttons};

pub struct LogBackend;

impl Backend for LogBackend {
    fn name(&self) -> &'static str {
        "log"
    }

    fn connect(&self, slot: u8) -> anyhow::Result<Box<dyn Pad>> {
        tracing::info!("P{} plugged in", slot + 1);
        Ok(Box::new(LogPad { slot, last: String::new() }))
    }
}

struct LogPad {
    slot: u8,
    /// Last printed line. Sticks send ~120 frames/s, so only print when the rounded view changes.
    last: String,
}

impl Pad for LogPad {
    fn update(&mut self, state: &GamepadState) -> anyhow::Result<()> {
        let line = describe(state);
        if line != self.last {
            tracing::info!("P{} {line}", self.slot + 1);
            self.last = line;
        }
        Ok(())
    }
}

impl Drop for LogPad {
    fn drop(&mut self) {
        tracing::info!("P{} unplugged", self.slot + 1);
    }
}

fn describe(s: &GamepadState) -> String {
    let pressed: Vec<&str> = buttons::NAMES
        .iter()
        .filter(|(bit, _)| s.buttons & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    let axis = |v: i16| v as f32 / i16::MAX as f32;
    format!(
        "[{}] L=({:+.1},{:+.1}) R=({:+.1},{:+.1}) LT={} RT={}",
        pressed.join(" "),
        axis(s.lx),
        axis(s.ly),
        axis(s.rx),
        axis(s.ry),
        s.lt,
        s.rt,
    )
}
