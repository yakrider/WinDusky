#![ allow (dead_code) ]

// todo .. Note : some of these are from github.com/mlaily/NegativeScreen .. and that has GPL

use windows::Win32::UI::Magnification::MAGCOLOREFFECT;


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
     0.3333333, -0.6666667, -0.6666667,  0.0,  0.0,
    -0.6666667,  0.3333333, -0.6666667,  0.0,  0.0,
    -0.6666667, -0.6666667,  0.3333333,  0.0,  0.0,
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
     1.0895080, -0.9326327, -0.9326330,  0.0,  0.0,
    -1.8177180,  0.1683074, -1.8416920,  0.0,  0.0,
    -0.2445895, -0.2478156,  1.7621850,  0.0,  0.0,
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
   -180.0, -180.0,  -180.0,  0.0,  1.0,
] };
