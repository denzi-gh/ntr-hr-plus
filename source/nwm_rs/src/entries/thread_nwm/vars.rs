use super::*;

pub static mut RELIABLE_STREAM_CB: *mut rp_cb = const_default();
pub static mut RELIABLE_STREAM_CB_LOCK: Handle = 0;
pub static mut RELIABLE_STREAM_CB_EVT: Handle = 0;
pub static mut RELIABLE_STREAM_CB_INITED: AtomicBool = const_default();

pub static mut SEG_MEM_SEM: Handle = 0;
pub static mut SEG_MEM_LOCK: Handle = 0;
pub static mut SEG_MEM_TERM_SEM: Handle = 0;

#[named]
pub unsafe fn once_reliable_stream_cb() -> Option<()> {
    unsafe {
        if let Some(m) = request_mem_from_pool::<{ mem::size_of::<rp_cb>() }>() {
            RELIABLE_STREAM_CB = m.to_ptr() as *mut rp_cb;
            let cb = &mut *RELIABLE_STREAM_CB;
            slice::from_raw_parts_mut(cb as *mut _ as *mut u8, mem::size_of::<rp_cb>()).fill(0);
        } else {
            return None;
        }

        let res = svcCreateMutex(&mut RELIABLE_STREAM_CB_LOCK, false);
        if res != 0 {
            ns_dbg_print!(failed, c_str!("Create nwm mutex failed"), res);
            return None;
        }
        let res = create_event(&mut RELIABLE_STREAM_CB_EVT);
        if res != 0 {
            ns_dbg_print!(failed, c_str!("Create nwm recv event"), res);
            return None;
        }
        RELIABLE_STREAM_CB_INITED.store(true, Ordering::Release);
    }
    Some(())
}

#[named]
pub unsafe fn init_reliable_stream_cb(qos: u32) -> Option<()> {
    unsafe {
        let cb = &mut *RELIABLE_STREAM_CB;
        if mp_init(
            (*cb.send_bufs.as_ptr()).len(),
            cb.send_bufs.len(),
            cb.send_bufs.as_mut_ptr().as_mut_ptr() as *mut _,
            &mut cb.send_pool,
        ) < 0
        {
            ns_dbg_print!(mp_init_failed, c_str!("send_pool"));
            return None;
        }

        if mp_init(
            (*cb.cur_send_bufs.as_ptr()).len(),
            cb.cur_send_bufs.len(),
            cb.cur_send_bufs.as_mut_ptr().as_mut_ptr() as *mut _,
            &mut cb.cur_send_pool,
        ) < 0
        {
            ns_dbg_print!(mp_init_failed, c_str!("cur_send_pool"));
            return None;
        }

        let res = rp_syn_init1(
            &mut cb.nwm_syn,
            0,
            ptr::null_mut(),
            0,
            ((RP_ARQ_BUFS_COUNT * qos + RP_QOS_MAX / 2) / RP_QOS_MAX) as i32,
            cb.nwm_syn_data.as_mut_ptr(),
        );
        if res != 0 {
            ns_dbg_print!(failed, c_str!("Nwm syn init"), res);
            return None;
        }
    }
    Some(())
}

pub unsafe fn init_ov_stats() {
    unsafe {
        slice::from_raw_parts_mut(
            config_consts::OV_STATS as *mut u8,
            mem::size_of_val(&*config_consts::OV_STATS),
        )
        .fill(0);
        (*config_consts::OV_STATS).kcp_mode = match entries::thread_nwm::get_reliable_stream() {
            entries::thread_nwm::ReliableStream::None => 0,
            entries::thread_nwm::ReliableStream::KCP => {
                if entries::thread_nwm::get_reliable_stream_delta_prog() {
                    2
                } else {
                    1
                }
            }
        };
    }
}

