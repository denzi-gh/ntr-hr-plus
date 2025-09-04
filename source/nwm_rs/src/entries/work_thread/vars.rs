use super::*;

pub struct Impl {
    w: WorkIndex,
    t: ThreadIndex,
}

impl Impl {
    pub fn work_ready_acquire(self) -> core::result::Result<WorkReady, WorkOtherReady> {
        #[cfg(not(feature = "o3ds"))]
        return unsafe {
            if !rp_need_core_syn!()
                || !SYN_HANDLES
                    .works
                    .get_mut(&self.w)
                    .work_ready_flag
                    .swap(true, Ordering::AcqRel)
            {
                Ok(WorkReady(self))
            } else {
                Err(WorkOtherReady(self))
            }
        };

        #[cfg(feature = "o3ds")]
        {
            return Ok(WorkReady(self));
        }
    }

    fn bctx(&self) -> &BlitCtx {
        unsafe { BLIT_CTXES.get(&self.w) }
    }

    fn bctx_mut(&self) -> &mut BlitCtx {
        unsafe { BLIT_CTXES.get_mut(&self.w) }
    }

    fn work_ready_params(&self) -> &mut entries::thread_screen::WorkReadyParams {
        unsafe { entries::thread_screen::work_ready_params(&self.w) }
    }
}

pub unsafe fn init(
    quality: [u32; RP_SCREEN_COUNT as usize],
    chroma_ss: [u32; RP_SCREEN_COUNT as usize],
    #[cfg(not(feature = "mem3"))] downsample: [u32; RP_SCREEN_COUNT as usize],
) {
    unsafe {
        LAST_ROW_LAST_N.store(0, Ordering::Release);
        LAST_ENCODED_SCREEN = ScreenIndex::init(0);
        LAST_SCREEN_LAST_ROW = 0;

        for i in ScreenIndex::all() {
            *CURRENT_FRAME_IDS.get_mut(&i) = 0;

            #[cfg(not(feature = "o3ds"))]
            {
                FRAME_TIMES
                    .get_mut(&i)
                    .store(SYSCLOCK_ARM11, Ordering::Release);
                *LAST_FRAME_TIMINGS.get_mut(&i) = get_system_tick().get() as u32;
            }
        }

        TERM_DSTS = const_default();
        #[cfg(not(feature = "o3ds"))]
        {
            TERM_INFOS = const_default();
        }
        JPEG_QUALITY = quality;
        JPEG_CHROMA_SS = chroma_ss;
        #[cfg(not(feature = "mem3"))]
        {
            JPEG_DOWNSAMPLE = downsample;
        }
        JPEG_EVEN_ODD = const_default();
    }
}

static mut CURRENT_FRAME_IDS: RangedArray<u8, SCREEN_COUNT> = const_default();
#[cfg(not(feature = "o3ds"))]
static mut LAST_FRAME_TIMINGS: RangedArray<u32, SCREEN_COUNT> = const_default();
#[cfg(not(feature = "o3ds"))]
static mut FRAME_TIMES: RangedArray<AtomicU32, SCREEN_COUNT> = const_default();
static mut LAST_ENCODED_SCREEN: ScreenIndex = const_default();
static mut LAST_SCREEN_LAST_ROW: u32 = const_default();
static mut LAST_ROW_LAST_N: AtomicU32 = const_default();

#[cfg(not(feature = "o3ds"))]
pub fn get_frame_time(s: ScreenIndex) -> &'static mut AtomicU32 {
    unsafe { FRAME_TIMES.get_mut(&s) }
}

pub struct WorkReady(Impl);

impl WorkReady {
    pub fn init_bctx(self) -> Option<BlitCtxInit> {
        #[cfg(feature = "o3ds")]
        entries::thread_screen::thread_ready_acquire()?;

        let bctx = self.0.bctx_mut();
        let work_ready = unsafe { ptr::read_volatile(&self.0.work_ready_params()) };

        unsafe {
            *bctx = BlitCtx {
                format: work_ready.format & 0xf,
                #[cfg(not(feature = "o3ds"))]
                src: entries::thread_screen::img_info(work_ready.is_top),
                #[cfg(feature = "o3ds")]
                src: entries::thread_screen::img_info(),
                frame_id: *CURRENT_FRAME_IDS.get(&is_top_index(work_ready.is_top)),
                is_top: work_ready.is_top,
                i_start: const_default(),
                i_count: const_default(),
                #[cfg(not(feature = "o3ds"))]
                should_capture: AtomicBool::new(false),
            };
        }

        #[cfg(not(feature = "o3ds"))]
        let format_changed = self.format_changed(bctx.is_top, bctx.format);
        Some(BlitCtxInit(
            self,
            #[cfg(not(feature = "o3ds"))]
            format_changed,
        ))
    }

