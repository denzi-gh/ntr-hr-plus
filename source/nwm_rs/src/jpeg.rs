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

pub struct JpegScreenShared {
    comp_infos: *const CompInfos,
    max_h_samp_factor: usize,
    max_v_samp_factor: usize,
    max_blocks_in_mcu: usize,
    mcu_row_size: usize,
    pub mcu_col_size: usize,
    pub mcus_per_row: usize,
    mcus: u16,
    delta_q_params: DeltaQParams,
}

pub struct JpegShared {
    quality: u32,
    chroma_ss: [u8; RP_SCREEN_COUNT as usize],
    downsample: [u8; RP_SCREEN_COUNT as usize],
    quant_tbls: QuantTbls,
    divisors: Divisors,
    div_shifts: [[u8; DCTSIZE2]; NUM_QUANT_TBLS],
    div_delta_q_shifts: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    core_count: CoreCount,
    pub screens: [JpegScreenShared; RP_SCREEN_COUNT as usize],
    pub last_restart_range: u32,
    qos_adj: f32,
    jpeg_tbls: JpegTbls,
    delta_q_tbls: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    delta_q0_tbls: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    work_sem: RangedArray<Handle, WORK_COUNT>,
    screen_sem: RangedArray<Handle, SCREEN_COUNT>,
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

struct JpegSharedMut {
    compressed_size: RangedArray<AtomicU32, SCREEN_COUNT>,
    work_inited: RangedArray<AtomicBool, WORK_COUNT>,
    work_sem_count: RangedArray<AtomicU8, WORK_COUNT>,
    screen_bool: RangedArray<AtomicBool, SCREEN_COUNT>,
    last_restart_interval: RangedArray<u16, SCREEN_COUNT>,
    delta_q: RangedArray<u8, SCREEN_COUNT>,
    work_delta_q: RangedArray<u8, WORK_COUNT>,
    dq_rescale_prev: RangedArray<s8, WORK_COUNT>,
    rp_shifts: RangedArray<[[u8; DCTSIZE2]; NUM_QUANT_TBLS], WORK_COUNT>,
    delta_q_cache: RangedArray<[DeltaQCache; DELTA_Q_CACHE_TOTAL as usize], WORK_COUNT>,
    delta_q_cache_next: RangedArray<[u8; MAX_COMPONENTS], WORK_COUNT>,
    delta_q_calc: [DeltaQManager; SCREEN_COUNT as usize],
    dq_prev_coeffs_top: [JCoef; DELTA_Q_PREV_COEFFS_TOP_N],
    dq_prev_coeffs_bot: [JCoef; DELTA_Q_PREV_COEFFS_BOT_N],
    rand32: Rand32,
}

impl JpegSharedMut {
    fn once(&mut self) {
        unsafe {
            ptr::write_bytes(self as *mut _ as *mut u8, 0, mem::size_of_val(self));
        }
        self.rand32 = Rand32::new(get_system_tick().get() as u64);
    }