#[named]
pub unsafe fn init_seg_mem_handles(qos: u32) -> Option<()> {
    unsafe {
        let res = svcCreateSemaphore(
            &mut SEG_MEM_SEM,
            ((RP_ARQ_ENCODE_BUFS_COUNT * qos + RP_QOS_MAX / 2) / RP_QOS_MAX) as s32,
            ((RP_ARQ_ENCODE_BUFS_COUNT * qos + RP_QOS_MAX / 2) / RP_QOS_MAX) as s32,
        );
        if res != 0 {
            ns_dbg_print!(create_semaphore_failed, c_str!("SEG_MEM_SEM"), res);
            return None;
        }
        let res = svcCreateMutex(&mut SEG_MEM_LOCK, false);
        if res != 0 {
            ns_dbg_print!(create_mutex_failed, c_str!("SEG_MEM_LOCK"), res);
            return None;
        }

        let term_sem_count = if get_reliable_stream_delta_prog() {
            WORK_COUNT as i32
        } else {
            1
        };
        let res = svcCreateSemaphore(&mut SEG_MEM_TERM_SEM, term_sem_count, term_sem_count);
        if res != 0 {
            ns_dbg_print!(create_semaphore_failed, c_str!("SEG_MEM_TERM_SEM"), res);
            return None;
        }
    }
    Some(())
}

pub fn cleanup_seg_mem_handles() {
    unsafe {
        let _ = svcCloseHandle(SEG_MEM_TERM_SEM);

        let _ = svcCloseHandle(SEG_MEM_LOCK);
        let _ = svcCloseHandle(SEG_MEM_SEM);
    }
}

pub type NwmHdr = [u8; entries::start_up::NwmHdr::N];
static mut CURRENT_NWM_HDR: NwmHdr = const_default();

pub fn get_current_nwm_hdr() -> *mut NwmHdr {
    unsafe { &mut CURRENT_NWM_HDR }
}

static mut KCP_CONV: u8 = 0;
#[unsafe(export_name = "rp_max_qos")]
static mut MAX_QOS: u32 = const_default();

#[unsafe(export_name = "rp_current_qos")]
static mut CURRENT_QOS: AtomicU32 = const_default();

pub fn rp_delta_q_qos() -> u32 {
    unsafe { CURRENT_QOS.load(Ordering::Acquire) }
}

#[unsafe(no_mangle)]
extern "C" fn rp_set_qos(qos: u32) {}

#[unsafe(no_mangle)]
extern "C" fn rp_udp_output(buf: *mut u8, len: s32, tick: *mut u32, kcp: *mut ikcpcb) -> s32 {
    0
}

#[unsafe(no_mangle)]
extern "C" fn nsControlRecv(fd: c_int) -> c_int {
    0
}

#[unsafe(no_mangle)]
extern "C" fn ikcp_seg_data_buf_malloc() -> *mut c_char {
    ptr::null_mut()
}

#[unsafe(no_mangle)]
extern "C" fn ikcp_seg_data_buf_free(dst: *const c_char) {}

#[unsafe(no_mangle)]
extern "C" fn rp_seg_data_buf_free(data_buf: *const c_char) {}

pub extern "C" fn kcp_thread_nwm(_: *mut c_void) {}

pub extern "C" fn thread_nwm(_: *mut c_void) {}

unsafe fn init_reliable_stream(flags: u32, qos: u32) -> Option<()> {
    let mut nwm_lock = if let Some(l) = NwmCbLock::lock() {
        l
    } else {
        return None;
    };

    unsafe {
        let reliable_stream = flags & RP_CONFIG_RELIABLE_STREAM_FLAG > 0;
        RELIABLE_STREAM.store(reliable_stream, Ordering::Release);
        RELIABLE_STREAM_DELTA_PROG.store(
            reliable_stream && flags & RP_CONFIG_RELIABLE_STREAM_DELTA_PROG > 0,
            Ordering::Release,
        );
        MAX_QOS = qos;

        set_packet_data_size();

        match get_reliable_stream() {
            ReliableStream::None => {}
            ReliableStream::KCP => {
                let kcp = &mut nwm_lock.get().ikcp;

                if ikcp_create(kcp, KCP_CONV as u16) < 0 {
                    return None;
                }
                let sndwnd = ((ARQ_BUFS_COUNT * qos + RP_QOS_MAX / 2) / RP_QOS_MAX) as i32;
                let curwnd = ((ARQ_CUR_BUFS_COUNT * qos + RP_QOS_MAX / 2) / RP_QOS_MAX) as i32;
                if ikcp_wndsize(kcp, sndwnd, curwnd) != 0 {
                    return None;
                }

                KCP_CONV += 1;
            }
        }
    }

    Some(())
}

