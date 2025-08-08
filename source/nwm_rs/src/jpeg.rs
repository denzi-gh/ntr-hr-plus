// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

#![allow(unused_macros)]

mod vars;

use crate::*;
pub use vars::*;

const DELTA_Q_COUNT: u8 = 32;
const DELTA_Q_MAX: f32 = 7f32;

const DELTA_Q_STEP: f32 = DELTA_Q_MAX / DELTA_Q_COUNT as f32;
const MIN_DCT_COMP_SIZE: usize = 9;

const SCALE_QD_F: f32 = (DELTA_Q_COUNT - 1) as f32;
const SCALE_QD_I_F: f32 = 1f32 / SCALE_QD_F;

pub struct JpegDqRet {
    pub delta_q: u8,
    pub mcus: u16,
}

#[derive(ConstDefault)]
struct DeltaQCoefs {
    m: f32,
    p: f32,
    d: f32,
}

#[derive(ConstDefault)]
struct DeltaQManager {
    f: [DeltaQCoefs; RP_DELTA_Q_COEFS_COUNT as usize],
    qb: f32,
    qc: f32,
    q: f32,
    nbits: f32,
    qd: u8,
}

#[derive(ConstDefault, Clone, Copy)]
struct QuantizeCounts {
    nbits: u16,
}

#[derive(ConstDefault)]
struct QuantizeRet {
    dc: QuantizeCounts,
    ac: QuantizeCounts,
}

struct DeltaQParams {
    qf: [f32; NUM_QUANT_TBLS],
    q_steps_i: f32,
    m: f32,
}

pub struct JpegShared {
    quality: u32,
    chroma_ss: u8,
    quant_tbls: QuantTbls,
    divisors: Divisors,
    div_shifts: [[u8; DCTSIZE2]; NUM_QUANT_TBLS],
    div_delta_q_shifts: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    core_count: CoreCount,
    comp_infos: *const CompInfos,
    pub max_h_samp_factor: usize,
    pub max_v_samp_factor: usize,
    pub max_blocks_in_mcu: usize,
    pub mcu_row_size: usize,
    pub mcu_col_size: usize,
    pub mcus_per_row: usize,
    pub mcus_top: u16,
    pub mcus_bot: u16,
    pub last_restart_range: u32,
    qos_adj: f32,
    jpeg_tbls: JpegTbls,
    delta_q_tbls: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    delta_q0_tbls: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    pub work_sem: [Handle; WORK_COUNT as usize],
    pub screen_sem: [Handle; SCREEN_COUNT as usize],
    delta_q_params: DeltaQParams,
}

const DELTA_Q_CACHE_COUNTS: [u8; MAX_COMPONENTS] = [10, 5, 5];
const DELTA_Q_CACHE_MAX: u8 = {
    let mut max = 0;
    let mut i = 0;
    loop {
        if i >= MAX_COMPONENTS {
            break;
        }
        if max < DELTA_Q_CACHE_COUNTS[i] {
            max = DELTA_Q_CACHE_COUNTS[i];
        }
        i += 1;
    }
    max
};
const DELTA_Q_CACHE_TOTAL: u8 = {
    let mut total = 0;
    let mut i = 0;
    loop {
        if i >= MAX_COMPONENTS {
            break;
        }
        total += DELTA_Q_CACHE_COUNTS[i];
        i += 1;
    }
    total
};

pub struct DeltaQCache {
    cache: JBlock,
    next: JBlock,
    xpos: u16,
    ypos: u16,
}

pub struct JpegSharedMut {
    pub compressed_size: [AtomicU32; SCREEN_COUNT as usize],
    pub work_inited: [bool; WORK_COUNT as usize],
    pub work_sem_count: [AtomicU8; WORK_COUNT as usize],
    pub screen_bool: [AtomicBool; SCREEN_COUNT as usize],
    pub last_restart_interval: [u16; SCREEN_COUNT as usize],
    delta_q: [u8; SCREEN_COUNT as usize],
    work_delta_q: [u8; WORK_COUNT as usize],
    dq_rescale_prev: [i8; WORK_COUNT as usize],
    rp_shifts: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; WORK_COUNT as usize],
    delta_q_cache: [[DeltaQCache; DELTA_Q_CACHE_TOTAL as usize]; WORK_COUNT as usize],
    delta_q_cache_next: [[u8; MAX_COMPONENTS]; WORK_COUNT as usize],
    delta_q_calc: [DeltaQManager; SCREEN_COUNT as usize],
    dq_prev_coeffs_top: [JCoef; DELTA_Q_PREV_COEFFS_TOP_N],
    dq_prev_coeffs_bot: [JCoef; DELTA_Q_PREV_COEFFS_BOT_N],
    rand32: Rand32,
}

