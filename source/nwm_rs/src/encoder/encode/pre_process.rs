// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

pub fn h2v1_downsample<const STEP: usize>(width: usize, input: *const u8, output: *mut u8) {
    let mut bias = 0;
    let input = unsafe { slice::from_raw_parts(input, width * STEP) };
    let output = unsafe { slice::from_raw_parts_mut(output, width / SAMP_FACTOR) };
    for (input, output) in input
        .as_chunks::<{ STEP }>()
        .0
        .as_chunks::<{ SAMP_FACTOR }>()
        .0
        .iter()
        .zip(output)
    {
        *output = ((input[0][0] as u16 + input[1][0] as u16 + bias as u16) >> 1) as u8;
        bias ^= 1; /* 1=>2, 2=>1 */
    }
}

pub fn h2v2_downsample<const STEP: usize>(width: usize, input: *const u8, output: *mut u8) {
    let in_width = width * STEP;
    let input0 = unsafe { slice::from_raw_parts(input, in_width) };
    let input1 = unsafe { slice::from_raw_parts(input.add(in_width), in_width) };
    let input0 = input0
        .as_chunks::<{ STEP }>()
        .0
        .as_chunks::<{ SAMP_FACTOR }>()
        .0
        .iter();
    let input1 = input1
        .as_chunks::<{ STEP }>()
        .0
        .as_chunks::<{ SAMP_FACTOR }>()
        .0
        .iter();
    let mut bias = 1;
    let output = unsafe { slice::from_raw_parts_mut(output, width / SAMP_FACTOR) };

    for ((input0, input1), output) in input0.zip(input1).zip(output) {
        *output = ((input0[0][0] as u16
            + input0[1][0] as u16
            + input1[0][0] as u16
            + input1[1][0] as u16
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
                    h2v2_downsample::<1>(width, input, output);
                } else {
                    h2v1_downsample::<1>(width, input, output);
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
                h2v2_downsample::<1>(width, prep, output);
            } else {
                h2v1_downsample::<1>(width, prep, output);
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
                    h2v2_downsample::<1>(width, input, output);
                } else {
                    h2v1_downsample::<1>(width, input, output);
                }
            }
        }
    }
}

impl<'a, 'b> LosslessEncode<'a, 'b> {
    // src count 1
    pub fn pre_process_full_novsamp(&mut self, src: &mut impl Iterator<Item = *const u8>) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = false;
        self.worker
            .data
            .color_convert_full::<1, H_SAMP, V_SAMP>(src, 0);
        downsample_full::<H_SAMP, V_SAMP>(&mut self.worker.data.bufs, 0);
    }

    // src count 1
    pub fn pre_process_full_nohsamp_novsamp(&mut self, src: &mut impl Iterator<Item = *const u8>) {
        const _H_SAMP: bool = false;
        const _V_SAMP: bool = false;

        self.worker.data.bufs.data.lossless.ptr = if let Some(src) = src.next() {
            src
        } else {
            ptr::null()
        };
    }
}

