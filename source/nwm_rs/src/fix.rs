use crate::*;

pub const SCALE_BITS: u32 = 16;

pub const ONE_HALF: u32 = 1 << (SCALE_BITS - 1);

pub const fn fix(x: c_double) -> u32 {
    (x * (1 << SCALE_BITS) as c_double + 0.5) as u32
}