impl JpegSharedMut {
    pub fn once(&mut self) {
        unsafe {
            slice::from_raw_parts_mut(self as *mut _ as *mut u8, mem::size_of_val(self)).fill(0);
        }
        self.rand32 = Rand32::new(get_system_tick().get() as u64);
    }

    fn init(&mut self, delta_prog: bool, max_blocks_in_mcu: usize, q_steps: f32) {
        if delta_prog {
            self.compressed_size = const_default();
            for s in 0..SCREEN_COUNT {
                self.delta_q[s as usize] = DELTA_Q_COUNT - 1;
            }
        }

        self.delta_q_calc = const_default();

        if delta_prog {
            for i in 0..SCREEN_COUNT as usize {
                for j in 0..RP_DELTA_Q_COEFS_COUNT as usize {
                    self.delta_q_calc[i].f[j].m = q_steps;
                    self.delta_q_calc[i].f[j].p = q_steps * q_steps;
                }
                self.delta_q_calc[i].nbits = (MIN_DCT_COMP_SIZE * max_blocks_in_mcu) as f32;
            }
        }
    }
}

const fn jdiv_round_up(a: usize, b: usize) -> usize
/* Compute a/b rounded up to next integer, ie, ceil(a/b) */
/* Assumes a >= 0, b > 0 */
{
    (a + b - 1) / b
}

impl JpegShared {
    fn init(
        &mut self,
        quality: u32,
        delta_prog: bool,
        core_count: CoreCount,
        hq: u32,
    ) -> (usize, f32) {
        self.quality = quality;
        self.quant_tbls
            .set_quality(if delta_prog { 100 } else { self.quality });
        self.divisors
            .set_divisors(&self.quant_tbls, &mut self.div_shifts);

        if delta_prog {
            for q in 0..DELTA_Q_COUNT {
                let div_shifts = &mut self.div_delta_q_shifts[q as usize];
                for i in 0..NUM_QUANT_TBLS {
                    let base_shifts = &self.div_shifts[i];
                    let shifts = &mut div_shifts[i];
                    let ltbl = &self.delta_q_tbls[q as usize][i];

                    for i in 0..DCTSIZE2 {
                        shifts[i] = base_shifts[i] + ltbl[i];
                    }
                }
            }
            const QOS_ADJ_B: f32 = u8::BITS as f32;
            const QOS_MIN_F: f32 = 0.625f32;
            const QOS_MAX_L_F: f32 = 0.875f32;
            const QOS_MAX_H_F: f32 = 0.75f32;
            self.qos_adj = QOS_ADJ_B * QOS_MIN_F
                + ((QOS_MAX_L_F
                    + (QOS_MAX_H_F - QOS_MAX_L_F)
                        * entries::thread_nwm::rp_delta_q_qos() as f32
                        * (1f32 / RP_QOS_MAX as f32)
                    - QOS_MIN_F)
                    * QOS_ADJ_B
                    * self.quality as f32
                    * (1f32 / RP_QUALITY_MAX as f32));
        }

        self.core_count = core_count;
        self.chroma_ss = hq as u8;
        self.set_comp_infos(hq, delta_prog)
    }

    fn once_delta_q_tbls(&mut self) {
        for d in (0..DELTA_Q_COUNT as usize).rev() {
            let f = DELTA_Q_MAX / DELTA_Q_COUNT as f32 * d as f32;

            for j in 0..NUM_QUANT_TBLS {
                let btbls = if j == 0 {
                    &STD_LUMINANCE_QUANT_TBL
                } else {
                    &STD_CHROMINANCE_QUANT_TBL
                };

                let mut log2_tbls: [f32; DCTSIZE2] = const_default();
                for i in 0..DCTSIZE2 {
                    let v = unsafe { log2f(btbls[i] as f32) };
                    log2_tbls[i] = v;
                }
                for i in 0..DCTSIZE2 {
                    let v = unsafe { roundf(f32::max(log2_tbls[i] - f, 0.0f32)) } as u8;
                    self.delta_q_tbls[d][j][i] = v;
                    let m = v - self.delta_q_tbls[DELTA_Q_COUNT as usize - 1][j][i];
                    self.delta_q0_tbls[d][j][i] = m;
                }
            }
        }
    }

