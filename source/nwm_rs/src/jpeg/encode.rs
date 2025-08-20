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

const fn subsamp_constraint<const H_SAMP: bool, const V_SAMP: bool>() {
    match (H_SAMP, V_SAMP) {
        (true, true) => (),
        (true, false) => (),
        (false, true) => panic!(),
        (false, false) => (),
    }
}

struct SubSampConst<const H_SAMP: bool, const V_SAMP: bool>;

impl<const H_SAMP: bool, const V_SAMP: bool> SubSampConst<H_SAMP, V_SAMP> {
    const ASSERT: () = subsamp_constraint::<H_SAMP, V_SAMP>();
}

pub struct JpegEncode<'a, 'b> {
    pub worker: &'b mut JpegWorker<'a>,
    pub dst: WorkerDst,
}

fn get_bpp_for_format(c: ColorSpace) -> u8 {
    match c {
        ColorSpace::XBGR => 4,
        ColorSpace::BGR => 3,
        _ => 2,
    }
}

impl<'a, 'b> JpegEncode<'a, 'b> {
    #[named]
    pub fn encode<F, G>(
        &mut self,
        src: &[u8],
        mut pre_progress: F,
        mut progress: G,
    ) -> Option<JpegDqRet>
    where
        F: FnMut(u32),
        G: FnMut(),
    {
        let bpp = get_bpp_for_format(self.worker.info.color_space);
        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        let pitch = screen.width as usize * bpp as usize;
        let mcus = screen.mcus;

        pre_progress(1);

        if !self.worker.shared.rel_stream && self.worker.thread_index.get() == 0 {
            self.write_headers();
        }

        self.reset_mcu();

        let w = self.worker.info.work_index;
        let s = is_top_index(is_top);

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
                (if is_top {
                    shared_mut.dq_prev_coeffs_top.as_mut_ptr()
                } else {
                    shared_mut.dq_prev_coeffs_bot.as_mut_ptr()
                } as *mut JBlock)
                    .add(
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

        if vss {
            match screen.downsample {
                RP_DOWNSAMPLE_QUARTER => {
                    let src_chunks = src
                        .chunks_exact(pitch * DOWNSAMPLE_FACTOR)
                        .array_chunks::<{ DCTSIZE * SAMP_FACTOR * DOWNSAMPLE_FACTOR }>();
                    let n = src_chunks.len();
                    for (i, chunks) in src_chunks.clone().enumerate() {
                        self.process(
                            prev,
                            i,
                            |this| {
                                /* Pre-process */
                                this.pre_process_quarter(chunks);
                                pre_progress(DOWNSAMPLE_FACTOR as u32);
                                true
                            },
                            || {
                                progress();
                            },
                        );
                    }

                    if let Some(rem) = src_chunks.into_remainder() {
                        self.process(
                            prev,
                            n,
                            |this| {
                                /* Pre-process */
                                if !this.pre_process_quarter_rem(rem) {
                                    return false;
                                }
                                pre_progress(DOWNSAMPLE_FACTOR as u32);
                                true
                            },
                            || {
                                progress();
                            },
                        );
                    }
                }
                _ => {
                    let src_chunks = src
                        .chunks_exact(pitch)
                        .array_chunks::<{ DCTSIZE * SAMP_FACTOR }>();
                    for (i, chunks) in src_chunks.enumerate() {
                        self.process(
                            prev,
                            i,
                            |this| {
                                /* Pre-process */
                                this.pre_process_full(chunks);
                                pre_progress(1);
                                true
                            },
                            || {
                                progress();
                            },
                        );
                    }
                }
            };
        } else {
            match screen.downsample {
                RP_DOWNSAMPLE_QUARTER => {
                    let pre_process = if hss {
                        Self::pre_process_quarter_novsamp
                    } else {
                        Self::pre_process_quarter_nohsamp_novsamp
                    };

                    let src_chunks = src
                        .chunks_exact(pitch * DOWNSAMPLE_FACTOR)
                        .array_chunks::<{ DCTSIZE * DOWNSAMPLE_FACTOR }>();
                    for (i, chunk) in src_chunks.enumerate() {
                        self.process(
                            prev,
                            i,
                            |this| {
                                pre_process(this, chunk);
                                pre_progress(DOWNSAMPLE_FACTOR as u32);
                                true
                            },
                            || {
                                progress();
                            },
                        );
                    }
                }
                _ => {
                    let pre_process = if hss {
                        Self::pre_process_full_novsamp::<true>
                    } else {
                        Self::pre_process_full_novsamp::<false>
                    };

                    let src_chunks = src.chunks_exact(pitch).array_chunks::<DCTSIZE>();
                    for (i, chunk) in src_chunks.enumerate() {
                        self.process(
                            prev,
                            i,
                            |this| {
                                pre_process(this, chunk);
                                pre_progress(1);
                                true
                            },
                            || {
                                progress();
                            },
                        );
                    }
                }
            }
        }

        self.flush_mcu();

        let mut delta_q = 0;
        if !self.worker.shared.rel_stream {
            if self.worker.thread_index == thread_index_last(self.worker.shared.core_count) {
                self.write_trailer();
            } else {
                self.write_rst();
            }
        }

        self.write_term();

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

        Some(JpegDqRet { delta_q, mcus })
    }

    fn process<F, G>(&mut self, prev: *mut JBlock, row_i: usize, do_pre_process: F, do_progress: G)
    where
        F: FnOnce(&mut Self) -> bool,
        G: FnOnce(),
    {
        if !do_pre_process(self) {
            return;
        }

        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);

        /* Compress and encode */
        self.do_process(
            if self.worker.shared.delta_prog {
                unsafe { prev.add(row_i * screen.mcus_per_row * screen.max_blocks_in_mcu) }
            } else {
                ptr::null_mut()
            },
            row_i as u8,
        );

        do_progress();
    }

    #[named]
    fn do_process(&mut self, prev: *mut JBlock, row_i: u8) {
        let mut delta_cache = false;
        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        for mcu_col_num in 0..screen.mcus_per_row {
            if self.worker.shared.delta_prog {
                let s = is_top_index(self.worker.info.is_top);
                let w = self.worker.info.work_index;
                let shared_mut = unsafe { &mut *self.worker.shared_mut.cell };

                if row_i == 0 && mcu_col_num == 0 {
                    if !shared_mut.work_inited.get(&w).swap(true, Ordering::AcqRel) {
                        let last_restart_interval = shared_mut.last_restart_interval.get_mut(&s);
                        let b = shared_mut.screen_bool.get(&s);
                        let need_sync = self.worker.info.restart_interval != *last_restart_interval;

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

                        self.compute_dq(prev);
                        shared_mut
                            .compressed_size
                            .get(&s)
                            .store(0, Ordering::Release);
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

            self.encode_mcu();
        }
    }
}
