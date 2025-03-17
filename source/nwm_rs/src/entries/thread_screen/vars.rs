use super::*;

static mut priority_is_top: bool = false;
static mut priority_factor: u32_ = 0;
static mut priority_factor_scaled: u32_ = 0;
static mut frame_counts: RangedArray<u32_, SCREEN_COUNT> = const_default();
static mut frame_queues: RangedArray<u32_, SCREEN_COUNT> = const_default();
static mut no_skip_frames: RangedArray<bool, SCREEN_COUNT> = const_default();
static mut last_frame_timings: [AtomicU32; SCREEN_COUNT as usize] = const_default();
static mut frame_times: [AtomicU32; SCREEN_COUNT as usize] = const_default();
static mut work_last_frame_timings: [AtomicU32; SCREEN_COUNT as usize] = const_default();
static mut work_frame_times: [AtomicU32; SCREEN_COUNT as usize] = const_default();
static mut blit_formats: RangedArray<u8_, SCREEN_COUNT> = const_default();
static mut port_game_pid: AtomicU32 = const_default();

pub const timing_allowance: u32_ = SYSCLOCK_ARM11 / 2;
const frame_time_factor: u32 = 3;
pub fn get_frame_time(s: u32) -> u32 {
    unsafe {
        frame_times
            .get_unchecked(s as usize)
            .load(Ordering::Relaxed)
    }
}

pub fn get_work_frame_time(s: u32) -> u32 {
    unsafe {
        work_frame_times
            .get_unchecked(s as usize)
            .load(Ordering::Relaxed)
    }
}

pub fn bpp_for_format(format: u32_) -> u32_ {
    let format = format & 0xf;
    if format == 0 {
        4
    } else if format == 1 {
        3
    } else if format == 2 || format == 3 {
        2
    } else {
        0
    }
}

type DmaHandles = RangedArray<Handle, IMG_WORK_COUNT>;

#[derive(ConstDefault)]
pub struct CapInfo {
    pub src: *mut u8_,
    pub pitch: u32_,
    pub format: u32_,
}

#[derive(ConstDefault)]
pub struct CapParams {
    pub dmas: DmaHandles,
    pub game: Handle,
    pub game_pid: u32_,
    pub game_fcram_base: u32_,
}

pub static mut cap_params: CapParams = const_default();

pub const QUEUED_WORK_COUNT: u32_ = WORK_COUNT;
pub const IMG_WORK_COUNT: u32_ = QUEUED_WORK_COUNT + 2; // queued + work + screen
pub type ImgWorkIndex = Ranged<IMG_WORK_COUNT>;

#[derive(ConstDefault)]
pub struct ImgBuf {
    src: *mut u8_,
    format: u32_,
}
pub type ImgBufs = RangedArray<ImgBuf, IMG_WORK_COUNT>;

#[derive(ConstDefault)]
pub struct ImgInfo {
    pub bufs: ImgBufs,
    pub screen: u8_,
    pub screen_prev: [u8_; QUEUED_WORK_COUNT as usize],
    pub screen_prev_n: u8_,
    pub work: u8_,
}

pub struct ImgInfoLock();

impl ImgInfoLock {
    pub unsafe fn lock() -> Option<Self> {
        let _ = ImgInfo::img_info_lock()?;
        Some(Self())
    }
}

impl Drop for ImgInfoLock {
    fn drop(&mut self) {
        unsafe { ImgInfo::img_info_unlock() };
    }
}

impl ImgInfo {
    #[named]
    unsafe fn img_info_lock() -> Option<()> {
        while !entries::work_thread::reset_threads() {
            let res = svcWaitSynchronization(img_infos_lock, THREAD_WAIT_NS);

            if res == 0 {
                return Some(());
            }
            if res != RES_TIMEOUT as s32 {
                nsDbgPrint!(waitForSyncFailed, c_str!("img_infos_lock"), res);
                entries::work_thread::set_reset_threads_ar();
                return None;
            }
        }
        None
    }

    #[named]
    unsafe fn img_info_unlock() {
        let res = svcReleaseMutex(img_infos_lock);
        if res != 0 {
            nsDbgPrint!(releaseMutexFailed, c_str!("img_infos_lock"), res);
        }
    }
}