    pub fn once(&mut self) {
        unsafe {
            slice::from_raw_parts_mut(self as *mut _ as *mut u8, mem::size_of_val(self)).fill(0);
        }
        self.jpeg_tbls = JpegTbls::once();
        self.once_delta_q_tbls();
    }

    fn set_comp_infos(&mut self, hq: u32, delta_prog: bool) -> (usize, f32) {
        let comp_infos = if hq as u8 == RP_CHROMASS_444 {
            &self.jpeg_tbls.comp_infos_444
        } else if hq as u8 == RP_CHROMASS_422 {
            &self.jpeg_tbls.comp_infos_422
        } else {
            &self.jpeg_tbls.comp_infos_420
        };
        self.comp_infos = comp_infos;
        self.max_h_samp_factor = 1;
        self.max_v_samp_factor = 1;
        self.max_blocks_in_mcu = 0;
        for i in 0..MAX_COMPONENTS {
            let info = &comp_infos.infos[i];
            self.max_h_samp_factor = cmp::max(self.max_h_samp_factor, info.h_samp_factor as usize);
            self.max_v_samp_factor = cmp::max(self.max_v_samp_factor, info.v_samp_factor as usize);
            self.max_blocks_in_mcu += info.h_samp_factor as usize * info.v_samp_factor as usize;
        }
        if self.max_blocks_in_mcu > MAX_BLOCKS_IN_MCU {
            panic!();
        }
        self.mcu_row_size = DCTSIZE * self.max_h_samp_factor;
        self.mcu_col_size = DCTSIZE * self.max_v_samp_factor;
        self.mcus_per_row = jdiv_round_up(GSP_SCREEN_WIDTH as usize, self.mcu_row_size);
        let mcu_rows_top = jdiv_round_up(GSP_SCREEN_HEIGHT_TOP as usize, self.mcu_col_size);
        self.mcus_top = (self.mcus_per_row * mcu_rows_top) as u16;
        let mcu_rows_bot = jdiv_round_up(GSP_SCREEN_HEIGHT_BOTTOM as usize, self.mcu_col_size);
        self.mcus_bot = (self.mcus_per_row * mcu_rows_bot) as u16;

        self.last_restart_range = if delta_prog { 64 } else { 32 };

        if delta_prog {
            let mut qf: [u16; NUM_QUANT_TBLS] = const_default();
            for ci in 0..MAX_COMPONENTS {
                let comp = &comp_infos.infos[ci];
                let mcu_we = comp.h_samp_exp;
                let mcu_he = comp.v_samp_exp;

                qf[comp.quant_tbl_no as usize] += 1 << mcu_we + mcu_he;
            }
            const QF_F: [f32; NUM_QUANT_TBLS] = [1.25f32, 2f32 / 3f32];
            let qf = {
                let mut ret: [f32; NUM_QUANT_TBLS] = const_default();
                for i in 0..NUM_QUANT_TBLS {
                    ret[i] = qf[i] as f32 * QF_F[i];
                }
                ret
            };
            self.delta_q_params.qf = qf;
            let qt = {
                let mut qt = 0f32;
                for i in 0..NUM_QUANT_TBLS {
                    qt += self.delta_q_params.qf[i];
                }
                qt
            };
            let q_step = (DELTA_Q_STEP * qt, DELTA_Q_STEP * (DCTSIZE2 - 1) as f32 * qt);
            let q_steps = q_step.0 + q_step.1;
            let q_steps_i = 1f32 / q_steps;
            self.delta_q_params.q_steps_i = q_steps_i * SCALE_QD_I_F;

            self.delta_q_params.m = (MIN_DCT_COMP_SIZE * self.max_blocks_in_mcu) as f32;

            (self.max_blocks_in_mcu, q_steps)
        } else {
            (0, 0f32)
        }
    }
}

#[derive(Copy, Clone, ConstDefault)]
pub struct ArqRpHdr {
    pub w: WorkIndex,
    pub t: ThreadIndex,
}

const _ARQ_RP_HDR_SIZE_ASSERT: () = {
    assert!(mem::size_of::<u16>() == ARQ_DATA_HDR_SIZE as usize);
};

const _ARQ_RP_W_SIZE_ASSERT: () = {
    assert!(WORK_COUNT - 1 <= ((1 << RP_KCP_HDR_W_NBITS) - 1));
};

// We store RP_CORE_COUNT_MAX as a special value to indicate term packet
const _ARQ_RP_T_SIZE_ASSERT: () = {
    assert!(RP_CORE_COUNT_MAX <= ((1 << RP_KCP_HDR_T_NBITS) - 1));
};

