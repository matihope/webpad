//! Output backends: where phone input ends up. See `docs/architecture.md` for the contract.

mod log;
#[cfg(windows)]
mod vigem;

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
    /// Virtual Xbox 360 controllers via the ViGEmBus driver (Windows)
    #[cfg(windows)]
    Vigem,
}

pub fn create(kind: BackendKind) -> anyhow::Result<Box<dyn Backend>> {
    Ok(match kind {
        BackendKind::Log => Box::new(log::LogBackend),
        #[cfg(windows)]
        BackendKind::Vigem => Box::new(vigem::VigemBackend::new()?),
        BackendKind::Auto => auto(),
    })
}

/// The OS gamepad backend if it works here, otherwise `log` with a loud explanation.
fn auto() -> Box<dyn Backend> {
    #[cfg(windows)]
    match vigem::VigemBackend::new() {
        Ok(backend) => return Box::new(backend),
        Err(e) => tracing::warn!("{e:#}; falling back to the log backend (no controllers will be created)"),
    }
    #[cfg(not(windows))]
    tracing::warn!(
        "no gamepad backend for this OS yet (see docs/roadmap.md); using the log backend, so no controllers will be created"
    );
    Box::new(log::LogBackend)
}
