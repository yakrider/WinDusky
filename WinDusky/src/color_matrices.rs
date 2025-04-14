#![ allow (dead_code) ]

// todo .. Note : some of these are from github.com/mlaily/NegativeScreen .. and that has GPL

use windows::Win32::UI::Magnification::MAGCOLOREFFECT;

// Grayscale transformation matrix
pub const COLOR_EFF_GRAYSCALE: MAGCOLOREFFECT = MAGCOLOREFFECT { transform : [
    //  R     G     B     A     Const
        0.3,  0.3,  0.3,  0.0,  0.0,
        0.6,  0.6,  0.6,  0.0,  0.0,
        0.1,  0.1,  0.1,  0.0,  0.0,
        0.0,  0.0,  0.0,  1.0,  0.0,
        0.0,  0.0,  0.0,  0.0,  1.0,
] };

// Red transformation matrix
pub const COLOR_EFF_RED: MAGCOLOREFFECT = MAGCOLOREFFECT { transform : [
    //  R     G     B     A     Const
        0.3,  0.0,  0.0,  0.0,  0.0,
        0.6,  0.0,  0.0,  0.0,  0.0,
        0.1,  0.0,  0.0,  0.0,  0.0,
        0.0,  0.0,  0.0,  1.0,  0.0,
        0.0,  0.0,  0.0,  0.0,  1.0,
] };

