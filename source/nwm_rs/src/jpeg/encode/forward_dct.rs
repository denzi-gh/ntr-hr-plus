// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

pub unsafe fn forward_dct(
    #[cfg(not(feature = "o3ds"))] delta_prog: bool,
    #[cfg(not(feature = "o3ds"))] update_prev: bool,
    #[cfg(not(feature = "o3ds"))] rescale_prev: bool,
    #[cfg(not(feature = "o3ds"))] rescale_prev_shr: bool,
    #[cfg(not(feature = "mem3"))] downsample: u8,
    input: &WorkerPrepBufDownsample,
    ci: CompIndex,
    h_samp: bool,
    v_samp: bool,
    output: &mut JBlock,
    ypos: u16,
    xpos: u16,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    #[cfg(not(feature = "o3ds"))] prev: *mut JBlock,
    #[cfg(not(feature = "o3ds"))] r_pshifts: &[u8; DCTSIZE2],
    #[cfg(not(feature = "o3ds"))] next: *mut JBlock,
) -> QuantizeRet {
    let samp = h_samp
        && if v_samp {
            need_subsamp_ci::<true, true>(ci)
        } else {
            need_subsamp_ci::<true, false>(ci)
        };

    unsafe {
        #[cfg(not(feature = "mem3"))]
        match downsample {
            RP_DOWNSAMPLE_QUARTER => convsamp(
                downsample_screen_width(RP_DOWNSAMPLE_QUARTER),
                samp,
                input.quarter.buf.get(ci, h_samp, v_samp).as_ptr(),
                ypos,
                xpos,
                output,
            ),
            RP_DOWNSAMPLE_EVEN_ODD => convsamp(
                downsample_screen_width(RP_DOWNSAMPLE_EVEN_ODD),
                samp,
                input.even_odd.get(ci, h_samp, v_samp).as_ptr(),
                ypos,
                xpos,
                output,
            ),
            _ => convsamp(
                downsample_screen_width(RP_DOWNSAMPLE_NONE),
                samp,
                input.full.get(ci, h_samp, v_samp).as_ptr(),
                ypos,
                xpos,
                output,
            ),
        }
        // RP_DOWNSAMPLE_EVEN_ODD
        #[cfg(feature = "mem3")]
        convsamp(
            downsample_screen_width(RP_DOWNSAMPLE_EVEN_ODD),
            samp,
            input.even_odd.get(ci, h_samp, v_samp).as_ptr(),
            ypos,
            xpos,
            output,
        );

        do_forward_dct(
            #[cfg(not(feature = "o3ds"))]
            delta_prog,
            #[cfg(not(feature = "o3ds"))]
            update_prev,
            #[cfg(not(feature = "o3ds"))]
            rescale_prev,
            #[cfg(not(feature = "o3ds"))]
            rescale_prev_shr,
            output,
            div_parts,
            div_shifts,
            #[cfg(not(feature = "o3ds"))]
            prev,
            #[cfg(not(feature = "o3ds"))]
            r_pshifts,
            #[cfg(not(feature = "o3ds"))]
            next,
        )
    }
}

#[inline(always)]
unsafe fn convsamp(
    width: usize,
    h_samp: bool,
    input: *const u8,
    ypos: u16,
    xpos: u16,
    output: &mut JBlock,
) {
    let width = if h_samp { width / SAMP_FACTOR } else { width };
    unsafe {
        do_convsamp(width, input, ypos, xpos, output);
    }
}

unsafe fn do_forward_dct(
    #[cfg(not(feature = "o3ds"))] delta_prog: bool,
    #[cfg(not(feature = "o3ds"))] update_prev: bool,
    #[cfg(not(feature = "o3ds"))] rescale_prev: bool,
    #[cfg(not(feature = "o3ds"))] rescale_prev_shr: bool,
    output: &mut JBlock,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    #[cfg(not(feature = "o3ds"))] prev: *mut JBlock,
    #[cfg(not(feature = "o3ds"))] r_pshifts: &[u8; DCTSIZE2],
    #[cfg(not(feature = "o3ds"))] next: *mut JBlock,
) -> QuantizeRet {
    fdct_ifast(output);
    #[cfg(feature = "o3ds")]
    let ret = do_quantize::<false, false, false, false>(output, div_parts, div_shifts);
    #[cfg(not(feature = "o3ds"))]
    let ret = quantize(
        delta_prog,
        update_prev,
        rescale_prev,
        rescale_prev_shr,
        output,
        div_parts,
        div_shifts,
        prev,
        r_pshifts,
        next,
    );
    ret
}