impl ArqRpHdr {
    pub unsafe fn write_hdr(&self, dst: *mut u8) {
        let hdr = (self.w.get() as u16) << (PID_NBITS + CID_NBITS)
            | (self.t.get() as u16) << (PID_NBITS + CID_NBITS + RP_KCP_HDR_W_NBITS);
        unsafe {
            ptr::copy_nonoverlapping(&hdr, dst as *mut _, 1);
        }
    }
}

const fn subsamp_constraint<const H_SAMP: bool, const V_SAMP: bool>() {
    match (H_SAMP, V_SAMP) {
        (true, true) => (),
        (true, false) => (),
        (false, true) => panic!(),
        (false, false) => (),
    }
}

struct SubSampConst<const H_SAMP: bool, const V_SAMP: bool>;

impl<const H_SAMP: bool, const V_SAMP: bool> SubSampConst<H_SAMP, V_SAMP> {
    const ASSERT: () = subsamp_constraint::<H_SAMP, V_SAMP>();
}

#[derive(Copy, Clone, ConstDefault)]
pub union WorkderDstUser {
    pub info: *const entries::thread_nwm::NwmThreadInfo,
    pub hdr: ArqRpHdr,
}

#[derive(Clone, ConstDefault)]
pub struct WorkerDst {
    pub blkn: u16,
    pub s: ScreenIndex,
    pub w: WorkIndex,
    pub dst: *mut u8,
    pub free_in_bytes: u16,
    pub user: WorkderDstUser,
}

impl WorkerDst {
    fn write_byte<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self, byte: u8) -> bool {
        if self.free_in_bytes == 0 {
            if !self.flush::<REL_STREAM, DELTA_Q>() {
                return false;
            }
        }
        unsafe { *self.dst = byte };
        self.dst = unsafe { self.dst.add(1) };
        self.free_in_bytes -= 1;

        true
    }

    fn write_bytes<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self, bytes: &[u8]) -> bool {
        let mut src = bytes.as_ptr();
        let mut len = bytes.len() as u16;

        if self.free_in_bytes > 0 {
            if self.free_in_bytes < len {
                unsafe {
                    ptr::copy_nonoverlapping(src, self.dst, self.free_in_bytes as usize);
                }
                len -= self.free_in_bytes;
                src = unsafe { src.add(self.free_in_bytes as usize) };
                unsafe { self.dst = self.dst.add(self.free_in_bytes as usize) };
                self.free_in_bytes = 0;
                if !self.flush::<REL_STREAM, DELTA_Q>() {
                    return false;
                }
            }
        } else {
            if !self.flush::<REL_STREAM, DELTA_Q>() {
                return false;
            }
        }

        unsafe {
            ptr::copy_nonoverlapping(src, self.dst, len as usize);
        }

        self.free_in_bytes -= len;
        self.dst = unsafe { self.dst.add(len as usize) };

        true
    }

    fn flush<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self) -> bool {
        unsafe {
            self.dq_update_size::<DELTA_Q>(entries::thread_nwm::PACKET_DATA_SIZE_KCP as u32);
            self.blkn = 0;
            entries::thread_nwm::rp_send_buffer::<REL_STREAM>(self, false)
        }
    }

    fn term<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self) -> bool {
        unsafe {
            self.dq_update_size::<DELTA_Q>(
                entries::thread_nwm::PACKET_DATA_SIZE_KCP as u32 - self.free_in_bytes as u32,
            );
            self.blkn = 0;
            entries::thread_nwm::rp_send_buffer::<REL_STREAM>(self, true)
        }
    }

    pub unsafe fn advance_to(&mut self, dst: *mut u8) {
        self.free_in_bytes -= unsafe { dst.offset_from_unsigned(self.dst) } as u16;
        self.dst = dst;
    }

    fn dq_update_size<const DLETA_Q: bool>(&mut self, size: u32) {
        if DLETA_Q {
            let comp_size = unsafe {
                (*JPEG)
                    .shared_mut
                    .compressed_size
                    .get_unchecked_mut(self.s.get() as usize)
            };
            unsafe { entries::thread_nwm::rp_dq_update_size(comp_size, size, self.blkn) }
        } else {
            unsafe { entries::thread_nwm::rp_update_size(self.w, size) }
        }
    }
}

#[derive(ConstDefault, Clone, Copy)]
pub struct CInfo {
    pub is_top: bool,
    pub color_space: ColorSpace,
    pub restart_interval: u16,
    pub work_index: WorkIndex,
    pub core_count: CoreCount,
}

