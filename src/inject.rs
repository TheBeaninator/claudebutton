//! Shared uinput virtual keyboard. Declares a FULL key range so libinput/X treat
//! it as a real keyboard and deliver its events to the focused window (a sparse
//! device gets misclassified and dropped).

use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, EventType, InputEvent, Key,
};

pub struct Injector {
    dev: VirtualDevice,
}

impl Injector {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<Key>::new();
        for c in 1..128u16 {
            keys.insert(Key::new(c));
        }
        let dev = VirtualDeviceBuilder::new()?
            .name("claudebutton")
            .with_keys(&keys)?
            .build()?;
        // Give udev/X a moment to enumerate the new keyboard before first event.
        sleep(Duration::from_millis(1000));
        Ok(Self { dev })
    }

    /// Press and release one key code.
    pub fn tap(&mut self, code: u16) -> Result<()> {
        self.dev.emit(&[InputEvent::new(EventType::KEY, code, 1)])?;
        sleep(Duration::from_millis(10));
        self.dev.emit(&[InputEvent::new(EventType::KEY, code, 0)])?;
        Ok(())
    }
}