    fn init(&mut self, delta_prog: bool, params: [(usize, f32); RP_SCREEN_COUNT as usize]) {
        if delta_prog {
            self.compressed_size = const_default();
            for s in ScreenIndex::all() {
                *self.delta_q.get_mut(&s) = DELTA_Q_COUNT - 1;
            }
        }

        self.delta_q_calc = const_default();

        if delta_prog {
            for i in 0..SCREEN_COUNT as usize {
                let (max_blocks_in_mcu, q_steps) = params[i];
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
        hq: [u32; RP_SCREEN_COUNT as usize],
        downsample: [u32; RP_SCREEN_COUNT as usize],
    ) -> [(usize, f32); RP_SCREEN_COUNT as usize] {
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
        self.chroma_ss = hq.map(|hq| hq as u8);
        self.downsample = downsample.map(|downsample| downsample as u8);
        self.last_restart_range = if delta_prog { 64 } else { 32 };
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

    fn once(&mut self) {
        unsafe {
            ptr::write_bytes(self as *mut _ as *mut u8, 0, mem::size_of_val(self));
        }
        self.jpeg_tbls = JpegTbls::once();
        self.once_delta_q_tbls();
    }

    fn set_comp_infos(
        &mut self,
        mut hq: [u32; RP_SCREEN_COUNT as usize],
        delta_prog: bool,
    ) -> [(usize, f32); RP_SCREEN_COUNT as usize] {
        let mut ret: [(usize, f32); RP_SCREEN_COUNT as usize] = const_default();

        for s in ScreenIndex::all() {
            let screen = s.index_into_mut(&mut self.screens);
            let hq = *s.index_into_mut(&mut hq) as u8;

            let comp_infos = if hq == RP_CHROMASS_444 {
                &self.jpeg_tbls.comp_infos_444
            } else if hq == RP_CHROMASS_422 {
                &self.jpeg_tbls.comp_infos_422
            } else {
                &self.jpeg_tbls.comp_infos_420
            };
            screen.comp_infos = comp_infos;
            screen.max_h_samp_factor = 1;
            screen.max_v_samp_factor = 1;
            screen.max_blocks_in_mcu = 0;
            for i in 0..MAX_COMPONENTS {
                let info = &comp_infos.infos[i];
                screen.max_h_samp_factor =
                    cmp::max(screen.max_h_samp_factor, info.h_samp_factor as usize);
                screen.max_v_samp_factor =
                    cmp::max(screen.max_v_samp_factor, info.v_samp_factor as usize);
                screen.max_blocks_in_mcu +=
                    info.h_samp_factor as usize * info.v_samp_factor as usize;
            }
            if screen.max_blocks_in_mcu > MAX_BLOCKS_IN_MCU {
                panic!();
            }
            screen.mcu_row_size = DCTSIZE * screen.max_h_samp_factor;
            screen.mcu_col_size = DCTSIZE * screen.max_v_samp_factor;
            screen.mcus_per_row = jdiv_round_up(GSP_SCREEN_WIDTH as usize, screen.mcu_row_size);
            if s.get() == RP_SCREEN_TOP as u32 {
                let mcu_rows_top =
                    jdiv_round_up(GSP_SCREEN_HEIGHT_TOP as usize, screen.mcu_col_size);
                screen.mcus = (screen.mcus_per_row * mcu_rows_top) as u16;
            } else {
                let mcu_rows_bot =
                    jdiv_round_up(GSP_SCREEN_HEIGHT_BOTTOM as usize, screen.mcu_col_size);
                screen.mcus = (screen.mcus_per_row * mcu_rows_bot) as u16;
            }

            *s.index_into_mut(&mut ret) = if delta_prog {
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
                screen.delta_q_params.qf = qf;
                let qt = {
                    let mut qt = 0f32;
                    for i in 0..NUM_QUANT_TBLS {
                        qt += screen.delta_q_params.qf[i];
                    }
                    qt
                };
                let q_step = (DELTA_Q_STEP * qt, DELTA_Q_STEP * (DCTSIZE2 - 1) as f32 * qt);
                let q_steps = q_step.0 + q_step.1;
                let q_steps_i = 1f32 / q_steps;
                screen.delta_q_params.q_steps_i = q_steps_i * SCALE_QD_I_F;

                screen.delta_q_params.m = (MIN_DCT_COMP_SIZE * screen.max_blocks_in_mcu) as f32;

                (screen.max_blocks_in_mcu, q_steps)
            } else {
                (0, 0f32)
            }
        }
        ret
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
    assert!(WORK_COUNT - 1 <= ((1 << entries::work_thread::RP_KCP_HDR_W_NBITS) - 1));
};

// We store RP_CORE_COUNT_MAX as a special value to indicate term packet
const _ARQ_RP_T_SIZE_ASSERT: () = {
    assert!(RP_CORE_COUNT_MAX <= ((1 << entries::work_thread::RP_KCP_HDR_T_NBITS) - 1));
};

impl ArqRpHdr {
    pub unsafe fn write_hdr(&self, dst: *mut u8) {
        let hdr = (self.w.get() as u16) << (PID_NBITS + CID_NBITS)
            | (self.t.get() as u16)
                << (PID_NBITS + CID_NBITS + entries::work_thread::RP_KCP_HDR_W_NBITS);
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

#[derive(Copy, Clone)]
pub enum WorkderDstUser {
    NoneInfo(*const entries::thread_nwm::NwmThreadInfo),
    KcpHdr(ArqRpHdr),
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
            let comp_size = unsafe { (*JPEG).shared_mut.compressed_size.get_mut(&self.s) };
            entries::thread_nwm::rp_dq_update_size(comp_size, size, self.blkn)
        } else {
            entries::thread_nwm::rp_update_size(self.w, size)
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

type LastDcVals = [s16; MAX_COMPONENTS];

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
    fn write_headers(&mut self) {
        /* File header */
        self.write_marker(M_SOI);
        self.write_jfif_app0();

        /* Frame header */
        for i in 0..NUM_QUANT_TBLS {
            self.write_dqt(i as usize);
        }
        self.write_sof(M_SOF0);

        /* Scan header */
        for i in 0..NUM_HUFF_TBLS {
            self.write_dht(i as usize, false);
            self.write_dht(i as usize, true);
        }
        if self.worker.shared.core_count.get() > 1 {
            self.write_dri();
        }
        self.write_sos();
    }

    fn write_marker(&mut self, mark: u8)
    /* Emit a marker code */
    {
        self.write_byte(0xFF);
        self.write_byte(mark);
    }

    fn write_byte(&mut self, value: u8) {
        self.dst.write_byte::<REL_STREAM, DELTA_Q>(value);
    }

    fn write_2bytes(&mut self, value: u16)
    /* Emit a 2-byte integer; these are always MSB first in JPEG files */
    {
        self.write_byte(((value >> 8) & 0xFF) as u8);
        self.write_byte((value & 0xFF) as u8);
    }

    fn write_jfif_app0(&mut self)
    /* Emit a JFIF-compliant APP0 marker */
    {
        /*
         * Length of APP0 block       (2 bytes)
         * Block ID                   (4 bytes - ASCII "JFIF")
         * Zero byte                  (1 byte to terminate the ID string)
         * Version Major, Minor       (2 bytes - major first)
         * Units                      (1 byte - 0x00 = none, 0x01 = inch, 0x02 = cm)
         * Xdpu                       (2 bytes - dots per unit horizontal)
         * Ydpu                       (2 bytes - dots per unit vertical)
         * Thumbnail X size           (1 byte)
         * Thumbnail Y size           (1 byte)
         */

        self.write_marker(M_APP0);

        self.write_2bytes(2 + 4 + 1 + 2 + 1 + 2 + 2 + 1 + 1); /* length */

        self.write_byte(0x4A); /* Identifier: ASCII "JFIF" */
        self.write_byte(0x46);
        self.write_byte(0x49);
        self.write_byte(0x46);
        self.write_byte(0);
        self.write_byte(1); /* Version fields */
        self.write_byte(1);
        self.write_byte(0); /* Pixel size information */
        self.write_2bytes(1);
        self.write_2bytes(1);
        self.write_byte(0); /* No thumbnail image */
        self.write_byte(0);
    }

    fn write_dqt(&mut self, index: usize)
    /* Emit a DQT marker */
    /* Returns the precision used (0 = 8bits, 1 = 16bits) for baseline checking */
    {
        let qtbl = &self.worker.shared.quant_tbls.quant_tbls[index];

        self.write_marker(M_DQT);
        self.write_2bytes((DCTSIZE2 + 1 + 2) as u16);
        self.write_byte(index as u8);
        for i in 0..DCTSIZE2 {
            /* The table entries must be emitted in zigzag order. */
            let qval =
                *unsafe { qtbl.quant_val.get_unchecked(JPEG_NATURAL_ORDER[i] as usize) } as u8;
            self.write_byte(qval);
        }
    }

    fn screen_height(&self) -> u32 {
        if self.worker.info.is_top {
            GSP_SCREEN_HEIGHT_TOP
        } else {
            GSP_SCREEN_HEIGHT_BOTTOM
        }
    }

    fn write_dht(&mut self, mut index: usize, is_ac: bool) {
        let tbl = if is_ac {
            &self.worker.shared.jpeg_tbls.huff_tbls.ac_huff_tbls[index]
        } else {
            &self.worker.shared.jpeg_tbls.huff_tbls.dc_huff_tbls[index]
        };
        if is_ac {
            index |= 0x10; /* output index has AC bit set */
        }

        self.write_marker(M_DHT);

        let mut length = 0 as u16;
        for i in 1..=16 as usize {
            length += tbl.bits[i] as u16;
        }

        self.write_2bytes((length + 2 + 1 + 16) as u16);
        self.write_byte(index as u8);

        for i in 1..=16 as usize {
            self.write_byte(tbl.bits[i]);
        }

        for i in 0..length as u8 {
            self.write_byte(tbl.huff_vals[i as usize]);
        }
    }

    fn write_dri(&mut self) {
        self.write_marker(M_DRI);
        self.write_2bytes(4); /* fixed length */
        self.write_2bytes(self.worker.info.restart_interval);
    }

    fn write_sos(&mut self) {
        self.write_marker(M_SOS);

        self.write_2bytes((2 * MAX_COMPONENTS + 2 + 1 + 3) as u16); /* length */

        self.write_byte(MAX_COMPONENTS as u8);

        let infos = unsafe {
            &(*is_top_index(self.worker.info.is_top)
                .index_into(&self.worker.shared.screens)
                .comp_infos)
                .infos
        };
        for i in 0..MAX_COMPONENTS {
            let comp = &infos[i];
            self.write_byte(comp.component_id);

            /* We emit 0 for unused field(s); this is recommended by the P&M text
             * but does not seem to be specified in the standard.
             */

            /* DC needs no table for refinement scan */
            let td = comp.dc_tbl_no;
            /* AC needs no table when not present */
            let ta = comp.ac_tbl_no;

            self.write_byte((td << 4) + ta);
        }

        self.write_byte(0);
        self.write_byte((DCTSIZE2 - 1) as u8);
        self.write_byte(0);
    }

    fn write_sof(&mut self, code: u8) {
        self.write_marker(code);

        self.write_2bytes((3 * MAX_COMPONENTS + 2 + 5 + 1) as u16); /* length */

        self.write_byte(8);
        self.write_2bytes(self.screen_height() as u16);
        self.write_2bytes(GSP_SCREEN_WIDTH as u16);

        self.write_byte(MAX_COMPONENTS as u8);

        for info in unsafe {
            &(*is_top_index(self.worker.info.is_top)
                .index_into(&self.worker.shared.screens)
                .comp_infos)
                .infos
        } {
            self.write_byte(info.component_id);
            self.write_byte((info.h_samp_factor << 4) + info.v_samp_factor);
            self.write_byte(info.quant_tbl_no);
        }
    }

    fn write_rst(&mut self) {
        self.write_marker(M_RST0 + self.worker.thread_index.get() as u8);
    }

    fn write_trailer(&mut self) {
        self.write_marker(M_EOI);
    }

    fn write_term(&mut self) {
        self.dst.term::<REL_STREAM, DELTA_Q>();
    }

    fn reset_mcu(&mut self) {
        self.worker.huff_state = const_default();
        self.worker.huff_state.free_bits = BIT_BUF_SIZE as isize;
        self.worker.last_dc_vals = const_default();
    }

    fn flush_mcu(&mut self) {
        let mut put_bits = BIT_BUF_SIZE as isize - self.worker.huff_state.free_bits;

        let mut localbuf: [u8; mem::size_of::<BitBufType>() * 4] = const_default();
        let put_buffer = self.worker.huff_state.c;
        let mut buf = EncodeBuffer::<_, REL_STREAM, DELTA_Q>::init(
            &mut self.worker.huff_state,
            &mut self.dst,
            &mut localbuf,
        );

        while put_bits >= 8 {
            put_bits -= 8;
            let temp = unsafe { core::intrinsics::unchecked_shr(put_buffer, put_bits) };
            unsafe { buf.emit_byte(temp as u8) }
        }
        if put_bits > 0 {
            /* fill partial byte with ones */
            let temp = (put_buffer << (8 - put_bits))
                | unsafe { core::intrinsics::unchecked_shr(0xFF, put_bits) };
            unsafe { buf.emit_byte(temp as u8) }
        }

        buf.store();
    }

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
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        let mcus = screen.mcus;

        pre_progress();

        if !REL_STREAM && self.worker.thread_index.get() == 0 {
            self.write_headers();
        }

        self.reset_mcu();

        let w = self.worker.info.work_index;
        let s = is_top_index(is_top);

        let prev = unsafe {
            if DELTA_Q {
                if src.len() == 0 {
                    wait_syn(
                        cname!(),
                        *self.worker.shared.work_sem.get(&w),
                        c_str!("work_sem"),
                    )?;
                }

                let shared_mut = &mut *self.worker.shared_mut.cell;
                (if is_top {
                    shared_mut.dq_prev_coeffs_top.as_mut_ptr()
                } else {
                    shared_mut.dq_prev_coeffs_bot.as_mut_ptr()
                } as *mut JBlock)
                    .add(
                        self.worker.info.restart_interval as usize
                            * screen.max_blocks_in_mcu
                            * self.worker.thread_index.get() as usize,
                    )
            } else {
                ptr::null_mut()
            }
        };

        let hss = screen.max_h_samp_factor == MAX_SAMP_FACTOR;
        let vss = screen.max_v_samp_factor == MAX_SAMP_FACTOR;

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
                        unsafe { prev.add(i * screen.mcus_per_row * screen.max_blocks_in_mcu) }
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
                    if DELTA_Q {
                        unsafe { prev.add(i * screen.mcus_per_row * screen.max_blocks_in_mcu) }
                    } else {
                        ptr::null_mut()
                    },
                    i as u8,
                );

                progress();
            }
        }

        self.flush_mcu();

        let mut delta_q = 0;
        if !REL_STREAM {
            if self.worker.thread_index == thread_index_last(self.worker.shared.core_count) {
                self.write_trailer();
            } else {
                self.write_rst();
            }
        }

        self.write_term();

        if DELTA_Q {
            unsafe {
                let shared_mut = &mut *self.worker.shared_mut.cell;
                delta_q = *shared_mut.work_delta_q.get(&w);

                let c = shared_mut.work_sem_count.get_mut(&w);

                if c.fetch_sub(1, Ordering::AcqRel) == 1 {
                    c.store(self.worker.info.core_count.get() as u8, Ordering::Release);
                    shared_mut
                        .work_inited
                        .get_mut(&w)
                        .store(false, Ordering::Release);

                    let b = shared_mut.screen_bool.get_mut(&s);
                    if b.swap(true, Ordering::AcqRel) {
                        b.store(false, Ordering::Release);
                    } else {
                        release_sem(
                            cname!(),
                            *self.worker.shared.screen_sem.get(&s),
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
                let output =
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

    #[named]
    fn process(&mut self, prev: *mut JBlock, row_i: u8) {
        let mut delta_cache = false;
        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        for mcu_col_num in 0..screen.mcus_per_row {
            if DELTA_Q {
                let s = is_top_index(self.worker.info.is_top);
                let w = self.worker.info.work_index;
                let shared_mut = unsafe { &mut *self.worker.shared_mut.cell };

                if row_i == 0 && mcu_col_num == 0 {
                    if !shared_mut.work_inited.get(&w).swap(true, Ordering::AcqRel) {
                        let last_restart_interval = shared_mut.last_restart_interval.get_mut(&s);
                        let b = shared_mut.screen_bool.get(&s);
                        let need_sync = self.worker.info.restart_interval != *last_restart_interval;

                        let need_sync = if !need_sync {
                            b.swap(true, Ordering::AcqRel)
                        } else {
                            need_sync
                        };

                        if need_sync {
                            if wait_syn(
                                cname!(),
                                *self.worker.shared.screen_sem.get(&s),
                                c_str!("screen_sem"),
                            )
                            .is_none()
                            {
                                return;
                            }

                            b.store(false, Ordering::Release);
                            *last_restart_interval = self.worker.info.restart_interval;
                        }

                        self.compute_dq(prev);
                        shared_mut
                            .compressed_size
                            .get(&s)
                            .store(0, Ordering::Release);
                        delta_cache = true;

                        unsafe {
                            release_sem_count(
                                cname!(),
                                *self.worker.shared.work_sem.get(&w),
                                c_str!("work_sem"),
                                self.worker.info.core_count.get() as s32 - 1,
                            );
                        }
                    } else {
                        if wait_syn(
                            cname!(),
                            *self.worker.shared.work_sem.get(&w),
                            c_str!("work_sem"),
                        )
                        .is_none()
                        {
                            return;
                        }
                    }
                }

                let dq_rescale_prev = *shared_mut.dq_rescale_prev.get(&w);
                let prev = unsafe { prev.add(mcu_col_num * screen.max_blocks_in_mcu) };

                if dq_rescale_prev > 0 {
                    self.compress_dq::<true, false>(delta_cache, mcu_col_num, prev);
                } else if dq_rescale_prev < 0 {
                    self.compress_dq::<true, true>(delta_cache, mcu_col_num, prev);
                } else {
                    self.compress_dq::<false, false>(delta_cache, mcu_col_num, prev);
                }
            } else {
                self.compress::<false, false, false>(mcu_col_num, ptr::null_mut());
            }

            self.encode_mcu();
        }
    }

    #[named]
    fn compute_dq(&mut self, prev: *mut JBlock) {
        let need_ov_stats = unsafe { (*config_consts::NTR_CONFIG).ex.plg.overlayStats > 0 };

        let s = is_top_index(self.worker.info.is_top);
        let screen = s.index_into(&self.worker.shared.screens);
        let w = self.worker.info.work_index;

        let shared_mut = unsafe { &mut *self.worker.shared_mut.cell };

        let delta_q = shared_mut.delta_q.get_mut(&s);
        let rand32 = &mut shared_mut.rand32;
        let cache = shared_mut.delta_q_cache.get_mut(&w);
        let cache_next_i = shared_mut.delta_q_cache_next.get_mut(&w);

        let prev_delta_q = *delta_q;
        let delta_q0 = &self.worker.shared.delta_q0_tbls[prev_delta_q as usize];
        let div_parts = &self.worker.shared.divisors.divisors;
        let div_shifts = &self.worker.shared.div_delta_q_shifts[DELTA_Q_COUNT as usize - 1];

        let mut delta_cache_start = 0;
        let mut blkn_start = 0;

        let mut qnv: [QuantizeRet; NUM_QUANT_TBLS] = const_default();
        let mut qnc: [u8; NUM_QUANT_TBLS] = const_default();

        let _need_wait_for_nwm = self.worker.thread_index.get() == 0;

        for ci in 0..MAX_COMPONENTS {
            cache_next_i[ci] = 0;

            let mut indices: [u8; DELTA_Q_CACHE_MAX as usize] = const_default();

            let comp = unsafe { &(*screen.comp_infos).infos[ci] };
            let qni = comp.quant_tbl_no;
            let mcu_we = comp.h_samp_exp;
            let mcu_he = comp.v_samp_exp;

            dq_cache_gen_unique_indices(rand32, &mut indices, DELTA_Q_CACHE_COUNTS[ci], unsafe {
                core::intrinsics::unchecked_shl(screen.mcus_per_row as u8, mcu_we + mcu_he)
            });

            for qi in 0..DELTA_Q_CACHE_COUNTS[ci] {
                let delta_cache_i = delta_cache_start + qi;
                let blkn = indices[qi as usize];

                let cache = unsafe { cache.get_unchecked_mut(delta_cache_i as usize) };

                let mcu_i = unsafe { core::intrinsics::unchecked_shr(blkn, mcu_we + mcu_he) };
                let mcu_r =
                    blkn & (unsafe { core::intrinsics::unchecked_shl(1, mcu_we + mcu_he) } - 1);

                let xpos = mcu_r & (unsafe { core::intrinsics::unchecked_shl(1, mcu_we) } - 1);
                let ypos = unsafe { core::intrinsics::unchecked_shr(mcu_r, mcu_we) };

                let xpos = xpos + unsafe { core::intrinsics::unchecked_shl(mcu_i, mcu_we) };
                let xpos = xpos as usize * DCTSIZE;
                let ypos = ypos as usize * DCTSIZE;

                cache.xpos = xpos as u16;
                cache.ypos = ypos as u16;

                let prev = unsafe {
                    prev.add(
                        mcu_i as usize * screen.max_blocks_in_mcu + mcu_r as usize + blkn_start,
                    )
                };

                let qn = if prev_delta_q == DELTA_Q_COUNT - 1 {
                    unsafe {
                        forward_dct::<DELTA_Q, false, false, false>(
                            &self.worker.bufs.prep[ci],
                            &mut cache.cache,
                            ypos as u16,
                            xpos as u16,
                            div_parts.get_unchecked(comp.quant_tbl_no as usize),
                            div_shifts.get_unchecked(comp.quant_tbl_no as usize),
                            prev,
                            delta_q0.get_unchecked(comp.quant_tbl_no as usize),
                            &mut cache.next,
                        )
                    }
                } else {
                    unsafe {
                        forward_dct::<DELTA_Q, false, true, false>(
                            &self.worker.bufs.prep[ci],
                            &mut cache.cache,
                            ypos as u16,
                            xpos as u16,
                            div_parts.get_unchecked(comp.quant_tbl_no as usize),
                            div_shifts.get_unchecked(comp.quant_tbl_no as usize),
                            prev,
                            delta_q0.get_unchecked(comp.quant_tbl_no as usize),
                            &mut cache.next,
                        )
                    }
                };

                let qnv = &mut *unsafe { qnv.get_unchecked_mut(qni as usize) };
                let update_qnv = |qnv: &mut QuantizeCounts, qn: &QuantizeCounts| {
                    qnv.nbits += qn.nbits;
                };
                update_qnv(&mut qnv.dc, &qn.dc);
                update_qnv(&mut qnv.ac, &qn.ac);
            }

            *unsafe { qnc.get_unchecked_mut(qni as usize) } += DELTA_Q_CACHE_COUNTS[ci];
            delta_cache_start += DELTA_Q_CACHE_COUNTS[ci];
            blkn_start += unsafe { core::intrinsics::unchecked_shl(1, mcu_we + mcu_he) };
        }

        const TARGET_FRAME_RATE: u32 = 60;
        let s_1 = is_top_index(!self.worker.info.is_top);
        let screen_1 = s_1.index_into(&self.worker.shared.screens);

        let frame_time = entries::work_thread::get_frame_time(s)
            .load(Ordering::Acquire)
            .max(SYSCLOCK_ARM11 / TARGET_FRAME_RATE);
        let frame_time_1 = entries::work_thread::get_frame_time(s_1)
            .load(Ordering::Acquire)
            .max(SYSCLOCK_ARM11 / TARGET_FRAME_RATE);

        let frame_rate_get_clamp_min = |frame_time: u32| {
            f32::min(
                TARGET_FRAME_RATE as f32,
                SYSCLOCK_ARM11 as f32 / frame_time as f32,
            )
        };
        let frame_rate_clamp_max = |frame_rate: f32, _s: ScreenIndex| frame_rate.max(1f32);

        let frame_rate = frame_rate_get_clamp_min(frame_time);
        let frame_rate = frame_rate_clamp_max(frame_rate, s);
        let frame_rate_1 = frame_rate_get_clamp_min(frame_time_1);
        let frame_rate_1 = frame_rate_clamp_max(frame_rate_1, s_1);

        let qr = (DELTA_Q_COUNT as u32 / 6
            + DELTA_Q_COUNT as u32 * self.worker.shared.quality * self.worker.shared.quality
                / 12000) as u8;
        let (qc, qc_1) = if s.get() == RP_SCREEN_TOP as u32 {
            let [qc, qc_1] = &mut shared_mut.delta_q_calc;
            (qc, qc_1)
        } else {
            let [qc_1, qc] = &mut shared_mut.delta_q_calc;
            (qc, qc_1)
        };

        let current_qos = entries::thread_nwm::rp_delta_q_qos() as f32;

        let mcus = screen.mcus as f32;
        let mcus_1 = screen_1.mcus as f32;
        let frame_rate_f = 1f32 / (frame_rate * mcus + frame_rate_1 * mcus_1);

        let mcusi = 1f32 / mcus;
        // let mcusi_1 = 1f32 / mcus_1;
        let qos_adj = self.worker.shared.qos_adj;
        let qos_b = current_qos * frame_rate_f * qos_adj;

        let comp_size = shared_mut.compressed_size.get(&s).load(Ordering::Acquire);
        let comp_size = {
            let size = comp_size & ((1 << entries::thread_nwm::JPEG_COMP_COUNT_SIZE_NBITS) - 1);
            let blkn = (comp_size >> entries::thread_nwm::JPEG_COMP_COUNT_SIZE_NBITS)
                & ((1 << entries::thread_nwm::JPEG_COMP_COUNT_BLKN_NBITS) - 1);
            if blkn > 0 {
                size as f32 * mcus * screen.max_blocks_in_mcu as f32 / blkn as f32
            } else {
                0f32
            }
        };
        let comp_size = comp_size * u8::BITS as f32 * mcusi;

        let (qos, qos_c) = if comp_size > 0f32 {
            let qos_d = if qc.qb > comp_size {
                qc.qb - comp_size
            } else if qc.qc < comp_size {
                qc.qc - comp_size
            } else {
                0f32
            };

            let update_coefs = |qe: &mut DeltaQCoefs, rb: f32| {
                let ri = 1f32 / rb;
                let r = 1f32 - ri;

                if false && need_ov_stats && qc.qd > 0 {
                    let qd = qc.qd as f32;

                    let nd = qc.nbits - comp_size;
                    let pm = qd * qe.m;
                    let pm = pm - nd;
                    let pm = pm * pm;

                    let w = unsafe { sqrtf(qd / (DELTA_Q_COUNT - 1) as f32) };
                    let wri = w * ri;
                    let wr = 1f32 - wri;
                    qe.p = qe.p * wr + pm * wri;

                    if nd > 0f32 {
                        let m = nd / qd;
                        qe.m = qe.m * wr + m * wri;
                    }
                }
                let qd_a = qe.d.abs();
                let qd_b = qos_d.abs();
                if qd_a * ri > qd_b || qd_b * ri > qd_a {
                    qe.d = qos_d;
                } else {
                    qe.d = qe.d * r + qos_d * ri;
                }
            };

            let rr: [f32; RP_DELTA_Q_COEFS_COUNT as usize] = [4f32];
            for i in 0..(if need_ov_stats {
                RP_DELTA_Q_COEFS_COUNT as usize
            } else {
                1
            }) {
                update_coefs(&mut qc.f[i], rr[i]);
            }

            let qd = qc.f[0].d.min(0f32);
            let qd_1 = qc_1.f[0].d.max(0f32);
            let qos_c =
                qos_b + qd + qd_1 * frame_rate_1.min(frame_rate) * mcus_1 * mcusi / frame_rate;

            (qos_c, qos_c)
        } else {
            (qos_b, qos_b)
        };

        let nbits = {
            let mut ret = 0f32;
            let qf = &screen.delta_q_params.qf;
            for i in 0..NUM_QUANT_TBLS {
                ret += (qnv[i].dc.nbits as f32 / qnc[i] as f32
                    + qnv[i].ac.nbits as f32 / qnc[i] as f32)
                    * qf[i];
            }
            ret + screen.delta_q_params.m // todo
        };

        {
            let q_steps_i = screen.delta_q_params.q_steps_i;
            let comp_d = if comp_size > 0f32 {
                qos - comp_size
            } else {
                0f32
            };
            let qd_1 = comp_d * q_steps_i;
            let nd = qc.nbits - nbits;
            let qd_2 = nd * q_steps_i;
            const QD2_THRES: f32 = 4f32;
            const QD2_MUL: f32 = 4f32 / 3f32;
            let qd2 = (if qd_2 < 0f32 {
                (qd_2 + SCALE_QD_I_F * QD2_THRES).min(0f32)
            } else {
                (qd_2 + SCALE_QD_I_F * QD2_THRES).min(0f32)
            }) * QD2_MUL;

            let scale_qd = |qd: f32, np: f32, pp: f32, ns: f32, ps: f32| {
                if qd < 0f32 {
                    (-unsafe { powf(-qd, np) } * (SCALE_QD_F * ns)).max(-SCALE_QD_F)
                } else {
                    (unsafe { powf(qd, pp) } * (SCALE_QD_F * ps)).min(SCALE_QD_F)
                }
            };

            let qd1: f32 = scale_qd(qd_1, 1.25f32, 1.25f32, 1f32, 1f32);
            let qd2 = scale_qd(qd2, 4f32 / 3f32, 4f32 / 3f32, 4f32, 4f32);

            let qd = qd1 + qd2;
            qc.q = qc.q * 0.75f32 + qd * (1f32 / 3f32);
            if qc.q > 0f32 && qd < 0f32 || qc.q < 0f32 && qd > 0f32 {
                qc.q = 0f32;
            } else {
                let q_thres = current_qos * (3f32 / RP_QOS_MAX as f32);
                if qc.q.abs() >= q_thres {
                    let q = (prev_delta_q as i32 + unsafe { roundf(qd) } as i32).clamp(0, qr as i32)
                        as u8;
                    *delta_q = q;
                    qc.qd = DELTA_Q_COUNT - 1 - q;
                    qc.nbits = nbits;
                    qc.q = 0f32;
                }
            }
        }

        qc.qb = qos_b;
        qc.qc = qos_c;
        if need_ov_stats {
            let ov_screen = unsafe {
                (*config_consts::OV_STATS)
                    .s
                    .get_unchecked_mut(s.get() as usize)
            };
            ov_screen.comp_size = (comp_size * 1000f32) as s32;
            let ov_screen = &mut ov_screen.delta_q;
            ov_screen.qb = (qos_b * 1000f32) as s32;
            ov_screen.qc = (qos_c * 1000f32) as s32;
            ov_screen.nbits = (nbits * 1000f32) as s32;
            ov_screen.qd = qc.qd as u32;
            for i in 0..RP_DELTA_Q_COEFS_COUNT as usize {
                let f = &mut ov_screen.f[i];
                f.d = (qc.f[i].d * 1000f32) as s32;
            }
        }

        let d_qrescale_prev = *delta_q as i8 - prev_delta_q as i8;
        *shared_mut.work_delta_q.get_mut(&w) = *delta_q;
        *shared_mut.dq_rescale_prev.get_mut(&w) = d_qrescale_prev;

        if d_qrescale_prev != 0 {
            for n in 0..NUM_QUANT_TBLS {
                let d_qshifts = unsafe {
                    &self
                        .worker
                        .shared
                        .delta_q_tbls
                        .get_unchecked(*delta_q as usize)
                        .get_unchecked(n)
                };

                let dq_shifts_prev = unsafe {
                    &self
                        .worker
                        .shared
                        .delta_q_tbls
                        .get_unchecked(prev_delta_q as usize)
                        .get_unchecked(n)
                };

                let rp_shifts = unsafe { shared_mut.rp_shifts.get_mut(&w).get_unchecked_mut(n) };

                for i in 0..DCTSIZE2 {
                    rp_shifts[i] = if d_qrescale_prev > 0 {
                        dq_shifts_prev[i] - d_qshifts[i]
                    } else {
                        d_qshifts[i] - dq_shifts_prev[i]
                    };
                }
            }
        }
    }

    #[named]
    fn compress<const DELTA_CACHE: bool, const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(
        &mut self,
        mcu_col_num: usize,
        prev: *mut JBlock,
    ) {
        let div_parts = &self.worker.shared.divisors.divisors;
        let w = self.worker.info.work_index;
        let mut blkn = 0;
        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);

        let shared_mut = unsafe { &mut *self.worker.shared_mut.cell };

        let cache = shared_mut.delta_q_cache.get_mut(&w);
        let cache_next_i = shared_mut.delta_q_cache_next.get_mut(&w);
        let delta_q = *shared_mut.work_delta_q.get(&w) as usize;
        let delta_q0 = unsafe { self.worker.shared.delta_q0_tbls.get_unchecked(delta_q) };
        let mut delta_cache_start = 0;

        let _need_wait_for_nwm = DELTA_Q && self.worker.thread_index.get() == 0;

        let comp_infos = unsafe { &*screen.comp_infos };
        for ci in 0..MAX_COMPONENTS {
            let comp = &comp_infos.infos[ci];

            let div_shifts = if DELTA_Q {
                unsafe {
                    self.worker
                        .shared
                        .div_delta_q_shifts
                        .get_unchecked(delta_q)
                        .get_unchecked(comp.quant_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .div_shifts
                        .get_unchecked(comp.quant_tbl_no as usize)
                }
            };

            let rp_shifts = unsafe {
                shared_mut
                    .rp_shifts
                    .get(&w)
                    .get_unchecked(comp.quant_tbl_no as usize)
            };

            let mcu_width = comp.h_samp_factor;
            let mcu_height = comp.v_samp_factor;

            let mcu_sample_width = mcu_width as u16 * DCTSIZE as u16;
            let xpos = mcu_col_num as u16 * mcu_sample_width;
            let mut ypos = 0;

            for _ in 0..mcu_height {
                let mut xpos = xpos;
                for _ in 0..mcu_width {
                    let mut cache_hit = false;
                    let output = unsafe { self.worker.bufs.mcu.get_unchecked_mut(blkn as usize) };
                    let prev = if DELTA_Q {
                        unsafe { prev.add(blkn as usize) }
                    } else {
                        ptr::null_mut()
                    };

                    if DELTA_CACHE {
                        for qi in cache_next_i[ci]..DELTA_Q_CACHE_COUNTS[ci] {
                            let delta_cache_i = delta_cache_start + qi;
                            let cache = unsafe { cache.get_unchecked_mut(delta_cache_i as usize) };

                            if cache.ypos > ypos {
                                break;
                            }

                            if cache.ypos == ypos && cache.xpos > xpos {
                                break;
                            }

                            if cache.xpos == xpos && cache.ypos == ypos {
                                if delta_q == DELTA_Q_COUNT as usize - 1 {
                                    *output = cache.cache;
                                    unsafe { *prev = cache.next };
                                } else {
                                    let delta_q0 = unsafe {
                                        delta_q0.get_unchecked(comp.quant_tbl_no as usize)
                                    };
                                    for i in 0..DCTSIZE2 {
                                        unsafe {
                                            let (off_prev, off_diff) = if RESCALE_PREV
                                                && RESCALE_PREV_SHR
                                            {
                                                let mask =
                                                    core::intrinsics::unchecked_shl(1, delta_q0[i])
                                                        - 1;
                                                let off_next = (cache.next[i] < 0) as JCoef
                                                    & ((cache.next[i] & mask) > 0) as JCoef;
                                                let off_prev = ((*prev)[i] < 0) as JCoef
                                                    & (((*prev)[i] & mask) > 0) as JCoef;
                                                let off_diff = (((*prev)[i] & mask)
                                                    > (cache.next[i] & mask))
                                                    as JCoef;
                                                (off_next, off_next - off_prev + off_diff)
                                            } else {
                                                let mask =
                                                    core::intrinsics::unchecked_shl(1, delta_q0[i])
                                                        - 1;
                                                let off_next = (cache.next[i] < 0) as JCoef
                                                    & ((cache.next[i] & mask) > 0) as JCoef;
                                                (off_next, off_next)
                                            };
                                            (*prev)[i] = core::intrinsics::unchecked_shr(
                                                cache.next[i],
                                                delta_q0[i],
                                            ) + off_prev;
                                            output[i] = core::intrinsics::unchecked_shr(
                                                cache.cache[i],
                                                delta_q0[i],
                                            ) + off_diff;
                                        }
                                    }
                                }
                                cache_next_i[ci] = qi + 1;
                                cache_hit = true;
                                break;
                            }
                        }
                    }

                    if !cache_hit {
                        unsafe {
                            forward_dct::<DELTA_Q, true, RESCALE_PREV, RESCALE_PREV_SHR>(
                                &self.worker.bufs.prep[ci],
                                output,
                                ypos,
                                xpos,
                                div_parts.get_unchecked(comp.quant_tbl_no as usize),
                                div_shifts,
                                prev,
                                rp_shifts,
                                ptr::null_mut(),
                            );
                        }
                    }

                    xpos += DCTSIZE as u16;
                    blkn += 1;
                }
                ypos += DCTSIZE as u16;
            }

            delta_cache_start += DELTA_Q_CACHE_COUNTS[ci];
        }
    }

    #[named]
    fn compress_dq<const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(
        &mut self,
        delta_cache: bool,
        mcu_col_num: usize,
        prev: *mut JBlock,
    ) {
        if delta_cache {
            self.compress::<true, RESCALE_PREV, RESCALE_PREV_SHR>(mcu_col_num, prev);
        } else {
            self.compress::<false, RESCALE_PREV, RESCALE_PREV_SHR>(mcu_col_num, prev);
        }
    }

    fn encode_mcu(&mut self) {
        let mut blkn = 0;

        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        let comp_infos = unsafe { &*screen.comp_infos };

        for ci in 0..MAX_COMPONENTS {
            let comp = &comp_infos.infos[ci];
            let mcu_width = comp.h_samp_factor;
            let mcu_height = comp.v_samp_factor;

            let dc_tbl = if DELTA_Q {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .dq_entropy_tbls
                        .dc_derived_tbls
                        .get_unchecked(comp.dc_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .entropy_tbls
                        .dc_derived_tbls
                        .get_unchecked(comp.dc_tbl_no as usize)
                }
            };
            let ac_tbl = if DELTA_Q {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .dq_entropy_tbls
                        .ac_derived_tbls
                        .get_unchecked(comp.ac_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .entropy_tbls
                        .ac_derived_tbls
                        .get_unchecked(comp.ac_tbl_no as usize)
                }
            };

            for _ in 0..mcu_height {
                for _ in 0..mcu_width {
                    let last_dc_val = self.worker.last_dc_vals[ci];
                    let dst = &mut self.dst;
                    let state = &mut self.worker.huff_state;
                    let block = unsafe { self.worker.bufs.mcu.get_unchecked(blkn) };
                    self.worker.last_dc_vals[ci] =
                        Self::encode_one_block(dst, state, block, last_dc_val, dc_tbl, ac_tbl);

                    blkn += 1;
                }
            }
        }
    }

    #[named]
    fn encode_one_block(
        dst: &mut WorkerDst,
        state: &mut HuffState,
        block: &[i16; DCTSIZE2],
        last_dc_val: i16,
        dc_derived_tbl: &DerivedTbl,
        ac_derived_tbl: &DerivedTbl,
    ) -> i16 {
        dst.blkn += 1;

        /* Although it is exceedingly rare, it is possible for a Huffman-encoded
         * coefficient block to be larger than the 128-byte unencoded block.  For each
         * of the 64 coefficients, PUT_BITS is invoked twice, and each invocation can
         * theoretically store 16 bits (for a maximum of 2048 bits or 256 bytes per
         * encoded block.)  If, for instance, one artificially sets the AC
         * coefficients to alternating values of 32767 and -32768 (using the JPEG
         * scanning order-- 1, 8, 16, etc.), then this will produce an encoded block
         * larger than 200 bytes.
         */
        const BUFSIZE: usize = DCTSIZE2 * 8;

        let mut localbuf: [u8; BUFSIZE] = const_default();
        let mut buf = EncodeBuffer::<_, REL_STREAM, DELTA_Q>::init(state, dst, &mut localbuf);

        let (val1, bits, b0) = {
            let val = block[0] as i32 - last_dc_val as i32;
            let sign1 = val >> (i32::BITS as u8 - 1);
            let val1 = val + sign1;
            let abs = val1 ^ sign1;
            (val1, jpeg_nbits_nonzero(abs) as i32, block[0])
        };

        unsafe {
            buf.put_code(
                *dc_derived_tbl.ehufco.get_unchecked(bits as usize),
                *dc_derived_tbl.ehufsi.get_unchecked(bits as usize),
                val1,
                bits,
            )
        };

        let mut r = 0;

        for jpeg_natural_order_of_k in JPEG_NATURAL_ORDER.into_iter().skip(1) {
            let val = *unsafe { block.get_unchecked(jpeg_natural_order_of_k as usize) } as i32;
            if val == 0 {
                r += 16;
            } else {
                let (val1, bits) = {
                    let sign1 = val >> (core::mem::size_of_val(&val) * 8 - 1);
                    let val1 = val + sign1;
                    let abs = val1 ^ sign1;
                    (val1, jpeg_nbits_nonzero(abs) as i32)
                };

                while r >= 16 * 16 {
                    r -= 16 * 16;
                    unsafe {
                        buf.put_bits(ac_derived_tbl.ehufco[0xf0], ac_derived_tbl.ehufsi[0xf0])
                    };
                }
                r += bits;
                unsafe {
                    buf.put_code(
                        *ac_derived_tbl.ehufco.get_unchecked(r as usize),
                        *ac_derived_tbl.ehufsi.get_unchecked(r as usize),
                        val1,
                        bits,
                    )
                };
                r = 0;
            }
        }

        if r > 0 {
            unsafe { buf.put_bits(ac_derived_tbl.ehufco[0], ac_derived_tbl.ehufsi[0]) };
        }

        buf.store();

        b0
    }
}

enum EncodeBufferBase<'a, const N: usize> {
    Local(&'a [u8; N]),
    Dst,
}
struct EncodeBuffer<'a, 'b, 'c, const N: usize, const REL_STREAM: bool, const DELTA_Q: bool> {
    buf: *mut u8,
    base: EncodeBufferBase<'a, N>,
    state: &'b mut HuffState,
    dst: &'c mut WorkerDst,
}

impl<'a, 'b, 'c, const N: usize, const REL_STREAM: bool, const DELTA_Q: bool>
    EncodeBuffer<'a, 'b, 'c, N, REL_STREAM, DELTA_Q>
where
    'a: 'c,
{
    pub fn init<'d: 'a>(
        state: &'b mut HuffState,
        dst: &'a mut WorkerDst,
        buf: &'d mut [u8; N],
    ) -> Self {
        if dst.free_in_bytes < N as u16 {
            EncodeBuffer {
                buf: buf.as_mut_ptr(),
                base: EncodeBufferBase::Local(buf),
                state,
                dst,
            }
        } else {
            EncodeBuffer {
                buf: dst.dst,
                base: EncodeBufferBase::Dst,
                state,
                dst,
            }
        }
    }

    pub fn store(self) {
        match self.base {
            EncodeBufferBase::Local(buf) => {
                let len = unsafe { self.buf.offset_from_unsigned(buf.as_ptr()) };
                self.dst.write_bytes::<REL_STREAM, DELTA_Q>(unsafe {
                    slice::from_raw_parts(buf.as_ptr(), len)
                });
            }
            EncodeBufferBase::Dst => unsafe { self.dst.advance_to(self.buf) },
        }
    }

    pub unsafe fn emit_byte(&mut self, b: u8) {
        unsafe {
            if REL_STREAM {
                *self.buf = b;
                self.buf = self.buf.add(1);
            } else {
                *self.buf = b;
                *(self.buf.add(1)) = 0;
                self.buf = self.buf.add(2 - (b < 0xFF) as usize);
            }
        }
    }

    unsafe fn flush(&mut self) {
        unsafe {
            if !REL_STREAM && (self.state.c & 0x80808080 & !(self.state.c + 0x01010101) > 0) {
                self.emit_byte((self.state.c >> 24) as u8);
                self.emit_byte((self.state.c >> 16) as u8);
                self.emit_byte((self.state.c >> 8) as u8);
                self.emit_byte(self.state.c as u8);
            } else {
                *self.buf = (self.state.c >> 24) as u8;
                *self.buf.add(1) = (self.state.c >> 16) as u8;
                *self.buf.add(2) = (self.state.c >> 8) as u8;
                *self.buf.add(3) = (self.state.c) as u8;
                self.buf = self.buf.add(4);
            }
        }
    }

    unsafe fn put_and_flush(&mut self, code: u32, size: u8) {
        self.state.c = unsafe {
            core::intrinsics::unchecked_shl(self.state.c, size as isize + self.state.free_bits)
                | core::intrinsics::unchecked_shr(code, -self.state.free_bits)
        };
        unsafe {
            self.flush();
        }
        self.state.free_bits += BIT_BUF_SIZE as isize;
        self.state.c = code;
    }

    pub unsafe fn put_bits(&mut self, code: u32, size: u8) {
        self.state.free_bits -= size as isize;
        if self.state.free_bits < 0 {
            unsafe {
                self.put_and_flush(code, size);
            }
        } else {
            self.state.c = unsafe { core::intrinsics::unchecked_shl(self.state.c, size) } | code;
        }
    }

    pub unsafe fn put_code(&mut self, code: u32, size: u8, mut temp: i32, mut nbits: i32) {
        temp &= unsafe { core::intrinsics::unchecked_shl(1, nbits) } - 1;
        temp |= unsafe { core::intrinsics::unchecked_shl(code as i32, nbits) };
        nbits += size as i32;
        unsafe {
            self.put_bits(temp as u32, nbits as u8);
        }
    }
}

unsafe fn forward_dct<
    const DELTA_Q: bool,
    const UPDATE_PREV: bool,
    const RESCALE_PREV: bool,
    const RESCALE_PREV_SHR: bool,
>(
    input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
    output: &mut JBlock,
    ypos: u16,
    xpos: u16,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    prev: *mut JBlock,
    r_pshifts: &[u8; DCTSIZE2],
    next: *mut JBlock,
) -> QuantizeRet {
    unsafe {
        convsamp(input, ypos, xpos, output);
    }
    fdct_ifast(output);
    quantize::<DELTA_Q, UPDATE_PREV, RESCALE_PREV, RESCALE_PREV_SHR>(
        output, div_parts, div_shifts, prev, r_pshifts, next,
    )
}

unsafe fn convsamp(
    input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
    ypos: u16,
    xpos: u16,
    output: &mut JBlock,
) {
    let mut oidx = 0;
    for yidx in 0..DCTSIZE {
        let input = unsafe { input.get_unchecked(ypos as usize + yidx) };
        for xidx in 0..DCTSIZE {
            output[oidx] =
                *unsafe { input.get_unchecked(xpos as usize + xidx) } as i16 - CENTERJSAMPLE as i16;

            oidx += 1;
        }
    }
}

fn multiply(v: i16, c: i32) -> i16 {
    const CONST_BITS: u8 = 8;
    ((v as i32 * c) >> CONST_BITS) as i16
}

fn fdct_ifast(inout: &mut JBlock) {
    const FIX_0_382683433: i32 = 98; /* FIX(0.382683433) */
    const FIX_0_541196100: i32 = 139; /* FIX(0.541196100) */
    const FIX_0_707106781: i32 = 181; /* FIX(0.707106781) */
    const FIX_1_306562965: i32 = 334; /* FIX(1.306562965) */

    /* Pass 1: process rows. */

    for i in (0..DCTSIZE2).step_by(DCTSIZE) {
        let tmp0 = inout[i + 0] + inout[i + 7];
        let tmp7 = inout[i + 0] - inout[i + 7];
        let tmp1 = inout[i + 1] + inout[i + 6];
        let tmp6 = inout[i + 1] - inout[i + 6];
        let tmp2 = inout[i + 2] + inout[i + 5];
        let tmp5 = inout[i + 2] - inout[i + 5];
        let tmp3 = inout[i + 3] + inout[i + 4];
        let tmp4 = inout[i + 3] - inout[i + 4];

        /* Even part */

        let tmp10 = tmp0 + tmp3; /* phase 2 */
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;

        inout[i + 0] = tmp10 + tmp11; /* phase 3 */
        inout[i + 4] = tmp10 - tmp11;

        let z1 = multiply(tmp12 + tmp13, FIX_0_707106781); /* c4 */
        inout[i + 2] = tmp13 + z1; /* phase 5 */
        inout[i + 6] = tmp13 - z1;

        /* Odd part */

        let tmp10 = tmp4 + tmp5; /* phase 2 */
        let tmp11 = tmp5 + tmp6;
        let tmp12 = tmp6 + tmp7;

        /* The rotator is modified from fig 4-8 to avoid extra negations. */
        let z5 = multiply(tmp10 - tmp12, FIX_0_382683433); /* c6 */
        let z2 = multiply(tmp10, FIX_0_541196100) + z5; /* c2-c6 */
        let z4 = multiply(tmp12, FIX_1_306562965) + z5; /* c2+c6 */
        let z3 = multiply(tmp11, FIX_0_707106781); /* c4 */

        let z11 = tmp7 + z3; /* phase 5 */
        let z13 = tmp7 - z3;

        inout[i + 5] = z13 + z2; /* phase 6 */
        inout[i + 3] = z13 - z2;
        inout[i + 1] = z11 + z4;
        inout[i + 7] = z11 - z4;
    }

    /* Pass 2: process columns. */

    for i in 0..DCTSIZE {
        let tmp0 = inout[i + DCTSIZE * 0] + inout[i + DCTSIZE * 7];
        let tmp7 = inout[i + DCTSIZE * 0] - inout[i + DCTSIZE * 7];
        let tmp1 = inout[i + DCTSIZE * 1] + inout[i + DCTSIZE * 6];
        let tmp6 = inout[i + DCTSIZE * 1] - inout[i + DCTSIZE * 6];
        let tmp2 = inout[i + DCTSIZE * 2] + inout[i + DCTSIZE * 5];
        let tmp5 = inout[i + DCTSIZE * 2] - inout[i + DCTSIZE * 5];
        let tmp3 = inout[i + DCTSIZE * 3] + inout[i + DCTSIZE * 4];
        let tmp4 = inout[i + DCTSIZE * 3] - inout[i + DCTSIZE * 4];

        /* Even part */

        let tmp10 = tmp0 + tmp3; /* phase 2 */
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;

        inout[i + DCTSIZE * 0] = tmp10 + tmp11; /* phase 3 */
        inout[i + DCTSIZE * 4] = tmp10 - tmp11;

        let z1 = multiply(tmp12 + tmp13, FIX_0_707106781); /* c4 */
        inout[i + DCTSIZE * 2] = tmp13 + z1; /* phase 5 */
        inout[i + DCTSIZE * 6] = tmp13 - z1;

        /* Odd part */

        let tmp10 = tmp4 + tmp5; /* phase 2 */
        let tmp11 = tmp5 + tmp6;
        let tmp12 = tmp6 + tmp7;

        /* The rotator is modified from fig 4-8 to avoid extra negations. */
        let z5 = multiply(tmp10 - tmp12, FIX_0_382683433); /* c6 */
        let z2 = multiply(tmp10, FIX_0_541196100) + z5; /* c2-c6 */
        let z4 = multiply(tmp12, FIX_1_306562965) + z5; /* c2+c6 */
        let z3 = multiply(tmp11, FIX_0_707106781); /* c4 */

        let z11 = tmp7 + z3; /* phase 5 */
        let z13 = tmp7 - z3;

        inout[i + DCTSIZE * 5] = z13 + z2; /* phase 6 */
        inout[i + DCTSIZE * 3] = z13 - z2;
        inout[i + DCTSIZE * 1] = z11 + z4;
        inout[i + DCTSIZE * 7] = z11 - z4;
    }
}

fn quantize<
    const DELTA_Q: bool,
    const UPDATE_PREV: bool,
    const RESCALE_PREV: bool,
    const RESCALE_PREV_SHR: bool,
>(
    inout: &mut JBlock,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    prev: *mut JBlock,
    rp_shifts: &[u8; DCTSIZE2],
    next: *mut JBlock,
) -> QuantizeRet {
    let mut ret = {
        let count = const_default::<QuantizeCounts>();
        QuantizeRet {
            dc: count,
            ac: count,
        }
    };
    for i in 0..DCTSIZE2 {
        let mut temp = inout[i];
        let recip = div_parts[i].recip as u16 as u32;
        let corr = div_parts[i].corr as u32;
        let shift = div_shifts[i];

        let sign1 = temp >> (core::mem::size_of_val(&temp) * 8 - 1);
        let abs = (temp + sign1) ^ sign1;

        let product = (abs as u32 + corr) * recip;
        let product = unsafe { core::intrinsics::unchecked_shr(product, shift) };
        temp = (product as i16 ^ sign1) - sign1;

        if DELTA_Q {
            if UPDATE_PREV {
                unsafe {
                    (*prev)[i] =
                        rescale_prev::<RESCALE_PREV, RESCALE_PREV_SHR>((*prev)[i], rp_shifts[i]);
                    let next = temp;
                    temp -= (*prev)[i];
                    (*prev)[i] = next;
                }
            } else {
                unsafe {
                    (*prev)[i] =
                        rescale_prev::<RESCALE_PREV, RESCALE_PREV_SHR>((*prev)[i], rp_shifts[i]);
                    (*next)[i] = temp;
                    temp -= (*prev)[i];

                    let nbits = jpeg_nbits_nonzero(temp.abs() as i32);
                    let update_counts = |c: &mut QuantizeCounts| {
                        c.nbits += nbits as u16;
                    };
                    if i == 0 {
                        update_counts(&mut ret.dc)
                    } else {
                        update_counts(&mut ret.ac)
                    }
                }
            }
        }

        inout[i] = temp;
    }
    ret
}

fn jpeg_nbits_nonzero(x: i32) -> u8 {
    (mem::size_of_val(&x) * 8 - x.leading_zeros() as usize) as u8
}

fn rescale_prev<const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(c: JCoef, s: u8) -> JCoef {
    unsafe {
        if RESCALE_PREV {
            if RESCALE_PREV_SHR {
                let mask = core::intrinsics::unchecked_shl(1, s) - 1;
                let off = (c < 0) as JCoef & ((c & mask) > 0) as JCoef;
                core::intrinsics::unchecked_shr(c, s) + off
            } else {
                core::intrinsics::unchecked_shl(c, s)
            }
        } else {
            c
        }
    }
}

fn dq_cache_gen_unique_indices(
    rand32: &mut Rand32,
    indices: &mut [u8; DELTA_Q_CACHE_MAX as usize],
    n: u8,
    m: u8,
) {
    for i in 0..n {
        let mut v = rand32.rand_range(0..(m - i) as u32) as u8;
        for j in 0..i {
            if v >= indices[j as usize] {
                v += 1;
            }
        }
        let mut done = false;
        for j in 0..i {
            if v < indices[j as usize] {
                for k in (j..i).rev() {
                    indices[k as usize + 1] = indices[k as usize];
                }
                indices[j as usize] = v;
                done = true;
                break;
            }
        }
        if !done {
            indices[i as usize] = v;
        }
    }
}

pub struct Jpeg {
    shared: JpegShared,
    shared_mut: JpegSharedMut,
    bufs: [WorkerBufs; RP_CORE_COUNT_MAX as usize],
    info: [CInfo; WORK_COUNT as usize],
}

impl Jpeg {
    pub unsafe fn once(&mut self) {
        self.shared.once();
        self.shared_mut.once();
    }

    #[named]
    pub fn init(
        &mut self,
        quality: u32,
        core_count: CoreCount,
        hq: [u32; RP_SCREEN_COUNT as usize],
        downsample: [u32; RP_SCREEN_COUNT as usize],
        delta_prog: bool,
    ) -> Option<()> {
        let shared_mut_params = self
            .shared
            .init(quality, delta_prog, core_count, hq, downsample);
        self.shared_mut.init(delta_prog, shared_mut_params);

        for w in WorkIndex::all() {
            let sem = self.shared.work_sem.get_mut(&w);
            if *sem > 0 {
                unsafe {
                    let _ = svcCloseHandle(*sem);
                }
                *sem = 0;
            }

            let res = unsafe { svcCreateSemaphore(sem, 0, core_count.get() as i32 - 1) };
            if res != 0 {
                ns_dbg_print!(create_semaphore_failed, c_str!("jpeg work_sem"), res);
                return None;
            }
            self.shared_mut
                .work_inited
                .get_mut(&w)
                .store(false, Ordering::Release);
            self.shared_mut
                .work_sem_count
                .get_mut(&w)
                .store(core_count.get() as u8, Ordering::Release);
        }
        for s in ScreenIndex::all() {
            let sem = self.shared.screen_sem.get_mut(&s);
            if *sem > 0 {
                unsafe {
                    let _ = svcCloseHandle(*sem);
                }
                *sem = 0;
            }

            let res = unsafe { svcCreateSemaphore(sem, 1, 1) };
            if res != 0 {
                ns_dbg_print!(create_semaphore_failed, c_str!("jpeg screen_sem"), res);
                return None;
            }

            self.shared_mut
                .screen_bool
                .get_mut(&s)
                .store(false, Ordering::Release);
            *self.shared_mut.last_restart_interval.get_mut(&s) = 0;
        }

        unsafe {
            ptr::write_bytes(
                self.shared_mut.dq_prev_coeffs_top.as_mut_ptr(),
                0,
                self.shared_mut.dq_prev_coeffs_top.len(),
            );

            ptr::write_bytes(
                self.shared_mut.dq_prev_coeffs_bot.as_mut_ptr(),
                0,
                self.shared_mut.dq_prev_coeffs_bot.len(),
            );
        }

        Some(())
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
