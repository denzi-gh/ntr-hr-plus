use super::*;

static mut priority_is_top: bool = false;
static mut priority_factor: u32_ = 0;
static mut priority_factor_scaled: u32_ = 0;
static mut frame_counts: RangedArray<u32_, SCREEN_COUNT> = const_default();
static mut frame_queues: RangedArray<u32_, SCREEN_COUNT> = const_default();
static mut screen_work_index: u32_ = const_default();
static mut no_skip_frames: RangedArray<bool, SCREEN_COUNT> = const_default();
static mut last_frame_timings: RangedArray<u32_, SCREEN_COUNT> = const_default();
static mut frame_times: [u32_; SCREEN_COUNT as usize] = const_default();
static mut blit_formats: RangedArray<u8_, SCREEN_COUNT> = const_default();
static mut port_game_pid: AtomicU32 = const_default();

const frame_time_factor: u32 = 3;
pub fn get_frame_time(s: ScreenIndex) -> u32 {
    unsafe { *frame_times.get_unchecked(s.get() as usize) }
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

type DmaHandles = RangedArray<Handle, WORK_COUNT>;

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

pub const IMG_WORK_COUNT: u32_ = 3;
pub type ImgWorkIndex = Ranged<IMG_WORK_COUNT>;
pub type ImgBufs = RangedArray<*mut u8_, IMG_WORK_COUNT>;

#[derive(ConstDefault)]
pub struct ImgInfo {
    pub bufs: ImgBufs,
    pub screen: u8_,
    pub screen_prev: u8_,
    pub work: u8_,
    pub has_new: bool,
    pub lock: Handle,
}

pub struct ImgInfoLock<'a>(&'a mut ImgInfo);

impl<'a> ImgInfoLock<'a> {
    pub unsafe fn lock(img: &'a mut ImgInfo) -> Option<Self> {
        let _ = img.img_info_lock()?;
        Some(Self(img))
    }
}

impl<'a> Drop for ImgInfoLock<'a> {
    fn drop(&mut self) {
        unsafe { self.0.img_info_unlock() };
    }
}

impl ImgInfo {
    #[named]
    unsafe fn img_info_lock(&self) -> Option<()> {
        while !entries::work_thread::reset_threads() {
            let res = svcWaitSynchronization(self.lock, THREAD_WAIT_NS);

            if res == 0 {
                return Some(());
            }
            if res != RES_TIMEOUT as s32 {
                nsDbgPrint!(waitForSyncFailed, c_str!("img_info.lock"), res);
                entries::work_thread::set_reset_threads_ar();
                return None;
            }
        }
        None
    }

    #[named]
    unsafe fn img_info_unlock(&self) {
        let res = svcReleaseMutex(self.lock);
        if res != 0 {
            nsDbgPrint!(releaseMutexFailed, c_str!("img_info.lock"), res);
        }
    }
}

pub type ImgInfos = RangedArray<ImgInfo, SCREEN_COUNT>;
static mut img_infos: ImgInfos = const_default();

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
    screen_work_index = WorkIndex::init().get();

    for i in 0..SCREEN_COUNT as usize {
        frame_times[i] = SYSCLOCK_ARM11;
    }
}

#[named]
pub unsafe fn init_img_info(is_top: bool, j: &ImgWorkIndex, m: &mut [u8]) -> Option<()> {
    let info = img_infos.get_b_mut(is_top);
    *info = const_default();
    *info.bufs.get_mut(&j) = m.as_mut_ptr();
    let res = svcCreateMutex(&mut info.lock, false);
    if res != 0 {
        nsDbgPrint!(createMutexFailed, c_str!("img_infos.lock"), res);
        return None;
    }
    Some(())
}

