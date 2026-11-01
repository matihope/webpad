//! Linux backend: virtual Xbox 360 controllers through `/dev/uinput`.
//!
//! The devices copy what the kernel's `xpad` driver exposes for a wired Xbox 360 pad
//! (name, IDs, buttons, axis ranges), so SDL, Steam and Wine map them like the real thing.
//! This talks to uinput with a handful of ioctls instead of the `evdev` crate, whose
//! 0.13 `UI_SET_PHYS` is encoded with the wrong size and fails with EINVAL.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;

use anyhow::Context;

use super::{Backend, Pad};
use crate::protocol::{GamepadState, buttons};

const UINPUT: &str = "/dev/uinput";

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;

const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_MODE: u16 = 0x13c;
const BTN_THUMBL: u16 = 0x13d;
const BTN_THUMBR: u16 = 0x13e;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

/// Protocol button bit → evdev key, in xpad's layout.
const KEYS: [(u16, u16); 11] = [
    (buttons::A, BTN_SOUTH),
    (buttons::B, BTN_EAST),
    (buttons::X, BTN_NORTH),
    (buttons::Y, BTN_WEST),
    (buttons::LB, BTN_TL),
    (buttons::RB, BTN_TR),
    (buttons::BACK, BTN_SELECT),
    (buttons::START, BTN_START),
    (buttons::GUIDE, BTN_MODE),
    (buttons::LS, BTN_THUMBL),
    (buttons::RS, BTN_THUMBR),
];

/// Axis code, minimum, maximum, fuzz, flat: xpad's values for a wired 360 pad.
const AXES: [(u16, i32, i32, i32, i32); 8] = [
    (ABS_X, -32768, 32767, 16, 128),
    (ABS_Y, -32768, 32767, 16, 128),
    (ABS_RX, -32768, 32767, 16, 128),
    (ABS_RY, -32768, 32767, 16, 128),
    (ABS_Z, 0, 255, 0, 0),
    (ABS_RZ, 0, 255, 0, 0),
    (ABS_HAT0X, -1, 1, 0, 0),
    (ABS_HAT0Y, -1, 1, 0, 0),
];

/// `_IOC` from <asm-generic/ioctl.h> (x86, ARM, RISC-V; not MIPS/PowerPC/SPARC).
const fn ioc(dir: u32, nr: u32, size: usize) -> libc::Ioctl {
    ((dir << 30) | ((size as u32) << 16) | ((b'U' as u32) << 8) | nr) as libc::Ioctl
}
const WRITE: u32 = 1;
const READ: u32 = 2;
const UI_DEV_CREATE: libc::Ioctl = ioc(0, 1, 0);
const UI_DEV_SETUP: libc::Ioctl = ioc(WRITE, 3, size_of::<libc::uinput_setup>());
const UI_ABS_SETUP: libc::Ioctl = ioc(WRITE, 4, size_of::<libc::uinput_abs_setup>());
const UI_SET_EVBIT: libc::Ioctl = ioc(WRITE, 100, size_of::<libc::c_int>());
const UI_SET_KEYBIT: libc::Ioctl = ioc(WRITE, 101, size_of::<libc::c_int>());
const UI_SET_ABSBIT: libc::Ioctl = ioc(WRITE, 103, size_of::<libc::c_int>());
const UI_SET_PHYS: libc::Ioctl = ioc(WRITE, 108, size_of::<*const libc::c_char>());
const fn ui_get_sysname(len: usize) -> libc::Ioctl {
    ioc(READ, 44, len)
}

pub struct UinputBackend;

impl UinputBackend {
    pub fn new() -> anyhow::Result<Self> {
        // Each pad opens its own handle; probe once so a permission problem shows up at startup,
        // where someone may be at the terminal to fix it. A missing module and missing access
        // can both need fixing, but each fix is offered only once.
        let mut offered = Vec::new();
        loop {
            let e = match OpenOptions::new().read(true).write(true).open(UINPUT) {
                Ok(_) => return Ok(Self),
                Err(e) => e,
            };
            if offered.contains(&e.kind()) || !offer_fix(e.kind())? {
                return Err(explain(e));
            }
            offered.push(e.kind());
        }
    }
}

impl Backend for UinputBackend {
    fn name(&self) -> &'static str {
        "uinput"
    }

    fn connect(&self, slot: u8) -> anyhow::Result<Box<dyn Pad>> {
        let file = open_uinput()?;
        // The phys path tells our pads apart (evtest, udev rules) without changing what games match on.
        create_device(&file, &format!("webpad/P{}", slot + 1)).context("create uinput device")?;
        match event_node(&file) {
            Some(node) => tracing::info!("P{} plugged in as /dev/input/{node}", slot + 1),
            None => tracing::info!("P{} plugged in", slot + 1),
        }
        Ok(Box::new(UinputPad { file }))
    }
}

