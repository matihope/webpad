//! Windows backend: virtual Xbox 360 controllers through the ViGEmBus driver.

use std::sync::Arc;

use anyhow::Context;
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

use super::{Backend, Pad};
use crate::protocol::GamepadState;

pub struct VigemBackend {
    client: Arc<Client>,
}

impl VigemBackend {
    pub fn new() -> anyhow::Result<Self> {
        let client = Client::connect().map_err(|e| match e {
            vigem_client::Error::BusNotFound => anyhow::anyhow!(
                "the ViGEmBus driver is not installed; get it from https://github.com/nefarius/ViGEmBus/releases"
            ),
            e => anyhow::anyhow!("could not connect to ViGEmBus: {e}"),
        })?;
        Ok(Self { client: Arc::new(client) })
    }
}

impl Backend for VigemBackend {
    fn name(&self) -> &'static str {
        "vigem"
    }

    fn connect(&self, slot: u8) -> anyhow::Result<Box<dyn Pad>> {
        let mut target = Xbox360Wired::new(self.client.clone(), TargetId::XBOX360_WIRED);
        target.plugin().context("ViGEm plugin")?;
        target.wait_ready().context("ViGEm wait_ready")?;
        // Windows picks the XInput index (lowest free one), not us; log it so mismatches are visible.
        match target.get_user_index() {
            Ok(index) => tracing::info!("P{} plugged in as Xbox 360 controller #{}", slot + 1, index + 1),
            Err(_) => tracing::info!("P{} plugged in as Xbox 360 controller", slot + 1),
        }
        Ok(Box::new(VigemPad { target }))
    }
}

/// Dropping the target unplugs the virtual controller.
struct VigemPad {
    target: Xbox360Wired<Arc<Client>>,
}

impl Pad for VigemPad {
    fn update(&mut self, s: &GamepadState) -> anyhow::Result<()> {
        // Protocol buttons and axes already use XInput's layout and conventions.
        self.target.update(&XGamepad {
            buttons: XButtons(s.buttons),
            left_trigger: s.lt,
            right_trigger: s.rt,
            thumb_lx: s.lx,
            thumb_ly: s.ly,
            thumb_rx: s.rx,
            thumb_ry: s.ry,
        })?;
        Ok(())
    }
}
