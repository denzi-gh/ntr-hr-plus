use crate::*;

mod gpu;
mod ranged;

pub use gpu::*;
pub use ranged::*;

pub const THREAD_WAIT_NS: s64 = NWM_THREAD_WAIT_NS as s64;
