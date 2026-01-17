// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

#[derive(ConstParamTy, PartialEq, Eq, Clone, Copy)]
pub enum StartStep {
    Normal,
    Even,
    Odd,
}

const fn get_start_step(start_step: StartStep) -> (usize, usize) {
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
        self.color_convert::<{ DOWNSAMPLE_FACTOR }, H_SAMP, V_SAMP, { StartStep::Normal }>(
            input,
            0,
            RP_DOWNSAMPLE_QUARTER,
        );
    }

    // input count DOWNSAMPLE_FACTOR
    pub fn color_convert_quarter_novsamp<const H_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
    ) {
        const V_SAMP: bool = false;
        self.color_convert::<DOWNSAMPLE_FACTOR, H_SAMP, V_SAMP, { StartStep::Normal }>(
            input,
            0,
            RP_DOWNSAMPLE_QUARTER,
        );
    }

    // input count S
    pub fn color_convert_full<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
        output_base: usize,
    ) {
        self.color_convert::<S, H_SAMP, V_SAMP, { StartStep::Normal }>(
            input,
            output_base,
            RP_DOWNSAMPLE_NONE,
        );
    }

    // input count S
    pub fn color_convert_even_odd<const S: usize, const H_SAMP: bool, const V_SAMP: bool>(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
        output_base: usize,
    ) {
        if self.worker.info.even_odd == false {
            self.color_convert::<S, H_SAMP, V_SAMP, { StartStep::Even }>(
                input,
                output_base,
                RP_DOWNSAMPLE_EVEN_ODD,
            );
        } else {
            self.color_convert::<S, H_SAMP, V_SAMP, { StartStep::Odd }>(
                input,
                output_base,
                RP_DOWNSAMPLE_EVEN_ODD,
            );
        };
    }

    // input count S
    pub fn color_convert<
        const S: usize,
        const H_SAMP: bool,
        const V_SAMP: bool,
        const START_STEP: StartStep,
    >(
        &mut self,
        input: &mut impl Iterator<Item = *const u8>,
        output_base: usize,
        downsample: u8,
    ) {
        let (_, step) = get_start_step(START_STEP);
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
            ColorSpace::XBGR => cconvert::<3, 2, 1, true, S, START_STEP>(
                input,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::BGR => cconvert::<2, 1, 0, false, S, START_STEP>(
                input,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls.rgb_ycc_tab,
            ),
            ColorSpace::RGB565 => cconvert2::<S, _, START_STEP>(
                input,
                rgb565_comps,
                &mut self.worker.bufs.color,
                width,
                &self.worker.shared.jpeg_tbls.color_conv_tbls,
            ),
            ColorSpace::RGB5A1 => cconvert2::<S, _, START_STEP>(
                input,
                rgb5a1_comps,
                &mut self.worker.bufs.color,
                width,
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
    y: *mut u8,
    cb: *mut u8,
    cr: *mut u8,
    ctab: &[i32; TABLE_SIZE],
) {
    unsafe {
        /* If the inputs are 0.._MAXJSAMPLE, the outputs of these equations
         * must be too; we do not need an explicit range-limiting operation.
         * Hence the value being shifted is never negative, and we don't
         * need the general RIGHT_SHIFT macro.
         */
        /* Y */
        *y =
            ((ctab[r as usize + R_Y_OFF] + ctab[g as usize + G_Y_OFF] + ctab[b as usize + B_Y_OFF])
                >> SCALEBITS) as u8;
        /* Cb */
        *cb = ((ctab[r as usize + R_CB_OFF]
            + ctab[g as usize + G_CB_OFF]
            + ctab[b as usize + B_CB_OFF])
            >> SCALEBITS) as u8;
        /* Cr */
        *cr = ((ctab[r as usize + R_CR_OFF]
            + ctab[g as usize + G_CR_OFF]
            + ctab[b as usize + B_CR_OFF])
            >> SCALEBITS) as u8;
    }
}

const fn cconvert_chunk() -> usize {
    4
}

const fn cconvert_p(a: bool) -> usize {
    if a { 4 } else { 3 }
}

const fn cconvert_chunk_p(p: usize, step: usize) -> usize {
    cconvert_chunk() * p * step
}

const fn cconvert_chunk_u32(p: usize, step: usize) -> usize {
    cconvert_chunk_p(p, step) / mem::size_of::<u32>()
}

const fn cconvert_step(start_step: StartStep) -> usize {
    get_start_step(start_step).1
}

// input count N
pub fn cconvert<
    const R: usize,
    const G: usize,
    const B: usize,
    const A: bool,
    const N: usize,
    const START_STEP: StartStep,
>(
    input: &mut impl Iterator<Item = *const u8>,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    tab: &[i32; TABLE_SIZE],
) {
    let chunk_u32 = cconvert_chunk_u32(cconvert_p(A), cconvert_step(START_STEP));
    match chunk_u32 {
        3 => cconvert1_chunk_u32::<R, G, B, A, N, START_STEP, 3>(input, output, width, tab),
        4 => cconvert1_chunk_u32::<R, G, B, A, N, START_STEP, 4>(input, output, width, tab),
        6 => cconvert1_chunk_u32::<R, G, B, A, N, START_STEP, 6>(input, output, width, tab),
        8 => cconvert1_chunk_u32::<R, G, B, A, N, START_STEP, 8>(input, output, width, tab),
        _ => panic!(),
    }
}

// input count N
pub fn cconvert1_chunk_u32<
    const R: usize,
    const G: usize,
    const B: usize,
    const A: bool,
    const N: usize,
    const START_STEP: StartStep,
    const CHUNK_U32: usize,
>(
    input: &mut impl Iterator<Item = *const u8>,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    tab: &[i32; TABLE_SIZE],
) {
    let (start, step) = get_start_step(START_STEP);
    let out_width = width / step;
    for i in 0..N {
        if let Some(input) = input.next() {
            unsafe {
                let output0 = output[0].ptr.add(out_width * i);
                let output1 = output[1].ptr.add(out_width * i);
                let output2 = output[2].ptr.add(out_width * i);

                for x in 0..out_width / cconvert_chunk() {
                    let input_0 = *(input.add(x * cconvert_chunk_p(cconvert_p(A), step))
                        as *const [u32; CHUNK_U32]);
                    let input = input_0.as_ptr() as *const u8;

                    for c in 0..cconvert_chunk() {
                        let input = input.add((c * step + start) * cconvert_p(A));

                        let r = *input.add(R);
                        let g = *input.add(G);
                        let b = *input.add(B);

                        let output0 = output0.add(x * cconvert_chunk() + c);
                        let output1 = output1.add(x * cconvert_chunk() + c);
                        let output2 = output2.add(x * cconvert_chunk() + c);

                        pconvert(r, g, b, output0, output1, output2, tab);
                    }
                }
            }
        }
    }
}

// input count N
pub fn cconvert2<const N: usize, F, const START_STEP: StartStep>(
    input: &mut impl Iterator<Item = *const u8>,
    comps: F,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    tab: &ColorConvTabs,
) where
    F: Fn(u16, &ColorConvTabs) -> (u8, u8, u8),
{
    let chunk_u32 = cconvert_chunk_u32(2, cconvert_step(START_STEP));
    match chunk_u32 {
        2 => cconvert2_chunk_u32::<N, F, START_STEP, 3>(input, comps, output, width, tab),
        4 => cconvert2_chunk_u32::<N, F, START_STEP, 4>(input, comps, output, width, tab),
        _ => panic!(),
    }
}

// input count N
pub fn cconvert2_chunk_u32<const N: usize, F, const START_STEP: StartStep, const CHUNK_U32: usize>(
    input: &mut impl Iterator<Item = *const u8>,
    comps: F,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    tab: &ColorConvTabs,
) where
    F: Fn(u16, &ColorConvTabs) -> (u8, u8, u8),
{
    let (start, step) = get_start_step(START_STEP);
    const P: usize = mem::size_of::<u16>();
    let out_width = width / step;
    for i in 0..N {
        if let Some(input) = input.next() {
            unsafe {
                let output0 = output[0].ptr.add(out_width * i);
                let output1 = output[1].ptr.add(out_width * i);
                let output2 = output[2].ptr.add(out_width * i);

                for x in 0..out_width / cconvert_chunk() {
                    let input_0 =
                        *(input.add(x * cconvert_chunk_p(P, step)) as *const [u32; CHUNK_U32]);
                    let input = input_0.as_ptr() as *const u8;

                    for c in 0..cconvert_chunk() {
                        let input = input.add((c * step + start) * P);

                        let output0 = output0.add(x * cconvert_chunk() + c);
                        let output1 = output1.add(x * cconvert_chunk() + c);
                        let output2 = output2.add(x * cconvert_chunk() + c);

                        let (r, g, b) = comps(*(input as *const u16), tab);

                        pconvert(r, g, b, output0, output1, output2, &tab.rgb_ycc_tab);
                    }
                }
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