pub type ImgInfos = RangedArray<ImgInfo, SCREEN_COUNT>;
static mut img_infos: ImgInfos = const_default();
static mut img_infos_lock: Handle = const_default();

pub unsafe fn reset_no_skip_frame(is_top: bool) -> bool {
    let b = *no_skip_frames.get_b(is_top);
    *no_skip_frames.get_b_mut(is_top) = false;
    b
}

pub unsafe fn set_no_skip_frame(is_top: bool) {
    *no_skip_frames.get_b_mut(is_top) = true;
}

pub unsafe fn set_port_game_pid(v: u32_) {
    port_game_pid.store(v, Ordering::Relaxed);
}

pub unsafe fn get_port_game_pid() -> u32_ {
    *port_game_pid.as_ptr()
}

pub unsafe fn reset_thread_vars(mode: u32_) {
    let is_top = (mode & 0xff00) > 0;
    let factor = mode & 0xff;
    priority_is_top = is_top;
    priority_factor = factor;
    priority_factor_scaled = FIX(factor as c_double);
    crate::entries::work_thread::no_skip_next_frames();

    for i in ScreenIndex::all() {
        *frame_counts.get_mut(&i) = 1;
        *frame_queues.get_mut(&i) = priority_factor_scaled;
    }

    for i in 0..SCREEN_COUNT as usize {
        last_frame_timings[i].store(svcGetSystemTick() as u32_, Ordering::Relaxed);
        frame_times[i].store(SYSCLOCK_ARM11, Ordering::Relaxed);
        work_last_frame_timings[i].store(svcGetSystemTick() as u32_, Ordering::Relaxed);
        work_frame_times[i].store(SYSCLOCK_ARM11, Ordering::Relaxed);
    }

    for i in ScreenIndex::all() {
        let info = img_infos.get_mut(&i);
        info.screen_prev_n = 0;
        for i in 0..QUEUED_WORK_COUNT as usize {
            info.screen_prev[i] = i as u8;
        }
        info.work = QUEUED_WORK_COUNT as u8;
        info.screen = QUEUED_WORK_COUNT as u8 + 1;
    }
}

#[named]
pub unsafe fn init_img_info() -> Option<()> {
    let res = svcCreateMutex(&mut img_infos_lock, false);
    if res != 0 {
        nsDbgPrint!(createMutexFailed, c_str!("img_infos_lock"), res);
        return None;
    }
    Some(())
}

pub unsafe fn init_img_info_buf(is_top: bool, j: &ImgWorkIndex, m: &mut [u8]) {
    let info = img_infos.get_b_mut(is_top);
    info.bufs.get_mut(&j).src = m.as_mut_ptr();
}

pub struct ScreenThreadVars(pub ());

impl ScreenThreadVars {
    pub fn priority_is_top(&self) -> bool {
        unsafe { priority_is_top }
    }

    pub fn priority_factor(&self) -> u32_ {
        unsafe { priority_factor }
    }

    pub fn priority_factor_scaled(&self) -> u32_ {
        unsafe { priority_factor_scaled }
    }

