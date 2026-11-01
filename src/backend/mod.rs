//! Output backends: where phone input ends up. See `docs/architecture.md` for the contract.

mod log;

use crate::protocol::GamepadState;

/// Creates one virtual device per player slot.
pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    /// Plug in a virtual device for `slot` (0-based).
    fn connect(&self, slot: u8) -> anyhow::Result<Box<dyn Pad>>;
}

/// One plugged-in virtual device. Dropping it unplugs the device.
pub trait Pad: Send {
    /// Apply the full controller state. Called on every state frame, so keep it cheap.
    fn update(&mut self, state: &GamepadState) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BackendKind {
    /// Best backend available on this OS
    Auto,
    /// Print input to the terminal (no device is created)
    Log,
}

pub fn create(kind: BackendKind) -> anyhow::Result<Box<dyn Backend>> {
    Ok(match kind {
        // Only the log backend exists so far; OS backends land in milestone 2 (docs/roadmap.md).
        BackendKind::Auto | BackendKind::Log => Box::new(log::LogBackend),
    })
}
