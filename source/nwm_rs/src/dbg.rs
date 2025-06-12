use crate::*;

// From c_str_macro crate
macro_rules! c_str {
    ($lit:expr) => {
        (concat!($lit, "\0").as_ptr() as *const c_char)
    };
}

macro_rules! nsDbgPrint {
    ($fn:ident $(, $es:expr)*) => {
        nsDbgPrint_t::$fn(
            c_str!(module_path!()),
            line!() as c_int,
            c_str!(function_name!())
            $(, $es)*
        )
    };
}

macro_rules! nsDbgPrint_fn {
    ($fn:ident, $fmt:expr $(, $vn:ident: $ty:ty)*) => {
        pub fn $fn(
            file_name: *const c_char,
            line_number: c_int,
            func_name: *const c_char
            $(, $vn: $ty)*
        ) {
            unsafe {
                nsDbgPrintVerbose(
                    file_name,
                    line_number,
                    func_name,
                    c_str!($fmt)
                    $(, $vn)*
                );
            }
        }
    };
}

pub struct nsDbgPrint_t {
    _z: (),
}

#[allow(dead_code)]
impl nsDbgPrint_t {
    nsDbgPrint_fn!(trace, "Tracing...\n");

    nsDbgPrint_fn!(int, "%s: %d\n", name: *const c_char, num: s32);

    nsDbgPrint_fn!(convSampPos, "xpos: %d, ypos: %d\n", xpos: s32, ypos: s32);

    nsDbgPrint_fn!(expBits, "num: %d, exp: %d\n", num: s32, exp: s32);

    nsDbgPrint_fn!(deltaProgQ, "Delta prog q: %d, projected size: %d, desired size: %d, comp count: %d\n", q: s32, projected_size: s32, desired_size: s32, comp_n: s32);

    nsDbgPrint_fn!(memUsage, "Mem usage: %08x\n", size: u32_);

    nsDbgPrint_fn!(mainLoopExit, "Nwm main loop exited\n");

    nsDbgPrint_fn!(mainLoopReset, "Nwm main loop restarted\n");

    nsDbgPrint_fn!(singalPortFailed, "Signal port event failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(openProcessFailed, "Open process failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(copyRemoteMemoryFailed, "Copy remote memory failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(gspInitFailed, "GSP init failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(fecalInitFailed, "FEC-AL init failed\n");

    nsDbgPrint_fn!(initJpegFailed, "JPEG init failed\n");

    nsDbgPrint_fn!(createEventFailed, "Create %s event failed %08x\n", name: *const c_char, ret: s32);

    nsDbgPrint_fn!(createRestartEventFailed, "Create restart event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(signalRestartEventFailed, "Signal restart event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(waitRestartEventFailed, "Wait restart event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(createScreenMutexFailed, "Create screen handles mutex failed %08x\n", ret: s32);

    nsDbgPrint_fn!(createPortEventFailed, "Create port event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(createNwmEventFailed, "Create nwm event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(createNwmMutexFailed, "Create nwm mutex failed %08x\n", ret: s32);

    nsDbgPrint_fn!(createNwmRecvEventFailed, "Create nwm recv event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(createNwmSvcFailed, "Create remote play service thread failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(setThreadPriorityFailed, "Set thread priority failed: %08x\n", res: s32);

    nsDbgPrint_fn!(createMutexFailed, "Create %s mutex failed %08x\n", name: *const c_char, ret: s32);

    nsDbgPrint_fn!(createSemaphoreFailed, "Create %s semaphore failed: %08x\n", name: *const c_char, res: s32);

    nsDbgPrint_fn!(releaseSemaphoreFailed, "Release %s semaphore failed (%x): %08x\n", name: *const c_char, w: u32_, res: s32);

    nsDbgPrint_fn!(releaseMutexFailed, "Release %s mutex failed: %08x\n", name: *const c_char, res: s32);

    nsDbgPrint_fn!(allocFailed, "Bad alloc, size: %x/%x\n", total_size: u32_, alloc_size: u32_);

    nsDbgPrint_fn!(sendBufferOverflow, "Send buffer overflow\n");

    nsDbgPrint_fn!(waitNwmEventSyncFailed, "Sync wait nwm event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(waitNwmEventClearFailed, "Clear wait nwm event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(waitNwmEventSignalFailed, "Signal wait nwm event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(nwmEventSignalFailed, "Signal nwm event failed %08x\n", ret: s32);

    nsDbgPrint_fn!(waitForSyncFailed, "Wait for %s sync failed: %08x\n", name: *const c_char, res: s32);

    nsDbgPrint_fn!(encodeMcuFailed, "Encode MCUs failed, restarting...\n");

    nsDbgPrint_fn!(nwmOutputOverflow, "Nwm output packet len overflow: %08x\n", len: s32);

    nsDbgPrint_fn!(nwmInputNothing, "Nwm input nothing\n");

    nsDbgPrint_fn!(nwmInputFailed, "Nwm input failed: %08x, errno = %08x\n", ret: s32, errno: s32);

    nsDbgPrint_fn!(kcpInputFailed, "KCP input failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(kcpSendFailed, "KCP send failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(kcpFlushFailed, "KCP flush failed: %08x\n", ret: s32);

    nsDbgPrint_fn!(kcpTimeout, "KCP timeout\n");

    nsDbgPrint_fn!(mpInitFailed, "Mem pool %s init failed\n", name: *const c_char);

    nsDbgPrint_fn!(rpSynInitFailed, "Nwm syn init failed\n");

    nsDbgPrint_fn!(mpAllocFailed, "Mem pool %s alloc failed\n", name: *const c_char);

    nsDbgPrint_fn!(mpFreeFailed, "Mem pool %s free failed\n", name: *const c_char);
}
