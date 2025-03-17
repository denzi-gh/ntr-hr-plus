#![allow(unused_macros)]

use crate::*;
pub mod vars;
use vars::*;

const DELTA_Q_COUNT: u8 = 32;
const DELTA_Q_MAX: f32 = 7.0f32;
#[allow(unused)]
const MAX_COEF_BITS: u8 = u8::BITS as u8 + 2;

const DELTA_Q_STEP: f32 = DELTA_Q_MAX / DELTA_Q_COUNT as f32;

pub struct JpegRet {
    pub deltaQ: u8,
}

#[derive(ConstDefault)]
struct DeltaQCoefs {
    m: f32,
    p: f32,
    q: f32,
    d: f32,
}

const QS_MIN: f32 = 1f32;

#[derive(ConstDefault)]
struct DeltaQManager {
    f: [DeltaQCoefs; RP_DELTA_Q_COEFS_COUNT as usize],
    m: f32,
    n: f32,
    qs: f32,
    qb: f32,
    qc: f32,
    q: f32,
    // c: i8,
    cc: f32,
    cn: f32,
}

#[derive(ConstDefault, Clone, Copy)]
struct QuantizeCounts {
    // all: u16,
    // min: u8,
    // max: u8,
    nbits: u16,
    // nleft: u16,
}

#[derive(ConstDefault)]
struct QuantizeRet {
    dc: QuantizeCounts,
    ac: QuantizeCounts,
}

// const DELTA_Q_N_COUNT: usize = 16;

struct DeltaQParams {
    qf: [f32; NUM_QUANT_TBLS],
    // q_step: (f32, f32),
    q_steps: f32,
    // q_step_i: (f32, f32),
    // q_steps_i: f32,
}

pub struct JpegShared<'a> {
    quality: u32,
    chromaSS: u8,
    quantTbls: QuantTbls,
    divisors: Divisors,
    divShifts: [[u8; DCTSIZE2]; NUM_QUANT_TBLS],
    divDeltaQShifts: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    coreCount: CoreCount,
    compInfos: &'a CompInfos,
    pub maxHSampFactor: usize,
    pub maxVSampFactor: usize,
    pub maxBlocksInMcu: usize,
    pub mcuRowSize: usize,
    pub mcuColSize: usize,
    pub mcusPerRow: usize,
    pub mcusTop: u16,
    pub mcusBot: u16,
    qosAdj: f32,
    jpegTbls: JpegTbls,
    deltaQTbls: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    deltaQ0Tbls: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    // deltaQNs: [[[u8; DELTA_Q_N_COUNT]; DCTSIZE2]; NUM_QUANT_TBLS],
    deltaQMs: [[(u16, u16); NUM_QUANT_TBLS]; DELTA_Q_COUNT as usize],
    pub workSem: [Handle; WORK_COUNT as usize],
    pub screenSem: [Handle; SCREEN_COUNT as usize],
    targetFrameRate: u8,
    deltaQParams: DeltaQParams,
}

const DELTA_Q_CACHE_COUNTS: [u8; MAX_COMPONENTS] = [4, 2, 2];
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
    hit: bool,
}

pub struct JpegSharedMut {
    pub compressedSize: [u32; SCREEN_COUNT as usize],
    pub workInited: [bool; WORK_COUNT as usize],
    pub screenSemCount: [u8; SCREEN_COUNT as usize],
    deltaQ: [u8; SCREEN_COUNT as usize],
    dQRescalePrev: [i8; SCREEN_COUNT as usize],
    rPShifts: [[[u8; DCTSIZE2]; NUM_QUANT_TBLS]; SCREEN_COUNT as usize],
    deltaQCache: [[DeltaQCache; DELTA_Q_CACHE_TOTAL as usize]; SCREEN_COUNT as usize],
    deltaQCalc: [DeltaQManager; SCREEN_COUNT as usize],
    rand32: Rand32,
}

impl JpegSharedMut {
    pub fn init(&mut self) {
        self.rand32 = Rand32::new(unsafe { svcGetSystemTick() });
    }
}

const fn jdiv_round_up(a: usize, b: usize) -> usize
/* Compute a/b rounded up to next integer, ie, ceil(a/b) */
/* Assumes a >= 0, b > 0 */
{
    (a + b - 1) / b
}

impl<'a> JpegShared<'a> {
    #[named]
    fn initDeltaQTbls(&mut self) {
        unsafe {
            for d in (0..DELTA_Q_COUNT as usize).rev() {
                let f = DELTA_Q_MAX / DELTA_Q_COUNT as f32 * d as f32;

                for j in 0..NUM_QUANT_TBLS {
                    let btbls = if j == 0 {
                        &std_luminance_quant_tbl
                    } else {
                        &std_chrominance_quant_tbl
                    };

                    let mut log2_tbls: [f32; DCTSIZE2] = const_default();
                    for i in 0..DCTSIZE2 {
                        let v = log2f(*btbls.get_unchecked(i) as f32);
                        // nsDbgPrint!(int, c_str!("log2"), v as i32);
                        log2_tbls[i] = v;
                    }
                    self.deltaQMs[d][j].1 = 0;
                    for i in 0..DCTSIZE2 {
                        let v = roundf(f32::max(log2_tbls.get_unchecked(i) - f, 0.0f32)) as u8;
                        self.deltaQTbls[d][j][i] = v;
                        let m = v - self.deltaQTbls[DELTA_Q_COUNT as usize - 1][j][i];
                        self.deltaQ0Tbls[d][j][i] = m;
                        if i == 0 {
                            self.deltaQMs[d][j].0 = m as u16;
                        } else {
                            self.deltaQMs[d][j].1 += m as u16;
                        }
                    }
                    // nsDbgPrint!(int, c_str!("qn"), *qn as i32);
                }
            }

            // for j in 0..NUM_QUANT_TBLS {
            //     for i in 0..DCTSIZE2 {
            //         for k in 0..DELTA_Q_N_COUNT as usize {
            //             self.deltaQNs[j][i][k] = (|| {
            //                 for d in (0..DELTA_Q_COUNT as usize).rev() {
            //                     if self.deltaQ0Tbls[d][j][i] >= k as u8 {
            //                         return DELTA_Q_COUNT - 1 - d as u8;
            //                     }
            //                 }
            //                 DELTA_Q_COUNT
            //             })();
            //         }
            //     }
            // }
        }
    }

    pub fn init(&mut self) {
        self.jpegTbls = JpegTbls::init();
        self.initDeltaQTbls();
    }

    fn setCompInfos<'b: 'a>(&'b mut self, hq: u32, deltaProg: bool) -> (usize, f32) {
        if hq as u8_ == RP_CHROMASS_444 {
            self.compInfos = &self.jpegTbls.compInfos444;
        } else if hq as u8_ == RP_CHROMASS_422 {
            self.compInfos = &self.jpegTbls.compInfos422;
        } else {
            self.compInfos = &self.jpegTbls.compInfos420;
        }
        self.maxHSampFactor = 1;
        self.maxVSampFactor = 1;
        self.maxBlocksInMcu = 0;
        for i in 0..MAX_COMPONENTS {
            self.maxHSampFactor = cmp::max(
                self.maxHSampFactor,
                self.compInfos.infos[i].h_samp_factor as usize,
            );
            self.maxVSampFactor = cmp::max(
                self.maxVSampFactor,
                self.compInfos.infos[i].v_samp_factor as usize,
            );
            self.maxBlocksInMcu += self.compInfos.infos[i].h_samp_factor as usize
                * self.compInfos.infos[i].v_samp_factor as usize;
        }
        if self.maxBlocksInMcu > MAX_BLOCKS_IN_MCU {
            panic!();
        }
        self.mcuRowSize = DCTSIZE * self.maxHSampFactor;
        self.mcuColSize = DCTSIZE * self.maxVSampFactor;
        self.mcusPerRow = jdiv_round_up(GSP_SCREEN_WIDTH as usize, self.mcuRowSize);
        self.mcusTop = (self.mcusPerRow
            * jdiv_round_up(GSP_SCREEN_HEIGHT_TOP as usize, self.mcuColSize))
            as u16;
        self.mcusBot = (self.mcusPerRow
            * jdiv_round_up(GSP_SCREEN_HEIGHT_BOTTOM as usize, self.mcuColSize))
            as u16;

        if deltaProg {
            let mut qf: [u16; NUM_QUANT_TBLS] = const_default();
            for ci in 0..MAX_COMPONENTS {
                let comp = &self.compInfos.infos[ci];
                let MCU_we = comp.h_samp_exp;
                let MCU_he = comp.v_samp_exp;

                unsafe {
                    *qf.get_unchecked_mut(comp.quant_tbl_no as usize) +=
                        core::intrinsics::unchecked_shl(1, MCU_we + MCU_he)
                };
            }
            let qf = {
                let mut ret: [f32; NUM_QUANT_TBLS] = const_default();
                for i in 0..NUM_QUANT_TBLS {
                    ret[i] = qf[i] as f32;
                }
                ret
            };
            self.deltaQParams.qf = qf;
            let qt = {
                let mut qt = 0f32;
                for i in 0..NUM_QUANT_TBLS {
                    qt += self.deltaQParams.qf[i];
                }
                qt
            };
            let q_step = (DELTA_Q_STEP * qt, DELTA_Q_STEP * (DCTSIZE2 - 1) as f32 * qt);
            let q_steps = q_step.0 + q_step.1;
            // let q_step_i = (1f32 / q_step.0, 1f32 / q_step.1);
            // let q_steps_i = 1f32 / q_steps;
            // self.deltaQParams.q_step = q_step;
            self.deltaQParams.q_steps = q_steps;
            // self.deltaQParams.q_step_i = q_step_i;
            // self.deltaQParams.q_steps_i = q_steps_i;
        }

        (self.maxBlocksInMcu, self.deltaQParams.q_steps)
    }
}