impl<'a, 'b> JpegEncode<'a, 'b> {
    pub fn pre_process_quarter_rem(
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

        let buf = unsafe { &mut self.worker.data.bufs.prep.quarter.buf };
        for i in n..DCTSIZE {
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
            self.worker
                .data
                .color_convert_quarter_vsamp::<H_SAMP>(chunk);

            for ci in CompIndex::all() {
                unsafe {
                    let input = ci
                        .index_into(&self.worker.data.bufs.color)
                        .buf
                        .full
                        .as_ptr();
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
                        h2v2_downsample::<1>(
                            width,
                            input,
                            ci.index_into_mut(&mut self.worker.data.bufs.prep.quarter.prep)
                                .as_mut_ptr()
                                .add(out_width * prep_base),
                        );
                    } else {
                        let output = self
                            .worker
                            .data
                            .bufs
                            .prep
                            .quarter
                            .buf
                            .get_mut(ci, H_SAMP, V_SAMP)
                            .as_mut_ptr()
                            .add(out_width * (output_base * SAMP_FACTOR + prep_base));
                        h2v2_downsample::<1>(width, input, output);
                    }
                }
            }
        }
        for ci in CompIndex::all() {
            downsample_quarter::<H_SAMP, V_SAMP>(&mut self.worker.data.bufs, output_base, ci);
        }
    }

    // src count DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR
    pub fn pre_process_quarter(&mut self, src: &mut impl Iterator<Item = *const u8>) {
        for output_base in 0..DCTSIZE {
            self.do_pre_process_quarter(output_base, src);
        }
    }

    // src count DCTSIZE * SAMP_FACTOR
    pub fn pre_process_even_odd(&mut self, src: &mut impl Iterator<Item = *const u8>) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        for output_base in 0..DCTSIZE {
            self.worker
                .data
                .color_convert_even_odd::<SAMP_FACTOR, H_SAMP, V_SAMP>(src, output_base);
            downsample_even_odd::<H_SAMP, V_SAMP>(&mut self.worker.data.bufs, output_base);
        }
    }

    // src count DCTSIZE * SAMP_FACTOR
    pub fn pre_process_full(&mut self, src: &mut impl Iterator<Item = *const u8>) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        for output_base in 0..DCTSIZE {
            self.worker
                .data
                .color_convert_full::<SAMP_FACTOR, H_SAMP, V_SAMP>(src, output_base);
            downsample_full::<H_SAMP, V_SAMP>(&mut self.worker.data.bufs, output_base);
        }
    }

    // src count DCTSIZE * DOWNSAMPLE_FACTOR
    pub fn pre_process_quarter_nohsamp_novsamp(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
    ) {
        const H_SAMP: bool = false;
        const V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for output_base in 0..DCTSIZE {
            self.worker
                .data
                .color_convert_quarter_novsamp::<H_SAMP>(src);

            for ci in CompIndex::all() {
                unsafe {
                    let input = ci
                        .index_into(&self.worker.data.bufs.color)
                        .buf
                        .full
                        .as_ptr();
                    let output = self
                        .worker
                        .data
                        .bufs
                        .prep
                        .quarter
                        .buf
                        .get_mut(ci, H_SAMP, V_SAMP)
                        .as_mut_ptr()
                        .add(out_width * output_base);
                    h2v2_downsample::<1>(width, input, output);
                }
            }
        }
    }

    // src count DCTSIZE * DOWNSAMPLE_FACTOR
    pub fn pre_process_quarter_novsamp(&mut self, src: &mut impl Iterator<Item = *const u8>) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for output_base in 0..DCTSIZE {
            self.worker
                .data
                .color_convert_quarter_novsamp::<H_SAMP>(src);

            for ci in CompIndex::all() {
                unsafe {
                    let input = ci
                        .index_into(&self.worker.data.bufs.color)
                        .buf
                        .full
                        .as_ptr();
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
                        h2v2_downsample::<1>(
                            width,
                            input,
                            ci.index_into_mut(&mut self.worker.data.bufs.prep.quarter.prep)
                                .as_mut_ptr(),
                        );
                    } else {
                        let output = self
                            .worker
                            .data
                            .bufs
                            .prep
                            .quarter
                            .buf
                            .get_mut(ci, H_SAMP, V_SAMP)
                            .as_mut_ptr()
                            .add(out_width * output_base);
                        h2v2_downsample::<1>(width, input, output);
                    }

                    downsample_quarter::<H_SAMP, V_SAMP>(
                        &mut self.worker.data.bufs,
                        output_base,
                        ci,
                    );
                }
            }
        }
    }

    // src count DCTSIZE
    pub fn pre_process_full_novsamp<const H_SAMP: bool>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
    ) {
        const V_SAMP: bool = false;
        for base in 0..DCTSIZE {
            self.worker
                .data
                .color_convert_full::<1, H_SAMP, V_SAMP>(src, base);
            downsample_full::<H_SAMP, V_SAMP>(&mut self.worker.data.bufs, base);
        }
    }

    // src count DCTSIZE
    pub fn pre_process_even_odd_novsamp<const H_SAMP: bool>(
        &mut self,
        src: &mut impl Iterator<Item = *const u8>,
    ) {
        const V_SAMP: bool = false;
        for base in 0..DCTSIZE {
            self.worker
                .data
                .color_convert_even_odd::<1, H_SAMP, V_SAMP>(src, base);
            downsample_even_odd::<H_SAMP, V_SAMP>(&mut self.worker.data.bufs, base);
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