/// Closing the file destroys the device, which unplugs the controller.
struct UinputPad {
    file: File,
}

impl Pad for UinputPad {
    fn update(&mut self, s: &GamepadState) -> anyhow::Result<()> {
        // The kernel drops values that didn't change, so sending the full state is cheap.
        let events = events(s);
        // SAFETY: input_event is plain old data; the kernel reads whole structs from the buffer.
        let bytes = unsafe {
            std::slice::from_raw_parts(events.as_ptr().cast::<u8>(), size_of_val(events.as_slice()))
        };
        self.file.write_all(bytes)?;
        Ok(())
    }
}

fn open_uinput() -> anyhow::Result<File> {
    OpenOptions::new().read(true).write(true).open(UINPUT).map_err(explain)
}

fn explain(e: io::Error) -> anyhow::Error {
    match e.kind() {
        io::ErrorKind::PermissionDenied => anyhow::anyhow!(
            "no permission to open {UINPUT} (webpad offers a fix when started from a terminal).\n\
             Until the next reboot:  {}\n\
             Permanently (as Steam's steam-devices package does):\n  \
             echo '{RULE}' | sudo tee {RULE_PATH} && sudo udevadm control --reload && sudo udevadm trigger --sysname-match=uinput\n\
             The rule covers whoever is logged in at the machine's screen, not SSH sessions",
            temporary_access()
        ),
        io::ErrorKind::NotFound => anyhow::anyhow!("{UINPUT} does not exist; load the module with `sudo modprobe uinput`"),
        _ => anyhow::anyhow!("could not open {UINPUT}: {e}"),
    }
}

/// Grants only this user access to only /dev/uinput. The ACL lives on the device node, which
/// is recreated at boot, so nothing on the machine changes permanently.
fn temporary_access() -> String {
    // SAFETY: getuid has no preconditions and cannot fail.
    format!("sudo setfacl -m u:{}:rw {UINPUT}", unsafe { libc::getuid() })
}

/// What Steam's `steam-devices` package installs: `uaccess` lets the logged-in user open uinput.
/// webpad only prints it; installing it is the user's call.
const RULE: &str = r#"KERNEL=="uinput", SUBSYSTEM=="misc", TAG+="uaccess", OPTIONS+="static_node=uinput""#;
const RULE_PATH: &str = "/etc/udev/rules.d/60-webpad.rules";

/// If someone is at the terminal, explain the problem, show the command that fixes it until the
/// next reboot and run it when they agree. Returns whether a fix ran, so the caller can retry.
fn offer_fix(kind: io::ErrorKind) -> anyhow::Result<bool> {
    use std::io::{BufRead, IsTerminal};

    let (problem, command) = match kind {
        io::ErrorKind::PermissionDenied => (
            "webpad needs permission to create virtual controllers (/dev/uinput).\n\
             This gives your user access until the next reboot; nothing is changed permanently",
            temporary_access(),
        ),
        io::ErrorKind::NotFound => (
            "/dev/uinput is missing because the uinput kernel module isn't loaded.\n\
             This loads it until the next reboot",
            "sudo modprobe uinput".to_owned(),
        ),
        _ => return Ok(false),
    };
    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        return Ok(false);
    }
    eprint!("\n{problem}:\n\n  {command}\n\nRun it now? [Y/n] ");
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    if answer.trim().to_lowercase().starts_with('n') {
        eprintln!();
        return Ok(false);
    }
    let mut words = command.split(' ');
    let status = std::process::Command::new(words.next().unwrap_or_default())
        .args(words)
        .status()
        .context("could not run sudo; run the command above as root instead")?;
    anyhow::ensure!(status.success(), "`{command}` failed ({status})");
    eprintln!();
    Ok(true)
}

fn ioctl<T>(file: &File, request: libc::Ioctl, arg: T, what: &str) -> anyhow::Result<()> {
    // SAFETY: each call site passes the argument type its request is defined with.
    if unsafe { libc::ioctl(file.as_raw_fd(), request, arg) } < 0 {
        return Err(io::Error::last_os_error()).context(what.to_owned());
    }
    Ok(())
}

