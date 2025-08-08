use super::*;

pub struct Impl {
    w: WorkIndex,
    t: ThreadIndex,
}

impl Impl {
    pub fn work_ready_acquire(self) -> core::result::Result<WorkReady, WorkOtherReady> {
        unsafe {
            if !SYN_HANDLES
                .works
                .get_mut(&self.w)
                .work_ready_flag
                .swap(true, Ordering::AcqRel)
            {
                Ok(WorkReady(self))
            } else {
                Err(WorkOtherReady(self))
            }
        }
    }

    fn bctx(&self) -> &mut BlitCtx {
        unsafe { BLIT_CTXES.get_mut(&self.w) }
    }

    fn work_ready_params(&self) -> &mut entries::thread_screen::WorkReadyParams {
        unsafe { entries::thread_screen::work_ready_params(&self.w) }
    }
}

static mut CURRENT_FRAME_IDS: RangedArray<u8, SCREEN_COUNT> = const_default();
static mut LAST_FRAME_TIMINGS: RangedArray<u32, SCREEN_COUNT> = const_default();
static mut FRAME_TIMES: RangedArray<u32, SCREEN_COUNT> = const_default();
static mut LAST_ENCODED_SCREEN: ScreenIndex = const_default();

static mut LAST_ROW_LAST_N: AtomicU32 = const_default();

pub struct WorkReady(Impl);

impl WorkReady {
    pub fn init_bctx(self) -> BlitCtxInit {
        let bctx = self.0.bctx();
        let work_ready = unsafe { ptr::read_volatile(&self.0.work_ready_params()) };

        unsafe {
            *bctx = BlitCtx {
                format: work_ready.format & 0xf,
                src: entries::thread_screen::img_info(work_ready.is_top),
                frame_id: *CURRENT_FRAME_IDS.get(&is_top_index(work_ready.is_top)),
                is_top: work_ready.is_top,
                i_start: const_default(),
                i_count: const_default(),
                should_capture: AtomicBool::new(false),
            };
        }

        let format_changed = self.format_changed(bctx.is_top, bctx.format);
        BlitCtxInit(self, format_changed)
    }

    fn format_changed(&self, is_top: bool, format: u32) -> bool {
        let blit_format = unsafe { BLIT_FORMATS.get_mut(&is_top_index(is_top)) };
        if *blit_format == format {
            false
        } else {
            *blit_format = format;
            true
        }
    }
}

pub struct BlitCtxInit(WorkReady, bool);

impl BlitCtxInit {
    #[named]
    fn dma_sync(&self) {
        wait_syn(cname!(), self.0.0.work_ready_params().dma, c_str!("dma"));

        let bctx = self.0.0.bctx();
        unsafe {
            let _ =
                svcInvalidateProcessDataCache(CUR_PROCESS_HANDLE, bctx.src as u32, bctx.src_len());
        }
    }

    pub fn sync(self) -> WorkSync {
        self.dma_sync();
        WorkSync(self.0, self.1)
    }
}

pub struct WorkSync(WorkReady, bool);

impl WorkSync {
    pub fn skip_frame(self) -> core::result::Result<WorkFrame, WorkSkipFrame> {
        let is_top = self.0.0.bctx().is_top;
        let is_top_index = is_top_index(is_top);

        let timing = get_system_tick();
        let last_timing = unsafe { *LAST_FRAME_TIMINGS.get(&is_top_index) };
        let frame_time = timing.get() as u32 - last_timing;

        if frame_time >= entries::thread_screen::frame_timing_allowance() {
            entries::thread_screen::set_no_skip_frame(is_top);
        }

        if !entries::thread_screen::no_skip_frame(is_top) && !self.1 && !self.frame_changed() {
            Err(WorkSkipFrame(self.0))
        } else {
            Ok(WorkFrame(self.0, timing.get() as u32, frame_time))
        }
    }

    fn frame_changed(&self) -> bool {
        let bctx = self.0.0.bctx();
        let src_len = bctx.src_len();

        let curr = bctx.src;
        let prev = entries::thread_screen::img_info_prev(bctx.is_top);

        unsafe {
            *slice::from_raw_parts(curr, src_len as usize)
                != *slice::from_raw_parts(prev, src_len as usize)
        }
    }
}

pub struct WorkSkipFrame(WorkReady);

impl WorkSkipFrame {
    pub fn skip_frame_release(self) -> Option<WorkReady> {
        unsafe {
            entries::thread_screen::skip_frame_release(
                self.0.0.w,
                entries::thread_screen::SkipFrameParams::SkipFrame(self.0.0.t),
            );
            entries::thread_screen::thread_ready_acquire(&self.0.0.t)?;
        }
        Some(self.0)
    }
}

pub struct WorkFrame(WorkReady, u32, u32);

