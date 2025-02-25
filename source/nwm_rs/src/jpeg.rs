#![allow(unused_macros)]

use crate::*;
pub mod vars;
use vars::*;

pub static mut targetBytesPerSec: u32 = const_default();

#[named]
pub fn deltaProgQuantTableInit() {
    let initFn = |input: &[u8; DCTSIZE2], output: &mut [f32; DCTSIZE2], max: &mut f32| {
        for j in 0..DCTSIZE {
            for i in 0..DCTSIZE {
                let k = j * DCTSIZE + i;
                // output[k] = unsafe { log2f(input[k] as f32) };
                let num = input[k] as f32 * aanscalefactor[j] * aanscalefactor[i] * 8.0f32;
                output[k] = unsafe { log2f(num) };

                // nsDbgPrint!(expBits, num as i32, output[k] as i32);

                // unsafe {
                //     aanscalefactortbl[k] = 1.0f32 / (aanscalefactor[j] * aanscalefactor[i] * 8.0f32)
                // };

                if output[k] > *max {
                    *max = output[k]
                }
            }
        }
    };

    unsafe {
        initFn(
            &std_luminance_quant_tbl,
            &mut std_luminance_quant_log2_tbl,
            &mut std_quant_log2_max,
        );
        initFn(
            &std_chrominance_quant_tbl,
            &mut std_chrominance_quant_log2_tbl,
            &mut std_quant_log2_max,
        );
    }
}

#[derive(ConstDefault)]
pub struct JpegShared {
    quantTbls: QuantTbls,
    divisors: Divisors,
    coreCount: CoreCount,
    compInfos: &'static CompInfos,
    pub maxHSampFactor: usize,
    pub maxVSampFactor: usize,
    pub maxBlocksInMcu: usize,
    pub mcuRowSize: usize,
    pub mcuColSize: usize,
    pub mcusPerRow: usize,
    pub mcusTop: usize,
    pub mcusBot: usize,
    pub deltaProg: bool,
    pub deltaProgMutex: [Handle; WORK_COUNT as usize],
}

pub struct JpegSharedMut {
    pub rand32: Rand32,
}

const fn jdiv_round_up(a: usize, b: usize) -> usize
/* Compute a/b rounded up to next integer, ie, ceil(a/b) */
/* Assumes a >= 0, b > 0 */
{
    (a + b - 1) / b
}

