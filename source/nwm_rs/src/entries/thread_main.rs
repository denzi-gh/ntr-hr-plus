use crate::*;

struct ThreadsStacks<'a> {
    aux1: &'a mut StackRegion<{ RP_THREAD_STACK_SIZE as usize }>,
    aux2: &'a mut StackRegion<{ RP_THREAD_STACK_SIZE as usize }>,
    nwm: &'a mut StackRegion<{ STACK_SIZE as usize }>,
    screen: &'a mut StackRegion<{ STACK_SIZE as usize }>,
}

pub type NwmBufs = RangedArray<*mut u8, WORK_COUNT>;

struct ThreadsStorage<'a> {
    stacks: ThreadsStacks<'a>,
    nwm_bufs: NwmBufs,
}

pub static mut THREAD_MAIN_HANDLE: Handle = 0;

static mut RESTART_SHUTDOWN: AtomicBool = const_default();
static mut RESTART_PENDING: AtomicBool = const_default();
static mut RESTART_READY_EVENT: Handle = const_default();
static mut RESTART_DONE_EVENT: Handle = const_default();

pub const NWM_BUFFER_SIZE: usize =
    (SEND_BUFS_SIZE / WORK_COUNT) as usize / mem::size_of::<usize>() * mem::size_of::<usize>();

fn once_jpeg() -> Option<()> {
    let jpeg = request_mem_from_pool::<{ mem::size_of::<jpeg::Jpeg>() }>()?;
    unsafe {
        jpeg::JPEG = jpeg.to_ptr() as *mut jpeg::Jpeg;
        let jpeg = &mut *jpeg::JPEG;

        jpeg.once();
    }

    Some(())
}

#[named]
fn once<'a>() -> Option<ThreadsStorage<'a>> {
    let res = unsafe { gspInit(1) };
    if res != 0 {
        ns_dbg_print!(failed, c_str!("GSP init"), res);
        return None;
    }

    let res = create_event(unsafe { &mut RESTART_READY_EVENT });
    if res != 0 {
        ns_dbg_print!(failed, c_str!("Create restart ready event"), res);
        return None;
    }
    let res = create_event(unsafe { &mut RESTART_DONE_EVENT });
    if res != 0 {
        ns_dbg_print!(failed, c_str!("Create restart done event"), res);
        return None;
    }

    let gf256_ctx_mem = request_mem_from_pool::<{ mem::size_of::<gf256_ctx>() }>()?;
    unsafe { GF256Ctx = gf256_ctx_mem.to_ptr() as *mut gf256_ctx };

    if unsafe { fecal_init_(FECAL_VERSION as i32) } != 0 {
        ns_dbg_print!(failed, c_str!("FEC-AL init"), res);
        return None;
    }

    let fecal_encoder_mem = request_mem_from_pool_vsize(unsafe { fecal_encoder_size() } as usize)?;
    unsafe { rp_kcp_fecal_encoder = fecal_encoder_mem.as_mut_ptr() as *mut _ };

    unsafe { rp_svc_increase_limits() };

    unsafe { entries::thread_nwm::once_reliable_stream_cb() }?;

    let mut nwm_bufs: NwmBufs = const_default();
    unsafe {
        let cb = &mut *entries::thread_nwm::RELIABLE_STREAM_CB;

        let m = cb.locked.send_bufs.as_mut_ptr().as_mut_ptr();
        for i in WorkIndex::all() {
            *i.index_into_mut(nwm_bufs.arr()) = m.add(NWM_BUFFER_SIZE * i.get() as usize);
        }
    }

    unsafe { entries::thread_screen::once_img_infos() }?;

    if once_jpeg() == None {
        ns_dbg_print!(failed, c_str!("JPEG init"), res);
        return None;
    }

    unsafe { entries::thread_screen::once_screen_handles() }?;

    let aux1_stack = request_mem_from_pool::<{ RP_THREAD_STACK_SIZE as usize }>()?;
    let aux2_stack = request_mem_from_pool::<{ RP_THREAD_STACK_SIZE as usize }>()?;
    let nwm_stack = request_mem_from_pool::<{ STACK_SIZE as usize }>()?;
    let screen_stack = request_mem_from_pool::<{ STACK_SIZE as usize }>()?;

    let mut svc_thread: Handle = 0;
    let res = create_thread_from_pool::<{ SMALL_STACK_SIZE as usize }>(
        &mut svc_thread,
        Some(handlePortThread),
        SVC_PORT_NWM.as_ptr() as u32,
        0x10,
        1,
    )
    .0;
    if res != 0 {
        ns_dbg_print!(failed, c_str!("Create remote play service thread"), res);
    }

    ns_dbg_print!(mem_usage, unsafe { plgGetMemoryUsage() });

    Some(ThreadsStorage {
        stacks: ThreadsStacks {
            aux1: stack_region_from_mem_region(aux1_stack),
            aux2: stack_region_from_mem_region(aux2_stack),
            nwm: stack_region_from_mem_region(nwm_stack),
            screen: stack_region_from_mem_region(screen_stack),
        },
        nwm_bufs,
    })
}