    pub fn frame_count(&self, is_top: bool) -> &'static mut u32_ {
        unsafe { frame_counts.get_b_mut(is_top) }
    }

    pub fn frame_queue(&self, is_top: bool) -> &'static mut u32_ {
        unsafe { frame_queues.get_b_mut(is_top) }
    }

    pub fn port_game_pid(&self) -> u32_ {
        unsafe { port_game_pid.load(Ordering::Relaxed) }
    }

    pub fn screen_index(is_top: bool) -> ImgWorkIndex {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            ImgWorkIndex::init_unchecked(iinfo.screen as u32_)
        }
    }

    pub fn screen_img(is_top: bool) -> *mut u8_ {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            iinfo
                .bufs
                .get_r(&ImgWorkIndex::init_unchecked(iinfo.screen as u32_))
                .src
        }
    }

    pub fn screen_prev_img(is_top: bool) -> *mut u8_ {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            iinfo
                .bufs
                .get_r(&ImgWorkIndex::init_unchecked(iinfo.screen_prev[0] as u32_))
                .src
        }
    }

    // #[named]
    pub fn screen_next(is_top: bool, format: u32_) {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            if let Some(_) = ImgInfoLock::lock() {
                iinfo.screen_prev_n += 1;
                if iinfo.screen_prev_n > QUEUED_WORK_COUNT as u8 {
                    iinfo.screen_prev_n = QUEUED_WORK_COUNT as u8;
                }
                let screen_temp = *iinfo
                    .screen_prev
                    .get_unchecked(iinfo.screen_prev_n as usize - 1);
                for i in (1..iinfo.screen_prev_n as usize).rev() {
                    *iinfo.screen_prev.get_unchecked_mut(i) =
                        *iinfo.screen_prev.get_unchecked(i - 1);
                }
                iinfo.screen_prev[0] = iinfo.screen;
                iinfo.screen = screen_temp;
                // nsDbgPrint!(int, c_str!("screen_prev_n"), iinfo.screen_prev_n as i32);

                let screen = ImgWorkIndex::init_unchecked(iinfo.screen as u32_);
                let screen = iinfo.bufs.get_mut(&screen);
                screen.format = format;
            };
        }
    }

    pub fn work_next() -> Option<ScreenEncodeVars> {
        unsafe {
            const s: usize = 0;
            const s1: usize = 1;
            let timing = svcGetSystemTick() as u32_;

            let last_timing = work_last_frame_timings.get_unchecked_mut(s);
            let work_frame_time_curr = timing - last_timing.load(Ordering::Relaxed);
            let work_frame_time_aggr = work_frame_times.get_unchecked_mut(s);

            let last_timing_1 = work_last_frame_timings.get_unchecked_mut(s1);
            let work_frame_time_1_curr = timing - last_timing_1.load(Ordering::Relaxed);
            let work_frame_time_1_aggr = work_frame_times.get_unchecked_mut(s1);

            let frame_time_ratio =
                get_frame_time(s as u32_) as f32 / get_frame_time(s1 as u32_) as f32;

            let is_top = if priority_is_top {
                if work_frame_time_curr >= timing_allowance {
                    true
                } else if work_frame_time_1_curr >= timing_allowance {
                    false
                } else if (work_frame_time_curr as f32)
                    / (work_frame_time_1_aggr.load(Ordering::Relaxed) as f32)
                    > frame_time_ratio
                {
                    true
                } else {
                    false
                }
            } else {
                if work_frame_time_1_curr >= timing_allowance {
                    false
                } else if work_frame_time_curr >= timing_allowance {
                    true
                } else if (work_frame_time_aggr.load(Ordering::Relaxed) as f32)
                    / (work_frame_time_1_curr as f32)
                    < frame_time_ratio
                {
                    false
                } else {
                    true
                }
            };

            let do_work_next = |is_top| {
                let iinfo = img_infos.get_b_mut(is_top);
                // nsDbgPrint!(int, c_str!("is_top"), is_top as i32);
                // nsDbgPrint!(int, c_str!("screen_prev_n"), iinfo.screen_prev_n as i32);
                if iinfo.screen_prev_n > 0 {
                    iinfo.screen_prev_n -= 1;
                    let screen_temp = iinfo.work;
                    iinfo.work = *iinfo
                        .screen_prev
                        .get_unchecked(iinfo.screen_prev_n as usize);
                    *iinfo
                        .screen_prev
                        .get_unchecked_mut(iinfo.screen_prev_n as usize) = screen_temp;
                } else {
                    return None;
                }

                let screen = ImgWorkIndex::init_unchecked(iinfo.work as u32_);
                let screen = iinfo.bufs.get(&screen);

                if is_top {
                    last_timing.store(timing, Ordering::Relaxed);
                    let work_frame_time = {
                        (work_frame_time_aggr.load(Ordering::Relaxed) * (frame_time_factor - 1)
                            + work_frame_time_curr.min(timing_allowance))
                            / frame_time_factor
                    };
                    work_frame_time_aggr.store(work_frame_time, Ordering::Relaxed);
                } else {
                    last_timing_1.store(timing, Ordering::Relaxed);
                    let work_frame_time_1 = {
                        (work_frame_time_1_aggr.load(Ordering::Relaxed) * (frame_time_factor - 1)
                            + work_frame_time_1_curr.min(timing_allowance))
                            / frame_time_factor
                    };
                    work_frame_time_1_aggr.store(work_frame_time_1, Ordering::Relaxed);
                }

                return Some(ScreenEncodeVars {
                    is_top,
                    format: screen.format,
                    src: screen.src,
                });
            };

            if let Some(_) = ImgInfoLock::lock() {
                if let Some(v) = do_work_next(is_top) {
                    Some(v)
                } else if let Some(v) = do_work_next(!is_top) {
                    Some(v)
                } else {
                    None
                }
            } else {
                None
            }
        }
    }

    pub fn get_last_frame_timing(is_top: bool) -> u32_ {
        unsafe {
            last_frame_timings
                .get_unchecked(if is_top { 0 } else { 1 })
                .load(Ordering::Relaxed)
        }
    }

    pub fn set_last_frame_timing(is_top: bool, timing: u32_) {
        unsafe {
            last_frame_timings
                .get_unchecked_mut(if is_top { 0 } else { 1 })
                .store(timing, Ordering::Relaxed)
        }
    }

    #[named]
    pub fn dma_sync(screen: ImgWorkIndex) {
        unsafe {
            let dma = *cap_params.dmas.get(&screen);

            let res = svcWaitSynchronization(dma, THREAD_WAIT_NS);
            if res != 0 {
                if res != RES_TIMEOUT as s32 {
                    nsDbgPrint!(waitForSyncFailed, c_str!("dmas"), res);
                    svcSleepThread(THREAD_WAIT_NS);
                }
            }
        }
    }

    pub fn frame_changed(is_top: bool, format: u32_, screen: ImgWorkIndex) -> bool {
        unsafe {
            let curr = Self::screen_img(is_top) as *const u8_;
            let prev = Self::screen_prev_img(is_top) as *const u8_;
            let src_len = bpp_for_format(format)
                * GSP_SCREEN_WIDTH
                * (if is_top {
                    GSP_SCREEN_HEIGHT_TOP
                } else {
                    GSP_SCREEN_HEIGHT_BOTTOM
                });

            Self::dma_sync(screen);
            let _ = svcInvalidateProcessDataCache(
                CUR_PROCESS_HANDLE,
                curr as usize as u32_,
                src_len as u32_,
            );

            *slice::from_raw_parts(curr, src_len as usize)
                != *slice::from_raw_parts(prev, src_len as usize)
        }
    }

    pub fn get_blit_format_changed(is_top: bool, format: u32_) -> bool {
        unsafe {
            let blit_format = blit_formats.get_b_mut(if is_top { false } else { true });
            let format = format as u8;
            if *blit_format == format {
                false
            } else {
                *blit_format = format;
                true
            }
        }
    }

    pub fn no_skip_frame(
        &self,
        is_top: bool,
        format: u32_,
        screen: ImgWorkIndex,
        no_skip_frame: bool,
    ) -> bool {
        let timing = unsafe { svcGetSystemTick() as u32_ };
        let last_timing = Self::get_last_frame_timing(is_top);
        let mut frame_time = timing - last_timing;
        if frame_time >= timing_allowance {
            unsafe { set_no_skip_frame(is_top) };
            frame_time = timing_allowance;
        }
        let skip_frame = !unsafe { reset_no_skip_frame(is_top) }
            && !no_skip_frame
            && !Self::get_blit_format_changed(is_top, format)
            && !Self::frame_changed(is_top, format, screen);

        if !skip_frame {
            Self::set_last_frame_timing(is_top, timing);
            unsafe {
                let s = if is_top { 0 } else { 1 };
                let ft = &mut frame_times[s];
                let ft_v = ft.load(Ordering::Relaxed);
                ft.store(
                    (ft_v * (frame_time_factor - 1) + frame_time) / frame_time_factor,
                    Ordering::Relaxed,
                );
                (*ov_stats).s[s].frame_time = ft_v;
            }
        }
        !skip_frame
    }

    #[named]
    pub fn port_screen_sync(&self, is_top: bool, wait: bool) -> bool {
        unsafe {
            let res = svcWaitSynchronization(
                *(*syn_handles).port_screen_ready.get_b(is_top),
                if wait { THREAD_WAIT_NS } else { 0 },
            );
            if res == 0 {
                return true;
            }
            if res != RES_TIMEOUT as s32 {
                nsDbgPrint!(waitForSyncFailed, c_str!("port_screen_ready"), res);
                if wait {
                    svcSleepThread(THREAD_WAIT_NS);
                }
            }
            false
        }
    }

    #[named]
    pub fn port_screens_sync(&self) -> Option<bool> {
        unsafe {
            let mut out = mem::MaybeUninit::uninit();
            let res = svcWaitSynchronizationN(
                out.as_mut_ptr(),
                (*syn_handles).port_screen_ready.as_mut_ptr(),
                SCREEN_COUNT as s32,
                false,
                THREAD_WAIT_NS,
            );
            if res != 0 {
                if res != RES_TIMEOUT as s32 {
                    nsDbgPrint!(waitForSyncFailed, c_str!("port_screen_ready"), res);
                    svcSleepThread(THREAD_WAIT_NS);
                    return None;
                }
                return None;
            }
            Some(out.assume_init() > 0)
        }
    }

    pub fn release(is_top: bool, format: u32_) {
        Self::screen_next(is_top, format);
    }
}