impl JpegShared {
    fn setCompInfos(&mut self, hq: u32) {
        if hq as u8_ == RP_CHROMASS_444 {
            self.compInfos = &jpegTbls.compInfos444;
        } else if hq as u8_ == RP_CHROMASS_422 {
            self.compInfos = &jpegTbls.compInfos422;
        } else {
            self.compInfos = &jpegTbls.compInfos420;
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
        self.mcusTop =
            self.mcusPerRow * jdiv_round_up(GSP_SCREEN_HEIGHT_TOP as usize, self.mcuColSize);
        self.mcusBot =
            self.mcusPerRow * jdiv_round_up(GSP_SCREEN_HEIGHT_BOTTOM as usize, self.mcuColSize);
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

#[derive(Copy, Clone, ConstDefault)]
pub union WorkderDstUser {
    pub info: *const crate::entries::NwmInfo,
    pub hdr: ArqRpHdr,
}

#[derive(Clone, ConstDefault)]
pub struct WorkerDst {
    pub dst: *mut u8,
    pub free_in_bytes: u16,
    pub user: WorkderDstUser,
}

impl WorkerDst {
    fn write_byte(&mut self, byte: u8) -> bool {
        if self.free_in_bytes == 0 {
            if !self.flush() {
                return false;
            }
        }
        unsafe { *self.dst = byte };
        self.dst = unsafe { self.dst.add(1) };
        self.free_in_bytes -= 1;

        true
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> bool {
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
                if !self.flush() {
                    return false;
                }
            }
        } else {
            if !self.flush() {
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

    fn flush(&mut self) -> bool {
        unsafe { crate::entries::rp_send_buffer(self, false) }
    }

    fn term(&mut self) -> bool {
        unsafe { crate::entries::rp_send_buffer(self, true) }
    }

    pub unsafe fn advance_to(&mut self, dst: *mut u8) {
        self.free_in_bytes -= dst.sub_ptr(self.dst) as u16;
        self.dst = dst;
    }
}

pub const DELTA_PROG_CACHE_COUNT: [u8; MAX_COMPONENTS] = [4, 2, 2];
pub const DELTA_PROG_CACHE_COUNT_TOTAL: u8 = {
    let mut count = 0;
    let mut i = 0;
    loop {
        if i >= DELTA_PROG_CACHE_COUNT.len() {
            break;
        }
        count += DELTA_PROG_CACHE_COUNT[i];
        i += 1;
    }
    count
};
pub const DELTA_PROG_CACHE_COUNT_MAX: u8 = {
    let mut count = 0;
    let mut i = 0;
    loop {
        if i >= DELTA_PROG_CACHE_COUNT.len() {
            break;
        }
        if count < DELTA_PROG_CACHE_COUNT[i] {
            count = DELTA_PROG_CACHE_COUNT[i]
        }
        i += 1;
    }
    count
};

#[derive(ConstDefault, Clone, Copy)]
pub struct DeltaProgCacheItem {
    pub index: u8,
    pub cache: JFBlock,
}

#[derive(ConstDefault, Clone, Copy)]
pub struct DeltaProgCache {
    pub cache: [DeltaProgCacheItem; DELTA_PROG_CACHE_COUNT_TOTAL as usize],
}

#[derive(ConstDefault, Clone, Copy)]
pub struct DeltaProgQ {
    pub q: i8,
    pub divisors: [[i8; DCTSIZE2]; NUM_QUANT_TBLS],
    pub cache: DeltaProgCache,
    pub tid: ThreadId,
}

#[derive(ConstDefault, Clone, Copy)]
pub struct CInfo {
    pub isTop: bool,
    pub colorSpace: ColorSpace,
    pub restartInterval: u16,
    pub restartInRowsPixels: u16,
    pub workIndex: WorkIndex,
    pub deltaProgQ: DeltaProgQ,
    pub partsRemain: u8,
}

type BitBufType = u32;

#[derive(ConstDefault)]
pub struct HuffState {
    c: BitBufType,
    free_bits: isize,
}

pub const BIT_BUF_SIZE: usize = mem::size_of::<BitBufType>() * 8;

#[derive(ConstDefault)]
pub struct JpegWorker<'a, const RS: bool> {
    shared: &'a JpegShared,
    shared_mut: *mut JpegSharedMut,
    bufs: &'a mut WorkerBufs,
    info: &'a mut CInfo,
    threadId: ThreadId,
    huffState: HuffState,
    last_dc_val: [i16; MAX_COMPONENTS],
}

pub struct JpegEncode<'a, 'c, const RS: bool> {
    worker: &'c mut JpegWorker<'a, RS>,
    dst: WorkerDst,
}

#[derive(ConstDefault)]
pub struct Jpeg {
    pub shared: JpegShared,
    pub shared_mut: JpegSharedMut,
    bufs: [WorkerBufs; RP_CORE_COUNT_MAX as usize],
    info: [CInfo; WORK_COUNT as usize],
}

impl Jpeg {
    pub fn reset<'a>(&'a mut self, quality: u32, coreCount: CoreCount, hq: u32) {
        self.shared.quantTbls.setQuality(quality);
        self.shared.divisors.setDivisors(&self.shared.quantTbls);
        self.shared.coreCount = coreCount;
        self.shared.setCompInfos(hq);
        unsafe { self.shared_mut.rand32 = Rand32::new(svcGetSystemTick()) };
    }

    pub fn setInfo(&mut self, info: CInfo) {
        *info.workIndex.index_into_mut(&mut self.info) = info;
    }

    pub unsafe fn getWorker<const RS: bool>(
        &mut self,
        workIndex: WorkIndex,
        threadId: ThreadId,
    ) -> JpegWorker<RS> {
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
    let output0 = if output0.ptr == ptr::null_mut() {
        &mut output0.buf
    } else {
        unsafe { slice::from_raw_parts_mut(output0.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N) }
    };
    let output1 = if output1.ptr == ptr::null_mut() {
        &mut output1.buf
    } else {
        unsafe { slice::from_raw_parts_mut(output1.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N) }
    };
    let output2 = if output2.ptr == ptr::null_mut() {
        &mut output2.buf
    } else {
        unsafe { slice::from_raw_parts_mut(output2.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N) }
    };
    for i in 0..N {
        let input: &[u8; GSP_SCREEN_WIDTH as usize * P] = input[i].try_into().unwrap();

        let output0 = unsafe { &mut output0.get_unchecked_mut(i) };
        let output1 = unsafe { &mut output1.get_unchecked_mut(i) };
        let output2 = unsafe { &mut output2.get_unchecked_mut(i) };

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
    let output0 = if output0.ptr == ptr::null_mut() {
        &mut output0.buf
    } else {
        unsafe { slice::from_raw_parts_mut(output0.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N) }
    };
    let output1 = if output1.ptr == ptr::null_mut() {
        &mut output1.buf
    } else {
        unsafe { slice::from_raw_parts_mut(output1.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N) }
    };
    let output2 = if output2.ptr == ptr::null_mut() {
        &mut output2.buf
    } else {
        unsafe { slice::from_raw_parts_mut(output2.ptr as *mut [u8; GSP_SCREEN_WIDTH as usize], N) }
    };
    for i in 0..N {
        let input: &[u8; GSP_SCREEN_WIDTH as usize * 2] = input[i].try_into().unwrap();

        let output0 = unsafe { &mut output0.get_unchecked_mut(i) };
        let output1 = unsafe { &mut output1.get_unchecked_mut(i) };
        let output2 = unsafe { &mut output2.get_unchecked_mut(i) };

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

struct EncodeBuffer<'a, 'c, 'd, const N: usize, const RS: bool> {
    buf: *mut u8,
    base: EncodeBufferBase<'a, N>,
    state: &'c mut HuffState,
    dst: &'d mut WorkerDst,
}

impl<'a, 'c, 'd, const N: usize, const RS: bool> EncodeBuffer<'a, 'c, 'd, N, RS>
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
                self.dst
                    .write_bytes(unsafe { slice::from_raw_parts(buf.as_ptr(), len) });
            }
            EncodeBufferBase::Dst => unsafe { self.dst.advance_to(self.buf) },
        }
    }

    pub unsafe fn EMIT_BYTE(&mut self, b: u8) {
        if RS {
            *self.buf = b;
            self.buf = self.buf.add(1);
        } else {
            *self.buf = b;
            *(self.buf.add(1)) = 0;
            self.buf = self.buf.add(2 - (b < 0xFF) as usize);
        }
    }

    unsafe fn FLUSH(&mut self) {
        if !RS && (self.state.c & 0x80808080 & !(self.state.c + 0x01010101) > 0) {
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
        self.state.c = (self.state.c << (size as isize + self.state.free_bits))
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
            self.state.c = (self.state.c << size) | code;
        }
    }

    pub unsafe fn PUT_CODE(&mut self, code: u32, size: u8, temp: &mut i32, nbits: &mut i32) {
        *temp &= (1 << *nbits) - 1;
        *temp |= (code as i32) << *nbits;
        *nbits += size as i32;
        self.PUT_BITS(*temp as u32, *nbits as u8);
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

impl<'a, const RS: bool> JpegWorker<'a, RS> {
    pub fn encode<F, G>(&'a mut self, dst: WorkerDst, src: &[u8], pre_progress: F, progress: G)
    where
        F: FnMut(),
        G: FnMut(),
    {
        JpegEncode { worker: self, dst }.encode(src, pre_progress, progress);
    }

    pub fn init(
        shared: &'a JpegShared,
        shared_mut: *mut JpegSharedMut,
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

impl<'a, 'c, const RS: bool> JpegEncode<'a, 'c, RS> {
    fn write_marker(&mut self, mark: u8)
    /* Emit a marker code */
    {
        self.write_byte(0xFF);
        self.write_byte(mark);
    }

    fn write_byte(&mut self, value: u8) {
        self.dst.write_byte(value);
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
            &jpegTbls.huffTbls.acHuffTbls[index]
        } else {
            &jpegTbls.huffTbls.dcHuffTbls[index]
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
        self.dst.term();
    }

    pub fn get_bpp_for_format(&self) -> u8 {
        match self.worker.info.colorSpace {
            ColorSpace::XBGR => 4,
            ColorSpace::BGR => 3,
            _ => 2,
        }
    }

    pub fn color_convert<const S: usize>(&mut self, input: &[&[u8]; S], output_base: usize) {
        for ci in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[ci];
            let color = &mut self.worker.bufs.color[ci];
            if comp.v_samp_factor < self.worker.shared.maxVSampFactor as u8
                || comp.h_samp_factor < self.worker.shared.maxHSampFactor as u8
            {
                color.ptr = ptr::null_mut();
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
                &jpegTbls.colorConvTbls.rgb_ycc_tab,
            ),
            ColorSpace::BGR => cconvert::<2, 1, 0, 3, { S }>(
                input,
                &mut self.worker.bufs.color,
                &jpegTbls.colorConvTbls.rgb_ycc_tab,
            ),
            ColorSpace::RGB565 => cconvert2::<{ S }, _>(
                input,
                rgb565_comps,
                &mut self.worker.bufs.color,
                &jpegTbls.colorConvTbls,
            ),
            ColorSpace::RGB5A1 => cconvert2::<{ S }, _>(
                input,
                rgb5a1_comps,
                &mut self.worker.bufs.color,
                &jpegTbls.colorConvTbls,
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

    pub fn downsample(&mut self, output_base: usize) {
        for ci in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[ci];
            let input = &self.worker.bufs.color[ci];
            if input.ptr == ptr::null_mut() {
                if comp.v_samp_factor < self.worker.shared.maxVSampFactor as u8 {
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
            self.color_convert(chunk, output_base);
            self.downsample(output_base);
        }
    }

    fn pre_process_no_vsubsamp(&mut self, src: [&[u8]; DCTSIZE]) {
        for (base, chunk) in src.array_chunks::<1>().enumerate() {
            self.color_convert(chunk, base);
            self.downsample(base);
        }
    }

    fn convsamp_f(
        input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
        ypos: u16,
        xpos: u16,
        output: &mut JFBlock,
    ) {
        let mut oidx = 0;
        for yidx in 0..DCTSIZE {
            let input = unsafe { input.get_unchecked(ypos as usize + yidx) };
            for xidx in 0..DCTSIZE {
                output[oidx] = *unsafe { input.get_unchecked(xpos as usize + xidx) } as f32
                    - (u8::MAX as f32 / 2.0f32);

                oidx += 1;
            }
        }
    }

    #[named]
    fn fdct_f(inout: &mut JFBlock, prev_coeffs: *const f32, prev_coeffs_pitch: u32) {
        const F_C4: f32 = 0.707106781f32;
        const F_C6: f32 = 0.382683433f32;
        const F_C2_Minus_C6: f32 = 0.541196100f32;
        const F_C2_Plus_C6: f32 = 1.306562965f32;

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

            let z1 = (tmp12 + tmp13) * F_C4; /* c4 */
            inout[i + 2] = tmp13 + z1; /* phase 5 */
            inout[i + 6] = tmp13 - z1;

            /* Odd part */

            let tmp10 = tmp4 + tmp5; /* phase 2 */
            let tmp11 = tmp5 + tmp6;
            let tmp12 = tmp6 + tmp7;

            /* The rotator is modified from fig 4-8 to avoid extra negations. */
            let z5 = (tmp10 - tmp12) * F_C6; /* c6 */
            let z2 = F_C2_Minus_C6 * tmp10 + z5; /* c2-c6 */
            let z4 = F_C2_Plus_C6 * tmp12 + z5; /* c2+c6 */
            let z3 = tmp11 * F_C4; /* c4 */

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

            let z1 = (tmp12 + tmp13) * F_C4; /* c4 */
            inout[i + DCTSIZE * 2] = tmp13 + z1; /* phase 5 */
            inout[i + DCTSIZE * 6] = tmp13 - z1;

            /* Odd part */

            let tmp10 = tmp4 + tmp5; /* phase 2 */
            let tmp11 = tmp5 + tmp6;
            let tmp12 = tmp6 + tmp7;

            /* The rotator is modified from fig 4-8 to avoid extra negations. */
            let z5 = (tmp10 - tmp12) * F_C6; /* c6 */
            let z2 = F_C2_Minus_C6 * tmp10 + z5; /* c2-c6 */
            let z4 = F_C2_Plus_C6 * tmp12 + z5; /* c2+c6 */
            let z3 = tmp11 * F_C4; /* c4 */

            let z11 = tmp7 + z3; /* phase 5 */
            let z13 = tmp7 - z3;

            inout[i + DCTSIZE * 5] = z13 + z2; /* phase 6 */
            inout[i + DCTSIZE * 3] = z13 - z2;
            inout[i + DCTSIZE * 1] = z11 + z4;
            inout[i + DCTSIZE * 7] = z11 - z4;
        }

        for j in 0..DCTSIZE {
            for i in 0..DCTSIZE {
                let k = j * DCTSIZE + i;
                // inout[k] *= unsafe { aanscalefactortbl[k] };
                let prev = unsafe { *prev_coeffs.add(j * prev_coeffs_pitch as usize + i) };
                inout[k] -= prev;
            }
        }
    }

    fn quantize_f(
        input: &JFBlock,
        output: &mut JBlock,
        divisors: &[i8; DCTSIZE2],
        prev_coeffs: *mut f32,
        prev_coeffs_pitch: u32,
    ) {
        for j in 0..DCTSIZE {
            for i in 0..DCTSIZE {
                let k = j * DCTSIZE + i;
                let temp = unsafe { ldexpf(input[k], 0 - divisors[k] as i32) };
                let temp = unsafe { truncf(temp) };
                output[k] = temp as JCoef;

                let temp = unsafe { ldexpf(temp, divisors[k] as i32) };
                unsafe { *prev_coeffs.add(j * prev_coeffs_pitch as usize + i) += temp };
            }
        }
    }

    fn forward_DCT_f(
        input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
        staging: &mut JFBlock,
        output: &mut JBlock,
        ypos: u16,
        xpos: u16,
        divisors: &[i8; DCTSIZE2],
        prev_coeffs: *mut f32,
        prev_coeffs_pitch: u32,
    ) {
        Self::convsamp_f(input, ypos, xpos, staging);
        Self::fdct_f(staging, prev_coeffs, prev_coeffs_pitch);
        Self::quantize_f(staging, output, divisors, prev_coeffs, prev_coeffs_pitch);
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

    fn quantize(inout: &mut JBlock, divisors: &[[i16; 3]; DCTSIZE2]) {
        for i in 0..DCTSIZE2 {
            let mut temp = inout[i];
            let recip = divisors[i][0] as u16 as u32;
            let corr = divisors[i][1] as u32;
            let shift = divisors[i][2] as u32;

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
            inout[i] = temp;
        }
    }

    fn forward_DCT(
        input: &[[u8; GSP_SCREEN_WIDTH as usize]; MAX_SAMP_FACTOR * DCTSIZE],
        output: &mut JBlock,
        ypos: u16,
        xpos: u16,
        divisors: &[[i16; 3]; DCTSIZE2],
    ) {
        Self::convsamp(input, ypos, xpos, output);
        Self::fdct_ifast(output);
        Self::quantize(output, divisors);
    }

    #[named]
    fn compress(&mut self, MCU_col_num: usize) {
        let mut blkn = 0;

        if MCU_col_num > self.worker.shared.mcusPerRow {
            panic!();
        }

        for ci in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[ci];
            let MCU_width = comp.h_samp_factor;
            let MCU_height = comp.v_samp_factor;

            let MCU_sample_width = MCU_width as u16 * DCTSIZE as u16;
            let xpos = MCU_col_num as u16 * MCU_sample_width;
            let mut ypos = 0;

            for _ in 0..MCU_height {
                let mut xpos = xpos;
                for _ in 0..MCU_width {
                    // nsDbgPrint!(convSampPos, xpos as i32, ypos as i32);

                    Self::forward_DCT(
                        &self.worker.bufs.prep[ci],
                        unsafe { self.worker.bufs.mcu.get_unchecked_mut(blkn as usize) },
                        ypos,
                        xpos,
                        unsafe {
                            self.worker
                                .shared
                                .divisors
                                .divisors
                                .get_unchecked(comp.quant_tbl_no as usize)
                        },
                    );

                    xpos += DCTSIZE as u16;
                    blkn += 1;
                }
                ypos += DCTSIZE as u16;
            }
        }
    }

    #[named]
    fn prev_coeffs_for_comp_and_row(&self, ci: usize, row_i: usize) -> *mut f32 {
        let prev_coeffs = *unsafe { delta_prog_prev_coeffs }.get_b_mut(if self.worker.info.isTop {
            false
        } else {
            true
        });
        let comp = &self.worker.shared.compInfos.infos[ci];
        let prev_coeffs = unsafe {
            prev_coeffs.add(
                core::intrinsics::unchecked_shr(GSP_SCREEN_WIDTH, comp.h_samp_exp) as usize
                    * if self.worker.info.isTop {
                        core::intrinsics::unchecked_shr(GSP_SCREEN_HEIGHT_TOP, comp.v_samp_exp)
                    } else {
                        core::intrinsics::unchecked_shr(GSP_SCREEN_HEIGHT_BOTTOM, comp.v_samp_exp)
                    } as usize
                    * ci,
            )
        };
        // nsDbgPrint!(int, c_str!("prev_coeffs offset comp"), unsafe {
        //     prev_coeffs.sub_ptr(
        //         *delta_prog_prev_coeffs.get_b_mut(if self.worker.info.isTop {
        //             false
        //         } else {
        //             true
        //         }),
        //     )
        // } as i32);
        let rows = self.worker.info.restartInRowsPixels as usize
            * self.worker.threadId.get() as usize
            + self.worker.shared.mcuColSize * row_i;
        // nsDbgPrint!(int, c_str!("rows"), rows as i32);
        // if rows
        //     >= if self.worker.info.isTop {
        //         GSP_SCREEN_HEIGHT_TOP
        //     } else {
        //         GSP_SCREEN_HEIGHT_BOTTOM
        //     } as usize
        // {
        //     nsDbgPrint!(
        //         int,
        //         c_str!("restart in rows pixels"),
        //         self.worker.info.restartInRowsPixels as i32
        //     );
        //     nsDbgPrint!(int, c_str!("threadId"), self.worker.threadId.get() as i32);
        //     nsDbgPrint!(
        //         int,
        //         c_str!("mcu col size"),
        //         self.worker.shared.mcuColSize as i32
        //     );
        //     nsDbgPrint!(int, c_str!("row_i"), row_i as i32);
        //     panic!()
        // }
        let prev_coeffs = unsafe {
            prev_coeffs.add(
                core::intrinsics::unchecked_shr(GSP_SCREEN_WIDTH, comp.h_samp_exp) as usize * rows,
            )
        };
        // nsDbgPrint!(int, c_str!("prev_coeffs offset row"), unsafe {
        //     prev_coeffs.sub_ptr(
        //         *delta_prog_prev_coeffs.get_b_mut(if self.worker.info.isTop {
        //             false
        //         } else {
        //             true
        //         }),
        //     )
        // } as i32);
        prev_coeffs
    }

    #[named]
    fn compress_delta_prog(&mut self, MCU_col_num: usize, row_i: usize) {
        let mut blkn = 0;

        if MCU_col_num > self.worker.shared.mcusPerRow {
            panic!();
        }

        let delta_prog = &self.worker.info.deltaProgQ;
        let mut delta_prog_cache_start = 0;

        for ci in 0..MAX_COMPONENTS {
            let prev_coeffs = self.prev_coeffs_for_comp_and_row(ci, row_i);
            // nsDbgPrint!(int, c_str!("prev_coeffs offset init"), unsafe {
            //     prev_coeffs.sub_ptr(
            //         *delta_prog_prev_coeffs.get_b_mut(if self.worker.info.isTop {
            //             false
            //         } else {
            //             true
            //         }),
            //     )
            // } as i32);

            let comp = &self.worker.shared.compInfos.infos[ci];
            let prev_coeffs_pitch =
                unsafe { core::intrinsics::unchecked_shr(GSP_SCREEN_WIDTH, comp.h_samp_exp) };
            let MCU_width = comp.h_samp_factor;
            let MCU_height = comp.v_samp_factor;

            let MCU_sample_width = MCU_width as u16 * DCTSIZE as u16;
            let xpos = MCU_col_num as u16 * MCU_sample_width;
            let mut ypos = 0;

            let divisors = unsafe {
                delta_prog
                    .divisors
                    .get_unchecked(comp.quant_tbl_no as usize)
            };
            let cache_blkn_start = MCU_col_num as u8 * MCU_width * MCU_height;

            for _ in 0..MCU_height {
                let mut xpos = xpos;
                for _ in 0..MCU_width {
                    let output = unsafe { self.worker.bufs.mcu.get_unchecked_mut(blkn as usize) };
                    let prev_coeffs = unsafe {
                        prev_coeffs.add(prev_coeffs_pitch as usize * ypos as usize + xpos as usize)
                    };
                    // nsDbgPrint!(int, c_str!("prev_coeffs offset next"), unsafe {
                    //     prev_coeffs.sub_ptr(
                    //         *delta_prog_prev_coeffs.get_b_mut(if self.worker.info.isTop {
                    //             false
                    //         } else {
                    //             true
                    //         }),
                    //     )
                    // }
                    //     as i32);

                    let mut cache_hit = false;

                    if delta_prog.tid == self.worker.threadId && row_i == 0 {
                        for delta_prog_cache_i in 0..DELTA_PROG_CACHE_COUNT[ci] {
                            let cache = unsafe {
                                delta_prog.cache.cache.get_unchecked(
                                    (delta_prog_cache_start + delta_prog_cache_i) as usize,
                                )
                            };
                            if cache.index == cache_blkn_start + blkn {
                                Self::quantize_f(
                                    &cache.cache,
                                    output,
                                    divisors,
                                    prev_coeffs,
                                    prev_coeffs_pitch,
                                );
                                cache_hit = true;
                                break;
                            }
                        }
                    }

                    if !cache_hit {
                        Self::forward_DCT_f(
                            &self.worker.bufs.prep[ci],
                            &mut self.worker.bufs.mcu_f,
                            output,
                            ypos,
                            xpos,
                            divisors,
                            prev_coeffs,
                            prev_coeffs_pitch,
                        );
                    }

                    xpos += DCTSIZE as u16;
                    blkn += 1;
                }
                ypos += DCTSIZE as u16;
            }

            delta_prog_cache_start += DELTA_PROG_CACHE_COUNT[ci];
        }
    }

    #[named]
    fn estimate_delta_prog_q(&mut self) {
        unsafe {
            let mutex = self
                .worker
                .shared
                .deltaProgMutex
                .get_unchecked(self.worker.info.workIndex.get() as usize);

            while !entries::reset_threads() {
                let res = svcWaitSynchronization(*mutex, THREAD_WAIT_NS);
                if res != 0 {
                    if res != RES_TIMEOUT as s32 {
                        nsDbgPrint!(waitForSyncFailed, c_str!("deltaProgMutex"), res);
                        entries::set_reset_threads_ar();
                        return;
                    }
                    continue;
                }
                break;
            }

            let deltaProgQ = &mut self.worker.info.deltaProgQ;
            if deltaProgQ.q == 0 {
                while !entries::reset_threads() {
                    let res = svcWaitSynchronization(
                        *delta_prog_prev_sem.get_b_mut(self.worker.info.isTop),
                        THREAD_WAIT_NS,
                    );
                    if res != 0 {
                        if res != RES_TIMEOUT as s32 {
                            nsDbgPrint!(waitForSyncFailed, c_str!("delta_prog_prev_sem"), res);
                            entries::set_reset_threads_ar();
                            break;
                        }
                        continue;
                    }
                    break;
                }

                if !entries::reset_threads() {
                    let q = self.do_estimate_delta_prog_q();
                    AtomicI8::from_mut(&mut self.worker.info.deltaProgQ.q)
                        .store(q, Ordering::Relaxed);
                }
            }

            let res = svcReleaseMutex(*mutex);
            if res != 0 {
                nsDbgPrint!(releaseMutexFailed, c_str!("deltaProgMutex"), res);
            }
        }
    }

    fn gen_unique_index(
        rand32: &mut Rand32,
        seen: &mut [u8; DELTA_PROG_CACHE_COUNT_MAX as usize],
        seen_n: u8,
        range: u8,
    ) -> u8 {
        // return seen_n;
        unsafe {
            loop {
                let r = rand32.rand_range(0..range as u32) as u8;
                let mut again = false;
                for i in 0..seen_n as usize {
                    if *seen.get_unchecked(i) == r {
                        again = true;
                        break;
                    }
                }
                if again {
                    continue;
                }

                *seen.get_unchecked_mut(seen_n as usize) = r;
                return r;
            }
        }
    }

    #[named]
    fn do_estimate_delta_prog_q(&mut self) -> i8 {
        unsafe {
            let mut delta_prog_cache_start = 0;
            let mut exp_for_quant: [f32; NUM_QUANT_TBLS] = const_default();
            let mut exp_count_for_quant: [u32; NUM_QUANT_TBLS] = const_default();
            let mut comp_count_for_quant: [u32; NUM_QUANT_TBLS] = const_default();

            for ci in 0..MAX_COMPONENTS {
                let prev_coeffs = self.prev_coeffs_for_comp_and_row(ci, 0);
                let delta_prog = &mut self.worker.info.deltaProgQ;

                let comp = &self.worker.shared.compInfos.infos[ci];
                let prev_coeffs_pitch =
                    core::intrinsics::unchecked_shr(GSP_SCREEN_WIDTH, comp.h_samp_exp);

                let MCU_width = comp.h_samp_factor;
                let MCU_height = comp.v_samp_factor;
                let MCU_count = MCU_width * MCU_height;
                let blkn_total = self.worker.shared.mcusPerRow as u8 * MCU_count;

                let exp_for_comp = exp_for_quant.get_unchecked_mut(comp.quant_tbl_no as usize);
                let exp_count_for_comp =
                    exp_count_for_quant.get_unchecked_mut(comp.quant_tbl_no as usize);
                let comp_count_for_comp =
                    comp_count_for_quant.get_unchecked_mut(comp.quant_tbl_no as usize);

                let exp_tbl = if comp.quant_tbl_no == 0 {
                    &std_luminance_quant_log2_tbl
                } else {
                    &std_chrominance_quant_log2_tbl
                };

                let mut blkn_saved: [u8; DELTA_PROG_CACHE_COUNT_MAX as usize] = const_default();

                for i in 0..DELTA_PROG_CACHE_COUNT[ci] {
                    let cache = delta_prog
                        .cache
                        .cache
                        .get_unchecked_mut((delta_prog_cache_start + i) as usize);

                    let blkn = Self::gen_unique_index(
                        &mut (*self.worker.shared_mut).rand32,
                        &mut blkn_saved,
                        i,
                        blkn_total,
                    );

                    let MCU_col_num = blkn / MCU_count;
                    let blkn_MCU = blkn % MCU_count;
                    let blkn_x = blkn_MCU % MCU_width;
                    let blkn_y = blkn_MCU / MCU_width;
                    let blkn_x = MCU_col_num * MCU_width + blkn_x;

                    let xpos = blkn_x as u16 * DCTSIZE as u16;
                    let ypos = blkn_y as u16 * DCTSIZE as u16;

                    let prev_coeffs =
                        prev_coeffs.add(prev_coeffs_pitch as usize * ypos as usize + xpos as usize);

                    // nsDbgPrint!(convSampPos, xpos as i32, ypos as i32);

                    Self::convsamp_f(&self.worker.bufs.prep[ci], ypos, xpos, &mut cache.cache);
                    Self::fdct_f(&mut cache.cache, prev_coeffs, prev_coeffs_pitch);

                    // if self.worker.threadId.get() == 0 {
                    //     nsDbgPrint!(int, c_str!("cache[0]"), cache.cache[0] as i32);
                    //     nsDbgPrint!(int, c_str!("prev_coeffs[0]"), *prev_coeffs as i32);
                    //     nsDbgPrint!(
                    //         int,
                    //         c_str!("prev_coeffs offset"),
                    //         prev_coeffs.sub_ptr(
                    //             *unsafe { delta_prog_prev_coeffs }
                    //                 .get_b_mut(if self.worker.info.isTop { false } else { true })
                    //         ) as i32
                    //     );
                    // }

                    cache.index = blkn;

                    for i in 0..DCTSIZE2 {
                        if cache.cache[i] != 0.0 {
                            let mut bits = mem::MaybeUninit::<i32>::uninit();
                            let _res = frexpf(cache.cache[i], bits.as_mut_ptr());
                            let bits = bits.assume_init();

                            // nsDbgPrint!(expBits, cache.cache[i] as i32, bits);

                            *exp_for_comp +=
                                f32::max(bits as f32 - exp_tbl[i] + std_quant_log2_max, 0.0f32);
                        }
                    }
                    *exp_count_for_comp += 1;
                }

                let mcus = if self.worker.info.isTop {
                    self.worker.shared.mcusTop as u32
                } else {
                    self.worker.shared.mcusBot as u32
                } * MCU_count as u32;
                *comp_count_for_comp += mcus;
                // nsDbgPrint!(int, c_str!("mcus"), mcus as i32);

                delta_prog_cache_start += DELTA_PROG_CACHE_COUNT[ci];
            }

            let target_frame_rate = 120u32; // TODO detect in screen capture thread
            let target_qos = targetBytesPerSec;

            let jpeg_size = target_qos / target_frame_rate;
            let jpeg_size = jpeg_size * entries::get_packet_data_size() as u32 / PACKET_SIZE;

            let mut exp_total = 0.0f32;
            let mut comp_total = 0u32;
            for i in 0..NUM_QUANT_TBLS {
                exp_total += exp_for_quant[i] * comp_count_for_quant[i] as f32
                    / exp_count_for_quant[i] as f32;
                comp_total += comp_count_for_quant[i] as u32;
            }
            let mut q = (exp_total as i32 - jpeg_size as i32 * u8::BITS as i32)
                / comp_total as i32
                / DCTSIZE2 as i32;

            // nsDbgPrint!(
            //     deltaProgQ,
            //     q,
            //     exp_total as i32 / 8i32,
            //     jpeg_size as i32,
            //     comp_total as i32
            // );
            if q < 0 {
                q = 0
            }
            if q > u8::BITS as i32 + std_quant_log2_max as i32 {
                q = u8::BITS as i32 + std_quant_log2_max as i32;
            }

            let delta_prog = &mut self.worker.info.deltaProgQ;
            for j in 0..NUM_QUANT_TBLS {
                let exp_tbl = if j == 0 {
                    &std_luminance_quant_log2_tbl
                } else {
                    &std_chrominance_quant_log2_tbl
                };

                let divs = &mut delta_prog.divisors[j];
                for i in 0..DCTSIZE2 {
                    divs[i] = (exp_tbl[i] as i32 + q - std_quant_log2_max as i32) as i8;
                }
            }
            delta_prog.tid = self.worker.threadId;

            let ret = q + 1;
            ret as i8
        }
    }

    fn encode_one_block(
        dst: &mut WorkerDst,
        state: &mut HuffState,
        block: &[i16; DCTSIZE2],
        last_dc_val: i16,
        dc_derived_tbl: &DerivedTbl,
        ac_derived_tbl: &DerivedTbl,
    ) {
        let mut localbuf: [u8; BUFSIZE] = const_default();
        let mut buf = EncodeBuffer::<_, RS>::init(state, dst, &mut localbuf);

        let mut temp = block[0] as i32 - last_dc_val as i32;
        let mut nbits = temp >> (core::mem::size_of_val(&temp) * 8 - 1);
        temp += nbits;
        nbits ^= temp;

        nbits = JPEG_NBITS(nbits) as i32;
        unsafe {
            buf.PUT_CODE(
                *dc_derived_tbl.ehufco.get_unchecked(nbits as usize),
                *dc_derived_tbl.ehufsi.get_unchecked(nbits as usize),
                &mut temp,
                &mut nbits,
            )
        };

        let mut r = 0;

        for jpeg_natural_order_of_k in jpeg_natural_order.into_iter().skip(1) {
            temp = *unsafe { block.get_unchecked(jpeg_natural_order_of_k as usize) } as i32;
            if temp == 0 {
                r += 16;
            } else {
                nbits = temp >> (core::mem::size_of_val(&temp) * 8 - 1);
                temp += nbits;
                nbits ^= temp;

                nbits = JPEG_NBITS_NONZERO(nbits) as i32;

                while r >= 16 * 16 {
                    r -= 16 * 16;
                    unsafe {
                        buf.PUT_BITS(ac_derived_tbl.ehufco[0xf0], ac_derived_tbl.ehufsi[0xf0])
                    };
                }
                r += nbits;
                unsafe {
                    buf.PUT_CODE(
                        *ac_derived_tbl.ehufco.get_unchecked(r as usize),
                        *ac_derived_tbl.ehufsi.get_unchecked(r as usize),
                        &mut temp,
                        &mut nbits,
                    )
                };
                r = 0;
            }
        }

        if r > 0 {
            unsafe { buf.PUT_BITS(ac_derived_tbl.ehufco[0], ac_derived_tbl.ehufsi[0]) };
        }

        buf.store();
    }

    fn encode_mcu(&mut self) {
        let mut blkn = 0;

        for ci in 0..MAX_COMPONENTS {
            let comp = &self.worker.shared.compInfos.infos[ci];
            let MCU_width = comp.h_samp_factor;
            let MCU_height = comp.v_samp_factor;

            for _ in 0..MCU_height {
                for _ in 0..MCU_width {
                    let last_dc_val = self.worker.last_dc_val[ci];
                    Self::encode_one_block(
                        &mut self.dst,
                        &mut self.worker.huffState,
                        unsafe { self.worker.bufs.mcu.get_unchecked(blkn) },
                        last_dc_val,
                        unsafe {
                            jpegTbls
                                .entropyTbls
                                .dc_derived_tbls
                                .get_unchecked(comp.dc_tbl_no as usize)
                        },
                        unsafe {
                            jpegTbls
                                .entropyTbls
                                .ac_derived_tbls
                                .get_unchecked(comp.ac_tbl_no as usize)
                        },
                    );
                    self.worker.last_dc_val[ci] =
                        (*unsafe { self.worker.bufs.mcu.get_unchecked(blkn) })[0];

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
        let mut buf =
            EncodeBuffer::<_, RS>::init(&mut self.worker.huffState, &mut self.dst, &mut localbuf);

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

    fn process(&mut self, deltaProg: bool, i: usize) {
        for MCU_col_num in 0..self.worker.shared.mcusPerRow {
            if deltaProg {
                if i == 0
                    && MCU_col_num == 0
                    && AtomicI8::from(self.worker.info.deltaProgQ.q).load(Ordering::Relaxed) == 0
                {
                    self.estimate_delta_prog_q();
                    if entries::reset_threads() {
                        return;
                    }
                }
                self.compress_delta_prog(MCU_col_num, i);
            } else {
                self.compress(MCU_col_num);
            }
            self.encode_mcu();
        }
    }

    #[named]
    pub fn encode<F, G>(&mut self, src: &[u8], mut pre_progress: F, mut progress: G)
    where
        F: FnMut(),
        G: FnMut(),
    {
        let bpp = self.get_bpp_for_format();
        let pitch = GSP_SCREEN_WIDTH as usize * bpp as usize;

        pre_progress();

        if !RS && self.worker.threadId.get() == 0 {
            self.write_headers();
        }
        let deltaProg = RS && self.worker.shared.deltaProg;

        self.reset_mcu();

        if self.worker.shared.maxVSampFactor == MAX_SAMP_FACTOR {
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
                self.process(deltaProg, i);

                progress();
            }
        } else {
            let src_chunks = src.chunks_exact(pitch).array_chunks::<DCTSIZE>();
            for (i, chunk) in src_chunks.enumerate() {
                self.pre_process_no_vsubsamp(chunk);

                pre_progress();

                /* Compress and encode */
                self.process(deltaProg, i);

                progress();
            }
        }

        if deltaProg {
            let res = AtomicU8::from_mut(&mut self.worker.info.partsRemain)
                .fetch_sub(1, Ordering::Relaxed);
            if res == 1 {
                let mut count = mem::MaybeUninit::uninit();
                let res = unsafe {
                    svcReleaseSemaphore(
                        count.as_mut_ptr(),
                        *delta_prog_prev_sem.get_b_mut(self.worker.info.isTop),
                        1,
                    )
                };
                if res != 0 {
                    nsDbgPrint!(
                        releaseSemaphoreFailed,
                        c_str!("delta_prog_prev_sem"),
                        self.worker.info.workIndex.get(),
                        res
                    );
                }
            } else {
            }
        }

        self.flush_mcu();

        if !RS {
            if self.worker.threadId.get() == self.worker.shared.coreCount.get() - 1 {
                self.write_trailer();
            } else {
                self.write_rst();
            }
        }

        self.write_term();
    }
}