struct InitVars {
    core_count: CoreCount,
    thread_prio: u32,
    qos: u32,
}

struct Init {
    vars: InitVars,
}

impl Init {
    fn init(vars: InitVars) -> Option<Self> {
        unsafe {
            init_syn_handles(vars.core_count)?;
            entries::thread_nwm::init_seg_mem_handles(vars.qos)?;
        }

        Some(Init { vars })
    }
}

impl Drop for Init {
    fn drop(&mut self) {
        unsafe {
            entries::thread_nwm::cleanup_seg_mem_handles();
            cleanup_syn_handles(self.vars.core_count);
        }
    }
}

struct Impl(());

#[named]
fn pause() -> Option<()> {
    unsafe {
        if RESTART_PENDING.load(Ordering::Acquire) {
            RESTART_PENDING.store(false, Ordering::Release);

            let res = svcSignalEvent(RESTART_READY_EVENT);
            if res != 0 {
                ns_dbg_print!(failed, c_str!("Signal restart event"), res);
            }

            if RESTART_SHUTDOWN.load(Ordering::Acquire) {
                return None;
            }

            let res = svcWaitSynchronization(RESTART_DONE_EVENT, -1);
            if res != 0 {
                ns_dbg_print!(failed, c_str!("Wait restart event"), res);
            }
        }
    }
    Some(())
}

#[named]
fn init(nwm_bufs: &NwmBufs) -> Option<Init> {
    clear_reset_threads();

    unsafe {
        set_core_count_in_use(RP_CONFIG.core_count().load(Ordering::Acquire));
        let core_count = core_count_in_use();

        let dst_port = RP_CONFIG.dst_port().load(Ordering::Acquire);
        let dst_flags = dst_port & 0xffff0000;
        let dst_port = dst_port & 0xffff;
        if dst_port == 0 {
            RP_CONFIG
                .dst_port()
                .store(RP_DST_PORT_DEFAULT | dst_flags, Ordering::Release)
        }

        let qos = RP_CONFIG.qos().load(Ordering::Acquire);
        entries::thread_nwm::init(dst_flags, qos)?;

        let mode = RP_CONFIG.mode().load(Ordering::Acquire);
        entries::thread_screen::init(mode);

        let thread_prio = RP_CONFIG.thread_prio().load(Ordering::Acquire);
        let res = svcSetThreadPriority(THREAD_MAIN_HANDLE, thread_prio as i32);
        if res != 0 {
            ns_dbg_print!(failed, c_str!("Set thread priority"), res);
        }

        entries::thread_nwm::init_reliable_stream_cb(qos)?;

        let jpeg = &mut *jpeg::JPEG;
        let quality = RP_CONFIG.quality().load(Ordering::Acquire);
        let chroma_ss = [
            RP_CONFIG
                .chroma_ss(ScreenIndex::init(RP_SCREEN_TOP as u32))
                .load(Ordering::Acquire),
            RP_CONFIG
                .chroma_ss(ScreenIndex::init(RP_SCREEN_BOT as u32))
                .load(Ordering::Acquire),
        ];
        let downsample = if entries::thread_nwm::get_reliable_stream()
            == entries::thread_nwm::ReliableStream::None
        {
            [0, 0]
        } else {
            [
                RP_CONFIG
                    .downsample(ScreenIndex::init(RP_SCREEN_TOP as u32))
                    .load(Ordering::Acquire),
                RP_CONFIG
                    .downsample(ScreenIndex::init(RP_SCREEN_BOT as u32))
                    .load(Ordering::Acquire),
            ]
        };
        let quality = [
            jpeg::downsample_quality_scale(downsample[RP_SCREEN_TOP as usize] as u8, quality),
            jpeg::downsample_quality_scale(downsample[RP_SCREEN_BOT as usize] as u8, quality),
        ];
        jpeg.init(
            quality,
            core_count,
            chroma_ss,
            downsample,
            entries::thread_nwm::get_reliable_stream() != entries::thread_nwm::ReliableStream::None,
            entries::thread_nwm::get_reliable_stream_delta_prog(),
        )?;
        entries::work_thread::init(quality, chroma_ss, downsample);

        entries::thread_nwm::init_nwm_infos(nwm_bufs, core_count);

        entries::thread_nwm::init_ov_stats();

        Init::init(InitVars {
            core_count,
            thread_prio,
            qos,
        })
    }
}

