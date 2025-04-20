#![ allow (dead_code) ]

use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::UI::Magnification::MAGCOLOREFFECT;



// No Color Effect
pub const COLOR_EFF__IDENTITY : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.0,  0.0,  0.0,  0.0,  0.0,
     0.0,  1.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };


// Simple Inversion
pub const COLOR_EFF__SIMPLE_INVERSION : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -1.0,  0.0,  0.0,  0.0,  0.0,
     0.0, -1.0,  0.0,  0.0,  0.0,
     0.0,  0.0, -1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,  1.0,
] };

// Smart Inversion
pub const COLOR_EFF__SMART_INVERSION_V1 : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.33,  -0.66,  -0.66,  0.0,  0.0,
    -0.66,   0.33,  -0.66,  0.0,  0.0,
    -0.66,  -0.66,   0.33,  0.0,  0.0,
      0.0,    0.0,    0.0,  1.0,  0.0,
      1.0,    1.0,    1.0,  0.0,  1.0,
] };

// Smart Inversion Alt 1
pub const COLOR_EFF__SMART_INVERSION_V2 : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.0, -1.0, -1.0,  0.0,  0.0,
    -1.0,  1.0, -1.0,  0.0,  0.0,
    -1.0, -1.0,  1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,  1.0,
] };

// Smart Inversion Alt 2
pub const COLOR_EFF__SMART_INVERSION_V3 : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.39, -0.62, -0.62,  0.0,  0.0,
    -1.21, -0.22, -1.22,  0.0,  0.0,
    -0.16, -0.16,  0.84,  0.0,  0.0,
      0.0,   0.0,   0.0,  1.0,  0.0,
      1.0,   1.0,   1.0,  0.0,  1.0,
] };

// Smart Inversion Alt 3
pub const COLOR_EFF__SMART_INVERSION_V4 : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.089,  -0.932,  -0.932,   0.0,  0.0,
    -1.817,   0.168,  -1.841,   0.0,  0.0,
    -0.244,  -0.247,   1.762,   0.0,  0.0,
       0.0,     0.0,     0.0,   1.0,  0.0,
       1.0,     1.0,     1.0,   0.0,  1.0,
] };

// Smart Inversion Alt 4
pub const COLOR_EFF__SMART_INVERSION_V5 : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.50, -0.78, -0.78,  0.0,  0.0,
    -0.56,  0.72, -0.56,  0.0,  0.0,
    -0.94, -0.94,  0.34,  0.0,  0.0,
      0.0,   0.0,   0.0,  1.0,  0.0,
      1.0,   1.0,   1.0,  0.0,  1.0,
] };

// Negative Sepia
pub const COLOR_EFF__NEGATIVE_SEPIA : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.393,  -0.349,  -0.272,  0.0,  0.0,
    -0.769,  -0.686,  -0.534,  0.0,  0.0,
    -0.189,  -0.168,  -0.131,  0.0,  0.0,
       0.0,     0.0,     0.0,  1.0,  0.0,
     1.351,   1.203,   0.937,  0.0,  1.0,
] };



// Cyan
pub const COLOR_EFF__CYAN : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.0,  0.3,  0.3,  0.0,  0.0,
     0.0,  0.6,  0.6,  0.0,  0.0,
     0.0,  0.1,  0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Negative Cyan
pub const COLOR_EFF__NEGATIVE_CYAN : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.0,  -0.3,  -0.3,  0.0,  0.0,
     0.0,  -0.6,  -0.6,  0.0,  0.0,
     0.0,  -0.1,  -0.1,  0.0,  0.0,
     0.0,   0.0,   0.0,  1.0,  0.0,
     0.0,   1.0,   1.0,  0.0,  1.0,
] };


// Yellow
pub const COLOR_EFF__YELLOW : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.3,  0.3,  0.0,  0.0,  0.0,
     0.6,  0.6,  0.0,  0.0,  0.0,
     0.1,  0.1,  0.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Negative Yellow
pub const COLOR_EFF__NEGATIVE_YELLOW : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     -0.3,  -0.3,  0.0,  0.0,  0.0,
     -0.6,  -0.6,  0.0,  0.0,  0.0,
     -0.1,  -0.1,  0.0,  0.0,  0.0,
      0.0,   0.0,  0.0,  1.0,  0.0,
      1.0,   1.0,  0.0,  0.0,  1.0,
] };


// Gold
pub const COLOR_EFF__GOLD : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.40,  0.30,  0.10,  0.0,  0.0,
     0.40,  0.30,  0.10,  0.0,  0.0,
     0.20,  0.15,  0.05,  0.0,  0.0,
      0.0,   0.0,   0.0,  1.0,  0.0,
      0.0,   0.0,   0.0,  0.0,  1.0,
] };

// Negative Gold
pub const COLOR_EFF__NEGATIVE_GOLD : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.40,  -0.30,  -0.10,  0.0,  0.0,
    -0.40,  -0.30,  -0.10,  0.0,  0.0,
    -0.20,  -0.15,  -0.05,  0.0,  0.0,
      0.0,    0.0,    0.0,  1.0,  0.0,
     1.00,   0.85,   0.20,  0.0,  1.0,
] };


