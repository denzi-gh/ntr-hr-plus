use crate::*;

pub extern "C" fn encode_thread_main(_: *mut c_void) {}

#[no_mangle]
extern "C" fn nwmPause(sleep: bool) {}

#[no_mangle]
extern "C" fn nwmUnpause() {}