    #[cfg(not(feature = "o3ds"))]
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

pub struct BlitCtxInit(WorkReady, #[cfg(not(feature = "o3ds"))] bool);

impl BlitCtxInit {
    #[named]
    #[cfg(not(feature = "o3ds"))]
    fn dma_sync(&self) {
        if wait_syn_once(cname!(), self.0.0.work_ready_params().dma, c_str!("dma")).is_none() {
            return;
        }

        let bctx = self.0.0.bctx();
        unsafe {
            let _ =
                svcInvalidateProcessDataCache(CUR_PROCESS_HANDLE, bctx.src as u32, bctx.src_len());
        }
    }

    pub fn sync(self) -> WorkSync {
        #[cfg(not(feature = "o3ds"))]
        self.dma_sync();
        WorkSync(
            self.0,
            #[cfg(not(feature = "o3ds"))]
            self.1,
        )
    }
}

pub struct WorkSync(WorkReady, #[cfg(not(feature = "o3ds"))] bool);

impl WorkSync {
    #[cfg(not(feature = "o3ds"))]
    pub fn skip_frame(self) -> core::result::Result<WorkFrame, WorkSkipFrame> {
        let is_top = self.0.0.bctx().is_top;
        let is_top_index = is_top_index(is_top);

        let timing = get_system_tick();
        let last_timing = unsafe { *LAST_FRAME_TIMINGS.get(&is_top_index) };
        let frame_time = timing.get() as u32 - last_timing;

        let fps_limit = RP_CONFIG.fps_limit(is_top_index).load(Ordering::Acquire);
        if fps_limit > 0 {
            let fps_limit = match fps_limit as u8 {
                RP_FPS_LIMIT_1 => 1,
                RP_FPS_LIMIT_2 => 2,
                RP_FPS_LIMIT_3 => 3,
                RP_FPS_LIMIT_4 => 4,
                RP_FPS_LIMIT_5 => 5,
                RP_FPS_LIMIT_6 => 6,
                RP_FPS_LIMIT_10 => 10,
                RP_FPS_LIMIT_12 => 12,
                RP_FPS_LIMIT_15 => 15,
                RP_FPS_LIMIT_20 => 20,
                RP_FPS_LIMIT_30 => 30,
                _ => 60,
            };
            let frame_time_limit = SYSCLOCK_ARM11 / fps_limit;
            let frame_time_limit = frame_time_limit - frame_time_limit / 10; // tol
            let prev_frame_time = get_frame_time(is_top_index).load(Ordering::Acquire);
            let curr_frame_time =
                (prev_frame_time + frame_time * (FRAME_TIME_FACTOR - 1)) / FRAME_TIME_FACTOR;
            if curr_frame_time < frame_time_limit {
                return Err(WorkSkipFrame(self.0));
            }
        };

        if RP_CONFIG
            .no_skip_frame(is_top_index)
            .load(Ordering::Acquire)
            > 0
        {
            return Ok(WorkFrame(self.0, timing.get() as u32, frame_time));
        }

        if frame_time >= entries::thread_screen::frame_timing_allowance() {
            entries::thread_screen::set_no_skip_frame(is_top);
        }

        if !entries::thread_screen::no_skip_frame(is_top) && !self.1 && !self.frame_changed() {
            Err(WorkSkipFrame(self.0))
        } else {
            Ok(WorkFrame(self.0, timing.get() as u32, frame_time))
        }
    }

    #[cfg(feature = "o3ds")]
    pub fn skip_frame(self) -> core::result::Result<WorkFrame, WorkSkipFrame> {
        Ok(WorkFrame(self.0))
    }

    #[cfg(not(feature = "o3ds"))]
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

pub struct WorkSkipFrame(#[cfg(not(feature = "o3ds"))] WorkReady);

impl WorkSkipFrame {
    pub fn skip_frame_release(self) -> Option<WorkReady> {
        #[cfg(not(feature = "o3ds"))]
        {
            unsafe {
                entries::thread_screen::skip_frame_release(
                    self.0.0.w,
                    entries::thread_screen::SkipFrameParams::SkipFrame(self.0.0.t),
                );
                entries::thread_screen::thread_ready_acquire(&self.0.0.t)?;
            }
            return Some(self.0);
        }
        #[cfg(feature = "o3ds")]
        {
            return None;
        }
    }
}

#[cfg(not(feature = "o3ds"))]
pub struct WorkFrame(WorkReady, u32, u32);

#[cfg(feature = "o3ds")]
pub struct WorkFrame(WorkReady);

impl WorkFrame {
    pub fn frame_release(self) -> Option<WorkAcquire> {
        let bctx = self.0.0.bctx();
        #[cfg(not(feature = "mem3"))]
        let downsample = unsafe { *is_top_index(bctx.is_top).index_into(&JPEG_DOWNSAMPLE) } as u8;
        #[cfg(feature = "mem3")]
        let downsample = RP_DOWNSAMPLE_NONE;
        #[cfg(not(feature = "o3ds"))]
        if entries::thread_nwm::get_reliable_stream() == entries::thread_nwm::ReliableStream::None {
            if !unsafe {
                entries::thread_nwm::nwm_done_acquire(
                    self.0.0.w,
                    bctx.frame_id,
                    bctx.is_top,
                    downsample,
                )
            } {
                return None;
            }
            unsafe {
                entries::thread_nwm::nwm_ready_release(&self.0.0.w);
            }
        }

        #[cfg(feature = "o3ds")]
        unsafe {
            entries::thread_nwm::nwm_start_frame(bctx.frame_id, bctx.is_top, downsample);
        }

        unsafe {
            #[cfg(not(feature = "o3ds"))]
            entries::thread_screen::img_info_next(bctx.is_top);
            *CURRENT_FRAME_IDS.get_mut(&is_top_index(bctx.is_top)) += 1;
        }

        if !self.init_work() {
            return None;
        }

        #[cfg(not(feature = "o3ds"))]
        unsafe {
            *LAST_FRAME_TIMINGS.get_mut(&is_top_index(bctx.is_top)) = self.1;
        }

        self.work_ready_release()
    }

    #[named]
    #[allow(unused_macros)]
    fn work_ready_release(self) -> Option<WorkAcquire> {
        #[cfg(not(feature = "o3ds"))]
        unsafe {
            entries::thread_screen::skip_frame_release(
                self.0.0.w,
                entries::thread_screen::SkipFrameParams::Frame,
            );

            entries::thread_screen::work_done_flag_release(self.0.0.w);
        }

        #[cfg(not(feature = "o3ds"))]
        unsafe {
            let bctx = self.0.0.bctx();

            let ft = FRAME_TIMES.get_mut(&is_top_index(bctx.is_top));
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

        #[cfg(not(feature = "o3ds"))]
        if rp_need_core_syn!() {
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
        }

        Some(WorkAcquire(self.0.0))
    }

    fn init_work(&self) -> bool {
        let bctx = self.0.0.bctx_mut();

        let w = self.0.0.w;
        let last_s = unsafe { &mut LAST_ENCODED_SCREEN };
        let is_top = bctx.is_top;
        let curr_s = is_top_index(is_top);

        let core_count = core_count_in_use();
        let thread_index_last = thread_index_last(core_count);
        let core_count_all = core_count.get();
        let core_count_other = core_count_all - 1;

        let l = unsafe { &mut LAST_ROW_LAST_N };
        let jpeg_shared = unsafe { jpeg::get_jpeg_shared() };
        let jpeg_screen = curr_s.index_into(&jpeg_shared.screens);

        #[cfg(not(feature = "mem3"))]
        let downsample = *unsafe { curr_s.index_into(&JPEG_DOWNSAMPLE) } as u8;
        #[cfg(feature = "mem3")]
        let downsample = RP_DOWNSAMPLE_NONE;

        #[cfg(not(feature = "mem3"))]
        let (mcus_per_row, mcu_rows) = if downsample == RP_DOWNSAMPLE_CHECKER {
            (
                jpeg_screen.checker.mcus_per_row as u32,
                jpeg_screen.checker.mcu_rows as u32,
            )
        } else {
            (jpeg_screen.mcus_per_row as u32, jpeg_screen.mcu_rows as u32)
        };
        #[cfg(feature = "mem3")]
        let mcu_rows = jpeg_screen.mcu_rows as u32;

        let mcu_rows_per_thread = unsafe {
            core::intrinsics::unchecked_div(mcu_rows + core_count_all - 1, core_count_all)
        };

        let n = mcu_rows_per_thread;
        let n_last = mcu_rows - mcu_rows_per_thread * core_count_other;

        let last_row_last_n_range = jpeg_shared.last_restart_range;
        const LAST_ROW_LAST_N_F: u32 = 4;

        let mut curr = l.load(Ordering::Acquire);
        let (v_adjusted, v_last_adjusted) = if curr > 0 && core_count_all > 1 {
            let next = loop {
                let next = if self.0.0.t == thread_index_last {
                    if curr < last_row_last_n_range {
                        curr + 1
                    } else {
                        curr
                    }
                } else {
                    if curr > core_count_other {
                        curr - core_count_other
                    } else {
                        curr
                    }
                };
                match l.compare_exchange_weak(curr, next, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => break next,
                    Err(temp) => curr = temp,
                }
            };
            let last_rows_last = unsafe { &mut LAST_SCREEN_LAST_ROW };
            let update_rows_last = *last_rows_last == 0
                || (next as i32 - *last_rows_last as i32).abs() as u32
                    >= unsafe {
                        core::intrinsics::unchecked_div(last_row_last_n_range, LAST_ROW_LAST_N_F)
                    }
                || *last_s != curr_s;

            let rows_last = if update_rows_last {
                cmp::max(
                    unsafe {
                        core::intrinsics::unchecked_div(
                            n_last * next + last_row_last_n_range / 2,
                            last_row_last_n_range,
                        )
                    },
                    1,
                )
            } else {
                *last_rows_last
            };
            let rows = unsafe {
                core::intrinsics::unchecked_div(
                    mcu_rows - rows_last + core_count_other - 1,
                    core_count_other,
                )
            };

            if update_rows_last {
                let rows_last = mcu_rows - rows * core_count_other;
                *last_rows_last = rows_last;
                *last_s = curr_s;
                (rows, rows_last)
            } else {
                (rows, rows_last)
            }
        } else {
            l.store(last_row_last_n_range, Ordering::Release);
            (n, n_last)
        };

        let restart_in_rows = v_adjusted as s32;
        #[cfg(not(feature = "mem3"))]
        let restart_interval = restart_in_rows as u32 * mcus_per_row;

        let even_odd = *unsafe { curr_s.index_into(&JPEG_EVEN_ODD) };
        let cinfo = jpeg::CInfo {
            is_top: bctx.is_top,
            color_space: match bctx.format {
                0 => jpeg::ColorSpace::XBGR,
                1 => jpeg::ColorSpace::BGR,
                2 => jpeg::ColorSpace::RGB565,
                3 => jpeg::ColorSpace::RGB5A1,
                _ => jpeg::ColorSpace::RGB4,
            },
            #[cfg(not(feature = "mem3"))]
            restart_interval: restart_interval as u16,
            work_index: w,
            #[cfg(not(feature = "o3ds"))]
            core_count,
            even_odd,
        };

        if downsample == RP_DOWNSAMPLE_CHECKER || downsample == RP_DOWNSAMPLE_EVEN_ODD {
            unsafe { *curr_s.index_into_mut(&mut JPEG_EVEN_ODD) = !even_odd };
        }

        unsafe {
            (*jpeg::JPEG).set_info(cinfo);
        }

        for j in ThreadIndex::up_to(&thread_index_last) {
            *bctx.i_start.get_mut(&j) = restart_in_rows as u32 * j.get();
            *bctx.i_count.get_mut(&j) = if j == thread_index_last {
                v_last_adjusted
            } else {
                v_adjusted
            };
        }

        #[cfg(not(feature = "o3ds"))]
        unsafe {
            *TERM_INFOS.get_mut(&w) = TermInfo {
                is_top: bctx.is_top,
                core_count,
                v_adjusted,
                v_last_adjusted,
                even_odd,
            };
        }

        entries::thread_nwm::rp_clear_size(w);

        true
    }
}

pub struct WorkOtherReady(#[cfg(not(feature = "o3ds"))] Impl);

impl WorkOtherReady {
    #[named]
    #[allow(unused_macros)]
    pub fn acquire(self) -> Option<WorkAcquire> {
        #[cfg(not(feature = "o3ds"))]
        unsafe {
            wait_syn(
                cname!(),
                SYN_HANDLES.threads.get(&self.0.t).work_ready,
                c_str!("work_ready"),
            )?;
            return Some(WorkAcquire(self.0));
        };

        #[cfg(feature = "o3ds")]
        return None;
    }
}

pub struct WorkAcquire(Impl);

pub struct JpegRet(
    #[cfg(not(feature = "o3ds"))] Impl,
    #[cfg(not(feature = "o3ds"))] jpeg::JpegDqRet,
);

impl Drop for JpegRet {
    #[named]
    #[allow(unused_macros)]
    fn drop(&mut self) {
        #[cfg(not(feature = "o3ds"))]
        {
            let w = self.0.w;
            let bctx = self.0.bctx();
            let syn = unsafe { SYN_HANDLES.works.get(&w) };

            let f = syn.work_done_count.fetch_add(1, Ordering::AcqRel);
            let core_count = core_count_in_use();
            if f == core_count.get() - 1 {
                entries::thread_screen::reset_no_skip_frame(bctx.is_top);

                #[cfg(not(feature = "o3ds"))]
                if !unsafe { send_term_dsts(w, self.1.delta_q as u16) } {
                    set_reset_threads();
                }

                let s = is_top_index(bctx.is_top);

                #[cfg(not(feature = "o3ds"))]
                let delta_prog = entries::thread_nwm::get_reliable_stream_delta_prog();
                #[cfg(feature = "o3ds")]
                let delta_prog = false;
                if !delta_prog {
                    let comp_size = entries::thread_nwm::rp_get_size(w) as f32 * u8::BITS as f32
                        / self.1.mcus as f32;
                    unsafe {
                        (*config_consts::OV_STATS).s[s.get() as usize].comp_size =
                            (comp_size * 1000f32) as s32
                    };
                }
                unsafe {
                    (*config_consts::OV_STATS).s[s.get() as usize].frame_time =
                        FRAME_TIMES.get(&s).load(Ordering::Acquire);
                }

                syn.work_done_count.store(0, Ordering::Release);
                syn.work_ready_flag.store(false, Ordering::Release);

                unsafe {
                    release_sem(
                        cname!(),
                        SYN_HANDLES.works.get(&w).work_done,
                        c_str!("work_done"),
                    );
                }
            }
        }
    }
}

static mut TERM_DSTS: RangedArray<RangedArray<*mut u8, RP_CORE_COUNT_MAX>, WORK_COUNT> =
    const_default();

#[cfg(not(feature = "o3ds"))]
pub unsafe fn set_term_dst(dst: *mut u8, w: WorkIndex, t: ThreadIndex) -> bool {
    let d = unsafe { TERM_DSTS.get_mut(&w).get_mut(&t) };
    if *d == ptr::null_mut() {
        *d = dst;
        return true;
    }
    return false;
}

#[named]
#[cfg(not(feature = "o3ds"))]
unsafe fn send_term_dsts(w: WorkIndex, delta_q: u16) -> bool {
    if *unsafe { TERM_DSTS.get(&w).get(&ThreadIndex::init(0)) } == ptr::null_mut() {
        return true;
    }

    if wait_syn(
        cname!(),
        unsafe { entries::thread_nwm::SEG_MEM_TERM_SEM },
        c_str!("SEG_MEM_TERM_SEM"),
    )
    .is_none()
    {
        return false;
    }

    let mut terms: [*mut u8; RP_CORE_COUNT_MAX as usize + 1] = const_default();
    let mut term_cur = 0;
    let mut term_size = 0;
    terms[term_cur] = if let Some(d) = unsafe { rp_term_data_buf_malloc() } {
        entries::thread_nwm::rp_data_buf_data(d)
    } else {
        return false;
    };

    let rp_packet_data_size = entries::thread_nwm::get_packet_data_size();
    let mut copy_to_terms = |mut data: *const u8, mut len: usize| {
        while len > 0 {
            let len_0 = rp_packet_data_size - term_size;
            if len_0 >= len {
                unsafe {
                    ptr::copy_nonoverlapping(
                        data,
                        terms.get_unchecked_mut(term_cur).add(term_size),
                        len,
                    );
                }
                term_size += len;
                break;
            } else {
                if len_0 > 0 {
                    unsafe {
                        ptr::copy_nonoverlapping(
                            data,
                            terms.get_unchecked_mut(term_cur).add(term_size),
                            len_0,
                        );
                    }
                    data = unsafe { data.add(len_0) };
                    len -= len_0;
                }
                term_cur += 1;
                term_size = 0;
                terms[term_cur] = if let Some(d) = unsafe { rp_term_data_buf_malloc() } {
                    entries::thread_nwm::rp_data_buf_data(d)
                } else {
                    return false;
                };
            }
        }
        true
    };

    let info = unsafe { TERM_INFOS.get(&w) };
    let downsample = unsafe { *is_top_index(info.is_top).index_into(&JPEG_DOWNSAMPLE) } as u8;
    let delta_prog = entries::thread_nwm::get_reliable_stream_delta_prog();
    let hdr = (downsample as u16)
        << (RP_KCP_HDR_QUALITY_NBITS + RP_KCP_HDR_T_NBITS + 1 + RP_KCP_HDR_CHROMASS_NBITS + 1)
        | (delta_prog as u16)
            << (RP_KCP_HDR_QUALITY_NBITS + RP_KCP_HDR_T_NBITS + 1 + RP_KCP_HDR_CHROMASS_NBITS)
        | (unsafe { *is_top_index(info.is_top).index_into(&JPEG_CHROMA_SS) as u16 })
            << (RP_KCP_HDR_QUALITY_NBITS + RP_KCP_HDR_T_NBITS + 1)
        | (is_top_index(info.is_top).get() as u16)
            << (RP_KCP_HDR_QUALITY_NBITS + RP_KCP_HDR_T_NBITS)
        | (info.core_count.get() as u16) << RP_KCP_HDR_QUALITY_NBITS
        | (if delta_prog {
            delta_q
        } else {
            unsafe { *is_top_index(info.is_top).index_into(&JPEG_QUALITY) as u16 }
        });

    let need_even_odd = downsample == RP_DOWNSAMPLE_CHECKER || downsample == RP_DOWNSAMPLE_EVEN_ODD;
    let ex_hdr = need_even_odd;
    let hdr = if ex_hdr {
        const EX_HDR_BIT: u32 = 15;
        assert!(
            RP_KCP_HDR_QUALITY_NBITS
                + RP_KCP_HDR_T_NBITS
                + 1
                + RP_KCP_HDR_CHROMASS_NBITS
                + 1
                + RP_KCP_HDR_DOWNSAMPLE_NBITS
                <= EX_HDR_BIT
        );
        hdr | 1 << EX_HDR_BIT
    } else {
        hdr
    };

    if !copy_to_terms(&hdr as *const u16 as *const _, mem::size_of_val(&hdr)) {
        return false;
    }

    if ex_hdr {
        let mut hdr: u16 = 0;

        if need_even_odd {
            hdr |= info.even_odd as u16;
        }

        if !copy_to_terms(&hdr as *const u16 as *const _, mem::size_of_val(&hdr)) {
            return false;
        }
    }

    let core_count = core_count_in_use();
    let mut sizes: RangedArray<u32, RP_CORE_COUNT_MAX> = const_default();
    for i in ThreadIndex::up_to(&thread_index_last(core_count)) {
        let dst = *unsafe { TERM_DSTS.get_mut(&w).get_mut(&i) };
        if dst == ptr::null_mut() {
            return false;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                dst.sub(mem::size_of::<u32>()) as *const _,
                sizes.get_mut(&i),
                1,
            )
        };
        let size = *sizes.get(&i) as u16;

        let hdr = size
            | ((if i == thread_index_last(core_count) {
                info.v_last_adjusted
            } else {
                info.v_adjusted
            } as u16
                & ((1 << RP_KCP_HDR_RC_NBITS) - 1))
                << RP_KCP_HDR_SIZE_NBITS);
        if !copy_to_terms(&hdr as *const u16 as *const _, mem::size_of_val(&hdr)) {
            return false;
        }
    }

    for i in ThreadIndex::up_to(&thread_index_last(core_count)) {
        let dst_ref = unsafe { TERM_DSTS.get_mut(&w).get_mut(&i) };
        let dst = *dst_ref;

        if !copy_to_terms(dst, *sizes.get(&i) as usize) {
            return false;
        }

        unsafe {
            rp_seg_data_buf_free(dst.sub(ARQ_DATA_HDR_SIZE as usize));
        }
        *dst_ref = ptr::null_mut();
    }

    for i in 0..=term_cur {
        let mut dst = *unsafe { terms.get_unchecked_mut(i) };

        if i == term_cur {
            unsafe {
                ptr::write_bytes(dst.add(term_size), 0, rp_packet_data_size - term_size);
            }
        }

        let mut size = rp_packet_data_size as u32;

        dst = unsafe { dst.sub(ARQ_DATA_HDR_SIZE as usize) };
        size += ARQ_DATA_HDR_SIZE;

        let hdr = (w.get() as u16) << (PID_NBITS + CID_NBITS)
            | (RP_CORE_COUNT_MAX as u16) << (PID_NBITS + CID_NBITS + RP_KCP_HDR_W_NBITS);
        unsafe {
            ptr::copy_nonoverlapping(&hdr, dst as *mut _, 1);
        }

        size |= 1 << 31;
        if i == term_cur {
            size |= 1 << 30;
        }
        unsafe { ptr::copy_nonoverlapping(&size, dst.sub(mem::size_of::<u32>()) as *mut _, 1) };

        let cb = unsafe { &mut *entries::thread_nwm::RELIABLE_STREAM_CB };
        while !reset_threads() {
            let res = unsafe { rp_syn_rel1(&mut cb.nwm_syn, dst as *mut _) };
            if res == 0 {
                break;
            }
            if res != RES_TIMEOUT as s32 {
                ns_dbg_print!(failed, c_str!("Wait for nwm_syn"), res);
                set_reset_threads();
                return false;
            }
        }
    }

    true
}

#[cfg(not(feature = "o3ds"))]
pub const RP_KCP_HDR_W_NBITS: u32 = 1;
#[cfg(not(feature = "o3ds"))]
pub const RP_KCP_HDR_T_NBITS: u32 = 2;
#[cfg(not(feature = "o3ds"))]
const RP_KCP_HDR_SIZE_NBITS: u32 = 11;
#[cfg(not(feature = "o3ds"))]
const RP_KCP_HDR_RC_NBITS: u32 = 5;

#[named]
#[cfg(not(feature = "o3ds"))]
pub unsafe fn rp_term_data_buf_malloc() -> Option<*mut c_char> {
    wait_syn(
        cname!(),
        unsafe { entries::thread_nwm::SEG_MEM_LOCK },
        c_str!("SEG_MEM_LOCK"),
    )?;

    let cb = unsafe { &mut *entries::thread_nwm::RELIABLE_STREAM_CB };
    let dst = unsafe { mp_malloc(&mut cb.locked.send_pool) } as *mut u8;

    let ret = if dst == ptr::null_mut() {
        ns_dbg_print!(msg, c_str!("Mem pool send alloc failed"));
        set_reset_threads();
        None
    } else {
        Some(dst)
    };

    unsafe {
        release_mutex(
            cname!(),
            entries::thread_nwm::SEG_MEM_LOCK,
            c_str!("SEG_MEM_LOCK"),
        );
    }
    ret
}

#[named]
#[cfg(not(feature = "o3ds"))]
#[unsafe(no_mangle)]
pub unsafe fn rp_term_data_buf_free_base(dst: *const ::libc::c_char) -> bool {
    if wait_syn(
        cname!(),
        unsafe { entries::thread_nwm::SEG_MEM_LOCK },
        c_str!("SEG_MEM_LOCK"),
    )
    .is_none()
    {
        return false;
    }

    let cb = unsafe { &mut *entries::thread_nwm::RELIABLE_STREAM_CB };
    let ret = if unsafe { mp_free(&mut cb.locked.send_pool, dst as *mut _) } < 0 {
        ns_dbg_print!(msg, c_str!("Mem pool send free failed"));
        false
    } else {
        true
    };
    unsafe {
        release_mutex(
            cname!(),
            entries::thread_nwm::SEG_MEM_LOCK,
            c_str!("SEG_MEM_LOCK"),
        );
    }
    ret
}

#[cfg(not(feature = "o3ds"))]
#[unsafe(no_mangle)]
unsafe fn rp_term_data_buf_free(dst: *const ::libc::c_char) -> bool {
    unsafe { rp_term_data_buf_free_base(dst.sub((NWM_HDR_SIZE + ARQ_OVERHEAD_SIZE) as usize)) }
}

#[named]
#[unsafe(no_mangle)]
#[cfg(not(feature = "o3ds"))]
unsafe fn rp_term_notify() {
    unsafe {
        release_sem(
            cname!(),
            entries::thread_nwm::SEG_MEM_TERM_SEM,
            c_str!("SEG_MEM_TERM_SEM"),
        );
    }
}

impl WorkAcquire {
    pub fn send_frame(self) -> Option<JpegRet> {
        let bctx = self.0.bctx_mut();
        let w = self.0.w;
        let t = self.0.t;

        let src = bctx.src;
        #[cfg(not(feature = "mem3"))]
        let downsample = *unsafe { is_top_index(bctx.is_top).index_into(&JPEG_DOWNSAMPLE) } as u8;
        #[cfg(feature = "mem3")]
        let downsample = RP_DOWNSAMPLE_NONE;
        let src_len = bctx.src_len() as usize;
        let src = if downsample == RP_DOWNSAMPLE_CHECKER {
            unsafe { slice::from_raw_parts(src, src_len) }
        } else {
            let (i_start, i_count) = match downsample {
                RP_DOWNSAMPLE_QUARTER => (
                    *bctx.i_start.get(&t) * jpeg::DOWNSAMPLE_FACTOR as u32,
                    *bctx.i_count.get(&t) * jpeg::DOWNSAMPLE_FACTOR as u32,
                ),
                RP_DOWNSAMPLE_EVEN_ODD | _ => (*bctx.i_start.get(&t), *bctx.i_count.get(&t)),
            };
            let pitch = bctx.pitch();

            let jpeg_shared = unsafe { jpeg::get_jpeg_shared() };
            let mcu_size = is_top_index(bctx.is_top)
                .index_into(&jpeg_shared.screens)
                .mcu_col_size;
            let j_start = mcu_size * pitch as usize * i_start as usize;
            let j_count = mcu_size * pitch as usize * i_count as usize;

            unsafe {
                slice::from_raw_parts(src, src_len)
                    .get_unchecked(j_start..cmp::min(j_start + j_count, src_len))
            }
        };

        let pre_progress = || {
            #[cfg(not(feature = "o3ds"))]
            capture_screen(&mut bctx.should_capture);
        };

        let progress = || {};

        #[cfg(not(feature = "o3ds"))]
        let s = is_top_index(bctx.is_top);
        let mut worker = unsafe { (*jpeg::JPEG).get_worker(w, t) };

        #[cfg(not(feature = "o3ds"))]
        let dst = {
            let (user, dst) = match entries::thread_nwm::get_reliable_stream() {
                entries::thread_nwm::ReliableStream::None => {
                    let ninfo = entries::thread_nwm::nwm_info(w).get(&t);
                    (
                        jpeg::WorkderDstUser {
                            none_info: ninfo as *const _,
                        },
                        ninfo.info.pos.load(Ordering::Acquire),
                    )
                }
                entries::thread_nwm::ReliableStream::KCP => {
                    let dst =
                        if let Some(dst) = unsafe { entries::thread_nwm::rp_data_buf_malloc() } {
                            entries::thread_nwm::rp_data_buf_data(dst)
                        } else {
                            return None;
                        };
                    let hdr = jpeg::ArqRpHdr { w, t };

                    (jpeg::WorkderDstUser { kcp_hdr: hdr }, dst)
                }
            };

            let dst = unsafe {
                (*jpeg::JPEG).worker_dst(
                    #[cfg(not(feature = "o3ds"))]
                    s,
                    w,
                    dst,
                    user,
                )
            };
            dst
        };

        #[cfg(feature = "o3ds")]
        let dst = {
            let dst = unsafe { entries::thread_nwm::nwm_info() };
            let dst = unsafe { (*jpeg::JPEG).worker_dst(dst) };
            dst
        };

        let jpeg_ret = worker.encode(dst, src, pre_progress, progress)?;
        #[cfg(feature = "o3ds")]
        let _ = jpeg_ret;

        if reset_threads() {
            return None;
        }

        Some(JpegRet(
            #[cfg(not(feature = "o3ds"))]
            self.0,
            #[cfg(not(feature = "o3ds"))]
            jpeg_ret,
        ))
    }
}

#[cfg(not(feature = "o3ds"))]
fn capture_screen(should_capture: &mut AtomicBool) {
    if should_capture.swap(true, Ordering::AcqRel) == false {
        unsafe {
            entries::thread_screen::work_index_next_wrapped();

            entries::thread_screen::screen_ready_release();
        }
    }
}

pub unsafe fn work_thread_loop(t: ThreadIndex) -> Option<()> {
    let mut work_index = WorkIndex::init(0);
    loop {
        #[cfg(not(feature = "o3ds"))]
        entries::thread_screen::thread_ready_acquire(&t)?;
        #[cfg(feature = "o3ds")]
        unsafe {
            let mut rp_config = RP_CONFIG_SAVED;
            rp_config.dstAddr = 0;
            if rp_config != *config_consts::RP_CONFIG {
                set_reset_threads();
                return None;
            }
        }
        safe_impl::send_frame(Impl { w: work_index, t })?;
        work_index.next_wrapped();
    }
}

#[derive(ConstDefault)]
pub struct BlitCtx {
    pub format: u32,
    pub src: *const u8,

    pub frame_id: u8,
    pub is_top: bool,

    pub i_start: RowIndices,
    pub i_count: RowIndices,

    #[cfg(not(feature = "o3ds"))]
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
#[cfg(not(feature = "o3ds"))]
static mut BLIT_FORMATS: RangedArray<u32, SCREEN_COUNT> = const_default();

#[derive(ConstDefault, Clone, Copy)]
#[cfg(not(feature = "o3ds"))]
pub struct TermInfo {
    pub is_top: bool,
    pub core_count: CoreCount,
    pub v_adjusted: u32,
    pub v_last_adjusted: u32,
    pub even_odd: bool,
}

#[cfg(not(feature = "o3ds"))]
static mut TERM_INFOS: RangedArray<TermInfo, WORK_COUNT> = const_default();
static mut JPEG_QUALITY: [u32; RP_SCREEN_COUNT as usize] = const_default();
static mut JPEG_CHROMA_SS: [u32; RP_SCREEN_COUNT as usize] = const_default();
#[cfg(not(feature = "mem3"))]
static mut JPEG_DOWNSAMPLE: [u32; RP_SCREEN_COUNT as usize] = const_default();
static mut JPEG_EVEN_ODD: [bool; RP_SCREEN_COUNT as usize] = const_default();
