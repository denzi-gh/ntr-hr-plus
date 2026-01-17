// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

pub enum StartStep {
    Normal,
    Even,
    Odd,
}

fn get_start_step(start_step: &StartStep) -> (usize, usize) {
    match start_step {
        StartStep::Normal => (0, 1),
        StartStep::Even => (0, 2),
        StartStep::Odd => (1, 2),
    }
}

impl<'a, 'b> JpegEncode<'a, 'b> {
    // input count DOWNSAMPLE_FACTOR
    pub fn color_convert_quarter_vsamp<const H_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
    ) {
        const V_SAMP: bool = true;
        self.color_convert::<{ DOWNSAMPLE_FACTOR }, H_SAMP, V_SAMP>(
            input,
            0,
            StartStep::Normal,
            RP_DOWNSAMPLE_QUARTER,
        );
    }

    // input count DOWNSAMPLE_FACTOR
    pub fn color_convert_quarter_novsamp<const H_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
    ) {
        const V_SAMP: bool = false;
        self.color_convert::<DOWNSAMPLE_FACTOR, H_SAMP, V_SAMP>(
            input,
            0,
            StartStep::Normal,
            RP_DOWNSAMPLE_QUARTER,
        );
    }

    // input count S
    pub fn color_convert_full<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
        output_base: usize,
    ) {
        self.color_convert::<S, H_SAMP, V_SAMP>(
            input,
            output_base,
            StartStep::Normal,
            RP_DOWNSAMPLE_NONE,
        );
    }

    // input count S
    pub fn color_convert_even_odd<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
        output_base: usize,
    ) {
        let start_step = if self.worker.info.even_odd == false {
            StartStep::Even
        } else {
            StartStep::Odd
        };

        self.color_convert::<S, H_SAMP, V_SAMP>(
            input,
            output_base,
            start_step,
            RP_DOWNSAMPLE_EVEN_ODD,
        );
    }

    // input count S
    #[inline(always)]
    pub fn color_convert<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
        output_base: usize,
        start_step: StartStep,
        downsample: u8,
    ) {
        let (_, step) = get_start_step(&start_step);
        let _ssamp_const = SubSampConst::<H_SAMP, V_SAMP>::ASSERT;

        let width = downsample_screen_width(RP_DOWNSAMPLE_NONE);

        for ci in CompIndex::all() {
            let color = ci.index_into_mut(&mut self.worker.bufs.color);
            if downsample == RP_DOWNSAMPLE_QUARTER || need_subsamp_ci::<H_SAMP, V_SAMP>(ci) {
                unsafe {
                    match downsample {
                        RP_DOWNSAMPLE_EVEN_ODD => color.ptr = color.buf.even_odd.as_mut_ptr(),
                        _ => color.ptr = color.buf.full.as_mut_ptr(),
                    }
                }
            } else {
                let output_step = S;
                let output_base = output_base * output_step;
                let out_width = width / step;
                let output_base = output_base * out_width;
                let output = unsafe {
                    match downsample {
                        RP_DOWNSAMPLE_EVEN_ODD => self
                            .worker
                            .bufs
                            .prep
                            .even_odd
                            .get_mut(ci, H_SAMP, V_SAMP)
                            .as_mut_ptr()
                            .add(output_base),
                        _ => self
                            .worker
                            .bufs
                            .prep
                            .full
                            .get_mut(ci, H_SAMP, V_SAMP)
                            .as_mut_ptr()
                            .add(output_base),
                    }
                };
                color.ptr = output;
            }
        }
        match self.worker.info.color_space {
            ColorSpace::XBGR => cconvert::<3, 2, 1, 4, { S }>(
                input,
                &mut self.worker.bufs.color,
                width,
                start_step,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::BGR => cconvert::<2, 1, 0, 3, { S }>(
                input,
                &mut self.worker.bufs.color,
                width,
                start_step,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::RGB565 => cconvert2::<{ S }, _>(
                input,
                rgb565_comps,
                &mut self.worker.bufs.color,
                width,
                start_step,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB5A1 => cconvert2::<{ S }, _>(
                input,
                rgb5a1_comps,
                &mut self.worker.bufs.color,
                width,
                start_step,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB4 => todo!(),
        }
    }
}

#[inline(always)]
pub fn pconvert(
    r: u8,
    g: u8,
    b: u8,
    y: &mut u8,
    cb: &mut u8,
    cr: &mut u8,
    ctab: &[i32; TABLE_SIZE],
) {
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

// input count N
#[inline(always)]
pub fn cconvert<const R: usize, const G: usize, const B: usize, const P: usize, const N: usize>(
    input: &mut impl Iterator<Item = *const u8>,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    start_step: StartStep,
    tab: &[i32; TABLE_SIZE],
) {
    let (start, step) = get_start_step(&start_step);
    let out_width = width / step;
    let [output0, output1, output2] = output;
    let output0 = unsafe { slice::from_raw_parts_mut(output0.ptr, out_width * N) };
    let output1 = unsafe { slice::from_raw_parts_mut(output1.ptr, out_width * N) };
    let output2 = unsafe { slice::from_raw_parts_mut(output2.ptr, out_width * N) };
    for i in 0..N {
        if let Some(input) = input.next() {
            let input = unsafe { slice::from_raw_parts(input, width * P) };

            let output0 = &mut output0[out_width * i..out_width * (i + 1)];
            let output1 = &mut output1[out_width * i..out_width * (i + 1)];
            let output2 = &mut output2[out_width * i..out_width * (i + 1)];

            for (((input, output0), output1), output2) in input
                .as_chunks::<P>()
                .0
                .iter()
                .skip(start)
                .step_by(step)
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
}

// input count N
#[inline(always)]
pub fn cconvert2<const N: usize, F>(
    input: &mut impl Iterator<Item = *const u8>,
    comps: F,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    start_step: StartStep,
    tab: &ColorConvTabs,
) where
    F: Fn(u16, &ColorConvTabs) -> (u8, u8, u8),
{
    let (start, step) = get_start_step(&start_step);
    let out_width = width / step;
    let [output0, output1, output2] = output;
    let output0 = unsafe { slice::from_raw_parts_mut(output0.ptr, out_width * N) };
    let output1 = unsafe { slice::from_raw_parts_mut(output1.ptr, out_width * N) };
    let output2 = unsafe { slice::from_raw_parts_mut(output2.ptr, out_width * N) };
    for i in 0..N {
        if let Some(input) = input.next() {
            let input = unsafe { slice::from_raw_parts(input, width * 2) };

            let output0 = &mut output0[out_width * i..out_width * (i + 1)];
            let output1 = &mut output1[out_width * i..out_width * (i + 1)];
            let output2 = &mut output2[out_width * i..out_width * (i + 1)];

            for (((input, output0), output1), output2) in input
                .as_chunks::<2>()
                .0
                .iter()
                .skip(start)
                .step_by(step)
                .zip(output0.into_iter())
                .zip(output1.into_iter())
                .zip(output2.into_iter())
            {
                let (r, g, b) = comps(input[0] as u16 | ((input[1] as u16) << 8), tab);

                pconvert(r, g, b, output0, output1, output2, &tab.rgb_ycc_tab);
            }
        }
    }
}

#[inline(always)]
pub fn rgb565_comps(input: u16, tab: &ColorConvTabs) -> (u8, u8, u8) {
    let r = tab.rb_5_tab[((input >> 11) & 0x1f) as usize];
    let g = tab.g_6_tab[((input >> 5) & 0x3f) as usize];
    let b = tab.rb_5_tab[(input & 0x1f) as usize];
    (r, g, b)
}

#[inline(always)]
pub fn rgb5a1_comps(input: u16, tab: &ColorConvTabs) -> (u8, u8, u8) {
    let r = tab.rb_5_tab[((input >> 11) & 0x1f) as usize];
    let g = tab.rb_5_tab[((input >> 6) & 0x1f) as usize];
    let b = tab.rb_5_tab[((input >> 1) & 0x1f) as usize];
    (r, g, b)
}