#[derive(Copy, Clone, ConstDefault)]
pub struct ArqRpHdr {
    pub w: WorkIndex,
    pub t: ThreadId,
}

const _arq_rp_hdr_size_assert: () = {
    assert!(mem::size_of::<u16>() == ARQ_DATA_HDR_SIZE as usize);
};

const _arq_rp_w_size_assert: () = {
    assert!(WORK_COUNT - 1 <= ((1 << RP_KCP_HDR_W_NBITS) - 1));
};

// We store RP_CORE_COUNT_MAX as a special value to indicate term packet
const _arq_rp_t_size_assert: () = {
    assert!(RP_CORE_COUNT_MAX <= ((1 << RP_KCP_HDR_T_NBITS) - 1));
};

impl ArqRpHdr {
    pub unsafe fn write_hdr(&self, dst: *mut u8) {
        let hdr = (self.w.get() as u16) << (PID_NBITS + CID_NBITS)
            | (self.t.get() as u16) << (PID_NBITS + CID_NBITS + RP_KCP_HDR_W_NBITS);
        ptr::copy_nonoverlapping(&hdr, dst as *mut _, 1);
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
    pub info: *const crate::entries::NwmInfo,
    pub hdr: ArqRpHdr,
}

#[derive(Clone, ConstDefault)]
pub struct WorkerDst {
    pub s: ScreenIndex,
    pub w: ScreenIndex,
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
            entries::rp_dq_update_size::<DELTA_Q>(
                self.s,
                self.w,
                entries::packet_data_size_kcp as u32,
            );
            crate::entries::rp_send_buffer::<REL_STREAM>(self, false)
        }
    }

    fn term<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self) -> bool {
        unsafe {
            entries::rp_dq_update_size::<DELTA_Q>(
                self.s,
                self.w,
                entries::packet_data_size_kcp as u32 - self.free_in_bytes as u32,
            );
            crate::entries::rp_send_buffer::<REL_STREAM>(self, true)
        }
    }

    pub unsafe fn advance_to(&mut self, dst: *mut u8) {
        self.free_in_bytes -= dst.sub_ptr(self.dst) as u16;
        self.dst = dst;
    }
}

#[derive(ConstDefault, Clone, Copy)]
pub struct CInfo {
    pub isTop: bool,
    pub colorSpace: ColorSpace,
    pub restartInterval: u16,
    pub workIndex: WorkIndex,
    pub coreCount: CoreCount,
}

type BitBufType = u32;

#[derive(ConstDefault)]
pub struct HuffState {
    c: BitBufType,
    free_bits: isize,
}

pub const BIT_BUF_SIZE: usize = mem::size_of::<BitBufType>() * 8;

#[derive(ConstDefault)]
pub struct JpegWorker<'a, 'b, const REL_STREAM: bool> {
    shared: &'a JpegShared<'b>,
    shared_mut: &'a mut JpegSharedMut,
    bufs: &'a mut WorkerBufs,
    info: &'a mut CInfo,
    threadId: ThreadId,
    huffState: HuffState,
    last_dc_val: [i16; MAX_COMPONENTS],
}

pub struct JpegEncode<'a, 'b, 'c, const REL_STREAM: bool, const DELTA_Q: bool> {
    worker: &'c mut JpegWorker<'a, 'b, REL_STREAM>,
    dst: WorkerDst,
}

#[derive(ConstDefault)]
pub struct Jpeg<'a> {
    pub shared: JpegShared<'a>,
    pub shared_mut: JpegSharedMut,
    bufs: [WorkerBufs; RP_CORE_COUNT_MAX as usize],
    info: [CInfo; WORK_COUNT as usize],
}

impl<'b> Jpeg<'b> {
    pub fn reset<'a: 'b>(
        &'a mut self,
        quality: u32,
        coreCount: CoreCount,
        hq: u32,
        deltaProg: bool,
    ) {
        self.shared.quality = quality;
        let quality = if deltaProg { 100 } else { quality };
        self.shared.quantTbls.setQuality(quality);
        self.shared
            .divisors
            .setDivisors(&self.shared.quantTbls, &mut self.shared.divShifts);

        if deltaProg {
            for q in 0..DELTA_Q_COUNT {
                let divShifts = &mut self.shared.divDeltaQShifts[q as usize];
                for i in 0..NUM_QUANT_TBLS {
                    let baseShifts = &self.shared.divShifts[i];
                    let shifts = &mut divShifts[i];
                    let ltbl = &self.shared.deltaQTbls[q as usize][i];

                    for i in 0..DCTSIZE2 {
                        shifts[i] = baseShifts[i] + ltbl[i];
                    }
                }
            }
            self.shared_mut.compressedSize = const_default();
            for s in 0..SCREEN_COUNT {
                self.shared_mut.deltaQ[s as usize] = DELTA_Q_COUNT - 1;
            }

            const qos_adj_b: f32 = u8::BITS as f32;
            const qos_min_f: f32 = 0.75f32;
            const qos_max_l_f: f32 = 0.9375f32;
            const qos_max_h_f: f32 = 0.875f32;
            self.shared.qosAdj = qos_adj_b * qos_min_f
                + ((qos_max_l_f
                    + (qos_max_h_f - qos_max_l_f)
                        * entries::rp_delta_q_qos() as f32
                        * (1f32 / RP_QOS_MAX as f32)
                    - qos_min_f)
                    * qos_adj_b
                    * self.shared.quality as f32
                    * (1f32 / RP_QUALITY_MAX as f32));
        }

        self.shared_mut.deltaQCalc = const_default();
        self.shared.coreCount = coreCount;
        self.shared.targetFrameRate = 60;
        self.shared.chromaSS = hq as u8;
        let (maxBlocksInMcu, q_steps) = self.shared.setCompInfos(hq, deltaProg);
        if deltaProg {
            for i in 0..SCREEN_COUNT as usize {
                self.shared_mut.deltaQCalc[i].m = (9 * maxBlocksInMcu) as f32 - QS_MIN;
                self.shared_mut.deltaQCalc[i].n = q_steps * DELTA_Q_COUNT as f32 + QS_MIN;
            }
        }
    }

    pub fn setInfo(&mut self, info: CInfo) {
        *info.workIndex.index_into_mut(&mut self.info) = info;
    }

    pub unsafe fn getWorker<'a, const REL_STREAM: bool>(
        &'a mut self,
        workIndex: WorkIndex,
        threadId: ThreadId,
    ) -> JpegWorker<'a, 'b, REL_STREAM> {
        JpegWorker::init(
            &self.shared,
            &mut self.shared_mut,
            threadId.index_into_mut(&mut self.bufs),
            workIndex.index_into_mut(&mut self.info),
            threadId,
        )
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
            .array_chunks::<P>()
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
            .array_chunks::<2>()
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

enum EncodeBufferBase<'a, const N: usize> {
    Local(&'a [u8; N]),
    Dst,
}

struct EncodeBuffer<'a, 'c, 'd, const N: usize, const REL_STREAM: bool, const DELTA_Q: bool> {
    buf: *mut u8,
    base: EncodeBufferBase<'a, N>,
    state: &'c mut HuffState,
    dst: &'d mut WorkerDst,
}

impl<'a, 'c, 'd, const N: usize, const REL_STREAM: bool, const DELTA_Q: bool>
    EncodeBuffer<'a, 'c, 'd, N, REL_STREAM, DELTA_Q>