type BitBufType = u32;

#[derive(ConstDefault)]
pub struct HuffState {
    c: BitBufType,
    free_bits: isize,
}

pub const BIT_BUF_SIZE: usize = mem::size_of::<BitBufType>() * 8;

#[derive(ConstDefault)]
pub struct JpegWorker<'a, const REL_STREAM: bool> {
    shared: &'a JpegShared,
    shared_mut: JpegSharedMutCell,
    bufs: &'a mut WorkerBufs,
    info: &'a CInfo,
    thread_index: ThreadIndex,
    huff_state: HuffState,
    last_dc_vals: LastDcVals,
}

type LastDcVals = RangedArray<i16, { MAX_COMPONENTS as u32 }>;

pub struct JpegSharedMutCell {
    cell: *mut JpegSharedMut,
}

struct JpegEncode<'a, 'b, const REL_STREAM: bool, const DELTA_Q: bool> {
    worker: &'b mut JpegWorker<'a, REL_STREAM>,
    dst: WorkerDst,
}

fn get_bpp_for_format(c: ColorSpace) -> u8 {
    match c {
        ColorSpace::XBGR => 4,
        ColorSpace::BGR => 3,
        _ => 2,
    }
}

impl<'a, 'b, const REL_STREAM: bool, const DELTA_Q: bool> JpegEncode<'a, 'b, REL_STREAM, DELTA_Q> {
    fn write_headers(&mut self) {}

    fn write_rst(&mut self) {}

    fn write_trailer(&mut self) {}

    fn write_term(&mut self) {}

    fn reset_mcu(&mut self) {
        self.worker.huff_state = const_default();
        self.worker.huff_state.free_bits = BIT_BUF_SIZE as isize;
        self.worker.last_dc_vals = const_default();
    }

    fn flush_mcu(&mut self) {}

    #[named]
    fn encode<F, G>(
        &mut self,
        src: &[u8],
        mut pre_progress: F,
        mut progress: G,
    ) -> Option<JpegDqRet>
    where
        F: FnMut(),
        G: FnMut(),
    {
        let bpp = get_bpp_for_format(self.worker.info.color_space);
        let pitch = GSP_SCREEN_WIDTH as usize * bpp as usize;
        let is_top = self.worker.info.is_top;
        let mcus = if is_top {
            self.worker.shared.mcus_top
        } else {
            self.worker.shared.mcus_bot
        };

        pre_progress();

        if !REL_STREAM && self.worker.thread_index.get() == 0 {
            self.write_headers();
        }

        self.reset_mcu();

        let w = self.worker.info.work_index.get() as usize;
        let s = is_top_index(is_top).get() as usize;

        let prev = unsafe {
            if DELTA_Q {
                if src.len() == 0 {
                    wait_syn(cname!(), self.worker.shared.work_sem[w], c_str!("work_sem"))?;
                }

                let cell = self.worker.shared_mut.cell;
                (if is_top {
                    (*cell).dq_prev_coeffs_top.as_mut_ptr()
                } else {
                    (*cell).dq_prev_coeffs_bot.as_mut_ptr()
                } as *mut JBlock)
                    .add(
                        self.worker.info.restart_interval as usize
                            * self.worker.shared.max_blocks_in_mcu
                            * self.worker.thread_index.get() as usize,
                    )
            } else {
                ptr::null_mut()
            }
        };

        let hss = self.worker.shared.max_h_samp_factor == MAX_SAMP_FACTOR;
        let vss = self.worker.shared.max_v_samp_factor == MAX_SAMP_FACTOR;

        if vss {
            let src_chunks = src
                .chunks_exact(pitch)
                .array_chunks::<{ DCTSIZE * MAX_SAMP_FACTOR }>();
            for (i, chunks) in src_chunks.enumerate() {
                /* Pre-process */
                if let (chunks, []) = chunks.as_chunks::<{ DCTSIZE }>() {
                    let mut chunks = chunks.iter();

                    let chunk0 = chunks.next().unwrap();
                    self.pre_process(*chunk0, false);

                    let chunk1 = chunks.next().unwrap();
                    self.pre_process(*chunk1, true);
                } else {
                    return None;
                }

                pre_progress();

                /* Compress and encode */
                self.process(
                    if DELTA_Q {
                        unsafe {
                            prev.add(
                                i * self.worker.shared.mcus_per_row
                                    * self.worker.shared.max_blocks_in_mcu,
                            )
                        }
                    } else {
                        ptr::null_mut()
                    },
                    i as u8,
                );

                progress();
            }
        } else {
            let pre_process = if hss {
                Self::pre_process_no_vsubsamp::<true>
            } else {
                Self::pre_process_no_vsubsamp::<false>
            };

            let src_chunks = src.chunks_exact(pitch).array_chunks::<DCTSIZE>();
            for (i, chunk) in src_chunks.enumerate() {
                pre_process(self, chunk);

                pre_progress();

                /* Compress and encode */
                self.process(
                    if !DELTA_Q {
                        ptr::null_mut()
                    } else {
                        unsafe {
                            prev.add(
                                i * self.worker.shared.mcus_per_row
                                    * self.worker.shared.max_blocks_in_mcu,
                            )
                        }
                    },
                    i as u8,
                );

                progress();
            }
        }

        self.flush_mcu();

        let mut delta_q = 0;
        if !REL_STREAM {
            if self.worker.thread_index.get()
                == thread_index_last(self.worker.shared.core_count).get()
            {
                self.write_trailer();
            } else {
                self.write_rst();
            }
        }

        self.write_term();

        if DELTA_Q {
            unsafe {
                let cell = self.worker.shared_mut.cell;
                delta_q = (*cell).work_delta_q[w];

                let c = &mut (*cell).work_sem_count[w];

                if c.fetch_sub(1, Ordering::AcqRel) == 1 {
                    c.store(self.worker.info.core_count.get() as u8, Ordering::Release);
                    (*cell).work_inited[w] = false;

                    let b = &mut (*cell).screen_bool[s];
                    if b.swap(true, Ordering::AcqRel) {
                        b.store(false, Ordering::Release);
                    } else {
                        release_sem(
                            cname!(),
                            self.worker.shared.screen_sem[s],
                            c_str!("screen_sem"),
                        );
                    }
                }
            }
        }

        Some(JpegDqRet { delta_q, mcus })
    }