#[named]
fn main(_impl_: Impl, s: &mut ThreadsStorage) -> Option<()> {
    pause()?;

    let init = init(&s.nwm_bufs)?;
    let core_count = init.vars.core_count.get();
    {
        let _aux1 = if core_count >= 2 {
            Some(JoinThread::create(CreateThread::create(
                Some(entries::thread_aux::thread_aux),
                1,
                s.stacks.aux1,
                init.vars.thread_prio as i32,
                3,
            )?))
        } else {
            None
        };

        let _aux2 = if core_count >= 3 {
            Some(JoinThread::create(CreateThread::create(
                Some(entries::thread_aux::thread_aux),
                2,
                s.stacks.aux2,
                RP_THREAD_PRIO_MAX as s32,
                1,
            )?))
        } else {
            None
        };

        let _nwm = JoinThread::create(CreateThread::create(
            Some(match entries::thread_nwm::get_reliable_stream() {
                entries::thread_nwm::ReliableStream::None => entries::thread_nwm::thread_nwm,
                entries::thread_nwm::ReliableStream::KCP => entries::thread_nwm::kcp_thread_nwm,
            }),
            0,
            s.stacks.nwm,
            RP_THREAD_PRIO_MIN as s32,
            2,
        )?);

        let _screen = JoinThread::create(CreateThread::create(
            Some(entries::thread_screen::thread_screen),
            0,
            s.stacks.screen,
            RP_THREAD_PRIO_MIN as s32,
            2,
        )?);

        unsafe {
            rp_svc_print_limits();
        }

        let t = ThreadIndex::init(0);
        unsafe {
            entries::work_thread::work_thread_loop(t);
        }
        set_reset_threads();
    }

    ns_dbg_print!(msg, c_str!("Nwm main loop restarted"));
    Some(())
}

fn main_loop(s: &mut ThreadsStorage) -> Option<()> {
    loop {
        main(Impl(()), s)?
    }
}

#[named]
pub extern "C" fn encode_thread_main(_: *mut c_void) {
    unsafe {
        __system_initSyscalls();
        if let Some(mut storage) = once() {
            main_loop(&mut storage);
        }
        ns_dbg_print!(msg, c_str!("Nwm main loop exited"));
        svcExitThread()
    }
}

#[unsafe(no_mangle)]
#[named]
extern "C" fn nwmPause(shutdown: bool) {
    unsafe { RESTART_SHUTDOWN.store(shutdown, Ordering::Release) };
    unsafe { RESTART_PENDING.store(true, Ordering::Release) };
    set_reset_threads();
    let res = unsafe { svcWaitSynchronization(RESTART_READY_EVENT, -1) };
    if res != 0 {
        ns_dbg_print!(failed, c_str!("Wait restart event"), res);
    }
}

#[unsafe(no_mangle)]
#[named]
extern "C" fn nwmUnpause() {
    let res = unsafe { svcSignalEvent(RESTART_DONE_EVENT) };
    if res != 0 {
        ns_dbg_print!(failed, c_str!("Signal restart event"), res);
    }
}