// Green
pub const COLOR_EFF__GREEN : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.0,  0.3,  0.0,  0.0,  0.0,
     0.0,  0.6,  0.0,  0.0,  0.0,
     0.0,  0.1,  0.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Negative Green
pub const COLOR_EFF__NEGATIVE_GREEN : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    0.0,  -0.3,  0.0,  0.0,  0.0,
    0.0,  -0.6,  0.0,  0.0,  0.0,
    0.0,  -0.1,  0.0,  0.0,  0.0,
    0.0,   0.0,  0.0,  1.0,  0.0,
    0.0,   1.0,  0.0,  0.0,  1.0,
] };


// Red
pub const COLOR_EFF__RED : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.3,  0.0,  0.0,  0.0,  0.0,
     0.6,  0.0,  0.0,  0.0,  0.0,
     0.1,  0.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Negative Red
pub const COLOR_EFF__NEGATIVE_RED : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.3,  0.0,  0.0,  0.0,  0.0,
    -0.6,  0.0,  0.0,  0.0,  0.0,
    -0.1,  0.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  0.0,  0.0,  0.0,  1.0,
] };


// Grayscale
pub const COLOR_EFF__GRAYSCALE : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     0.3,  0.3,  0.3,  0.0,  0.0,
     0.6,  0.6,  0.6,  0.0,  0.0,
     0.1,  0.1,  0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Negative Grayscale
pub const COLOR_EFF__NEGATIVE_GRAYSCALE : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -0.3, -0.3, -0.3,  0.0,  0.0,
    -0.6, -0.6, -0.6,  0.0,  0.0,
    -0.1, -0.1, -0.1,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  0.0,  1.0,
] };


// Black-and-White
pub const COLOR_EFF__BLACK_AND_WHITE : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    127.0,   127.0,   127.0,  0.0,  0.0,
    127.0,   127.0,   127.0,  0.0,  0.0,
    127.0,   127.0,   127.0,  0.0,  0.0,
      0.0,     0.0,     0.0,  1.0,  0.0,
   -180.0,  -180.0,  -180.0,  0.0,  1.0,
] };

// Negative Black-and-White
pub const COLOR_EFF__NEGATIVE_BLACK_AND_WHITE : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -127.0,  -127.0,  -127.0,  0.0,  0.0,
    -127.0,  -127.0,  -127.0,  0.0,  0.0,
    -127.0,  -127.0,  -127.0,  0.0,  0.0,
       0.0,     0.0,     0.0,  1.0,  0.0,
     180.0,   180.0,   180.0,  0.0,  1.0,
] };




pub const COLOR_EFFECTS : [MAGCOLOREFFECT; 22] = [

    COLOR_EFF__IDENTITY,

    COLOR_EFF__SIMPLE_INVERSION,
    COLOR_EFF__SMART_INVERSION_V1,
    COLOR_EFF__SMART_INVERSION_V2,
    COLOR_EFF__SMART_INVERSION_V3,
    COLOR_EFF__SMART_INVERSION_V4,
    COLOR_EFF__SMART_INVERSION_V5,
    COLOR_EFF__NEGATIVE_SEPIA,

    COLOR_EFF__NEGATIVE_CYAN,
    COLOR_EFF__NEGATIVE_GREEN,
    COLOR_EFF__NEGATIVE_RED,
    COLOR_EFF__NEGATIVE_YELLOW,
    COLOR_EFF__NEGATIVE_GOLD,
    COLOR_EFF__NEGATIVE_GRAYSCALE,
    COLOR_EFF__NEGATIVE_BLACK_AND_WHITE,

    COLOR_EFF__CYAN,
    COLOR_EFF__GREEN,
    COLOR_EFF__RED,
    COLOR_EFF__YELLOW,
    COLOR_EFF__GOLD,
    COLOR_EFF__GRAYSCALE,
    COLOR_EFF__BLACK_AND_WHITE,
];


// todo .. we gotta make these so we can externally refer to them as enums, rather than having to specify their index !!

#[derive (Debug, Copy, Clone)]
pub struct ColorEffect (pub usize);

impl Default for ColorEffect {
    fn default() -> ColorEffect { ColorEffect(4) }
}
impl ColorEffect {
    pub fn new (idx:usize) -> ColorEffect { ColorEffect(idx) }
}


#[derive (Debug, Default)]
pub struct ColorEffectAtomic (AtomicUsize);


impl ColorEffectAtomic {

    pub fn new (effect : ColorEffect) -> ColorEffectAtomic {
        ColorEffectAtomic (AtomicUsize::new (effect.0))
    }

    pub fn get (&self) -> MAGCOLOREFFECT {
        COLOR_EFFECTS [self.0.load(Ordering::Relaxed)]
    }

    pub fn cycle_next (&self) -> MAGCOLOREFFECT {
        let cur = self.0.load(Ordering::Acquire);
        let idx = (cur + 1) % COLOR_EFFECTS.len();
        self.0.store (idx, Ordering::Release);
        COLOR_EFFECTS[idx]
    }
    pub fn cycle_prev (&self) -> MAGCOLOREFFECT {
        let cur = self.0.load(Ordering::Acquire);
        let idx = (COLOR_EFFECTS.len() + cur - 1) % (COLOR_EFFECTS.len());
        self.0.store (idx, Ordering::Release);
        COLOR_EFFECTS[idx]
    }
}