fn create_device(file: &File, phys: &str) -> anyhow::Result<()> {
    ioctl(file, UI_SET_EVBIT, EV_KEY as libc::c_int, "enable keys")?;
    for (_, key) in KEYS {
        ioctl(file, UI_SET_KEYBIT, key as libc::c_int, "add key")?;
    }
    ioctl(file, UI_SET_EVBIT, EV_ABS as libc::c_int, "enable axes")?;
    for (code, minimum, maximum, fuzz, flat) in AXES {
        ioctl(file, UI_SET_ABSBIT, code as libc::c_int, "add axis")?;
        // SAFETY: all-zero is a valid uinput_abs_setup.
        let mut setup: libc::uinput_abs_setup = unsafe { std::mem::zeroed() };
        setup.code = code;
        setup.absinfo.minimum = minimum;
        setup.absinfo.maximum = maximum;
        setup.absinfo.fuzz = fuzz;
        setup.absinfo.flat = flat;
        ioctl(file, UI_ABS_SETUP, &setup as *const libc::uinput_abs_setup, "set up axis")?;
    }
    let phys = CString::new(phys)?;
    ioctl(file, UI_SET_PHYS, phys.as_ptr(), "set phys")?;

    // SAFETY: all-zero is a valid uinput_setup.
    let mut setup: libc::uinput_setup = unsafe { std::mem::zeroed() };
    setup.id = libc::input_id { bustype: 0x03 /* BUS_USB */, vendor: 0x045e, product: 0x028e, version: 0x0110 };
    for (dst, src) in setup.name.iter_mut().zip(b"Microsoft X-Box 360 pad") {
        *dst = *src as libc::c_char;
    }
    ioctl(file, UI_DEV_SETUP, &setup as *const libc::uinput_setup, "device setup")?;
    ioctl(file, UI_DEV_CREATE, 0 as libc::c_int, "create device")
}

/// The `eventN` node udev made for this device, for the log line.
fn event_node(file: &File) -> Option<String> {
    let mut name = [0u8; 64];
    // SAFETY: the kernel writes at most `name.len()` bytes into the buffer.
    if unsafe { libc::ioctl(file.as_raw_fd(), ui_get_sysname(name.len()), name.as_mut_ptr()) } < 0 {
        return None;
    }
    let sysname = std::ffi::CStr::from_bytes_until_nul(&name).ok()?.to_str().ok()?;
    std::fs::read_dir(format!("/sys/devices/virtual/input/{sysname}"))
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|entry| entry.starts_with("event"))
}

fn event(type_: u16, code: u16, value: i32) -> libc::input_event {
    // SAFETY: all-zero is a valid input_event; a zero timestamp makes the kernel stamp it.
    let mut event: libc::input_event = unsafe { std::mem::zeroed() };
    event.type_ = type_;
    event.code = code;
    event.value = value;
    event
}

fn events(s: &GamepadState) -> Vec<libc::input_event> {
    let pressed = |bit: u16| (s.buttons & bit != 0) as i32;
    let mut events: Vec<_> = KEYS.iter().map(|&(bit, key)| event(EV_KEY, key, pressed(bit))).collect();
    // evdev's Y axes point down. `!` maps 32767 → -32768 and -32768 → 32767, like xpad does.
    events.extend([
        event(EV_ABS, ABS_X, s.lx as i32),
        event(EV_ABS, ABS_Y, !s.ly as i32),
        event(EV_ABS, ABS_RX, s.rx as i32),
        event(EV_ABS, ABS_RY, !s.ry as i32),
        event(EV_ABS, ABS_Z, s.lt as i32),
        event(EV_ABS, ABS_RZ, s.rt as i32),
        event(EV_ABS, ABS_HAT0X, pressed(buttons::DPAD_RIGHT) - pressed(buttons::DPAD_LEFT)),
        event(EV_ABS, ABS_HAT0Y, pressed(buttons::DPAD_DOWN) - pressed(buttons::DPAD_UP)),
        event(EV_SYN, 0 /* SYN_REPORT */, 0),
    ]);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(events: &[libc::input_event], type_: u16, code: u16) -> i32 {
        events.iter().find(|e| e.type_ == type_ && e.code == code).unwrap().value
    }

    #[test]
    fn y_axes_flip_and_dpad_becomes_hat() {
        let s = GamepadState {
            buttons: buttons::DPAD_UP | buttons::DPAD_LEFT | buttons::A,
            ly: i16::MAX,
            ry: i16::MIN,
            lx: -100,
            ..Default::default()
        };
        let ev = events(&s);
        assert_eq!(value(&ev, EV_ABS, ABS_Y), i16::MIN as i32);
        assert_eq!(value(&ev, EV_ABS, ABS_RY), i16::MAX as i32);
        assert_eq!(value(&ev, EV_ABS, ABS_X), -100);
        assert_eq!(value(&ev, EV_ABS, ABS_HAT0X), -1);
        assert_eq!(value(&ev, EV_ABS, ABS_HAT0Y), -1);
        assert_eq!(value(&ev, EV_KEY, BTN_SOUTH), 1);
        assert_eq!(value(&ev, EV_KEY, BTN_EAST), 0);
        assert_eq!(ev.last().unwrap().type_, EV_SYN);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn ioctl_numbers_match_the_kernel_headers() {
        // Values from <linux/uinput.h> on x86_64.
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_SETUP, 0x405c5503);
        assert_eq!(UI_ABS_SETUP, 0x401c5504);
        assert_eq!(UI_SET_EVBIT, 0x40045564);
        assert_eq!(UI_SET_PHYS, 0x4008556c);
    }
}