    fn need_subsamp<const COMP_I: u8, const H_SAMP: bool, const V_SAMP: bool>() -> bool {
        (H_SAMP || V_SAMP) && COMP_I != 0
    }

    fn need_subsamp_ci<const H_SAMP: bool, const V_SAMP: bool>(ci: u8) -> bool {
        if ci == 0 {
            Self::need_subsamp::<0, H_SAMP, V_SAMP>()
        } else if ci == 1 {
            Self::need_subsamp::<1, H_SAMP, V_SAMP>()
        } else if ci == 2 {
            Self::need_subsamp::<2, H_SAMP, V_SAMP>()
        } else {
            false
        }
    }

    pub fn color_convert<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &[&[u8]; S],
        output_base: usize,
    ) {
        let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

        for ci in 0..MAX_COMPONENTS {
            let color = &mut self.worker.bufs.color[ci];
            if Self::need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                color.ptr = &mut color.buf[0][0];
            } else {
                let output_base = output_base * S as usize;
                let output_step = S;
                let output: &mut u8 =
                    &mut self.worker.bufs.prep[ci][output_base..output_base + output_step][0][0];
                color.ptr = output;
            }
        }
        match self.worker.info.color_space {
            ColorSpace::XBGR => cconvert::<3, 2, 1, 4, { S }>(
                input,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::BGR => cconvert::<2, 1, 0, 3, { S }>(
                input,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::RGB565 => cconvert2::<{ S }, _>(
                input,
                rgb565_comps,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB5A1 => cconvert2::<{ S }, _>(
                input,
                rgb5a1_comps,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB4 => todo!(),
        }
    }

    fn h2v1_downsample(
        input: &[u8; GSP_SCREEN_WIDTH as usize],
        output: &mut [u8; GSP_SCREEN_WIDTH as usize],
    ) {
        let mut bias = 0;
        for (input, output) in input
            .as_chunks::<{ MAX_SAMP_FACTOR }>()
            .0
            .iter()
            .zip(output)
        {
            *output = ((input[0] as u16
                + input[1] as u16
                + input[0] as u16
                + input[1] as u16
                + bias as u16)
                >> 2) as u8;
            bias ^= 1; /* 1=>2, 2=>1 */
        }
    }

    fn h2v2_downsample(
        input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR],
        output: &mut [u8; GSP_SCREEN_WIDTH as usize],
    ) {
        let [input0, input1] = input;
        let input0 = input0.as_chunks::<{ MAX_SAMP_FACTOR }>().0.iter();
        let input1 = input1.as_chunks::<{ MAX_SAMP_FACTOR }>().0.iter();
        let mut bias = 1;

        for ((input0, input1), output) in input0.zip(input1).zip(output) {
            *output = ((input0[0] as u16
                + input0[1] as u16
                + input1[0] as u16
                + input1[1] as u16
                + bias as u16)
                >> 2) as u8;
            bias ^= 3; /* 1=>2, 2=>1 */
        }
    }

    pub fn downsample<const H_SAMP: bool, const V_SAMP: bool>(&mut self, output_base: usize) {
        let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

        for ci in 0..MAX_COMPONENTS {
            let input = &self.worker.bufs.color[ci];
            if Self::need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                if V_SAMP {
                    let output = &mut self.worker.bufs.prep[ci][output_base];
                    Self::h2v2_downsample(&input.buf, output);
                } else {
                    let output = &mut self.worker.bufs.prep[ci][output_base];
                    Self::h2v1_downsample(&input.buf[0], output);
                }
            }
        }
    }

    fn pre_process(&mut self, src: [&[u8]; DCTSIZE], which_half: bool) {
        for (base, chunk) in src.as_chunks::<{ MAX_SAMP_FACTOR }>().0.iter().enumerate() {
            let output_base = if which_half { base + DCTSIZE / 2 } else { base };
            self.color_convert::<_, true, true>(chunk, output_base);
            self.downsample::<true, true>(output_base);
        }
    }

    fn pre_process_no_vsubsamp<const H_SAMP: bool>(&mut self, src: [&[u8]; DCTSIZE]) {
        for (base, chunk) in src.as_chunks::<1>().0.iter().enumerate() {
            self.color_convert::<_, H_SAMP, false>(chunk, base);
            self.downsample::<H_SAMP, false>(base);
        }
    }

    fn process(&mut self, prev: *mut JBlock, row_i: u8) {}
}