where
    'a: 'd,
{
    pub fn init<'e: 'a>(
        state: &'c mut HuffState,
        dst: &'a mut WorkerDst,
        buf: &'e mut [u8; N],
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
                let len = unsafe { self.buf.sub_ptr(buf.as_ptr()) };
                self.dst.write_bytes::<REL_STREAM, DELTA_Q>(unsafe {
                    slice::from_raw_parts(buf.as_ptr(), len)
                });
            }
            EncodeBufferBase::Dst => unsafe { self.dst.advance_to(self.buf) },
        }
    }

    pub unsafe fn EMIT_BYTE(&mut self, b: u8) {
        if REL_STREAM {
            *self.buf = b;
            self.buf = self.buf.add(1);
        } else {
            *self.buf = b;
            *(self.buf.add(1)) = 0;
            self.buf = self.buf.add(2 - (b < 0xFF) as usize);
        }
    }

    unsafe fn FLUSH(&mut self) {
        if !REL_STREAM && (self.state.c & 0x80808080 & !(self.state.c + 0x01010101) > 0) {
            self.EMIT_BYTE((self.state.c >> 24) as u8);
            self.EMIT_BYTE((self.state.c >> 16) as u8);
            self.EMIT_BYTE((self.state.c >> 8) as u8);
            self.EMIT_BYTE(self.state.c as u8);
        } else {
            *self.buf = (self.state.c >> 24) as u8;
            *self.buf.add(1) = (self.state.c >> 16) as u8;
            *self.buf.add(2) = (self.state.c >> 8) as u8;
            *self.buf.add(3) = (self.state.c) as u8;
            self.buf = self.buf.add(4);
        }
    }

    unsafe fn PUT_AND_FLUSH(&mut self, code: u32, size: u8) {
        self.state.c =
            core::intrinsics::unchecked_shl(self.state.c, size as isize + self.state.free_bits)
                | core::intrinsics::unchecked_shr(code, -self.state.free_bits);
        self.FLUSH();
        self.state.free_bits += BIT_BUF_SIZE as isize;
        self.state.c = code;
    }

    pub unsafe fn PUT_BITS(&mut self, code: u32, size: u8) {
        self.state.free_bits -= size as isize;
        if self.state.free_bits < 0 {
            self.PUT_AND_FLUSH(code, size);
        } else {
            self.state.c = core::intrinsics::unchecked_shl(self.state.c, size) | code;
        }
    }

    pub unsafe fn PUT_CODE(&mut self, code: u32, size: u8, mut temp: i32, mut nbits: i32) {
        temp &= core::intrinsics::unchecked_shl(1, nbits) - 1;
        temp |= core::intrinsics::unchecked_shl(code as i32, nbits);
        nbits += size as i32;
        self.PUT_BITS(temp as u32, nbits as u8);
    }
}

fn JPEG_NBITS_NONZERO(x: i32) -> u8 {
    (mem::size_of_val(&x) * 8 - x.leading_zeros() as usize) as u8
}

fn JPEG_NBITS(x: i32) -> u8 {
    if x > 0 {
        JPEG_NBITS_NONZERO(x)
    } else {
        0
    }
}

impl<'a, 'b, const REL_STREAM: bool> JpegWorker<'a, 'b, REL_STREAM> {
    pub fn encode<F, G>(
        &'a mut self,
        dst: WorkerDst,
        src: &[u8],
        pre_progress: F,
        progress: G,
    ) -> JpegRet
    where
        F: FnMut(),
        G: FnMut(),
    {
        let delta_prog = REL_STREAM && unsafe { entries::get_reliable_stream_delta_prog() };
        if delta_prog {
            JpegEncode::<_, true> { worker: self, dst }.encode(src, pre_progress, progress)
        } else {
            JpegEncode::<_, false> { worker: self, dst }.encode(src, pre_progress, progress)
        }
    }

    pub fn init(
        shared: &'a JpegShared<'b>,
        shared_mut: &'a mut JpegSharedMut,
        bufs: &'a mut WorkerBufs,
        info: &'a mut CInfo,
        tid: ThreadId,
    ) -> Self {
        JpegWorker {
            shared,
            shared_mut,
            bufs,
            info,
            threadId: tid,
            huffState: const_default(),
            last_dc_val: const_default(),
        }
    }
}

