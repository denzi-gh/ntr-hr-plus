// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

pub fn h2v1_downsample(width: usize, input: *const u8, output: *mut u8) {
    let mut bias = 0;
    let input = unsafe { slice::from_raw_parts(input, width) };
    let output = unsafe { slice::from_raw_parts_mut(output, width / SAMP_FACTOR) };
    for (input, output) in input.as_chunks::<{ SAMP_FACTOR }>().0.iter().zip(output) {
        *output = ((input[0] as u16 + input[1] as u16 + bias as u16) >> 1) as u8;
        bias ^= 1; /* 1=>2, 2=>1 */
    }
}

pub fn h2v2_downsample(width: usize, input: *const u8, output: *mut u8) {
    let in_width = width;
    let input0 = unsafe { slice::from_raw_parts(input, in_width) };
    let input1 = unsafe { slice::from_raw_parts(input.add(in_width), in_width) };
    let input0 = input0.as_chunks::<{ SAMP_FACTOR }>().0.iter();
    let input1 = input1.as_chunks::<{ SAMP_FACTOR }>().0.iter();
    let mut bias = 1;
    let output = unsafe { slice::from_raw_parts_mut(output, width / SAMP_FACTOR) };

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

pub fn downsample_full<const H_SAMP: bool, const V_SAMP: bool>(
    bufs: &mut WorkerBufs,
    output_base: usize,
) {
    let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

    let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
    let out_width = width / SAMP_FACTOR;

    for ci in CompIndex::all() {
        if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
            unsafe {
                let input = ci.index_into(&bufs.color).buf.full.as_ptr();
                let output = bufs
                    .prep
                    .full
                    .get_mut(ci, H_SAMP, V_SAMP)
                    .as_mut_ptr()
                    .add(output_base * out_width);
                if V_SAMP {
                    h2v2_downsample(width, input, output);
                } else {
                    h2v1_downsample(width, input, output);
                }
            }
        }
    }
}

pub fn downsample_quarter<const H_SAMP: bool, const V_SAMP: bool>(
    bufs: &mut WorkerBufs,
    output_base: usize,
    ci: CompIndex,
) {
    let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

    let width = downsample_screen_width(RP_DOWNSAMPLE_QUARTER);
    let out_width = width / SAMP_FACTOR;

    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
        unsafe {
            let prep = ci.index_into_mut(&mut bufs.prep.quarter.prep).as_ptr();
            let output = bufs
                .prep
                .quarter
                .buf
                .get_mut(ci, H_SAMP, V_SAMP)
                .as_mut_ptr()
                .add(output_base * out_width);
            if V_SAMP {
                h2v2_downsample(width, prep, output);
            } else {
                h2v1_downsample(width, prep, output);
            }
        }
    }
}

pub fn downsample_even_odd<const H_SAMP: bool, const V_SAMP: bool>(
    bufs: &mut WorkerBufs,
    output_base: usize,
) {
    let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

    let width = downsample_screen_width(RP_DOWNSAMPLE_EVEN_ODD);
    let out_width = width / SAMP_FACTOR;

    for ci in CompIndex::all() {
        if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
            unsafe {
                let input = ci.index_into(&bufs.color).buf.even_odd.as_ptr();
                let output = bufs
                    .prep
                    .even_odd
                    .get_mut(ci, H_SAMP, V_SAMP)
                    .as_mut_ptr()
                    .add(output_base * out_width);
                if V_SAMP {
                    h2v2_downsample(width, input, output);
                } else {
                    h2v1_downsample(width, input, output);
                }
            }
        }
    }
}

impl<'a> WorkerCommon<'a> {
    // src count 1
    pub fn lossless_pre_process_nohsamp_novsamp(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
    ) {
        const _H_SAMP: bool = false;
        const _V_SAMP: bool = false;

        self.bufs.data.lossless.ptr = if let Some(src) = src.next() {
            src
        } else {
            ptr::null()
        };
    }

