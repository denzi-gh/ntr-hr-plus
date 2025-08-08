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
    pub work_sem_count: [u8; WORK_COUNT as usize],
    pub screen_bool: [bool; SCREEN_COUNT as usize],
    pub last_restart_interval: [u16; SCREEN_COUNT as usize],
    delta_q: [u8; SCREEN_COUNT as usize],
    work_delta_q: [u8; WORK_COUNT as usize],
    dq_rescale_prev: [i8; WORK_COUNT as usize],
    rp_shifts: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; WORK_COUNT as usize],
    delta_q_cache: [[DeltaQCache; DELTA_Q_CACHE_TOTAL as usize]; WORK_COUNT as usize],
    delta_q_cache_next: [[u8; MAX_COMPONENTS]; WORK_COUNT as usize],
    delta_q_calc: [DeltaQManager; SCREEN_COUNT as usize],
    rand32: Rand32,
}

impl JpegSharedMut {
    pub fn once(&mut self) {
        unsafe {
            slice::from_raw_parts_mut(self as *mut _ as *mut u8, mem::size_of_val(self)).fill(0);
        }
        self.rand32 = Rand32::new(get_system_tick().get() as u64);
    }
}

const fn jdiv_round_up(a: usize, b: usize) -> usize
/* Compute a/b rounded up to next integer, ie, ceil(a/b) */
/* Assumes a >= 0, b > 0 */
{
    (a + b - 1) / b
}

impl JpegShared {
    fn init_delta_q_tbls(&mut self) {
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
        self.init_delta_q_tbls();
    }

    fn set_comp_infos(&mut self, hq: u32, delta_prog: bool) {
        let comp_infos = if hq as u8_ == RP_CHROMASS_444 {
            &self.jpeg_tbls.comp_infos_444
        } else if hq as u8_ == RP_CHROMASS_422 {
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
    pub info: *const entries::thread_nwm::NwmInfo,
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
        self.free_in_bytes -= unsafe { dst.sub_ptr(self.dst) } as u16;
        self.dst = dst;
    }

    fn dq_update_size<const DLETA_Q: bool>(&mut self, size: u32) {
        if DLETA_Q {
            let comp_size = unsafe {
                (*entries::work_thread::JPEG)
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
    shared_mut: JpegSharedMutCell<'a>,
    bufs: &'a mut WorkerBufs,
    info: &'a CInfo,
    thread_index: ThreadIndex,
    huff_state: HuffState,
    last_dc_val: [i16; MAX_COMPONENTS],
}

pub struct JpegSharedMutCell<'a> {
    cell: &'a core::cell::UnsafeCell<JpegSharedMut>,
}

pub struct JpegEncode<'a, 'b, const REL_STREAM: bool, const DELTA_Q: bool> {
    worker: &'b mut JpegWorker<'a, REL_STREAM>,
    dst: WorkerDst,
}

pub struct Jpeg {
    shared: JpegShared,
    shared_mut: JpegSharedMut,
    bufs: [WorkerBufs; RP_CORE_COUNT_MAX as usize],
    info: [CInfo; WORK_COUNT as usize],
    dq_prev_coeffs_top: [JCoef; DELTA_Q_PREV_COEFFS_TOP_N],
    dq_prev_coeffs_bot: [JCoef; DELTA_Q_PREV_COEFFS_BOT_N],
}

const DELTA_Q_PREV_COEFFS_TOP_N: usize =
    GSP_SCREEN_WIDTH as usize * GSP_SCREEN_HEIGHT_TOP as usize * MAX_COMPONENTS;
const DELTA_Q_PREV_COEFFS_BOT_N: usize =
    GSP_SCREEN_WIDTH as usize * GSP_SCREEN_HEIGHT_BOTTOM as usize * MAX_COMPONENTS;
