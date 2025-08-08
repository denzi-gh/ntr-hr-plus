use crate::*;

pub unsafe extern "C" fn thread_aux(p: *mut c_void) {
    unsafe {
        __system_initSyscalls();
        let t = ThreadIndex::init(p as u32);
        entries::work_thread::work_thread_loop(t);
        set_reset_threads();
        svcExitThread()
    }
}