static mut MIN_SEND_INTERVAL_TICK: u32 = const_default();
static mut MIN_SEND_INTERVAL_NS: u32 = const_default();

unsafe fn init_min_send_interval(qos: u32) {
    unsafe {
        (*config_consts::OV_STATS).kcp_qos = qos;
        CURRENT_QOS.store(qos, Ordering::Release);
        let tick = (SYSCLOCK_ARM11 as u64 * PACKET_SIZE as u64) / qos as u64;
        MIN_SEND_INTERVAL_TICK = tick as u32;
        MIN_SEND_INTERVAL_NS = DurationTick::init(tick as s64).get_ns().get() as u32;
    }
}

static mut NWM_WORK_INDEX: WorkIndex = WorkIndex::init(0);
static mut NWM_THREAD_INDEX: ThreadIndex = ThreadIndex::init(0);

static mut NWM_NEED_SYN: RangedArray<bool, WORK_COUNT> = const_default();
static mut RP_OUTPUT_NEXT_TICK: u32 = const_default();
static mut CUR_SEG_MEM_COUNT: u32 = 0;

pub unsafe fn init(dst_flags: u32, qos: u32) -> Option<()> {
    unsafe {
        init_reliable_stream(dst_flags, qos)?;
        init_min_send_interval(qos);

        for i in WorkIndex::all() {
            *NWM_NEED_SYN.get_mut(&i) = true;
        }
        NWM_WORK_INDEX = WorkIndex::init(0);
        NWM_THREAD_INDEX = ThreadIndex::init(0);

        RP_OUTPUT_NEXT_TICK = get_system_tick().get() as u32 + MIN_SEND_INTERVAL_TICK;
        CUR_SEG_MEM_COUNT = 0;
    }

    Some(())
}

#[derive(PartialEq, Eq)]
pub enum ReliableStream {
    None,
    KCP,
}

static mut RELIABLE_STREAM: AtomicBool = const_default();
static mut RELIABLE_STREAM_DELTA_PROG: AtomicBool = const_default();

pub fn get_reliable_stream() -> ReliableStream {
    if unsafe { RELIABLE_STREAM.load(Ordering::Acquire) } {
        ReliableStream::KCP
    } else {
        ReliableStream::None
    }
}

pub fn get_reliable_stream_delta_prog() -> bool {
    unsafe { RELIABLE_STREAM_DELTA_PROG.load(Ordering::Acquire) }
}

#[derive(ConstDefault)]
pub struct DataInfo {
    pub send_pos: *mut u8,
    pub pos: AtomicPtr<u8>,
    pub flag: AtomicU32,
}

#[derive(ConstDefault)]
pub struct NwmThreadInfo {
    pub buf: *mut u8,
    pub buf_packet_last: *mut u8,
    pub info: DataInfo,
}

pub type NwmWorkInfo = RangedArray<NwmThreadInfo, RP_CORE_COUNT_MAX>;
pub type NwmInfo = RangedArray<NwmWorkInfo, WORK_COUNT>;
static mut NWM_INFOS: NwmInfo = const_default();
static mut PACKET_DATA_SIZE: usize = 0;

pub fn get_packet_data_size() -> usize {
    unsafe { PACKET_DATA_SIZE }
}

pub const fn get_packet_data_size_const<const RS: bool>() -> usize {
    if RS {
        PACKET_DATA_SIZE_KCP
    } else {
        PACKET_DATA_SIZE_COMPAT
    }
}