    // src count DOWNSAMPLE_FACTOR
    pub fn lossless_pre_process_quarter_nohsamp_novsamp<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
    ) {
        const H_SAMP: bool = false;
        const V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);

        self.color_copy_quarter_nohsamp_novsamp(src);

        for ci in CompIndex::all() {
            unsafe {
                let input = ci.index_into(&self.bufs.color).buf.full.as_ptr();
                let output = self
                    .bufs
                    .prep
                    .quarter
                    .buf
                    .get_mut(ci, H_SAMP, V_SAMP)
                    .as_mut_ptr();
                h2v2_downsample(width, input, output);
            }
        }
    }

    pub fn pre_process_quarter_rem<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        src_count: usize,
    ) -> bool {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        let mut n = 0;
        for output_base in 0..src_count / (SAMP_FACTOR * DOWNSAMPLE_FACTOR) {
            self.do_pre_process_quarter(output_base, src);
            n = output_base + 1;
        }

        if n == 0 {
            return false;
        }

        let buf = unsafe { &mut self.bufs.prep.quarter.buf };
        for i in n..COUNT {
            let k = i - n;
            for ci in CompIndex::all() {
                let samp_factor = if H_SAMP && need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
                    SAMP_FACTOR
                } else {
                    1
                };
                let width = downsample_screen_width(RP_DOWNSAMPLE_QUARTER) / samp_factor;

                let vs = SAMP_FACTOR / samp_factor;
                for j in 0..vs {
                    let l = (n - k) * vs - j - 1;
                    let m = i * vs + j;

                    let buf = buf.get_mut(ci, H_SAMP, V_SAMP).as_mut_ptr();

                    unsafe {
                        let src = buf.add(width * l);
                        let dst = buf.add(width * m);

                        ptr::copy_nonoverlapping(src, dst, width);
                    }
                }
            }
        }

        true
    }

    // chunk count SAMP_FACTOR * DOWNSAMPLE_FACTOR
    pub fn do_pre_process_quarter(
        &mut self,
        output_base: usize,
        chunk: &mut impl Iterator<Item = *const u8>,
    ) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for prep_base in 0..SAMP_FACTOR {
            self.color_convert_quarter_vsamp::<H_SAMP>(chunk);

            for ci in CompIndex::all() {
                unsafe {
                    let input = ci.index_into(&self.bufs.color).buf.full.as_ptr();
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
                        h2v2_downsample(
                            width,
                            input,
                            ci.index_into_mut(&mut self.bufs.prep.quarter.prep)
                                .as_mut_ptr()
                                .add(out_width * prep_base),
                        );
                    } else {
                        let output = self
                            .bufs
                            .prep
                            .quarter
                            .buf
                            .get_mut(ci, H_SAMP, V_SAMP)
                            .as_mut_ptr()
                            .add(out_width * (output_base * SAMP_FACTOR + prep_base));
                        h2v2_downsample(width, input, output);
                    }
                }
            }
        }
        for ci in CompIndex::all() {
            downsample_quarter::<H_SAMP, V_SAMP>(&mut self.bufs, output_base, ci);
        }
    }

    // src count COUNT * SAMP_FACTOR * DOWNSAMPLE_FACTOR
    pub fn pre_process_quarter<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        for output_base in start..start + COUNT {
            self.do_pre_process_quarter(output_base, src);
        }
    }

    // src count COUNT * SAMP_FACTOR
    pub fn pre_process_even_odd<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        for output_base in start..start + COUNT {
            self.color_convert_even_odd::<SAMP_FACTOR, H_SAMP, V_SAMP>(src, output_base);
            downsample_even_odd::<H_SAMP, V_SAMP>(&mut self.bufs, output_base);
        }
    }

    // src count COUNT * SAMP_FACTOR
    pub fn pre_process_full<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        for output_base in start..start + COUNT {
            self.color_convert_full::<SAMP_FACTOR, H_SAMP, V_SAMP>(src, output_base);
            downsample_full::<H_SAMP, V_SAMP>(&mut self.bufs, output_base);
        }
    }

    // src count COUNT * DOWNSAMPLE_FACTOR
    pub fn pre_process_quarter_nohsamp_novsamp<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        const H_SAMP: bool = false;
        const V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for output_base in start..start + COUNT {
            self.color_convert_quarter_novsamp::<H_SAMP>(src);

            for ci in CompIndex::all() {
                unsafe {
                    let input = ci.index_into(&self.bufs.color).buf.full.as_ptr();
                    let output = self
                        .bufs
                        .prep
                        .quarter
                        .buf
                        .get_mut(ci, H_SAMP, V_SAMP)
                        .as_mut_ptr()
                        .add(out_width * output_base);
                    h2v2_downsample(width, input, output);
                }
            }
        }
    }

    // src count COUNT * DOWNSAMPLE_FACTOR
    pub fn pre_process_quarter_novsamp<const COUNT: usize>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for output_base in start..start + COUNT {
            self.color_convert_quarter_novsamp::<H_SAMP>(src);

            for ci in CompIndex::all() {
                unsafe {
                    let input = ci.index_into(&self.bufs.color).buf.full.as_ptr();
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
                        h2v2_downsample(
                            width,
                            input,
                            ci.index_into_mut(&mut self.bufs.prep.quarter.prep)
                                .as_mut_ptr(),
                        );
                    } else {
                        let output = self
                            .bufs
                            .prep
                            .quarter
                            .buf
                            .get_mut(ci, H_SAMP, V_SAMP)
                            .as_mut_ptr()
                            .add(out_width * output_base);
                        h2v2_downsample(width, input, output);
                    }

                    downsample_quarter::<H_SAMP, V_SAMP>(&mut self.bufs, output_base, ci);
                }
            }
        }
    }

    // src count COUNT
    pub fn pre_process_full_novsamp<const COUNT: usize, const H_SAMP: bool>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        const V_SAMP: bool = false;
        for base in start..start + COUNT {
            self.color_convert_full::<1, H_SAMP, V_SAMP>(src, base);
            downsample_full::<H_SAMP, V_SAMP>(&mut self.bufs, base);
        }
    }

    // src count COUNT
    pub fn pre_process_even_odd_novsamp<const COUNT: usize, const H_SAMP: bool>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
        start: usize,
    ) {
        const V_SAMP: bool = false;
        for base in start..start + COUNT {
            self.color_convert_even_odd::<1, H_SAMP, V_SAMP>(src, base);
            downsample_even_odd::<H_SAMP, V_SAMP>(&mut self.bufs, base);
        }
    }
}

pub const fn need_subsamp<const COMP_I: u8, const H_SAMP: bool, const V_SAMP: bool>() -> bool {
    (H_SAMP || V_SAMP) && COMP_I != 0
}

pub const fn need_subsamp_ci<const H_SAMP: bool, const V_SAMP: bool>(ci: CompIndex) -> bool {
    match ci.get() {
        2 => need_subsamp::<2, H_SAMP, V_SAMP>(),
        1 => need_subsamp::<1, H_SAMP, V_SAMP>(),
        _ => need_subsamp::<0, H_SAMP, V_SAMP>(),
    }
}