#[derive(ConstDefault)]
pub struct ScreenEncodeVars {
    pub is_top: bool,
    pub format: u32_,
    pub src: *mut u8_,
}

#[derive(Copy, Clone, ConstDefault)]
pub struct ScreenWorkVars {
    work_index: WorkIndex,
}

impl ScreenWorkVars {
    pub fn init(work_index: WorkIndex) -> Self {
        Self { work_index }
    }

    pub fn is_top(&self) -> bool {
        unsafe { screen_encode_vars.get(&self.work_index).is_top }
    }

    pub fn format(&self) -> u32_ {
        unsafe { screen_encode_vars.get(&self.work_index).format }
    }

    pub fn src(&self) -> *mut u8_ {
        unsafe { screen_encode_vars.get(&self.work_index).src }
    }

    pub fn work_index(&self) -> WorkIndex {
        self.work_index
    }

    #[named]
    pub unsafe fn release_work_done(&self) {
        let mut count = mem::MaybeUninit::uninit();
        let res = svcReleaseSemaphore(
            count.as_mut_ptr(),
            (*syn_handles).works.get(&self.work_index).work_done,
            1,
        );
        if res != 0 {
            nsDbgPrint!(
                releaseSemaphoreFailed,
                c_str!("work_done"),
                self.work_index.get(),
                res
            );
        }
    }

