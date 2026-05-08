#![cfg_attr(not(feature = "std"), no_std)]

slint::include_modules!();

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Shared parameter block usable in both `std` (desktop) and `no_std` (MCU)
/// builds. Floats are stored as `AtomicU32` via bit-cast so the same type
/// compiles without `std`'s `Mutex`.
pub struct Params {
    gain:   AtomicU32,
    delay:  AtomicU32,
    reverb: AtomicU32,
    bypass: AtomicBool,
}

impl Params {
    pub const fn new() -> Self {
        Self {
            gain:   AtomicU32::new(0x3F000000), // 0.5
            delay:  AtomicU32::new(0x3E800000), // 0.25
            reverb: AtomicU32::new(0x3E99999A), // ~0.3
            bypass: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_gain(&self, v: f32) {
        self.gain.store(v.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn delay(&self) -> f32 {
        f32::from_bits(self.delay.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_delay(&self, v: f32) {
        self.delay.store(v.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn reverb(&self) -> f32 {
        f32::from_bits(self.reverb.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_reverb(&self, v: f32) {
        self.reverb.store(v.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn bypass(&self) -> bool {
        self.bypass.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_bypass(&self, v: bool) {
        self.bypass.store(v, Ordering::Relaxed);
    }

    /// Dispatch a `(name, value)` event from the Slint `param-changed`
    /// callback to the right field. Unknown names are ignored.
    pub fn apply_named(&self, name: &str, value: f32) {
        match name {
            "gain"   => self.set_gain(value),
            "delay"  => self.set_delay(value),
            "reverb" => self.set_reverb(value),
            _ => {}
        }
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::new()
    }
}