pub struct Jpeg {
    shared: JpegShared,
    shared_mut: JpegSharedMut,
    bufs: [WorkerBufs; RP_CORE_COUNT_MAX as usize],
    info: [CInfo; WORK_COUNT as usize],
}

impl Jpeg {
    pub fn init(&mut self, quality: u32, core_count: CoreCount, hq: u32, delta_prog: bool) {
        let (max_blocks_in_mcu, q_steps) = self.shared.init(quality, delta_prog, core_count, hq);
        self.shared_mut.init(delta_prog, max_blocks_in_mcu, q_steps);
    }

    pub fn set_info(&mut self, info: CInfo) {
        *info.work_index.index_into_mut(&mut self.info) = info;
    }

    pub unsafe fn get_worker<'a, const REL_STREAM: bool>(
        &'a mut self,
        work_index: WorkIndex,
        thread_index: ThreadIndex,
    ) -> JpegWorker<'a, REL_STREAM> {
        JpegWorker {
            shared: &self.shared,
            shared_mut: JpegSharedMutCell {
                cell: &mut self.shared_mut,
            },
            bufs: thread_index.index_into_mut(&mut self.bufs),
            info: work_index.index_into_mut(&mut self.info),
            thread_index,
            huff_state: const_default(),
            last_dc_vals: const_default(),
        }
    }
}

fn pconvert(r: u8, g: u8, b: u8, y: &mut u8, cb: &mut u8, cr: &mut u8, ctab: &[i32; TABLE_SIZE]) {
    /* If the inputs are 0.._MAXJSAMPLE, the outputs of these equations
     * must be too; we do not need an explicit range-limiting operation.
     * Hence the value being shifted is never negative, and we don't
     * need the general RIGHT_SHIFT macro.
     */
    /* Y */
    *y = ((ctab[r as usize + R_Y_OFF] + ctab[g as usize + G_Y_OFF] + ctab[b as usize + B_Y_OFF])
        >> SCALEBITS) as u8;
    /* Cb */
    *cb =
        ((ctab[r as usize + R_CB_OFF] + ctab[g as usize + G_CB_OFF] + ctab[b as usize + B_CB_OFF])
            >> SCALEBITS) as u8;
    /* Cr */
    *cr =
        ((ctab[r as usize + R_CR_OFF] + ctab[g as usize + G_CR_OFF] + ctab[b as usize + B_CR_OFF])
            >> SCALEBITS) as u8;
}