const PACKET_DATA_SIZE_COMPAT: usize = {
    let size = (PACKET_SIZE - DATA_HDR_SIZE) as usize;
    assert!(size % mem::size_of::<usize>() == 0);
    size
};

pub const PACKET_DATA_SIZE_KCP: usize =
    (PACKET_SIZE - ARQ_OVERHEAD_SIZE - ARQ_DATA_HDR_SIZE) as usize;

unsafe fn set_packet_data_size() {
    unsafe {
        PACKET_DATA_SIZE = match get_reliable_stream() {
            ReliableStream::None => get_packet_data_size_const::<false>(),
            ReliableStream::KCP => get_packet_data_size_const::<true>(),
        }
    }
}

pub unsafe fn init_nwm_infos(nwm_bufs: &entries::thread_main::NwmBufs, core_count: CoreCount) {
    unsafe {
        let hdr_size = (NWM_HDR_SIZE as usize + DATA_HDR_SIZE as usize + mem::size_of::<usize>()
            - 1)
            / mem::size_of::<usize>()
            * mem::size_of::<usize>();
        let packet_data_size = PACKET_DATA_SIZE;
        for i in WorkIndex::all() {
            for j in ThreadIndex::up_to(&thread_index_last(core_count)) {
                let info = NWM_INFOS.get_mut(&i).get_mut(&j);
                let buf_size = (entries::thread_main::NWM_BUFFER_SIZE / core_count.get() as usize
                    - hdr_size)
                    / packet_data_size
                    * packet_data_size
                    + hdr_size;
                let buf = nwm_bufs.get(&i).add(j.get() as usize * buf_size);
                info.buf = buf.add(hdr_size);
                info.buf_packet_last = buf.add(buf_size as usize - packet_data_size);
            }
        }
    }
}

pub fn nwm_info(work_index: WorkIndex) -> &'static mut NwmWorkInfo {
    unsafe { NWM_INFOS.get_mut(&work_index) }
}

#[derive(ConstDefault)]
struct DataHdr([u8; DATA_HDR_SIZE as usize]);
static mut DATA_BUF_HDRS: RangedArray<DataHdr, WORK_COUNT> = const_default();

impl DataHdr {
    fn init(frame_id: u8, is_top: bool) -> Self {
        Self([frame_id, is_top as u8, 0, 0])
    }
}

#[named]
pub unsafe fn nwm_done_acquire(w: WorkIndex, frame_id: u8, is_top: bool) -> bool {
    unsafe {
        if wait_syn(
            cname!(),
            SYN_HANDLES.works.get(&w).nwm_done,
            c_str!("nwm_done"),
        )
        .is_none()
        {
            return false;
        }

        let ninfo = NWM_INFOS.get_mut(&w);
        for j in ThreadIndex::up_to(&thread_index_last(core_count_in_use())) {
            let ninfo = ninfo.get_mut(&j);
            let info = &mut ninfo.info;
            let buf = ninfo.buf;

            *info = DataInfo {
                send_pos: buf,
                pos: AtomicPtr::new(buf),
                flag: AtomicU32::new(0),
            };
        }

        let hdr = DATA_BUF_HDRS.get_mut(&w);
        *hdr = DataHdr::init(frame_id, is_top);

        true
    }
}

#[named]
pub unsafe fn nwm_ready_release(w: &WorkIndex) {
    unsafe {
        release_sem(
            cname!(),
            SYN_HANDLES.works.get(&w).nwm_ready,
            c_str!("nwm_ready"),
        )
    }
}

#[derive(PartialEq, Eq)]
pub struct NwmCbLock();

impl NwmCbLock {
    pub fn lock() -> Option<Self> {
        nwm_cb_lock()?;
        Some(Self())
    }

    pub fn lock_ns(wait: s64) -> Option<Self> {
        nwm_cb_lock_ns(wait)?;
        Some(Self())
    }

    pub fn get(&mut self) -> &mut rp_cb {
        unsafe { &mut *RELIABLE_STREAM_CB }
    }

