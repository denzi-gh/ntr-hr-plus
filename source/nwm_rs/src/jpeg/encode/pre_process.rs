// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

impl<'a, 'b> JpegEncode<'a, 'b> {
    pub fn color_convert_quarter_vsamp<const H_SAMP: bool>(
        &mut self,
        input: &[&[u8]; DOWNSAMPLE_FACTOR],
        width: usize,
    ) {
        const V_SAMP: bool = true;
        const DOWNSAMPLE: bool = true;
        self.color_convert::<{ DOWNSAMPLE_FACTOR }, H_SAMP, V_SAMP, DOWNSAMPLE>(input, 0, width);
    }

    pub fn color_convert_quarter_novsamp<const H_SAMP: bool>(
        &mut self,
        input: &[&[u8]; DOWNSAMPLE_FACTOR],
        width: usize,
    ) {
        const V_SAMP: bool = false;
        const DOWNSAMPLE: bool = true;
        self.color_convert::<DOWNSAMPLE_FACTOR, H_SAMP, V_SAMP, DOWNSAMPLE>(input, 0, width);
    }

    pub fn color_convert_full<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &[&[u8]; S],
        output_base: usize,
        width: usize,
    ) {
        const DOWNSAMPLE: bool = false;
        self.color_convert::<S, H_SAMP, V_SAMP, DOWNSAMPLE>(input, output_base, width);
    }

    pub fn color_convert<
        const S: usize,
        const H_SAMP: bool,
        const V_SAMP: bool,
        const DOWNSAMPLE: bool,
    >(
        &mut self,
        input: &[&[u8]; S],
        output_base: usize,
        width: usize,
    ) {
        let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

        for ci in 0..MAX_COMPONENTS {
            let color = &mut self.worker.bufs.color[ci];
            if DOWNSAMPLE || need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                unsafe {
                    color.ptr = color.buf.full.as_mut_ptr();
                }
            } else {
                let output_step = S;
                let output_base = output_base * output_step;
                let output_base = output_base * downsample_screen_width(RP_DOWNSAMPLE_NONE);
                let output =
                    unsafe { self.worker.bufs.prep.full[ci].as_mut_ptr().add(output_base) };
                color.ptr = output;
            }
        }
        match self.worker.info.color_space {
            ColorSpace::XBGR => cconvert::<3, 2, 1, 4, { S }>(
                input,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::BGR => cconvert::<2, 1, 0, 3, { S }>(
                input,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::RGB565 => cconvert2::<{ S }, _>(
                input,
                rgb565_comps,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB5A1 => cconvert2::<{ S }, _>(
                input,
                rgb5a1_comps,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB4 => todo!(),
        }
    }

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
        let input0 = unsafe { slice::from_raw_parts(input, width) };
        let input1 = unsafe { slice::from_raw_parts(input.add(width), width) };
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

    pub fn downsample_full<const H_SAMP: bool, const V_SAMP: bool>(&mut self, output_base: usize) {
        let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for ci in 0..MAX_COMPONENTS {
            if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                unsafe {
                    let input = self.worker.bufs.color[ci].buf.full.as_ptr();
                    let output = self.worker.bufs.prep.full[ci]
                        .as_mut_ptr()
                        .add(output_base * out_width);
                    if V_SAMP {
                        Self::h2v2_downsample(width, input, output);
                    } else {
                        Self::h2v1_downsample(width, input, output);
                    }
                }
            }
        }
    }

    pub fn downsample_quarter<const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        output_base: usize,
    ) {
        let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

        let width = downsample_screen_width(RP_DOWNSAMPLE_QUARTER);
        let out_width = width / SAMP_FACTOR;

        for ci in 0..MAX_COMPONENTS {
            if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                unsafe {
                    let prep = self.worker.bufs.prep.quarter.prep[ci].as_ptr();
                    let output = self.worker.bufs.prep.quarter.buf[ci]
                        .as_mut_ptr()
                        .add(output_base * out_width);
                    if V_SAMP {
                        Self::h2v2_downsample(width, prep, output);
                    } else {
                        Self::h2v1_downsample(width, prep, output);
                    }
                }
            }
        }
    }

    pub fn pre_process_quarter_rem<'t, T: Iterator<Item = &'t [u8]>>(&mut self, src: T) -> bool {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        let mut n = 0;
        for (output_base, chunk) in src
            .array_chunks::<{ SAMP_FACTOR * DOWNSAMPLE_FACTOR }>()
            .enumerate()
        {
            self.do_pre_process_quarter(output_base, &chunk);
            n = output_base + 1;
        }

        if n == 0 {
            return false;
        }

        let buf = unsafe { &mut self.worker.bufs.prep.quarter.buf };
        for i in n..DCTSIZE {
            for ci in 0..MAX_COMPONENTS {
                for j in 0..SAMP_FACTOR {
                    let k = i * SAMP_FACTOR + j;
                    let buf = buf[ci].as_mut_ptr();
                    let width = downsample_screen_width(RP_DOWNSAMPLE_QUARTER);

                    let width = if H_SAMP && need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                        width / SAMP_FACTOR
                    } else {
                        width
                    };

                    unsafe {
                        let src = buf.add(width * (k - 1));
                        let dst = buf.add(width * k);

                        ptr::copy_nonoverlapping(src, dst, width);
                    }
                }
            }
        }

        true
    }

    pub fn do_pre_process_quarter(
        &mut self,
        output_base: usize,
        chunk: &[&[u8]; SAMP_FACTOR * DOWNSAMPLE_FACTOR],
    ) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for (prep_base, chunk) in chunk.as_chunks::<DOWNSAMPLE_FACTOR>().0.iter().enumerate() {
            self.color_convert_quarter_vsamp::<H_SAMP>(
                chunk,
                downsample_screen_width(RP_DOWNSAMPLE_NONE),
            );

            for ci in 0..MAX_COMPONENTS {
                unsafe {
                    let input = self.worker.bufs.color[ci].buf.full.as_ptr();
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                        Self::h2v2_downsample(
                            width,
                            input,
                            self.worker.bufs.prep.quarter.prep[ci]
                                .as_mut_ptr()
                                .add(out_width * prep_base),
                        );
                    } else {
                        let output = self.worker.bufs.prep.quarter.buf[ci]
                            .as_mut_ptr()
                            .add(out_width * (output_base * SAMP_FACTOR + prep_base));
                        Self::h2v2_downsample(width, input, output);
                    }
                }
            }
        }
        self.downsample_quarter::<H_SAMP, V_SAMP>(output_base);
    }

    pub fn pre_process_quarter(&mut self, src: [&[u8]; DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR]) {
        for (output_base, chunk) in src
            .as_chunks::<{ SAMP_FACTOR * DOWNSAMPLE_FACTOR }>()
            .0
            .iter()
            .enumerate()
        {
            self.do_pre_process_quarter(output_base, chunk);
        }
    }

    pub fn pre_process_full(&mut self, src: [&[u8]; DCTSIZE * SAMP_FACTOR]) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        for (output_base, chunk) in src.as_chunks::<{ SAMP_FACTOR }>().0.iter().enumerate() {
            self.color_convert_full::<_, H_SAMP, V_SAMP>(
                chunk,
                output_base,
                downsample_screen_width(RP_DOWNSAMPLE_NONE),
            );
            self.downsample_full::<H_SAMP, V_SAMP>(output_base);
        }
    }

    pub fn pre_process_quarter_nohsamp_novsamp(
        &mut self,
        src: [&[u8]; DCTSIZE * DOWNSAMPLE_FACTOR],
    ) {
        const H_SAMP: bool = false;
        const _V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for (output_base, chunk) in src
            .as_chunks::<{ DOWNSAMPLE_FACTOR }>()
            .0
            .iter()
            .enumerate()
        {
            self.color_convert_quarter_novsamp::<H_SAMP>(
                chunk,
                downsample_screen_width(RP_DOWNSAMPLE_NONE),
            );

            for ci in 0..MAX_COMPONENTS {
                unsafe {
                    let input = self.worker.bufs.color[ci].buf.full.as_ptr();
                    let output = self.worker.bufs.prep.quarter.buf[ci]
                        .as_mut_ptr()
                        .add(out_width * output_base);
                    Self::h2v2_downsample(width, input, output);
                }
            }
        }
    }

    pub fn pre_process_quarter_novsamp(&mut self, src: [&[u8]; DCTSIZE * DOWNSAMPLE_FACTOR]) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = false;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);
        let out_width = width / SAMP_FACTOR;

        for (output_base, chunk) in src
            .as_chunks::<{ DOWNSAMPLE_FACTOR }>()
            .0
            .iter()
            .enumerate()
        {
            self.color_convert_quarter_novsamp::<H_SAMP>(
                chunk,
                downsample_screen_width(RP_DOWNSAMPLE_NONE),
            );

            for ci in 0..MAX_COMPONENTS {
                unsafe {
                    let input = self.worker.bufs.color[ci].buf.full.as_ptr();
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                        Self::h2v2_downsample(
                            width,
                            input,
                            self.worker.bufs.prep.quarter.prep[ci].as_mut_ptr(),
                        );
                    } else {
                        let output = self.worker.bufs.prep.quarter.buf[ci]
                            .as_mut_ptr()
                            .add(out_width * output_base);
                        Self::h2v2_downsample(width, input, output);
                    }
                }
            }

            self.downsample_quarter::<H_SAMP, V_SAMP>(output_base);
        }
    }

    pub fn pre_process_full_novsamp<const H_SAMP: bool>(&mut self, src: [&[u8]; DCTSIZE]) {
        const V_SAMP: bool = false;
        for (base, chunk) in src.as_chunks::<1>().0.iter().enumerate() {
            self.color_convert_full::<_, H_SAMP, V_SAMP>(
                chunk,
                base,
                downsample_screen_width(RP_DOWNSAMPLE_NONE),
            );
            self.downsample_full::<H_SAMP, V_SAMP>(base);
        }
    }
}

pub fn need_subsamp<const COMP_I: u8, const H_SAMP: bool, const V_SAMP: bool>() -> bool {
    (H_SAMP || V_SAMP) && COMP_I != 0
}

pub fn need_subsamp_ci<const H_SAMP: bool, const V_SAMP: bool>(ci: u8) -> bool {
    if ci == 0 {
        need_subsamp::<0, H_SAMP, V_SAMP>()
    } else if ci == 1 {
        need_subsamp::<1, H_SAMP, V_SAMP>()
    } else if ci == 2 {
        need_subsamp::<2, H_SAMP, V_SAMP>()
    } else {
        false
    }
}
