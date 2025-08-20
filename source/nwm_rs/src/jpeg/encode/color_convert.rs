// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

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

pub fn cconvert<const R: usize, const G: usize, const B: usize, const P: usize, const N: usize>(
    input: &[&[u8]; N],
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    tab: &[i32; TABLE_SIZE],
) {
    let [output0, output1, output2] = output;
    let output0 = unsafe { slice::from_raw_parts_mut(output0.ptr, width * N) };
    let output1 = unsafe { slice::from_raw_parts_mut(output1.ptr, width * N) };
    let output2 = unsafe { slice::from_raw_parts_mut(output2.ptr, width * N) };
    for i in 0..N {
        let input = unsafe { slice::from_raw_parts(input[i].as_ptr(), width * P) };

        let output0 = &mut output0[width * i..width * (i + 1)];
        let output1 = &mut output1[width * i..width * (i + 1)];
        let output2 = &mut output2[width * i..width * (i + 1)];

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

pub fn cconvert2<const N: usize, F>(
    input: &[&[u8]; N],
    comps: F,
    output: &mut [WorkerColorBuf; MAX_COMPONENTS],
    width: usize,
    tab: &ColorConvTabs,
) where
    F: Fn(u16, &ColorConvTabs) -> (u8, u8, u8),
{
    let [output0, output1, output2] = output;
    let output0 = unsafe { slice::from_raw_parts_mut(output0.ptr, width * N) };
    let output1 = unsafe { slice::from_raw_parts_mut(output1.ptr, width * N) };
    let output2 = unsafe { slice::from_raw_parts_mut(output2.ptr, width * N) };
    for i in 0..N {
        let input = unsafe { slice::from_raw_parts(input[i].as_ptr(), width * 2) };

        let output0 = &mut output0[width * i..width * (i + 1)];
        let output1 = &mut output1[width * i..width * (i + 1)];
        let output2 = &mut output2[width * i..width * (i + 1)];

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

pub fn rgb565_comps(input: u16, tab: &ColorConvTabs) -> (u8, u8, u8) {
    let r = tab.rb_5_tab[((input >> 11) & 0x1f) as usize];
    let g = tab.g_6_tab[((input >> 5) & 0x3f) as usize];
    let b = tab.rb_5_tab[(input & 0x1f) as usize];
    (r, g, b)
}

pub fn rgb5a1_comps(input: u16, tab: &ColorConvTabs) -> (u8, u8, u8) {
    let r = tab.rb_5_tab[((input >> 11) & 0x1f) as usize];
    let g = tab.rb_5_tab[((input >> 6) & 0x1f) as usize];
    let b = tab.rb_5_tab[((input >> 1) & 0x1f) as usize];
    (r, g, b)
}