impl<'a, 'b, 'c, const REL_STREAM: bool, const DELTA_Q: bool>
    JpegEncode<'a, 'b, 'c, REL_STREAM, DELTA_Q>
{
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
        let qtbl = &self.worker.shared.quantTbls.quantTbls[index];

        self.write_marker(M_DQT);
        self.write_2bytes((DCTSIZE2 + 1 + 2) as u16);
        self.write_byte(index as u8);
        for i in 0..DCTSIZE2 {
            /* The table entries must be emitted in zigzag order. */
            let qval =
                *unsafe { qtbl.quantval.get_unchecked(jpeg_natural_order[i] as usize) } as u8;
            self.write_byte(qval);
        }
    }

    fn write_sof(&mut self, code: u8) {
        self.write_marker(code);

        self.write_2bytes((3 * MAX_COMPONENTS + 2 + 5 + 1) as u16); /* length */

        self.write_byte(8);
        self.write_2bytes(self.screen_height() as u16);
        self.write_2bytes(GSP_SCREEN_WIDTH as u16);

        self.write_byte(MAX_COMPONENTS as u8);

        for info in &self.worker.shared.compInfos.infos {
            self.write_byte(info.component_id);
            self.write_byte((info.h_samp_factor << 4) + info.v_samp_factor);
            self.write_byte(info.quant_tbl_no);
        }
    }

    fn screen_height(&self) -> u32 {
        if self.worker.info.isTop {
            GSP_SCREEN_HEIGHT_TOP
        } else {
            GSP_SCREEN_HEIGHT_BOTTOM
        }
    }

    fn write_dht(&mut self, mut index: usize, is_ac: bool) {
        let tbl = if is_ac {
            &self.worker.shared.jpegTbls.huffTbls.acHuffTbls[index]
        } else {
            &self.worker.shared.jpegTbls.huffTbls.dcHuffTbls[index]
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
            self.write_byte(tbl.huffval[i as usize]);
        }
    }

    fn write_dri(&mut self) {
        self.write_marker(M_DRI);
        self.write_2bytes(4); /* fixed length */
        self.write_2bytes(self.worker.info.restartInterval);
    }

    fn write_sos(&mut self) {
        self.write_marker(M_SOS);

        self.write_2bytes((2 * MAX_COMPONENTS + 2 + 1 + 3) as u16); /* length */

        self.write_byte(MAX_COMPONENTS as u8);

        for i in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[i];
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
        if self.worker.shared.coreCount.get() > 1 {
            self.write_dri();
        }
        self.write_sos();
    }

    fn write_rst(&mut self) {
        self.write_marker(M_RST0 + self.worker.threadId.get() as u8);
    }

    fn write_trailer(&mut self) {
        self.write_marker(M_EOI);
    }

    fn write_term(&mut self) {
        self.dst.term::<REL_STREAM, DELTA_Q>();
    }

    pub fn get_bpp_for_format(&self) -> u8 {
        match self.worker.info.colorSpace {
            ColorSpace::XBGR => 4,
            ColorSpace::BGR => 3,
            _ => 2,
        }
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
        match self.worker.info.colorSpace {
            ColorSpace::XBGR => cconvert::<3, 2, 1, 4, { S }>(
                input,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpegTbls.colorConvTbls.rgb_ycc_tab,
            ),
            ColorSpace::BGR => cconvert::<2, 1, 0, 3, { S }>(
                input,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpegTbls.colorConvTbls.rgb_ycc_tab,
            ),
            ColorSpace::RGB565 => cconvert2::<{ S }, _>(
                input,
                rgb565_comps,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpegTbls.colorConvTbls,
            ),
            ColorSpace::RGB5A1 => cconvert2::<{ S }, _>(
                input,
                rgb5a1_comps,
                &mut self.worker.bufs.color,
                &self.worker.shared.jpegTbls.colorConvTbls,
            ),
            ColorSpace::RGB4 => todo!(),
        }
    }

    fn h2v1_downsample(
        input: &[u8; GSP_SCREEN_WIDTH as usize],
        output: &mut [u8; GSP_SCREEN_WIDTH as usize],
    ) {
        let mut bias = 0;
        for (input, output) in input.array_chunks::<{ MAX_SAMP_FACTOR }>().zip(output) {
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
        let input0 = input0.array_chunks::<{ MAX_SAMP_FACTOR }>();
        let input1 = input1.array_chunks::<{ MAX_SAMP_FACTOR }>();
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
        for (base, chunk) in src.array_chunks::<{ MAX_SAMP_FACTOR }>().enumerate() {
            let output_base = if which_half { base + DCTSIZE / 2 } else { base };
            self.color_convert::<_, true, true>(chunk, output_base);
            self.downsample::<true, true>(output_base);
        }
    }

    fn pre_process_no_vsubsamp<const H_SAMP: bool>(&mut self, src: [&[u8]; DCTSIZE]) {
        for (base, chunk) in src.array_chunks::<1>().enumerate() {
            self.color_convert::<_, H_SAMP, false>(chunk, base);
            self.downsample::<H_SAMP, false>(base);
        }
    }

    fn convsamp(
        input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
        ypos: u16,
        xpos: u16,
        output: &mut JBlock,
    ) {
        let mut oidx = 0;
        for yidx in 0..DCTSIZE {
            let input = unsafe { input.get_unchecked(ypos as usize + yidx) };
            for xidx in 0..DCTSIZE {
                output[oidx] = *unsafe { input.get_unchecked(xpos as usize + xidx) } as i16
                    - CENTERJSAMPLE as i16;

                oidx += 1;
            }
        }
    }

    fn fdct_ifast(inout: &mut JBlock) {
        const CONST_BITS: u8 = 8;

        const FIX_0_382683433: i32 = 98; /* FIX(0.382683433) */
        const FIX_0_541196100: i32 = 139; /* FIX(0.541196100) */
        const FIX_0_707106781: i32 = 181; /* FIX(0.707106781) */
        const FIX_1_306562965: i32 = 334; /* FIX(1.306562965) */

        fn MULTIPLY(v: i16, c: i32) -> i16 {
            ((v as i32 * c) >> CONST_BITS) as i16
        }

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

            let z1 = MULTIPLY(tmp12 + tmp13, FIX_0_707106781); /* c4 */
            inout[i + 2] = tmp13 + z1; /* phase 5 */
            inout[i + 6] = tmp13 - z1;

            /* Odd part */

            let tmp10 = tmp4 + tmp5; /* phase 2 */
            let tmp11 = tmp5 + tmp6;
            let tmp12 = tmp6 + tmp7;

            /* The rotator is modified from fig 4-8 to avoid extra negations. */
            let z5 = MULTIPLY(tmp10 - tmp12, FIX_0_382683433); /* c6 */
            let z2 = MULTIPLY(tmp10, FIX_0_541196100) + z5; /* c2-c6 */
            let z4 = MULTIPLY(tmp12, FIX_1_306562965) + z5; /* c2+c6 */
            let z3 = MULTIPLY(tmp11, FIX_0_707106781); /* c4 */

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

            let z1 = MULTIPLY(tmp12 + tmp13, FIX_0_707106781); /* c4 */
            inout[i + DCTSIZE * 2] = tmp13 + z1; /* phase 5 */
            inout[i + DCTSIZE * 6] = tmp13 - z1;

            /* Odd part */

            let tmp10 = tmp4 + tmp5; /* phase 2 */
            let tmp11 = tmp5 + tmp6;
            let tmp12 = tmp6 + tmp7;

            /* The rotator is modified from fig 4-8 to avoid extra negations. */
            let z5 = MULTIPLY(tmp10 - tmp12, FIX_0_382683433); /* c6 */
            let z2 = MULTIPLY(tmp10, FIX_0_541196100) + z5; /* c2-c6 */
            let z4 = MULTIPLY(tmp12, FIX_1_306562965) + z5; /* c2+c6 */
            let z3 = MULTIPLY(tmp11, FIX_0_707106781); /* c4 */

            let z11 = tmp7 + z3; /* phase 5 */
            let z13 = tmp7 - z3;

            inout[i + DCTSIZE * 5] = z13 + z2; /* phase 6 */
            inout[i + DCTSIZE * 3] = z13 - z2;
            inout[i + DCTSIZE * 1] = z11 + z4;
            inout[i + DCTSIZE * 7] = z11 - z4;
        }
    }

    fn rescale_prev<const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(
        c: JCoef,
        s: u8,
    ) -> JCoef {
        unsafe {
            if RESCALE_PREV {
                if RESCALE_PREV_SHR {
                    let mask = core::intrinsics::unchecked_shl(1, s) - 1;
                    let off = (c >> (JCoef::BITS as u8 - 1)) & ((c & mask) > 0) as JCoef;
                    core::intrinsics::unchecked_shr(c, s) + off
                } else {
                    core::intrinsics::unchecked_shl(c, s)
                }
            } else {
                c
            }
        }
    }

    fn quantize<const UPDATE_PREV: bool, const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(
        inout: &mut JBlock,
        divParts: &[DivisorPart; DCTSIZE2],
        divShifts: &[u8; DCTSIZE2],
        prev: *mut JBlock,
        rPShifts: &[u8; DCTSIZE2],
        next: *mut JBlock,
        // deltaQNs: *const [[u8; DELTA_Q_N_COUNT]; DCTSIZE2],
        // deltaQ0: *const [u8; DCTSIZE2],
    ) -> QuantizeRet {
        let mut ret = {
            let count = const_default::<QuantizeCounts>();
            // count.min = DELTA_Q_COUNT as u8 - 1;
            QuantizeRet {
                dc: count,
                ac: count,
            }
        };
        for i in 0..DCTSIZE2 {
            let mut temp = inout[i];
            let recip = divParts[i].recip as u16 as u32;
            let corr = divParts[i].corr as u32;
            let shift = divShifts[i];

            if temp < 0 {
                temp = -temp;
                let mut product = (temp as u32 + corr) * recip;
                product = unsafe { core::intrinsics::unchecked_shr(product, shift) };
                temp = product as i16;
                temp = -temp;
            } else {
                let mut product = (temp as u32 + corr) * recip;
                product = unsafe { core::intrinsics::unchecked_shr(product, shift) };
                temp = product as i16;
            }

            if DELTA_Q {
                if UPDATE_PREV {
                    unsafe {
                        (*prev)[i] = Self::rescale_prev::<RESCALE_PREV, RESCALE_PREV_SHR>(
                            (*prev)[i],
                            rPShifts[i],
                        );
                        let next = temp;
                        temp -= (*prev)[i];
                        (*prev)[i] = next;
                    }
                } else {
                    unsafe {
                        (*prev)[i] = Self::rescale_prev::<RESCALE_PREV, RESCALE_PREV_SHR>(
                            (*prev)[i],
                            rPShifts[i],
                        );
                        (*next)[i] = temp;
                        temp -= (*prev)[i];

                        let nbits = JPEG_NBITS_NONZERO(temp.abs() as i32);
                        // let qn = *(*deltaQNs)[i].get_unchecked(nbits as usize);
                        let update_counts = |c: &mut QuantizeCounts| {
                            // if qn == DELTA_Q_COUNT {
                            //     c.all += DELTA_Q_COUNT as u16 - 1;
                            //     c.max = DELTA_Q_COUNT - 1;
                            //     c.nleft += (nbits - (*deltaQ0)[i]) as u16;
                            // } else {
                            //     c.all += qn as u16;
                            //     c.max = c.max.max(qn);
                            //     c.min = c.min.min(qn);
                            // }
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

    fn forward_DCT<
        const UPDATE_PREV: bool,
        const RESCALE_PREV: bool,
        const RESCALE_PREV_SHR: bool,
    >(
        input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
        output: &mut JBlock,
        ypos: u16,
        xpos: u16,
        divParts: &[DivisorPart; DCTSIZE2],
        divShifts: &[u8; DCTSIZE2],
        prev: *mut JBlock,
        rPShifts: &[u8; DCTSIZE2],
        next: *mut JBlock,
        // deltaQNs: *const [[u8; DELTA_Q_N_COUNT]; DCTSIZE2],
        // deltaQ0: *const [u8; DCTSIZE2],
    ) -> QuantizeRet {
        Self::convsamp(input, ypos, xpos, output);
        Self::fdct_ifast(output);
        Self::quantize::<UPDATE_PREV, RESCALE_PREV, RESCALE_PREV_SHR>(
            output, divParts, divShifts, prev, rPShifts, next,
        )
    }

    #[named]
    fn compress<const DELTA_CACHE: bool, const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(
        &mut self,
        MCU_col_num: usize,
        prev: *mut JBlock,
    ) {
        let divParts = &self.worker.shared.divisors.divisors;
        let s = if self.worker.info.isTop { 0 } else { 1 };
        let mut blkn = 0;
        let cache = unsafe { self.worker.shared_mut.deltaQCache.get_unchecked_mut(s) };
        let deltaQ = unsafe { *self.worker.shared_mut.deltaQ.get_unchecked(s) as usize };
        let deltaQ0 = unsafe { self.worker.shared.deltaQ0Tbls.get_unchecked(deltaQ) };
        let mut delta_cache_start = 0;

        for ci in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[ci];

            let divShifts = if DELTA_Q {
                unsafe {
                    self.worker
                        .shared
                        .divDeltaQShifts
                        .get_unchecked(deltaQ)
                        .get_unchecked(comp.quant_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .divShifts
                        .get_unchecked(comp.quant_tbl_no as usize)
                }
            };

            let rPShifts = unsafe {
                self.worker
                    .shared_mut
                    .rPShifts
                    .get_unchecked(s)
                    .get_unchecked(comp.quant_tbl_no as usize)
            };

            let MCU_width = comp.h_samp_factor;
            let MCU_height = comp.v_samp_factor;

            let MCU_sample_width = MCU_width as u16 * DCTSIZE as u16;
            let xpos = MCU_col_num as u16 * MCU_sample_width;
            let mut ypos = 0;

            for _ in 0..MCU_height {
                let mut xpos = xpos;
                for _ in 0..MCU_width {
                    let mut cache_hit = false;
                    let output = unsafe { self.worker.bufs.mcu.get_unchecked_mut(blkn as usize) };
                    let prev = if !DELTA_Q {
                        ptr::null_mut()
                    } else {
                        unsafe { prev.add(blkn as usize) }
                    };

                    if DELTA_CACHE {
                        for qi in 0..DELTA_Q_CACHE_COUNTS[ci] {
                            let delta_cache_i = delta_cache_start + qi;
                            let cache = unsafe { cache.get_unchecked_mut(delta_cache_i as usize) };

                            if cache.hit {
                                continue;
                            }

                            if cache.xpos == xpos && cache.ypos == ypos {
                                if deltaQ == DELTA_Q_COUNT as usize - 1 {
                                    *output = cache.cache;
                                    unsafe { *prev = cache.next };
                                } else {
                                    let deltaQ0 = unsafe {
                                        deltaQ0.get_unchecked(comp.quant_tbl_no as usize)
                                    };
                                    for i in 0..DCTSIZE2 {
                                        unsafe {
                                            let (off_prev, off_diff) =
                                                if RESCALE_PREV && RESCALE_PREV_SHR {
                                                    let mask = core::intrinsics::unchecked_shl(
                                                        1, deltaQ0[i],
                                                    ) - 1;
                                                    let off_next = (cache.next[i]
                                                        >> (JCoef::BITS as u8 - 1))
                                                        & ((cache.next[i] & mask) > 0) as JCoef;
                                                    let off_prev = ((*prev)[i]
                                                        >> (JCoef::BITS as u8 - 1))
                                                        & (((*prev)[i] & mask) > 0) as JCoef;
                                                    let off_diff = (((*prev)[i] & mask)
                                                        > (cache.next[i] & mask))
                                                        as JCoef;
                                                    (off_next, off_next - off_prev + off_diff)
                                                } else {
                                                    let mask = core::intrinsics::unchecked_shl(
                                                        1, deltaQ0[i],
                                                    ) - 1;
                                                    let off_next = (cache.next[i]
                                                        >> (JCoef::BITS as u8 - 1))
                                                        & ((cache.next[i] & mask) > 0) as JCoef;
                                                    (off_next, off_next)
                                                };
                                            (*prev)[i] = core::intrinsics::unchecked_shr(
                                                cache.next[i],
                                                deltaQ0[i],
                                            ) + off_prev;
                                            output[i] = core::intrinsics::unchecked_shr(
                                                cache.cache[i],
                                                deltaQ0[i],
                                            ) + off_diff;
                                        }
                                    }
                                }
                                // nsDbgPrint!(int, c_str!("blkn"), blkn as i32);
                                cache.hit = true;
                                cache_hit = true;
                                break;
                            }
                        }
                    }

                    if !cache_hit {
                        Self::forward_DCT::<true, RESCALE_PREV, RESCALE_PREV_SHR>(
                            &self.worker.bufs.prep[ci],
                            output,
                            ypos,
                            xpos,
                            unsafe { divParts.get_unchecked(comp.quant_tbl_no as usize) },
                            divShifts,
                            prev,
                            rPShifts,
                            ptr::null_mut(),
                        );
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
        MCU_col_num: usize,
        prev: *mut JBlock,
    ) {
        if delta_cache {
            self.compress::<true, RESCALE_PREV, RESCALE_PREV_SHR>(MCU_col_num, prev);
        } else {
            self.compress::<false, RESCALE_PREV, RESCALE_PREV_SHR>(MCU_col_num, prev);
        }
    }

    #[allow(unused)]
    fn coef_fix(s: i16, m: u8) -> i16 {
        unsafe {
            if s >= core::intrinsics::unchecked_shl(1, m) {
                s - (core::intrinsics::unchecked_shl(1, m + 1) - 1)
            } else if s <= -core::intrinsics::unchecked_shl(1, m) {
                s + (core::intrinsics::unchecked_shl(1, m + 1) - 1)
            } else {
                s
            }
        }
    }

    #[allow(unused)]
    #[named]
    fn coef_check(s: i16, m: u8) {
        unsafe {
            if s >= core::intrinsics::unchecked_shl(1, m)
                || s <= -core::intrinsics::unchecked_shl(1, m)
            {
                nsDbgPrint!(int, c_str!("s"), s as i32);
                nsDbgPrint!(int, c_str!("m"), m as i32);
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
        let mut localbuf: [u8; BUFSIZE] = const_default();
        let mut buf = EncodeBuffer::<_, REL_STREAM, DELTA_Q>::init(state, dst, &mut localbuf);

        let (val1, bits, b0) = if DELTA_Q {
            let val = block[0] as i32 - last_dc_val as i32;
            // let val = Self::coef_fix(val as i16, MAX_COEF_BITS) as i32;
            let sign1 = val >> (i32::BITS as u8 - 1);
            let val1 = val + sign1;
            let abs = val1 ^ sign1;
            (val1, JPEG_NBITS(abs) as i32, block[0])
        } else {
            let val = block[0] as i32 - last_dc_val as i32;
            let sign1 = val >> (i32::BITS as u8 - 1);
            let val1 = val + sign1;
            let abs = val1 ^ sign1;
            (val1, JPEG_NBITS(abs) as i32, block[0])
        };

        unsafe {
            buf.PUT_CODE(
                *dc_derived_tbl.ehufco.get_unchecked(bits as usize),
                *dc_derived_tbl.ehufsi.get_unchecked(bits as usize),
                val1,
                bits,
            )
        };

        let mut r = 0;

        for jpeg_natural_order_of_k in jpeg_natural_order.into_iter().skip(1) {
            let val = *unsafe { block.get_unchecked(jpeg_natural_order_of_k as usize) } as i32;
            if val == 0 {
                r += 16;
            } else {
                let (val1, bits) = {
                    let sign1 = val >> (core::mem::size_of_val(&val) * 8 - 1);
                    let val1 = val + sign1;
                    let abs = val1 ^ sign1;
                    (val1, JPEG_NBITS_NONZERO(abs) as i32)
                };

                while r >= 16 * 16 {
                    r -= 16 * 16;
                    unsafe {
                        buf.PUT_BITS(ac_derived_tbl.ehufco[0xf0], ac_derived_tbl.ehufsi[0xf0])
                    };
                }
                r += bits;
                unsafe {
                    buf.PUT_CODE(
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
            unsafe { buf.PUT_BITS(ac_derived_tbl.ehufco[0], ac_derived_tbl.ehufsi[0]) };
        }

        buf.store();

        b0
    }

    fn encode_mcu(&mut self) {
        let mut blkn = 0;

        for ci in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[ci];
            let MCU_width = comp.h_samp_factor;
            let MCU_height = comp.v_samp_factor;

            let dc_tbl = if DELTA_Q {
                unsafe {
                    self.worker
                        .shared
                        .jpegTbls
                        .dQEntropyTbls
                        .dc_derived_tbls
                        .get_unchecked(comp.dc_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .jpegTbls
                        .entropyTbls
                        .dc_derived_tbls
                        .get_unchecked(comp.dc_tbl_no as usize)
                }
            };
            let ac_tbl = if DELTA_Q {
                unsafe {
                    self.worker
                        .shared
                        .jpegTbls
                        .dQEntropyTbls
                        .ac_derived_tbls
                        .get_unchecked(comp.ac_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .jpegTbls
                        .entropyTbls
                        .ac_derived_tbls
                        .get_unchecked(comp.ac_tbl_no as usize)
                }
            };

            for _ in 0..MCU_height {
                for _ in 0..MCU_width {
                    let last_dc_val = self.worker.last_dc_val[ci];
                    let dst = &mut self.dst;
                    let state = &mut self.worker.huffState;
                    let block = unsafe { self.worker.bufs.mcu.get_unchecked(blkn) };
                    self.worker.last_dc_val[ci] =
                        Self::encode_one_block(dst, state, block, last_dc_val, dc_tbl, ac_tbl);

                    blkn += 1;
                }
            }
        }
    }

    fn reset_mcu(&mut self) {
        self.worker.huffState = const_default();
        self.worker.huffState.free_bits = BIT_BUF_SIZE as isize;
        self.worker.last_dc_val = const_default();
    }

    fn flush_mcu(&mut self) {
        let mut put_bits = BIT_BUF_SIZE as isize - self.worker.huffState.free_bits;

        let mut localbuf: [u8; mem::size_of::<BitBufType>() * 4] = const_default();
        let put_buffer = self.worker.huffState.c;
        let mut buf = EncodeBuffer::<_, REL_STREAM, DELTA_Q>::init(
            &mut self.worker.huffState,
            &mut self.dst,
            &mut localbuf,
        );

        while put_bits >= 8 {
            put_bits -= 8;
            let temp = unsafe { core::intrinsics::unchecked_shr(put_buffer, put_bits) };
            unsafe { buf.EMIT_BYTE(temp as u8) }
        }
        if put_bits > 0 {
            /* fill partial byte with ones */
            let temp = (put_buffer << (8 - put_bits))
                | unsafe { core::intrinsics::unchecked_shr(0xFF, put_bits) };
            unsafe { buf.EMIT_BYTE(temp as u8) }
        }

        buf.store();
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

    #[named]
    fn compute_dq(&mut self, prev: *mut JBlock) {
        unsafe {
            let s = if self.worker.info.isTop { 0 } else { 1 };
            // nsDbgPrint!(int, c_str!("s"), s as i32);
            let deltaQ = self.worker.shared_mut.deltaQ.get_unchecked_mut(s);
            let rand32 = &mut self.worker.shared_mut.rand32;
            let cache = self.worker.shared_mut.deltaQCache.get_unchecked_mut(s);
            let prevDeltaQ = *deltaQ;
            let deltaQ0 = self
                .worker
                .shared
                .deltaQ0Tbls
                .get_unchecked(prevDeltaQ as usize);
            let divParts = &self.worker.shared.divisors.divisors;
            let divShifts = self
                .worker
                .shared
                .divDeltaQShifts
                .get_unchecked(DELTA_Q_COUNT as usize - 1);

            let mut delta_cache_start = 0;
            let mut blkn_start = 0;

            let mut qnv: [QuantizeRet; NUM_QUANT_TBLS] = const_default();
            // for i in 0..NUM_QUANT_TBLS {
            //     qnv[i].dc.min = DELTA_Q_COUNT as u8 - 1;
            //     qnv[i].ac.min = DELTA_Q_COUNT as u8 - 1;
            // }
            let mut qnc: [u8; NUM_QUANT_TBLS] = const_default();

            for ci in 0..MAX_COMPONENTS {
                let mut indices: [u8; DELTA_Q_CACHE_MAX as usize] = const_default();

                let comp = &self.worker.shared.compInfos.infos[ci];
                let qni = comp.quant_tbl_no;
                let MCU_we = comp.h_samp_exp;
                let MCU_he = comp.v_samp_exp;

                Self::dq_cache_gen_unique_indices(
                    rand32,
                    &mut indices,
                    DELTA_Q_CACHE_COUNTS[ci],
                    core::intrinsics::unchecked_shl(
                        self.worker.shared.mcusPerRow as u8,
                        MCU_we + MCU_he,
                    ),
                );
                for qi in 0..DELTA_Q_CACHE_COUNTS[ci] {
                    let delta_cache_i = delta_cache_start + qi;
                    let blkn = indices[qi as usize];

                    let cache = cache.get_unchecked_mut(delta_cache_i as usize);

                    let mcu_i = core::intrinsics::unchecked_shr(blkn, MCU_we + MCU_he);
                    let mcu_r = blkn & (core::intrinsics::unchecked_shl(1, MCU_we + MCU_he) - 1);

                    let xpos = mcu_r & (core::intrinsics::unchecked_shl(1, MCU_we) - 1);
                    let ypos = core::intrinsics::unchecked_shr(mcu_r, MCU_we);

                    let xpos = xpos + core::intrinsics::unchecked_shl(mcu_i, MCU_we);
                    let xpos = xpos as usize * DCTSIZE;
                    let ypos = ypos as usize * DCTSIZE;

                    cache.xpos = xpos as u16;
                    cache.ypos = ypos as u16;
                    cache.hit = false;

                    let prev = prev.add(
                        mcu_i as usize * self.worker.shared.maxBlocksInMcu
                            + mcu_r as usize
                            + blkn_start,
                    );

                    let qn = if prevDeltaQ == DELTA_Q_COUNT - 1 {
                        Self::forward_DCT::<false, false, false>(
                            &self.worker.bufs.prep[ci],
                            &mut cache.cache,
                            ypos as u16,
                            xpos as u16,
                            divParts.get_unchecked(comp.quant_tbl_no as usize),
                            divShifts.get_unchecked(comp.quant_tbl_no as usize),
                            prev,
                            deltaQ0.get_unchecked(comp.quant_tbl_no as usize),
                            &mut cache.next,
                        )
                    } else {
                        Self::forward_DCT::<false, true, false>(
                            &self.worker.bufs.prep[ci],
                            &mut cache.cache,
                            ypos as u16,
                            xpos as u16,
                            divParts.get_unchecked(comp.quant_tbl_no as usize),
                            divShifts.get_unchecked(comp.quant_tbl_no as usize),
                            prev,
                            deltaQ0.get_unchecked(comp.quant_tbl_no as usize),
                            &mut cache.next,
                        )
                    };
                    let qnv = &mut *qnv.get_unchecked_mut(qni as usize);
                    let update_qnv = |qnv: &mut QuantizeCounts, qn: &QuantizeCounts| {
                        // qnv.all += qn.all;
                        // qnv.max = qnv.max.max(qn.max);
                        // qnv.min = qnv.min.min(qn.min);
                        qnv.nbits += qn.nbits;
                        // qnv.nleft += qn.nleft;
                    };
                    update_qnv(&mut qnv.dc, &qn.dc);
                    update_qnv(&mut qnv.ac, &qn.ac);
                }

                *qnc.get_unchecked_mut(qni as usize) += DELTA_Q_CACHE_COUNTS[ci];
                delta_cache_start += DELTA_Q_CACHE_COUNTS[ci];
                blkn_start += core::intrinsics::unchecked_shl(1, MCU_we + MCU_he);
            }

            let s1 = if s == 0 { 1 } else { 0 };
            let frame_time = entries::get_frame_time(s as u32)
                .max(SYSCLOCK_ARM11 / self.worker.shared.targetFrameRate as u32);
            let frame_time_1 = entries::get_frame_time(s1 as u32)
                .max(SYSCLOCK_ARM11 / self.worker.shared.targetFrameRate as u32);
            let frame_rate = f32::min(
                self.worker.shared.targetFrameRate as f32,
                SYSCLOCK_ARM11 as f32 / frame_time as f32,
            );
            let frame_rate_1 = f32::min(
                self.worker.shared.targetFrameRate as f32,
                SYSCLOCK_ARM11 as f32 / frame_time_1 as f32,
            );
            let frame_rate_f = 1f32 / (frame_rate + frame_rate_1);

            let qr = (DELTA_Q_COUNT as u32 / 6
                + DELTA_Q_COUNT as u32 * self.worker.shared.quality * self.worker.shared.quality
                    / 12000) as u8;
            // let qr = ((DELTA_Q_COUNT as u32 - 1)
            //     * self.worker.shared.quality
            //     * self.worker.shared.quality
            //     / 10000) as u8;
            // nsDbgPrint!(int, c_str!("qr"), qr as i32);
            let [qc, qc1] = self
                .worker
                .shared_mut
                .deltaQCalc
                .get_many_unchecked_mut([s, s1]);
            let qs = qc.qs;

            let work_frame_rate = f32::min(
                self.worker.shared.targetFrameRate as f32,
                SYSCLOCK_ARM11 as f32 / entries::get_work_frame_time(s as u32) as f32,
            );
            let work_frame_rate_1 = f32::min(
                self.worker.shared.targetFrameRate as f32,
                SYSCLOCK_ARM11 as f32 / entries::get_work_frame_time(s1 as u32) as f32,
            );

            let current_qos = entries::rp_delta_q_qos() as f32;
            let mcus = (if s == 0 {
                self.worker.shared.mcusTop
            } else {
                self.worker.shared.mcusBot
            }) as f32;
            let mcus1 = (if s == 1 {
                self.worker.shared.mcusTop
            } else {
                self.worker.shared.mcusBot
            }) as f32;
            let mcus_f = 1f32 / (mcus + mcus1);
            let mcusi = 1f32 / mcus;
            let qos_adj = self.worker.shared.qosAdj;
            let qos_b = current_qos * frame_rate_f * mcusi * qos_adj;
            let qos_b = qos_b * 2f32 * mcus * mcus_f;

            let comp_size = *self.worker.shared_mut.compressedSize.get_unchecked(s);

            let (qos, qos_c) = if comp_size > 0 && qs > 0f32 {
                let comp_size = (comp_size * u8::BITS) as f32 * mcusi;

                let qos_d = if qc.qb > comp_size {
                    qc.qb - comp_size
                } else if qc.qc < comp_size {
                    qc.qc - comp_size
                } else {
                    0f32
                };
                // nsDbgPrint!(int, c_str!("qos_d"), qos_d as i32);

                // nsDbgPrint!(int, c_str!("comp_size"), comp_size as i32);
                let comp_size = {
                    let adj_size = comp_size - qc.m;
                    if QS_MIN > adj_size {
                        qc.m = comp_size - QS_MIN;
                        QS_MIN
                    } else {
                        adj_size
                    }
                };

                let update_coefs = |qc: &mut DeltaQCoefs, rb: f32| {
                    let ri = 1f32 / rb;
                    let r = (rb - 1f32) * ri;

                    qc.p = qc.p * r + comp_size * ri;
                    qc.q = qc.q * r + qs * ri;

                    qc.m = qc.q / qc.p;

                    qc.d = qc.d * r + qos_d * ri;
                };

                let rr: [f32; RP_DELTA_Q_COEFS_COUNT as usize] = [3f32, 6f32, 15f32];
                for i in 0..RP_DELTA_Q_COEFS_COUNT as usize {
                    update_coefs(&mut qc.f[i], rr[i]);
                }

                const qd1_f: f32 = 1f32 / 1.75f32;
                let qd1 = (qc1.f[0].d * qd1_f
                    + (qc1.f[1].d + qc1.f[2].d * 0.5f32) * (0.5f32 * qd1_f))
                    .max(0f32);
                let qos_c = qos_b + qd1 * work_frame_rate_1 * mcus1 * mcusi / work_frame_rate;

                let qos = qos_c - qc.m;
                let q0 = qos * qc.f[0].m;
                let q1 = qos * qc.f[1].m;
                let q2 = qos * qc.f[2].m;

                const q2_f: f32 = 1f32 / 1.5f32;
                (
                    (if q0 < q1 {
                        q0
                    } else {
                        q0 * q2_f
                            + if q1 < q2 {
                                q1
                            } else {
                                q1 * q2_f + q2 * (0.5f32 * q2_f)
                            } * (0.5f32 * q2_f)
                    })
                    .max(QS_MIN),
                    qos_c,
                )
            } else {
                (qos_b, qos_b)
            };

            let qnc = {
                let mut rq: [f32; NUM_QUANT_TBLS] = const_default();
                for i in 0..NUM_QUANT_TBLS {
                    rq[i] = 1f32 / qnc[i] as f32;
                }
                rq
            };

            let qnv = {
                let mut ret: [(f32, f32); NUM_QUANT_TBLS] = const_default();
                for i in 0..NUM_QUANT_TBLS {
                    ret[i] = (
                        qnv[i].dc.nbits as f32 * qnc[i] as f32,
                        qnv[i].ac.nbits as f32 * qnc[i] as f32,
                    );
                }
                ret
            };

            let mut calc_size = |q: u8| {
                let mut size = 0f32;
                // const BORKED: bool = false;
                for i in 0..NUM_QUANT_TBLS {
                    let calc_size = |ns: f32, qb: u16| {
                        // if BORKED {
                        //     let h = DELTA_Q_STEP * n_coefs;
                        //     let q = DELTA_Q_COUNT - 1 - q;
                        //     let nbits = ns.nbits as f32 * qnc[i] as f32;
                        //     let nleft = ns.nleft as f32 * qnc[i] as f32;
                        //     let size = if q <= ns.min {
                        //         (nbits - h * q as f32).max(0f32)
                        //     } else if q >= ns.max {
                        //         nleft
                        //     } else {
                        //         let v = ns.all as f32 * qnc[i] as f32 / n_coefs - ns.min as f32;
                        //         let w = (ns.max - ns.min) as f32;
                        //         let q = (ns.max - q) as f32;
                        //         let a = (nbits - ns.min as f32 * h - nleft).max(0f32);
                        //         powf(q / w, w / v) * a + nleft
                        //     };
                        //     size
                        // } else {
                        // nsDbgPrint!(int, c_str!("nbits"), nbits as i32);
                        // nsDbgPrint!(int, c_str!("qb"), qb as i32);
                        ns - qb as f32
                        // }
                    };

                    let qm = &self.worker.shared.deltaQMs.get_unchecked(q as usize)[i];
                    let qf = &self.worker.shared.deltaQParams.qf;
                    size += (calc_size(
                        // &qnv[i].dc,
                        // 1f32,
                        // q,
                        qnv[i].0, qm.0,
                    ) + calc_size(
                        // &qnv[i].ac,
                        // (DCTSIZE2 - 1) as f32,
                        // q,
                        qnv[i].1, qm.1,
                    )) * qf[i];
                }
                // nsDbgPrint!(int, c_str!("size"), size as i32);
                // if BORKED {
                //     size.max(qs_min)
                // } else {
                let nd = qc.n + size;
                if nd > QS_MIN {
                    nd
                } else {
                    qc.n = QS_MIN - size;
                    QS_MIN
                }
                // }
            };

            // nsDbgPrint!(int, c_str!("qos"), qos as i32);
            let (q, qs) = {
                let mut q_min = 0;
                let mut q_max = qr + 1; // exclusive range
                let mut q_prev = prevDeltaQ.min(qr);
                loop {
                    let qs = calc_size(q_prev);
                    // if q == q_prev {
                    //     break (q, qs);
                    // }
                    // q_prev = q;
                    // nsDbgPrint!(int, c_str!("q"), q as i32);
                    // nsDbgPrint!(int, c_str!("qs"), qs as i32);
                    if qs > qos {
                        q_max = q_prev;
                    } else {
                        q_min = q_prev;
                    }
                    q_prev = (q_max - q_min) / 2 + q_min;
                    if q_min == q_prev {
                        break (q_prev, qs);
                    }
                }
            };
            // nsDbgPrint!(int, c_str!("qs"), qs as i32);

            // let (q, qs) = {
            //     let mut size = (0f32, 0f32);
            //     let qr = (DELTA_Q_COUNT - 1 - qr) as f32;
            //     let qps = &self.worker.shared.deltaQParams;
            //     let qf = &qps.qf;
            //     let q_step = &qps.q_step;
            //     let q_steps = &qps.q_steps;
            //     let q_step_i = &qps.q_step_i;
            //     let q_steps_i = &qps.q_steps_i;
            //     for i in 0..NUM_QUANT_TBLS {
            //         size = (size.0 + qnv[i].0 * qf[i], size.1 + qnv[i].1 * qf[i]);
            //     }
            //     let steps = (size.0 * q_step_i.0, size.1 * q_step_i.1);
            //     let steps_min = steps.0.min(steps.1);
            //     let steps_rm = (steps.0 - steps_min, steps.1 - steps_min);
            //     let size_step = steps_min * (q_step.0 + q_step.1);
            //     let size_rm = size.0 + size.1 - size_step;
            //     let qos = qos - qc.n;
            //     let default = || {
            //         let size_min_rm = qos - size_rm;
            //         let step = size_min_rm * q_steps_i;
            //         let q_step = floorf(steps_min - step).clamp(qr, (DELTA_Q_COUNT - 1) as f32);
            //         (
            //             DELTA_Q_COUNT - 1 - q_step as u8,
            //             size_rm + (steps_min - q_step) * q_steps,
            //         )
            //     };
            //     let (q, qs) = if qos >= size_rm {
            //         default()
            //     } else {
            //         if steps_rm.0 > 0f32 {
            //             let q = ceilf(steps_rm.0 - qos * q_step_i.0 + steps_min)
            //                 .clamp(qr, (DELTA_Q_COUNT - 1) as f32);
            //             let q_rm = steps_rm.0 + (q - steps_min);
            //             (DELTA_Q_COUNT - 1 - q as u8, q_rm * q_step.0)
            //         } else if steps_rm.1 > 0f32 {
            //             let steps_rm_1 = steps_rm.1;
            //             let steps_rm_1_i = 1f32 / steps_rm.1;
            //             let size_rm_i = 1f32 / size_rm;
            //             let a = steps_rm_1_i * steps_rm_1_i * size_rm;
            //             let a_i = steps_rm_1 * steps_rm_1 * size_rm_i;
            //             if qos >= 0f32 {
            //                 let q = ceilf(steps_rm.1 - sqrtf(qos * a_i) + steps_min)
            //                     .clamp(qr, (DELTA_Q_COUNT - 1) as f32);
            //                 let q_rm = steps_rm.1 - (q - steps_min);
            //                 (DELTA_Q_COUNT - 1 - q as u8, q_rm * q_rm * a)
            //             } else {
            //                 let q = ceilf(steps_rm.1 - qos * q_step_i.1 + steps_min)
            //                     .clamp(qr, (DELTA_Q_COUNT - 1) as f32);
            //                 let q_rm = steps_rm.1 + (q - steps_min);
            //                 (DELTA_Q_COUNT - 1 - q as u8, q_rm * q_step.1)
            //             }
            //         } else {
            //             default()
            //         }
            //     };
            //     let nd = qc.n + qs;
            //     let qs = if nd > QS_MIN {
            //         nd
            //     } else {
            //         qc.n = QS_MIN - qs;
            //         QS_MIN
            //     };
            //     (q, qs)
            // };

            let qr = 10f32;
            let qri = 1f32 / qr;
            {
                let ri = qri;
                let r = (qr - 1f32) * ri;

                let qq = q as i8 - prevDeltaQ as i8;
                let qt = q as f32;

                qc.cc = qc.cc * r + (qt - qc.q) * ri;
                qc.cn = qc.cn * r;

                qc.q = qc.q * (qr - 1f32) * qri + qt * qri;
                if qq < 0 {
                    const qq_f: f32 = 1f32 / (1.5f32);
                    *deltaQ = roundf(qc.q * qq_f + qt * (0.5f32 * qq_f)) as u8;
                    qc.cc = qc.cc * r;
                    qc.cn = 1f32;
                } else {
                    if qc.cc.abs() > qc.cn * qr {
                        *deltaQ = roundf(qc.q) as u8;
                        qc.cc = qc.cc * r;
                        qc.cn = 1f32;
                    }
                }
            }
            qc.qb = qos_b;
            qc.qc = qos_c;
            qc.qs = qs;
            // nsDbgPrint!(int, c_str!("q"), q as i32);
            // *deltaQ = if self.worker.shared.quality <= 10 {
            //     self.worker
            //         .shared_mut
            //         .rand32
            //         .rand_range(0..DELTA_Q_COUNT as u32) as u8
            // } else {
            //     (self.worker.shared.quality * (DELTA_Q_COUNT as u32 - 1) / 100) as u8
            // };
            // *deltaQ = (*deltaQ + 1) % DELTA_Q_COUNT;
            // nsDbgPrint!(int, c_str!("deltaQ"), *deltaQ as i32);
            if (*ntr_config).ex.plg.overlayStats > 0 {
                let ov_screen = (*ov_stats).s.get_unchecked_mut(s);
                ov_screen.comp_size = comp_size;
                let ov_screen = &mut ov_screen.delta_q;
                ov_screen.s = (qc.qs * 1000f32) as s32;
                ov_screen.q = (qc.q * 1000f32) as s32;
                ov_screen.n = (qc.n * 1000f32) as s32;
                for i in 0..RP_DELTA_Q_COEFS_COUNT as usize {
                    let f = &mut ov_screen.f[i];
                    f.p = (qc.f[i].p * 1000f32) as s32;
                    f.q = (qc.f[i].q * 1000f32) as s32;
                    f.m = (qc.f[i].m * 1000f32) as s32;
                }
            }

            let dQRescalePrev = *deltaQ as i8 - prevDeltaQ as i8;
            *self.worker.shared_mut.dQRescalePrev.get_unchecked_mut(s) = dQRescalePrev;

            if dQRescalePrev != 0 {
                // nsDbgPrint!(int, c_str!("dQRescalePrev"), dQRescalePrev as i32);
                for t in 0..NUM_QUANT_TBLS {
                    let dQShifts = &self
                        .worker
                        .shared
                        .deltaQTbls
                        .get_unchecked(*self.worker.shared_mut.deltaQ.get_unchecked(s) as usize)
                        .get_unchecked(t);

                    let dQShiftsPrev = &self
                        .worker
                        .shared
                        .deltaQTbls
                        .get_unchecked(prevDeltaQ as usize)
                        .get_unchecked(t);

                    let rPShifts = self
                        .worker
                        .shared_mut
                        .rPShifts
                        .get_unchecked_mut(s)
                        .get_unchecked_mut(t);

                    for i in 0..DCTSIZE2 {
                        rPShifts[i] = if dQRescalePrev > 0 {
                            dQShiftsPrev[i] - dQShifts[i]
                        } else {
                            dQShifts[i] - dQShiftsPrev[i]
                        };
                    }
                }
            }
        }
    }

    #[named]
    fn process(&mut self, prev: *mut JBlock, row_i: u8) {
        let mut delta_cache = false;
        for MCU_col_num in 0..self.worker.shared.mcusPerRow {
            if DELTA_Q {
                let s = if self.worker.info.isTop { 0 } else { 1 };
                if row_i == 0 && MCU_col_num == 0 {
                    let w = self.worker.info.workIndex.get() as usize;
                    unsafe {
                        if !AtomicBool::from_ptr(
                            self.worker.shared_mut.workInited.get_unchecked_mut(w),
                        )
                        .swap(true, Ordering::Relaxed)
                        {
                            while !entries::reset_threads() {
                                let res = svcWaitSynchronization(
                                    *self.worker.shared.screenSem.get_unchecked(s),
                                    THREAD_WAIT_NS,
                                );
                                if res != 0 {
                                    if res != RES_TIMEOUT as s32 {
                                        nsDbgPrint!(
                                            waitForSyncFailed,
                                            c_str!("jpeg screenSem"),
                                            res
                                        );
                                        entries::set_reset_threads_ar();
                                        return;
                                    }
                                    continue;
                                }
                                break;
                            }

                            self.compute_dq(prev);
                            *self.worker.shared_mut.compressedSize.get_unchecked_mut(s) = 0;
                            delta_cache = true;

                            let mut count = mem::MaybeUninit::uninit();
                            let res = svcReleaseSemaphore(
                                count.as_mut_ptr(),
                                *self.worker.shared.workSem.get_unchecked(w),
                                self.worker.info.coreCount.get() as i32 - 1,
                            );
                            if res != 0 {
                                nsDbgPrint!(
                                    releaseSemaphoreFailed,
                                    c_str!("jpeg workSem"),
                                    w as u32,
                                    res
                                );
                            }
                        } else {
                            while !entries::reset_threads() {
                                let res = svcWaitSynchronization(
                                    *self.worker.shared.workSem.get_unchecked(w),
                                    THREAD_WAIT_NS,
                                );
                                if res != 0 {
                                    if res != RES_TIMEOUT as s32 {
                                        nsDbgPrint!(waitForSyncFailed, c_str!("jpeg workSem"), res);
                                        entries::set_reset_threads_ar();
                                        return;
                                    }
                                    continue;
                                }
                                break;
                            }
                        }
                    }
                }
                let dQRescalePrev =
                    unsafe { *self.worker.shared_mut.dQRescalePrev.get_unchecked(s) };
                let prev = unsafe { prev.add(MCU_col_num * self.worker.shared.maxBlocksInMcu) };

                if dQRescalePrev > 0 {
                    self.compress_dq::<true, false>(delta_cache, MCU_col_num, prev);
                } else if dQRescalePrev < 0 {
                    self.compress_dq::<true, true>(delta_cache, MCU_col_num, prev);
                } else {
                    self.compress_dq::<false, false>(delta_cache, MCU_col_num, prev);
                }
            } else {
                self.compress::<false, false, false>(MCU_col_num, ptr::null_mut());
            }
            self.encode_mcu();
        }
    }

    #[named]
    pub fn encode<F, G>(&mut self, src: &[u8], mut pre_progress: F, mut progress: G) -> JpegRet
    where
        F: FnMut(),
        G: FnMut(),
    {
        let bpp = self.get_bpp_for_format();
        let pitch = GSP_SCREEN_WIDTH as usize * bpp as usize;

        pre_progress();

        if !REL_STREAM && self.worker.threadId.get() == 0 {
            self.write_headers();
        }

        self.reset_mcu();

        let w = self.worker.info.workIndex.get() as usize;
        let s = if self.worker.info.isTop { 0 } else { 1 };

        let prev = unsafe {
            if DELTA_Q {
                if src.len() == 0 {
                    while !entries::reset_threads() {
                        let res = svcWaitSynchronization(
                            *self.worker.shared.workSem.get_unchecked(w),
                            THREAD_WAIT_NS,
                        );
                        if res != 0 {
                            if res != RES_TIMEOUT as s32 {
                                nsDbgPrint!(waitForSyncFailed, c_str!("jpeg workSem"), res);
                                entries::set_reset_threads_ar();
                                return JpegRet { deltaQ: 0 };
                            }
                            continue;
                        }
                        break;
                    }
                }

                (delta_q_prev_coeffs[s] as *mut JBlock).add(
                    self.worker.info.restartInterval as usize
                        * self.worker.shared.maxBlocksInMcu
                        * self.worker.threadId.get() as usize,
                )
            } else {
                ptr::null_mut()
            }
        };

        let hss = self.worker.shared.maxHSampFactor == MAX_SAMP_FACTOR;
        let vss = self.worker.shared.maxVSampFactor == MAX_SAMP_FACTOR;

        if vss {
            let src_chunks = src
                .chunks_exact(pitch)
                .array_chunks::<{ DCTSIZE * MAX_SAMP_FACTOR }>();
            for (i, chunks) in src_chunks.enumerate() {
                /* Pre-process */
                let mut chunks = chunks.array_chunks::<{ DCTSIZE }>();

                let chunk0 = chunks.next().unwrap();
                self.pre_process(*chunk0, false);

                let chunk1 = chunks.next().unwrap();
                self.pre_process(*chunk1, true);

                pre_progress();

                /* Compress and encode */
                self.process(
                    if !DELTA_Q {
                        ptr::null_mut()
                    } else {
                        unsafe {
                            prev.add(
                                i * self.worker.shared.mcusPerRow
                                    * self.worker.shared.maxBlocksInMcu,
                            )
                        }
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
                                i * self.worker.shared.mcusPerRow
                                    * self.worker.shared.maxBlocksInMcu,
                            )
                        }
                    },
                    i as u8,
                );

                progress();
            }
        }

        self.flush_mcu();

        let mut deltaQ = 0;
        if !REL_STREAM {
            if self.worker.threadId.get() == self.worker.shared.coreCount.get() - 1 {
                self.write_trailer();
            } else {
                self.write_rst();
            }
        }

        self.write_term();

        if DELTA_Q {
            unsafe {
                deltaQ = *self.worker.shared_mut.deltaQ.get_unchecked(s);

                let c = self.worker.shared_mut.screenSemCount.get_unchecked_mut(s);
                if AtomicU8::from_ptr(c).fetch_sub(1, Ordering::Relaxed) == 1 {
                    *c = self.worker.info.coreCount.get() as u8;
                    *self.worker.shared_mut.workInited.get_unchecked_mut(w) = false;

                    // nsDbgPrint!(
                    //     int,
                    //     c_str!("compressedSize"),
                    //     *self.worker.shared_mut.compressedSize.get_unchecked(s) as i32
                    // );

                    let mut count = mem::MaybeUninit::uninit();
                    let res = svcReleaseSemaphore(
                        count.as_mut_ptr(),
                        *self.worker.shared.screenSem.get_unchecked(s),
                        1,
                    );
                    if res != 0 {
                        nsDbgPrint!(
                            releaseSemaphoreFailed,
                            c_str!("jpeg screenSem"),
                            w as u32,
                            res
                        );
                    }
                }
            }
        }

        JpegRet { deltaQ }
    }
}