fn cconvert<const R: usize, const G: usize, const B: usize, const P: usize, const N: usize>(
    input: &[&[u8]; N],
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    tab: &[i32; TABLE_SIZE],
) where
    [(); GSP_SCREEN_WIDTH as usize * P]:,
{
    let [output0, output1, output2] = output;
    let output0 = unsafe {
        slice::from_raw_parts_mut(output0.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N)
    };
    let output1 = unsafe {
        slice::from_raw_parts_mut(output1.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N)
    };
    let output2 = unsafe {
        slice::from_raw_parts_mut(output2.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N)
    };
    for i in 0..N {
        let input: &[u8; GSP_SCREEN_WIDTH as usize * P] = input[i].try_into().unwrap();

        let output0 = unsafe { output0.get_unchecked_mut(i) };
        let output1 = unsafe { output1.get_unchecked_mut(i) };
        let output2 = unsafe { output2.get_unchecked_mut(i) };

        for (((input, output0), output1), output2) in input
            .as_chunks::<P>()
            .0
            .iter()
            .zip(output0.into_iter())
            .zip(output1.into_iter())
            .zip(output2.into_iter())
        {
            let r = input[R];
            let g = input[G];
            let b = input[B];

            pconvert(r, g, b, output0, output1, output2, tab);
        }
    }
}

fn cconvert2<const N: usize, F>(
    input: &[&[u8]; N],
    comps: F,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    tab: &ColorConvTabs,
) where
    F: Fn(u16, &ColorConvTabs) -> (u8, u8, u8),
    [(); GSP_SCREEN_WIDTH as usize * 2]:,
{
    let [output0, output1, output2] = output;
    let output0 = unsafe {
        slice::from_raw_parts_mut(output0.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N)
    };
    let output1 = unsafe {
        slice::from_raw_parts_mut(output1.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N)
    };
    let output2 = unsafe {
        slice::from_raw_parts_mut(output2.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N)
    };
    for i in 0..N {
        let input: &[u8; GSP_SCREEN_WIDTH as usize * 2] = input[i].try_into().unwrap();

        let output0 = unsafe { output0.get_unchecked_mut(i) };
        let output1 = unsafe { output1.get_unchecked_mut(i) };
        let output2 = unsafe { output2.get_unchecked_mut(i) };

        for (((input, output0), output1), output2) in input
            .as_chunks::<2>()
            .0
            .iter()
            .zip(output0.into_iter())
            .zip(output1.into_iter())
            .zip(output2.into_iter())
        {
            let (r, g, b) = comps(input[0] as u16 | ((input[1] as u16) << 8), tab);

            pconvert(r, g, b, output0, output1, output2, &tab.rgb_ycc_tab);
        }
    }
}

fn rgb565_comps(input: u16, tab: &ColorConvTabs) -> (u8, u8, u8) {
    let r = tab.rb_5_tab[((input >> 11) & 0x1f) as usize];
    let g = tab.g_6_tab[((input >> 5) & 0x3f) as usize];
    let b = tab.rb_5_tab[(input & 0x1f) as usize];
    (r, g, b)
}
fn rgb5a1_comps(input: u16, tab: &ColorConvTabs) -> (u8, u8, u8) {
    let r = tab.rb_5_tab[((input >> 11) & 0x1f) as usize];
    let g = tab.rb_5_tab[((input >> 6) & 0x1f) as usize];
    let b = tab.rb_5_tab[((input >> 1) & 0x1f) as usize];
    (r, g, b)
}

impl<'a, const REL_STREAM: bool> JpegWorker<'a, REL_STREAM> {
    pub fn encode<F, G>(
        &'a mut self,
        dst: WorkerDst,
        src: &[u8],
        pre_progress: F,
        progress: G,
    ) -> Option<JpegDqRet>
    where
        F: FnMut(),
        G: FnMut(),
    {
        let delta_prog = REL_STREAM && entries::thread_nwm::get_reliable_stream_delta_prog();
        if delta_prog {
            JpegEncode::<_, true> { worker: self, dst }.encode(src, pre_progress, progress)
        } else {
            JpegEncode::<_, false> { worker: self, dst }.encode(src, pre_progress, progress)
        }
    }
}

pub unsafe fn get_jpeg_shared() -> &'static JpegShared {
    unsafe { &(*JPEG).shared }
}

const DELTA_Q_PREV_COEFFS_TOP_N: usize =
    GSP_SCREEN_WIDTH as usize * GSP_SCREEN_HEIGHT_TOP as usize * MAX_COMPONENTS;
const DELTA_Q_PREV_COEFFS_BOT_N: usize =
    GSP_SCREEN_WIDTH as usize * GSP_SCREEN_HEIGHT_BOTTOM as usize * MAX_COMPONENTS;