impl WorkFrame {
    pub fn frame_release(self) -> Option<WorkAcquire> {
        let bctx = self.0.0.bctx();
        if entries::thread_nwm::get_reliable_stream() == entries::thread_nwm::ReliableStream::None {
            if !unsafe {
                entries::thread_nwm::nwm_done_acquire(self.0.0.w, bctx.frame_id, bctx.is_top)
            } {
                return None;
            }
            unsafe {
                entries::thread_nwm::nwm_ready_release(&self.0.0.w);
            }
        }

        unsafe {
            entries::thread_screen::img_info_next(bctx.is_top);
            *CURRENT_FRAME_IDS.get_mut(&is_top_index(bctx.is_top)) += 1;
        }

        if !self.init_work() {
            return None;
        }

        unsafe {
            *LAST_FRAME_TIMINGS.get_mut(&is_top_index(bctx.is_top)) = self.1;
        }

        self.work_ready_release()
    }

    #[named]
    fn work_ready_release(self) -> Option<WorkAcquire> {
        unsafe {
            entries::thread_screen::skip_frame_release(
                self.0.0.w,
                entries::thread_screen::SkipFrameParams::Frame,
            );

            entries::thread_screen::work_done_flag_release(self.0.0.w);
        }

        unsafe {
            let bctx = self.0.0.bctx();

            let ft = AtomicU32::from_mut(FRAME_TIMES.get_mut(&is_top_index(bctx.is_top)));
            let mut cur = ft.load(Ordering::Acquire);

            let frame_time = self.2;
            loop {
                let new =
                    if cur / FRAME_TIME_MAX_F > frame_time || frame_time / FRAME_TIME_MAX_F > cur {
                        frame_time
                    } else {
                        (cur * (FRAME_TIME_FACTOR - 1) + frame_time) / FRAME_TIME_FACTOR
                    };
                match ft.compare_exchange_weak(cur, new, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => break,
                    Err(tmp) => cur = tmp,
                }
            }
        }

        for j in ThreadIndex::up_to(&thread_index_last(core_count_in_use())) {
            if j != self.0.0.t {
                unsafe {
                    release_sem(
                        cname!(),
                        SYN_HANDLES.threads.get(&j).work_ready,
                        c_str!("work_ready"),
                    );
                }
            }
        }

        Some(WorkAcquire(self.0.0))
    }

    fn init_work(&self) -> bool {
        let bctx = self.0.0.bctx();

        let w = self.0.0.w;
        let last_s = unsafe { LAST_ENCODED_SCREEN };
        let curr_s = is_top_index(bctx.is_top);

        let core_count = core_count_in_use();
        let core_count_other = core_count.get() - 1;
        let thread_index_last = thread_index_last(core_count);

        let l = unsafe { &mut LAST_ROW_LAST_N };

        true
    }
}

pub struct WorkOtherReady(Impl);

impl WorkOtherReady {
    #[named]
    pub fn acquire(self) -> Option<WorkAcquire> {
        unsafe {
            wait_syn(
                cname!(),
                SYN_HANDLES.threads.get(&self.0.t).work_ready,
                c_str!("work_ready"),
            )?;
            Some(WorkAcquire(self.0))
        }
    }
}

pub struct WorkAcquire(Impl);

pub unsafe fn work_thread_loop(t: ThreadIndex) -> Option<()> {
    let mut work_index = WorkIndex::init(0);
    loop {
        entries::thread_screen::thread_ready_acquire(&t)?;
        safe_impl::send_frame(Impl { w: work_index, t })?;
        work_index.next_wrapped();
    }
}

#[derive(ConstDefault)]
pub struct BlitCtx {
    pub format: u32,
    pub src: *mut u8,

    pub frame_id: u8,
    pub is_top: bool,

    pub i_start: RowIndices,
    pub i_count: RowIndices,

    pub should_capture: AtomicBool,
}

pub type RowIndices = RangedArray<u32, RP_CORE_COUNT_MAX>;

impl BlitCtx {
    pub fn pitch(&self) -> u32 {
        self.bpp() * self.width()
    }

    pub fn src_len(&self) -> u32 {
        self.height() * self.pitch()
    }

    pub fn width(&self) -> u32 {
        GSP_SCREEN_WIDTH
    }

    pub fn height(&self) -> u32 {
        if self.is_top {
            GSP_SCREEN_HEIGHT_TOP
        } else {
            GSP_SCREEN_HEIGHT_BOTTOM
        }
    }

    pub fn bpp(&self) -> u32 {
        let format = self.format & 0xf;
        if format == 0 {
            4
        } else if format == 1 {
            3
        } else {
            2
        }
    }
}

static mut BLIT_CTXES: RangedArray<BlitCtx, WORK_COUNT> = const_default();
static mut BLIT_FORMATS: RangedArray<u32, SCREEN_COUNT> = const_default();