    pub fn unlock(&mut self) -> NwmCbUnlock {
        unsafe { nwm_cb_unlock() };
        NwmCbUnlock(self)
    }
}

pub struct NwmCbUnlock<'a>(&'a mut NwmCbLock);

impl Drop for NwmCbUnlock<'_> {
    fn drop(&mut self) {
        if nwm_cb_lock().is_none() {
            panic!()
        }
    }
}

impl Drop for NwmCbLock {
    fn drop(&mut self) {
        unsafe { nwm_cb_unlock() };
    }
}

fn nwm_cb_lock() -> Option<()> {
    nwm_cb_lock_ns(THREAD_WAIT_NS.get())
}

#[named]
fn nwm_cb_lock_ns(wait: s64) -> Option<()> {
    wait_syn(
        cname!(),
        unsafe { RELIABLE_STREAM_CB_LOCK },
        c_str!("RELIABLE_STREAM_CB_LOCK"),
    )?;
    Some(())
}

#[named]
unsafe fn nwm_cb_unlock() {
    unsafe {
        release_mutex(
            cname!(),
            RELIABLE_STREAM_CB_LOCK,
            c_str!("RELIABLE_STREAM_CB_LOCK"),
        );
    }
}

static mut RP_FRAME_COMPRESSED_SIZE: [AtomicU32; WORK_COUNT as usize] = const_default();

pub const JPEG_COMP_COUNT_SIZE_NBITS: u32 = 19;
pub const JPEG_COMP_COUNT_BLKN_NBITS: u32 = 13;

const _JPEG_COMP_COUNT_NBITS_ASSERT: () = {
    assert!(JPEG_COMP_COUNT_SIZE_NBITS + JPEG_COMP_COUNT_BLKN_NBITS <= u32::BITS);
};

pub unsafe fn rp_dq_update_size(comp_size: &mut AtomicU32, size: u32, blkn: u16) {
    let mut curr = comp_size.load(Ordering::Acquire);
    loop {
        let prev_size = curr & ((1 << JPEG_COMP_COUNT_SIZE_NBITS) - 1);
        let prev_blkn =
            (curr >> JPEG_COMP_COUNT_SIZE_NBITS) & ((1 << JPEG_COMP_COUNT_BLKN_NBITS) - 1);

        let next_size = prev_size + size;
        let next_blkn = prev_blkn + blkn as u32;
        let next = (next_size & ((1 << JPEG_COMP_COUNT_SIZE_NBITS) - 1))
            | ((next_blkn & ((1 << JPEG_COMP_COUNT_BLKN_NBITS) - 1)) << JPEG_COMP_COUNT_SIZE_NBITS);

        match comp_size.compare_exchange_weak(curr, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(temp) => curr = temp,
        }
    }
}

pub fn rp_update_size(w: WorkIndex, size: u32) {
    unsafe {
        RP_FRAME_COMPRESSED_SIZE
            .get_unchecked_mut(w.get() as usize)
            .fetch_add(size, Ordering::AcqRel);
    }
}

pub fn rp_get_size(w: WorkIndex) -> u32 {
    unsafe {
        RP_FRAME_COMPRESSED_SIZE
            .get_unchecked_mut(w.get() as usize)
            .load(Ordering::Acquire)
    }
}

pub fn rp_clear_size(w: WorkIndex) {
    unsafe {
        RP_FRAME_COMPRESSED_SIZE
            .get_unchecked_mut(w.get() as usize)
            .store(0, Ordering::Release)
    }
}

pub unsafe fn rp_send_buffer<const RS: bool>(dst: &mut jpeg::WorkerDst, term: bool) -> bool {
    false
}

pub unsafe fn rp_data_buf_malloc() -> Option<*mut c_char> {
    None
}

unsafe fn rp_data_buf_free(dst: *const ::libc::c_char) {}

pub fn rp_data_buf_data(dst: *mut c_char) -> *mut c_char {
    unsafe { dst.add((NWM_HDR_SIZE + ARQ_OVERHEAD_SIZE + ARQ_DATA_HDR_SIZE) as usize) }
}
