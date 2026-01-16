// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

#![allow(unused_imports)]

mod color_convert;
mod compress;
mod encode_buffer;
mod forward_dct;
mod pre_process;
mod write;

use super::*;

pub use color_convert::*;
pub use compress::*;
pub use encode_buffer::*;
pub use forward_dct::*;
pub use pre_process::*;
pub use write::*;

pub const fn subsamp_constraint(h_samp: bool, v_samp: bool) {
    match (h_samp, v_samp) {
        (true, true) => (),
        (true, false) => (),
        (false, true) => panic!(),
        (false, false) => (),
    }
}

struct SubSampConst<const H_SAMP: bool, const V_SAMP: bool>;

impl<const H_SAMP: bool, const V_SAMP: bool> SubSampConst<H_SAMP, V_SAMP> {
    const ASSERT: () = subsamp_constraint(H_SAMP, V_SAMP);
}

pub struct JpegEncode<'a, 'b> {
    pub worker: &'b mut JpegWorker<'a>,
    pub dst: WorkerDst,
}

#[cfg(not(feature = "mem3"))]
fn get_bpp_for_format(c: ColorSpace) -> u8 {
    match c {
        ColorSpace::XBGR => 4,
        ColorSpace::BGR => 3,
        _ => 2,
    }
}

impl<'a, 'b> JpegEncode<'a, 'b> {
    #[named]
    #[allow(unused_macros)]
    #[inline(never)]
    pub fn encode<F, G>(
        &mut self,
        #[cfg(not(feature = "mem3"))] src: &[u8],
        #[cfg(feature = "mem3")] src: *const u8,
        #[cfg(feature = "mem3")] pitch: u32,
        #[cfg(not(feature = "o3ds"))] mut pre_progress: F,
        #[cfg(feature = "mem3")]
        #[allow(unused)]
        mut progress: G,
    ) -> Option<JpegDqRet>
    where
        F: FnMut(),
        G: FnMut(),
    {
        #[cfg(not(feature = "mem3"))]
        let bpp = get_bpp_for_format(self.worker.info.color_space);
        let is_top = self.worker.info.is_top;
        #[cfg(not(feature = "o3ds"))]
        let w = self.worker.info.work_index;
        let s = is_top_index(is_top);
        let screen = s.index_into(&self.worker.shared.screens);
        #[cfg(not(feature = "mem3"))]
        let pitch = GSP_SCREEN_WIDTH as usize * bpp as usize;
        #[allow(unused)]
        let mcus = screen.mcus;

        #[cfg(not(feature = "o3ds"))]
        if !self.worker.shared.rel_stream && self.worker.thread_index.get() == 0 {
            self.write_headers();
        }
        #[cfg(feature = "o3ds")]
        self.write_headers();

        self.reset_mcu();

        #[cfg(not(feature = "o3ds"))]
        let prev = unsafe {
            if self.worker.shared.delta_prog {
                if src.len() == 0 {
                    wait_syn(
                        cname!(),
                        *self.worker.shared.work_sem.get(&w),
                        c_str!("work_sem"),
                    )?;
                }

                let shared_mut = &mut *self.worker.shared_mut.cell;

                let prev = if is_top {
                    shared_mut.dq_prev_coeffs_top.as_mut_ptr()
                } else {
                    shared_mut.dq_prev_coeffs_bot.as_mut_ptr()
                };

                let prev = match screen.downsample {
                    RP_DOWNSAMPLE_CHECKER | RP_DOWNSAMPLE_EVEN_ODD => {
                        let count = (if is_top {
                            shared_mut.dq_prev_coeffs_top.len()
                        } else {
                            shared_mut.dq_prev_coeffs_bot.len()
                        }) / 2;
                        prev.add(count * self.worker.info.even_odd as usize)
                    }
                    RP_DOWNSAMPLE_QUARTER | _ => prev,
                };

                (prev as *mut JBlock).add(
                    self.worker.info.restart_interval as usize
                        * screen.max_blocks_in_mcu
                        * self.worker.thread_index.get() as usize,
                )
            } else {
                ptr::null_mut()
            }
        };

        let hss = screen.max_h_samp_factor == SAMP_FACTOR;
        let vss = screen.max_v_samp_factor == SAMP_FACTOR;

        #[cfg(not(feature = "mem3"))]
        if screen.downsample == RP_DOWNSAMPLE_CHECKER {
            let mcu_total_start =
                self.worker.info.restart_interval * self.worker.thread_index.get() as u16;
            let mcu_total_count = self.worker.info.restart_interval;

            let mut mcu_curr_start = 0;
            let mut mcu_curr_count = 0;

            'f: for mcu_y in 0..screen.mcu_rows {
                let params = &screen.checker.mcu_row_params[mcu_y as usize];
                let mcu_col_count = params.mcu_col_end - params.mcu_col_start;

                for mcu_i in 0..mcu_col_count {
                    if mcu_curr_start >= mcu_total_start {
                        let _mcu_x = params.mcu_col_start + mcu_i;

                        #[cfg(not(feature = "o3ds"))]
                        if mcu_curr_count == j_max_half_factor(mcu_total_count as usize) as u16 {
                            pre_progress();
                        }

                        mcu_curr_count += 1;

                        if mcu_curr_count >= mcu_total_count {
                            break 'f;
                        }
                    } else {
                        mcu_curr_start += 1;
                    }
                }
            }
        } else {
            let src_iter = &mut src.chunks_exact(pitch).map(|x| x.as_ptr());
            let height = src.len() / pitch;
            if vss {
                // vss == true
                match screen.downsample {
                    RP_DOWNSAMPLE_QUARTER => {
                        let n = height / (DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR);
                        let rem = height % (DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR);
                        #[allow(unused)]
                        for i in 0..n {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                i,
                                |this| {
                                    /* Pre-process */
                                    this.pre_process_quarter(src_iter);
                                    #[cfg(not(feature = "o3ds"))]
                                    if i == j_max_half_factor(n) {
                                        pre_progress();
                                    }
                                    true
                                },
                                || {},
                            );
                        }

                        if rem > 0 {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                n,
                                |this| {
                                    /* Pre-process */
                                    if !this.pre_process_quarter_rem(src_iter, rem) {
                                        return false;
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    }
                    RP_DOWNSAMPLE_EVEN_ODD => {
                        let n = height / (DCTSIZE * SAMP_FACTOR);
                        #[allow(unused)]
                        for i in 0..n {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                i,
                                |this| {
                                    /* Pre-process */
                                    this.pre_process_even_odd(src_iter);
                                    #[cfg(not(feature = "o3ds"))]
                                    if i == j_max_half_factor(n) {
                                        pre_progress();
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    }
                    _ => {
                        let n = height / (DCTSIZE * SAMP_FACTOR);
                        #[allow(unused)]
                        for i in 0..n {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                i,
                                |this| {
                                    /* Pre-process */
                                    this.pre_process_full(src_iter);
                                    #[cfg(not(feature = "o3ds"))]
                                    if i == j_max_half_factor(n) {
                                        pre_progress();
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    }
                };
            } else {
                // vss == false
                match screen.downsample {
                    RP_DOWNSAMPLE_QUARTER => {
                        let pre_process = if hss {
                            Self::pre_process_quarter_novsamp
                        } else {
                            Self::pre_process_quarter_nohsamp_novsamp
                        };

                        let n = height / (DCTSIZE * DOWNSAMPLE_FACTOR);
                        #[allow(unused)]
                        for i in 0..n {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                i,
                                |this| {
                                    pre_process(this, src_iter);
                                    #[cfg(not(feature = "o3ds"))]
                                    if i == j_max_half_factor(n) {
                                        pre_progress();
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    }
                    RP_DOWNSAMPLE_EVEN_ODD => {
                        let pre_process = if hss {
                            Self::pre_process_even_odd_novsamp::<true>
                        } else {
                            Self::pre_process_even_odd_novsamp::<false>
                        };

                        let n = height / DCTSIZE;
                        #[allow(unused)]
                        for i in 0..n {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                i,
                                |this| {
                                    pre_process(this, src_iter);
                                    #[cfg(not(feature = "o3ds"))]
                                    if i == j_max_half_factor(n) {
                                        pre_progress();
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    }
                    _ => {
                        let pre_process = if hss {
                            Self::pre_process_full_novsamp::<true>
                        } else {
                            Self::pre_process_full_novsamp::<false>
                        };

                        let n = height / DCTSIZE;
                        #[allow(unused)]
                        for i in 0..n {
                            self.process(
                                #[cfg(not(feature = "o3ds"))]
                                prev,
                                #[cfg(not(feature = "o3ds"))]
                                i,
                                |this| {
                                    pre_process(this, src_iter);
                                    #[cfg(not(feature = "o3ds"))]
                                    if i == j_max_half_factor(n) {
                                        pre_progress();
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    }
                }
            }
        }

        #[cfg(feature = "mem3")]
        {
            let height = if is_top {
                GSP_SCREEN_HEIGHT_TOP
            } else {
                GSP_SCREEN_HEIGHT_BOTTOM
            } as usize;
            let src = unsafe { slice::from_raw_parts(src, pitch as usize * height) };
            let src_iter = &mut src.chunks_exact(pitch as usize).map(|x| x.as_ptr());
            match screen.downsample {
                RP_DOWNSAMPLE_QUARTER => {
                    if vss {
                        let count = height / (DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR);
                        let rem = height % (DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR);
                        for _ in 0..count {
                            self.process(
                                |this| {
                                    /* Pre-process */
                                    this.pre_process_quarter(src_iter);
                                    true
                                },
                                || {},
                            );
                        }

                        if rem > 0 {
                            self.process(
                                |this| {
                                    /* Pre-process */
                                    if !this.pre_process_quarter_rem(src_iter, rem) {
                                        return false;
                                    }
                                    true
                                },
                                || {},
                            );
                        }
                    } else {
                        let pre_process = if hss {
                            Self::pre_process_quarter_novsamp
                        } else {
                            Self::pre_process_quarter_nohsamp_novsamp
                        };

                        for _ in 0..height / (DCTSIZE * DOWNSAMPLE_FACTOR) {
                            self.process(
                                |this| {
                                    pre_process(this, src_iter);
                                    true
                                },
                                || {},
                            );
                        }
                    }
                }
                RP_DOWNSAMPLE_EVEN_ODD => {
                    if vss {
                        // vss == true
                        for _ in 0..height / (DCTSIZE * SAMP_FACTOR) {
                            self.process(
                                |this| {
                                    /* Pre-process */
                                    this.pre_process_even_odd(src_iter);
                                    true
                                },
                                || {},
                            );
                        }
                    } else {
                        // vss == false
                        let pre_process = if hss {
                            Self::pre_process_even_odd_novsamp::<true>
                        } else {
                            Self::pre_process_even_odd_novsamp::<false>
                        };

                        for _ in 0..height / DCTSIZE {
                            self.process(
                                |this| {
                                    pre_process(this, src_iter);
                                    true
                                },
                                || {},
                            );
                        }
                    }
                }
                _ => {
                    if vss {
                        // vss == true
                        for _ in 0..height / (DCTSIZE * SAMP_FACTOR) {
                            self.process(
                                |this| {
                                    /* Pre-process */
                                    this.pre_process_full(src_iter);
                                    true
                                },
                                || {},
                            );
                        }
                    } else {
                        // vss == false
                        let pre_process = if hss {
                            Self::pre_process_full_novsamp::<true>
                        } else {
                            Self::pre_process_full_novsamp::<false>
                        };

                        for _ in 0..height / DCTSIZE {
                            self.process(
                                |this| {
                                    pre_process(this, src_iter);
                                    true
                                },
                                || {},
                            );
                        }
                    }
                }
            }
        }

        self.flush_mcu();

        #[cfg(not(feature = "o3ds"))]
        let mut delta_q = 0;
        #[cfg(not(feature = "o3ds"))]
        if !self.worker.shared.rel_stream {
            if self.worker.thread_index == thread_index_last(self.worker.shared.core_count) {
                self.write_trailer();
            } else {
                self.write_rst();
            }
        }
        #[cfg(feature = "o3ds")]
        self.write_trailer();

        self.write_term();

        #[cfg(not(feature = "o3ds"))]
        if self.worker.shared.delta_prog {
            unsafe {
                let shared_mut = &mut *self.worker.shared_mut.cell;
                delta_q = *shared_mut.work_delta_q.get(&w);

                let c = shared_mut.work_sem_count.get_mut(&w);

                if c.fetch_sub(1, Ordering::AcqRel) == 1 {
                    c.store(self.worker.info.core_count.get() as u8, Ordering::Release);
                    shared_mut
                        .work_inited
                        .get_mut(&w)
                        .store(false, Ordering::Release);

                    if rp_need_core_syn!() {
                        let b = shared_mut.screen_bool.get_mut(&s);
                        if b.swap(true, Ordering::AcqRel) {
                            b.store(false, Ordering::Release);
                        } else {
                            release_sem(
                                cname!(),
                                *self.worker.shared.screen_sem.get(&s),
                                c_str!("screen_sem"),
                            );
                        }
                    }
                }
            }
        }

        #[cfg(not(feature = "o3ds"))]
        let ret = Some(JpegDqRet { delta_q, mcus });
        #[cfg(feature = "o3ds")]
        let ret = Some(JpegDqRet {});
        ret
    }

    fn process<F, G>(
        &mut self,
        #[cfg(not(feature = "o3ds"))] prev: *mut JBlock,
        #[cfg(not(feature = "o3ds"))] row_i: usize,
        do_pre_process: F,
        do_progress: G,
    ) where
        F: FnOnce(&mut Self) -> bool,
        G: FnOnce(),
    {
        if !do_pre_process(self) {
            return;
        }

        #[cfg(not(feature = "o3ds"))]
        let is_top = self.worker.info.is_top;
        #[cfg(not(feature = "o3ds"))]
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);

        /* Compress and encode */
        #[cfg(not(feature = "o3ds"))]
        self.do_process(
            if self.worker.shared.delta_prog {
                unsafe { prev.add(row_i * screen.mcus_per_row * screen.max_blocks_in_mcu) }
            } else {
                ptr::null_mut()
            },
            row_i as u8,
        );

        #[cfg(feature = "o3ds")]
        self.do_process();

        do_progress();
    }

    #[named]
    #[allow(unused_macros)]
    fn do_process(
        &mut self,
        #[cfg(not(feature = "o3ds"))] prev: *mut JBlock,
        #[cfg(not(feature = "o3ds"))] row_i: u8,
    ) {
        #[cfg(not(feature = "o3ds"))]
        let mut delta_cache = false;
        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        for mcu_col_num in 0..screen.mcus_per_row {
            #[cfg(not(feature = "o3ds"))]
            if self.worker.shared.delta_prog {
                let s = is_top_index(self.worker.info.is_top);
                let w = self.worker.info.work_index;
                let shared_mut = unsafe { &mut *self.worker.shared_mut.cell };

                if row_i == 0 && mcu_col_num == 0 {
                    if !shared_mut.work_inited.get(&w).swap(true, Ordering::AcqRel) {
                        let last_restart_interval = shared_mut.last_restart_interval.get_mut(&s);

                        if rp_need_core_syn!() {
                            let b = shared_mut.screen_bool.get(&s);
                            let need_sync =
                                self.worker.info.restart_interval != *last_restart_interval;

                            let need_sync = if !need_sync {
                                b.swap(true, Ordering::AcqRel)
                            } else {
                                need_sync
                            };

                            if need_sync {
                                if wait_syn(
                                    cname!(),
                                    *self.worker.shared.screen_sem.get(&s),
                                    c_str!("screen_sem"),
                                )
                                .is_none()
                                {
                                    return;
                                }

                                b.store(false, Ordering::Release);
                                *last_restart_interval = self.worker.info.restart_interval;
                            }
                        }

                        self.compute_dq(prev);
                        unsafe {
                            shared_mut
                                .compressed_size
                                .get(&s)
                                .get_unchecked(self.worker.info.even_odd as usize)
                                .store(0, Ordering::Release)
                        };
                        delta_cache = true;

                        unsafe {
                            release_sem_count(
                                cname!(),
                                *self.worker.shared.work_sem.get(&w),
                                c_str!("work_sem"),
                                self.worker.info.core_count.get() as s32 - 1,
                            );
                        }
                    } else {
                        if wait_syn(
                            cname!(),
                            *self.worker.shared.work_sem.get(&w),
                            c_str!("work_sem"),
                        )
                        .is_none()
                        {
                            return;
                        }
                    }
                }

                let dq_rescale_prev = *shared_mut.dq_rescale_prev.get(&w);
                let prev = unsafe { prev.add(mcu_col_num * screen.max_blocks_in_mcu) };

                if dq_rescale_prev > 0 {
                    self.compress(mcu_col_num, prev, delta_cache, true, false);
                } else if dq_rescale_prev < 0 {
                    self.compress(mcu_col_num, prev, delta_cache, true, true);
                } else {
                    self.compress(mcu_col_num, prev, delta_cache, false, false);
                }
            } else {
                self.compress(mcu_col_num, ptr::null_mut(), false, false, false);
            }

            #[cfg(feature = "o3ds")]
            self.compress(mcu_col_num);

            self.encode_mcu();
        }
    }
}

#[cfg(not(feature = "o3ds"))]
pub const fn j_max_half_factor(v: usize) -> usize {
    v / 2
}
