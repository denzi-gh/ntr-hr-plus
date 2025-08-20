// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

impl<'a, 'b, const REL_STREAM: bool, const DELTA_Q: bool> JpegEncode<'a, 'b, REL_STREAM, DELTA_Q> {
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
                    color.ptr = (*color.buf.full.as_mut_ptr()).as_mut_ptr();
                }
            } else {
                let output_base = output_base * S as usize;
                let output_step = S;
                let output = unsafe {
                    &mut self.worker.bufs.prep.full[ci][output_base..output_base + output_step][0]
                        [0]
                };
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

    pub fn h2v1_downsample<const WIDTH: usize>(input: &[u8; WIDTH], output: *mut u8) {
        let mut bias = 0;
        let output = unsafe { slice::from_raw_parts_mut(output, WIDTH / SAMP_FACTOR) };
        for (input, output) in input.as_chunks::<{ SAMP_FACTOR }>().0.iter().zip(output) {
            *output = ((input[0] as u16 + input[1] as u16 + bias as u16) >> 1) as u8;
            bias ^= 1; /* 1=>2, 2=>1 */
        }
    }

    pub fn h2v2_downsample<const WIDTH: usize>(input: &[[u8; WIDTH]; SAMP_FACTOR], output: *mut u8) {
        let [input0, input1] = input;
        let input0 = input0.as_chunks::<{ SAMP_FACTOR }>().0.iter();
        let input1 = input1.as_chunks::<{ SAMP_FACTOR }>().0.iter();
        let mut bias = 1;
        let output = unsafe { slice::from_raw_parts_mut(output, WIDTH / SAMP_FACTOR) };

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

        for ci in 0..MAX_COMPONENTS {
            let input = &self.worker.bufs.color[ci];
            if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                unsafe {
                    if V_SAMP {
                        let output = self.worker.bufs.prep.full[ci][output_base].as_mut_ptr();
                        Self::h2v2_downsample(&input.buf.full, output);
                    } else {
                        let output = self.worker.bufs.prep.full[ci][output_base].as_mut_ptr();
                        Self::h2v1_downsample(&input.buf.full[0], output);
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

        for ci in 0..MAX_COMPONENTS {
            if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                unsafe {
                    if V_SAMP {
                        let prep = &mut self.worker.bufs.prep.quarter.prep[ci];
                        let output =
                            self.worker.bufs.prep.quarter.buf[ci][output_base].as_mut_ptr();
                        Self::h2v2_downsample(prep, output);
                    } else {
                        let prep = &mut self.worker.bufs.prep.quarter.prep[ci][0];
                        let output =
                            self.worker.bufs.prep.quarter.buf[ci][output_base].as_mut_ptr();
                        Self::h2v1_downsample(prep, output);
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
            for (prep_base, chunk) in chunk.as_chunks::<DOWNSAMPLE_FACTOR>().0.iter().enumerate() {
                self.color_convert_quarter_vsamp::<H_SAMP>(
                    chunk,
                    downsample_screen_width(RP_DOWNSAMPLE_NONE),
                );

                for ci in 0..MAX_COMPONENTS {
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                        unsafe {
                            Self::h2v2_downsample(
                                &self.worker.bufs.color[ci].buf.full,
                                self.worker.bufs.prep.quarter.prep[ci][prep_base].as_mut_ptr(),
                            );
                        }
                    } else {
                        unsafe {
                            let output = self.worker.bufs.prep.quarter.buf[ci]
                                [output_base * SAMP_FACTOR + prep_base]
                                .as_mut_ptr();
                            Self::h2v2_downsample(&self.worker.bufs.color[ci].buf.full, output);
                        }
                    }
                }
            }
            self.downsample_quarter::<H_SAMP, V_SAMP>(output_base);
            n = output_base + 1;
        }

        if n == 0 {
            return false;
        }

        for i in n..DCTSIZE {
            for ci in 0..MAX_COMPONENTS {
                for j in 0..SAMP_FACTOR {
                    let k = i * SAMP_FACTOR + j;
                    unsafe {
                        self.worker.bufs.prep.quarter.buf[ci][k] =
                            self.worker.bufs.prep.quarter.buf[ci][k - 1];
                    }
                }
            }
        }

        true
    }

    pub fn pre_process_quarter(&mut self, src: [&[u8]; DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR]) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = true;
        for (output_base, chunk) in src
            .as_chunks::<{ SAMP_FACTOR * DOWNSAMPLE_FACTOR }>()
            .0
            .iter()
            .enumerate()
        {
            for (prep_base, chunk) in chunk.as_chunks::<DOWNSAMPLE_FACTOR>().0.iter().enumerate() {
                self.color_convert_quarter_vsamp::<H_SAMP>(
                    chunk,
                    downsample_screen_width(RP_DOWNSAMPLE_NONE),
                );

                for ci in 0..MAX_COMPONENTS {
                    if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                        unsafe {
                            Self::h2v2_downsample(
                                &self.worker.bufs.color[ci].buf.full,
                                self.worker.bufs.prep.quarter.prep[ci][prep_base].as_mut_ptr(),
                            );
                        }
                    } else {
                        unsafe {
                            let output = self.worker.bufs.prep.quarter.buf[ci]
                                [output_base * SAMP_FACTOR + prep_base]
                                .as_mut_ptr();
                            Self::h2v2_downsample(&self.worker.bufs.color[ci].buf.full, output);
                        }
                    }
                }
            }
            self.downsample_quarter::<H_SAMP, V_SAMP>(output_base);
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

    pub fn pre_process_quarter_nohsamp_novsamp(&mut self, src: [&[u8]; DCTSIZE * DOWNSAMPLE_FACTOR]) {
        const H_SAMP: bool = false;
        const _V_SAMP: bool = false;
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
                    let output = self.worker.bufs.prep.quarter.buf[ci][output_base].as_mut_ptr();
                    Self::h2v2_downsample(&self.worker.bufs.color[ci].buf.full, output);
                }
            }
        }
    }

    pub fn pre_process_quarter_novsamp(&mut self, src: [&[u8]; DCTSIZE * DOWNSAMPLE_FACTOR]) {
        const H_SAMP: bool = true;
        const V_SAMP: bool = false;
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
                if need_subsamp_ci::<H_SAMP, V_SAMP>(ci as u8) {
                    unsafe {
                        Self::h2v2_downsample(
                            &self.worker.bufs.color[ci].buf.full,
                            self.worker.bufs.prep.quarter.prep[ci][0].as_mut_ptr(),
                        );
                    }
                } else {
                    unsafe {
                        let output =
                            self.worker.bufs.prep.quarter.buf[ci][output_base].as_mut_ptr();
                        Self::h2v2_downsample(&self.worker.bufs.color[ci].buf.full, output);
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