    pub unsafe fn release_skip(&self) {
        let w = self.work_index();
        let syn = (*syn_handles).works.get(&w);
        let f = syn.work_done_count.fetch_add(1, Ordering::AcqRel);
        let core_count = entries::work_thread::get_core_count_in_use();
        if f == core_count.get() - 1 {
            syn.work_done_count.store(0, Ordering::Release);
            syn.work_begin_flag.store(false, Ordering::Release);
            self.release_work_done();
        }
    }

    #[named]
    pub unsafe fn release_work_begin_ready(self, t: &ThreadId, skip: bool) {
        let mut count = mem::MaybeUninit::uninit();
        (*syn_handles)
            .works
            .get(&self.work_index())
            .work_begin_skip
            .store(skip, Ordering::Release);
        for j in ThreadId::up_to(&entries::work_thread::get_core_count_in_use()) {
            if j != *t {
                let res = svcReleaseSemaphore(
                    count.as_mut_ptr(),
                    (*syn_handles).threads.get(&j).work_begin_ready,
                    1,
                );
                if res != 0 {
                    nsDbgPrint!(
                        releaseSemaphoreFailed,
                        c_str!("work_begin_ready"),
                        self.work_index().get(),
                        res
                    );
                }
            }
        }
    }
}

pub static mut screen_encode_vars: RangedArray<ScreenEncodeVars, WORK_COUNT> = const_default();

pub extern "C" fn thread_screen(_: *mut c_void) {
    unsafe {
        __system_initSyscalls();
        safe_impl::thread_screen_loop();
        svcExitThread()
    }
}