#[allow(dead_code)]
pub unsafe fn get_img_info(is_top: bool, j: &ImgWorkIndex) -> &mut *mut u8 {
    img_infos.get_b_mut(is_top).bufs.get_mut(&j)
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

    pub fn screen_img(is_top: bool) -> *mut u8_ {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            *iinfo
                .bufs
                .get_r(&ImgWorkIndex::init_unchecked(iinfo.screen as u32_))
        }
    }

    pub fn screen_prev_img(is_top: bool) -> *mut u8_ {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            *iinfo
                .bufs
                .get_r(&ImgWorkIndex::init_unchecked(iinfo.screen_prev as u32_))
        }
    }

    pub fn work_img(is_top: bool) -> *mut u8_ {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            *iinfo
                .bufs
                .get_r(&ImgWorkIndex::init_unchecked(iinfo.work as u32_))
        }
    }

    pub fn screen_next(is_top: bool) -> bool {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            let ret = iinfo.has_new;
            if let Some(mut l) = ImgInfoLock::lock(iinfo) {
                let iinfo = &mut l.0;
                iinfo.screen_prev = iinfo.screen;
                iinfo.screen = (iinfo.screen + 1) % IMG_WORK_COUNT as u8_;
                if iinfo.screen == iinfo.work {
                    iinfo.screen = (iinfo.screen + 1) % IMG_WORK_COUNT as u8_;
                }
                iinfo.has_new = true;
            };
            !ret
        }
    }

    pub fn work_next(is_top: bool) {
        unsafe {
            let iinfo = img_infos.get_b_mut(is_top);
            if let Some(mut l) = ImgInfoLock::lock(iinfo) {
                let iinfo = &mut l.0;
                iinfo.work = iinfo.screen_prev;
                iinfo.has_new = false;
            };
        }
    }

    pub fn get_last_frame_timing(is_top: bool) -> u32_ {
        unsafe { *last_frame_timings.get_b(is_top) }
    }

    pub fn set_last_frame_timing(is_top: bool, timing: u32_) {
        unsafe { *last_frame_timings.get_b_mut(is_top) = timing }
    }

    #[named]
    pub fn dma_sync(w: WorkIndex) {
        unsafe {
            let dma = *cap_params.dmas.get(&w);

            let res = svcWaitSynchronization(dma, THREAD_WAIT_NS);
            if res != 0 {
                if res != RES_TIMEOUT as s32 {
                    nsDbgPrint!(waitForSyncFailed, c_str!("dmas"), res);
                    svcSleepThread(THREAD_WAIT_NS);
                }
            }
        }
    }

    pub fn frame_changed(is_top: bool, format: u32_, w: WorkIndex) -> bool {
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

            Self::dma_sync(w);
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

    pub fn no_skip_frame(&self, is_top: bool, format: u32_, w: WorkIndex) -> bool {
        let timing = unsafe { svcGetSystemTick() as u32_ };
        let last_timing = Self::get_last_frame_timing(is_top);
        let timing_allowance =
            if unsafe { crate::entries::thread_nwm::get_reliable_stream_delta_prog() } {
                SYSCLOCK_ARM11 / 2
            } else {
                SYSCLOCK_ARM11
            };
        let frame_time = timing - last_timing;
        if frame_time >= timing_allowance {
            unsafe { set_no_skip_frame(is_top) };
        }
        let skip_frame = !unsafe { reset_no_skip_frame(is_top) }
            && !Self::get_blit_format_changed(is_top, format)
            && !Self::frame_changed(is_top, format, w);

        if !skip_frame {
            Self::set_last_frame_timing(is_top, timing);
            unsafe {
                let s = if is_top { 0 } else { 1 };
                let cur = &mut frame_times[s];
                *cur = (*cur * (frame_time_factor - 1) + frame_time) / frame_time_factor;
                (*ov_stats).s[s].frame_time = *cur;
            }
        }

        !skip_frame
    }

    pub fn screen_work_index(&self) -> WorkIndex {
        unsafe { WorkIndex::init_unchecked(screen_work_index) }
    }

    pub fn screen_work_index_next(&self) {
        let mut w = ScreenThreadVars(()).screen_work_index();
        w.next_wrapped();
        unsafe {
            screen_work_index = w.get();
        }
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

    #[named]
    pub fn release(&self, is_top: bool, format: u32_, work_index: WorkIndex) {
        unsafe {
            *screen_encode_vars.get_mut(&work_index) = ScreenEncodeVars::init(is_top, format);

            if ScreenThreadVars::screen_next(is_top) {
                let mut count = mem::MaybeUninit::<s32>::uninit();
                for j in ThreadId::up_to(&crate::entries::work_thread::get_core_count_in_use()) {
                    let res = svcReleaseSemaphore(
                        count.as_mut_ptr(),
                        (*syn_handles).threads.get(&j).work_ready,
                        1,
                    );
                    if res != 0 {
                        nsDbgPrint!(
                            releaseSemaphoreFailed,
                            c_str!("work_ready"),
                            work_index.get(),
                            res
                        );
                    }
                }
            }
            self.screen_work_index_next();
        }
    }
}

#[derive(ConstDefault)]
pub struct ScreenEncodeVars {
    is_top: bool,
    format: u32_,
}

#[derive(Copy, Clone, ConstDefault)]
pub struct ScreenWorkVars {
    work_index: WorkIndex,
}

impl ScreenEncodeVars {
    pub fn init(is_top: bool, format: u32_) -> Self {
        ScreenEncodeVars { is_top, format }
    }
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

    pub fn work_index(&self) -> WorkIndex {
        self.work_index
    }
}

static mut screen_encode_vars: RangedArray<ScreenEncodeVars, WORK_COUNT> = const_default();

#[named]
pub unsafe fn screen_encode_acquire(t: &ThreadId) -> Option<()> {
    loop {
        if crate::entries::work_thread::reset_threads() {
            return None;
        }

        let res = svcWaitSynchronization((*syn_handles).threads.get(&t).work_ready, THREAD_WAIT_NS);

        if res != 0 {
            if res != RES_TIMEOUT as s32 {
                nsDbgPrint!(waitForSyncFailed, c_str!("work_ready"), res);
                svcSleepThread(THREAD_WAIT_NS);
            }
            continue;
        }

        return Some(());
    }
}

pub extern "C" fn thread_screen(_: *mut c_void) {
    unsafe {
        __system_initSyscalls();
        safe_impl::thread_screen_loop();
        svcExitThread()
    }
}
