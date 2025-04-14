#![ allow (dead_code) ]

// todo .. Note : some of these are from github.com/mlaily/NegativeScreen .. and that has GPL

use std::sync::atomic::{AtomicUsize, Ordering};
use once_cell::sync::Lazy;
use windows::Win32::UI::Magnification::MAGCOLOREFFECT;


// Simple Inversion
pub const COLOR_EFF__IDENTITY: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.0,  0.0,  0.0,  0.0,  0.0,
     0.0,  1.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Simple Inversion
pub const COLOR_EFF__SIMPLE_INVERSION: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -1.0,  0.0,  0.0,  0.0,  0.0,
     0.0, -1.0,  0.0,  0.0,  0.0,
     0.0,  0.0, -1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,  1.0,
] };

// Smart Inversion
pub const COLOR_EFF__SMART_INVERSION: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.333333,  -0.666666,  -0.666666,   0.0,  0.0,
    -0.666666,   0.333333,  -0.666666,   0.0,  0.0,
    -0.666666,  -0.666666,   0.333333,   0.0,  0.0,
     0.0,        0.0,        0.0,        1.0,  0.0,
     1.0,        1.0,        1.0,        0.0,  1.0,
] };

// Smart Inversion Alt 1
pub const COLOR_EFF__SMART_INVERSION_ALT1: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.0, -1.0, -1.0,  0.0,  0.0,
    -1.0,  1.0, -1.0,  0.0,  0.0,
    -1.0, -1.0,  1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,  1.0,
] };

// Smart Inversion Alt 2
pub const COLOR_EFF__SMART_INVERSION_ALT2: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.39, -0.62, -0.62,  0.0,  0.0,
    -1.21, -0.22, -1.22,  0.0,  0.0,
    -0.16, -0.16,  0.84,  0.0,  0.0,
     0.0,   0.0,   0.0,   1.0,  0.0,
     1.0,   1.0,   1.0,   0.0,  1.0,
] };

// Smart Inversion Alt 3
pub const COLOR_EFF__SMART_INVERSION_ALT3: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.089508,  -0.932632,  -0.932633,   0.0,  0.0,
    -1.817718,   0.168307,  -1.841692,   0.0,  0.0,
    -0.244589,  -0.247815,   1.762185,   0.0,  0.0,
     0.0,        0.0,        0.0,        1.0,  0.0,
     1.0,        1.0,        1.0,        0.0,  1.0,
] };

// Smart Inversion Alt 4
pub const COLOR_EFF__SMART_INVERSION_ALT4: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.50, -0.78, -0.78,  0.0,  0.0,
    -0.56,  0.72, -0.56,  0.0,  0.0,
    -0.94, -0.94,  0.34,  0.0,  0.0,
     0.0,   0.0,   0.0,   1.0,  0.0,
     1.0,   1.0,   1.0,   0.0,  1.0,
] };

// Negative Sepia
pub const COLOR_EFF__NEGATIVE_SEPIA: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.393, -0.349, -0.272,  0.0,  0.0,
    -0.769, -0.686, -0.534,  0.0,  0.0,
    -0.189, -0.168, -0.131,  0.0,  0.0,
     0.0,    0.0,    0.0,    1.0,  0.0,
     1.351,  1.203,  0.937,  0.0,  1.0,
] };

// Negative Grayscale
pub const COLOR_EFF__NEGATIVE_GRAYSCALE: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.3, -0.3, -0.3,  0.0,  0.0,
    -0.6, -0.6, -0.6,  0.0,  0.0,
    -0.1, -0.1, -0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,  1.0,
] };

// Negative Red
pub const COLOR_EFF__NEGATIVE_RED: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.3,  0.0,  0.0,  0.0,  0.0,
    -0.6,  0.0,  0.0,  0.0,  0.0,
    -0.1,  0.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  0.0,  0.0,  0.0,  1.0,
] };

// Red
pub const COLOR_EFF__RED: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.3,  0.0,  0.0,  0.0,  0.0,
     0.6,  0.0,  0.0,  0.0,  0.0,
     0.1,  0.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Negative Green
pub const COLOR_EFF__NEGATIVE_CYAN: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.0, -0.3, -0.3,  0.0,  0.0,
     0.0, -0.6, -0.6,  0.0,  0.0,
     0.0, -0.1, -0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  1.0,  1.0,  0.0,  1.0,
] };

// Green
pub const COLOR_EFF__CYAN: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.0,  0.3,  0.3,  0.0,  0.0,
     0.0,  0.6,  0.6,  0.0,  0.0,
     0.0,  0.1,  0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Grayscale
pub const COLOR_EFF__GRAYSCALE: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.3,  0.3,  0.3,  0.0,  0.0,
     0.6,  0.6,  0.6,  0.0,  0.0,
     0.1,  0.1,  0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Binary (Black and White)
pub const COLOR_EFF__BLACK_AND_WHITE: MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    127.0,  127.0,   127.0,  0.0,  0.0,
    127.0,  127.0,   127.0,  0.0,  0.0,
    127.0,  127.0,   127.0,  0.0,  0.0,
      0.0,    0.0,     0.0,  1.0,  0.0,
     -0.5,   -0.5,    -0.5,  1.0,  1.0,
] };


pub const COLOR_EFFECTS : [MAGCOLOREFFECT; 15] = [
    COLOR_EFF__IDENTITY,
    COLOR_EFF__SIMPLE_INVERSION,
    COLOR_EFF__SMART_INVERSION,
    COLOR_EFF__SMART_INVERSION_ALT1,
    COLOR_EFF__SMART_INVERSION_ALT2,
    COLOR_EFF__SMART_INVERSION_ALT3,
    COLOR_EFF__SMART_INVERSION_ALT4,
    COLOR_EFF__NEGATIVE_SEPIA,
    COLOR_EFF__NEGATIVE_GRAYSCALE,
    COLOR_EFF__NEGATIVE_RED,
    COLOR_EFF__RED,
    COLOR_EFF__NEGATIVE_CYAN,
    COLOR_EFF__CYAN,
    COLOR_EFF__GRAYSCALE,
    COLOR_EFF__BLACK_AND_WHITE,
];


pub struct ColorEffectsCycler (AtomicUsize);

pub static COLOR_EFFECTS_CYCLER : Lazy <ColorEffectsCycler> = Lazy::new (ColorEffectsCycler::new);

impl ColorEffectsCycler {

    pub fn new () -> ColorEffectsCycler {
        ColorEffectsCycler (AtomicUsize::new(0))
    }
    pub fn cycle_next (&self) -> MAGCOLOREFFECT {
        let cur = self.0.load(Ordering::Acquire);
        let idx = (cur + 1) % COLOR_EFFECTS.len();
        self.0.store (idx, Ordering::Release);
        COLOR_EFFECTS [idx]
    }
    pub fn cycle_prev (&self) -> MAGCOLOREFFECT {
        let cur = self.0.load(Ordering::Acquire);
        let idx = (COLOR_EFFECTS.len() + cur - 1) % (COLOR_EFFECTS.len());
        self.0.store (idx, Ordering::Release);
        COLOR_EFFECTS [idx]
    }

}