#[inline(always)]
unsafe fn do_convsamp(width: usize, input: *const u8, ypos: u16, xpos: u16, output: &mut JBlock) {
    let xmax = xpos as usize + DCTSIZE;
    let self_input = width >= xmax;

    let xmax_dct_half = xpos as usize + DCTSIZE / 2;
    let self_dct = width >= xmax_dct_half;

    if !self_input && !self_dct {
        assert!(width >= xpos as usize);
    }

    let mut oidx = 0;
    for yidx in 0..DCTSIZE {
        let input = unsafe { input.add(width * (ypos as usize + yidx)) };
        for xidx in 0..DCTSIZE {
            let idx = xpos as usize + xidx;

            if self_input || idx < width {
                output[oidx] = unsafe { *input.add(idx) } as i16 - CENTERJSAMPLE as i16;
            } else if self_dct {
                let eidx = oidx - (idx - width) - 1;
                output[oidx] = output[eidx];
            } else {
                output[oidx] = 0;
            }

            oidx += 1;
        }
    }
}

#[inline(always)]
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

#[inline(always)]
#[cfg(not(feature = "o3ds"))]
fn quantize(
    delta_q: bool,
    update_prev: bool,
    rescale_prev: bool,
    rescale_prev_shr: bool,
    inout: &mut JBlock,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    prev: *mut JBlock,
    rp_shifts: &[u8; DCTSIZE2],
    next: *mut JBlock,
) -> QuantizeRet {
    if !delta_q {
        do_quantize::<false, false, false, false>(
            inout, div_parts, div_shifts, prev, rp_shifts, next,
        )
    } else {
        do_quantize_delta_q(
            update_prev,
            rescale_prev,
            rescale_prev_shr,
            inout,
            div_parts,
            div_shifts,
            prev,
            rp_shifts,
            next,
        )
    }
}

#[inline(always)]
#[cfg(not(feature = "o3ds"))]
fn do_quantize_delta_q(
    update_prev: bool,
    rescale_prev: bool,
    rescale_prev_shr: bool,
    inout: &mut JBlock,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    prev: *mut JBlock,
    rp_shifts: &[u8; DCTSIZE2],
    next: *mut JBlock,
) -> QuantizeRet {
    if update_prev {
        do_quantize_update_prev::<true>(
            rescale_prev,
            rescale_prev_shr,
            inout,
            div_parts,
            div_shifts,
            prev,
            rp_shifts,
            next,
        )
    } else {
        do_quantize_update_prev::<false>(
            rescale_prev,
            rescale_prev_shr,
            inout,
            div_parts,
            div_shifts,
            prev,
            rp_shifts,
            next,
        )
    }
}

#[inline(always)]
#[cfg(not(feature = "o3ds"))]
fn do_quantize_update_prev<const UPDATE_PREV: bool>(
    rescale_prev: bool,
    rescale_prev_shr: bool,
    inout: &mut JBlock,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    prev: *mut JBlock,
    rp_shifts: &[u8; DCTSIZE2],
    next: *mut JBlock,
) -> QuantizeRet {
    if rescale_prev {
        if rescale_prev_shr {
            do_quantize::<true, UPDATE_PREV, true, true>(
                inout, div_parts, div_shifts, prev, rp_shifts, next,
            )
        } else {
            do_quantize::<true, UPDATE_PREV, true, false>(
                inout, div_parts, div_shifts, prev, rp_shifts, next,
            )
        }
    } else {
        do_quantize::<true, UPDATE_PREV, false, false>(
            inout, div_parts, div_shifts, prev, rp_shifts, next,
        )
    }
}

#[inline(always)]
fn do_quantize<
    const DELTA_Q: bool,
    const UPDATE_PREV: bool,
    const RESCALE_PREV: bool,
    const RESCALE_PREV_SHR: bool,
>(
    inout: &mut JBlock,
    div_parts: &[DivisorPart; DCTSIZE2],
    div_shifts: &[u8; DCTSIZE2],
    #[cfg(not(feature = "o3ds"))] prev: *mut JBlock,
    #[cfg(not(feature = "o3ds"))] rp_shifts: &[u8; DCTSIZE2],
    #[cfg(not(feature = "o3ds"))] next: *mut JBlock,
) -> QuantizeRet {
    #[cfg(feature = "o3ds")]
    let ret = QuantizeRet();
    #[cfg(not(feature = "o3ds"))]
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

        #[cfg(not(feature = "o3ds"))]
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

#[inline(always)]
pub fn jpeg_nbits_nonzero(x: i32) -> u8 {
    (mem::size_of_val(&x) * 8 - x.leading_zeros() as usize) as u8
}

#[inline(always)]
#[cfg(not(feature = "o3ds"))]
pub fn rescale_prev<const RESCALE_PREV: bool, const RESCALE_PREV_SHR: bool>(
    c: JCoef,
    s: u8,
) -> JCoef {
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
