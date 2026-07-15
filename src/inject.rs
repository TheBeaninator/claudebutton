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

use crate::keys;

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

    /// Emit a single key event (press or release), each its own synced report.
    fn emit1(&mut self, code: u16, val: i32) -> Result<()> {
        self.dev.emit(&[InputEvent::new(EventType::KEY, code, val)])?;
        Ok(())
    }

    /// Press and release one key code.
    pub fn tap(&mut self, code: u16) -> Result<()> {
        self.emit1(code, 1)?;
        sleep(Duration::from_millis(10));
        self.emit1(code, 0)?;
        Ok(())
    }

    /// Type a run of text as individual key taps, holding Shift for characters
    /// that need it. '\n' / '\r' become Enter; characters we can't map are
    /// skipped rather than aborting the whole macro.
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                self.tap(Key::KEY_ENTER.code())?;
                continue;
            }
            let Some((code, shift)) = keys::char_key(ch) else {
                continue;
            };
            if shift {
                self.emit1(Key::KEY_LEFTSHIFT.code(), 1)?;
                sleep(Duration::from_millis(4));
                self.tap(code)?;
                self.emit1(Key::KEY_LEFTSHIFT.code(), 0)?;
                sleep(Duration::from_millis(4));
            } else {
                self.tap(code)?;
            }
        }
        Ok(())
    }
}
