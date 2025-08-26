// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

impl JpegSharedMut {
    fn once(&mut self) {
        unsafe {
            ptr::write_bytes(self as *mut _ as *mut u8, 0, mem::size_of_val(self));
        }
        self.rand32 = Rand32::new(get_system_tick().get() as u64);
    }

    fn init(&mut self, delta_prog: bool, params: [(usize, f32); RP_SCREEN_COUNT as usize]) {
        if delta_prog {
            self.compressed_size = const_default();
            for s in ScreenIndex::all() {
                for i in 0..DOWNSAMPLE_FACTOR {
                    (*self.delta_q.get_mut(&s))[i] = DELTA_Q_COUNT - 1;
                }
            }
        }

        self.delta_q_calc = const_default();

        if delta_prog {
            for i in 0..SCREEN_COUNT as usize {
                let (max_blocks_in_mcu, q_steps) = params[i];

                for k in 0..DOWNSAMPLE_FACTOR {
                    for j in 0..RP_DELTA_Q_COEFS_COUNT as usize {
                        self.delta_q_calc[i][k].f[j].m = q_steps;
                        self.delta_q_calc[i][k].f[j].p = q_steps * q_steps;
                    }
                    self.delta_q_calc[i][k].nbits = (MIN_DCT_COMP_SIZE * max_blocks_in_mcu) as f32;
                }
            }
        }
    }
}

impl JpegShared {
    fn init(
        &mut self,
        quality: [u32; RP_SCREEN_COUNT as usize],
        rel_stream: bool,
        delta_prog: bool,
        core_count: CoreCount,
        hq: [u32; RP_SCREEN_COUNT as usize],
        downsample: [u32; RP_SCREEN_COUNT as usize],
    ) -> [(usize, f32); RP_SCREEN_COUNT as usize] {
        self.rel_stream = rel_stream;
        self.delta_prog = delta_prog;

        self.quality = quality;
        for s in ScreenIndex::all() {
            let screen = s.index_into_mut(&mut self.screens);
            let quality = *s.index_into(&quality);

            if !delta_prog || s.get() == RP_SCREEN_TOP as u32 {
                screen
                    .quant_tbls
                    .set_quality(if delta_prog { 100 } else { quality });
                screen
                    .divisors
                    .set_divisors(&screen.quant_tbls, &mut screen.div_shifts);
            }

            if delta_prog {
                const QOS_ADJ_B: f32 = u8::BITS as f32;
                const QOS_MIN_F: f32 = 0.625f32;
                const QOS_MAX_L_F: f32 = 0.875f32;
                const QOS_MAX_H_F: f32 = 0.75f32;
                screen.qos_adj = QOS_ADJ_B * QOS_MIN_F
                    + ((QOS_MAX_L_F
                        + (QOS_MAX_H_F - QOS_MAX_L_F)
                            * entries::thread_nwm::rp_delta_q_qos() as f32
                            * (1f32 / RP_QOS_MAX as f32)
                        - QOS_MIN_F)
                        * QOS_ADJ_B
                        * quality as f32
                        * (1f32 / RP_QUALITY_MAX as f32));
            }
        }

        if delta_prog {
            for s in 1..RP_SCREEN_COUNT as usize {
                self.screens[s].quant_tbls = self.screens[RP_SCREEN_TOP as usize].quant_tbls;
                self.screens[s].divisors = self.screens[RP_SCREEN_TOP as usize].divisors;
                self.screens[s].div_shifts = self.screens[RP_SCREEN_TOP as usize].div_shifts;
            }

            for q in 0..DELTA_Q_COUNT {
                let div_shifts = &mut self.div_delta_q_shifts[q as usize];
                for i in 0..NUM_QUANT_TBLS {
                    let base_shifts = &self.screens[RP_SCREEN_TOP as usize].div_shifts[i];
                    let shifts = &mut div_shifts[i];
                    let ltbl = &self.delta_q_tbls[q as usize][i];

                    for i in 0..DCTSIZE2 {
                        shifts[i] = base_shifts[i] + ltbl[i];
                    }
                }
            }
        }

        self.core_count = core_count;
        self.last_restart_range = if delta_prog { 64 } else { 32 };
        self.set_comp_infos(hq, downsample, delta_prog)
    }

    fn once_delta_q_tbls(&mut self) {
        for d in (0..DELTA_Q_COUNT as usize).rev() {
            let f = DELTA_Q_MAX / DELTA_Q_COUNT as f32 * d as f32;

            for j in 0..NUM_QUANT_TBLS {
                let btbls = if j == 0 {
                    &STD_LUMINANCE_QUANT_TBL
                } else {
                    &STD_CHROMINANCE_QUANT_TBL
                };

                let mut log2_tbls: [f32; DCTSIZE2] = const_default();
                for i in 0..DCTSIZE2 {
                    let v = unsafe { log2f(btbls[i] as f32) };
                    log2_tbls[i] = v;
                }
                for i in 0..DCTSIZE2 {
                    let v = unsafe { roundf(f32::max(log2_tbls[i] - f, 0.0f32)) } as u8;
                    self.delta_q_tbls[d][j][i] = v;
                    let m = v - self.delta_q_tbls[DELTA_Q_COUNT as usize - 1][j][i];
                    self.delta_q0_tbls[d][j][i] = m;
                }
            }
        }
    }

    fn once(&mut self) {
        unsafe {
            ptr::write_bytes(self as *mut _ as *mut u8, 0, mem::size_of_val(self));
        }
        self.jpeg_tbls = JpegTbls::once();
        self.once_delta_q_tbls();
    }

    fn set_comp_infos(
        &mut self,
        hq: [u32; RP_SCREEN_COUNT as usize],
        downsample: [u32; RP_SCREEN_COUNT as usize],
        delta_prog: bool,
    ) -> [(usize, f32); RP_SCREEN_COUNT as usize] {
        let mut ret: [(usize, f32); RP_SCREEN_COUNT as usize] = const_default();

        for s in ScreenIndex::all() {
            let is_top = s.get() == RP_SCREEN_TOP as u32;
            let screen = s.index_into_mut(&mut self.screens);
            let hq = *s.index_into(&hq) as u8;

            screen.downsample = *s.index_into(&downsample) as u8;

            if screen.downsample == RP_DOWNSAMPLE_CHECKER {
                screen.width = downsample_checker_screen_dim(is_top) as u16;
                screen.height = screen.width;
            } else {
                screen.width = downsample_screen_width(screen.downsample) as u16;
                screen.height = downsample_screen_height(screen.downsample, is_top) as u16;
            }
            screen.checker = const_default();

            let comp_infos = if hq == RP_CHROMASS_444 {
                &self.jpeg_tbls.comp_infos_444
            } else if hq == RP_CHROMASS_422 {
                &self.jpeg_tbls.comp_infos_422
            } else {
                &self.jpeg_tbls.comp_infos_420
            };
            screen.comp_infos = comp_infos;
            screen.max_h_samp_factor = 1;
            screen.max_v_samp_factor = 1;
            screen.max_blocks_in_mcu = 0;
            for i in 0..MAX_COMPONENTS {
                let info = &comp_infos.infos[i];
                screen.max_h_samp_factor =
                    cmp::max(screen.max_h_samp_factor, info.h_samp_factor as usize);
                screen.max_v_samp_factor =
                    cmp::max(screen.max_v_samp_factor, info.v_samp_factor as usize);
                screen.max_blocks_in_mcu +=
                    info.h_samp_factor as usize * info.v_samp_factor as usize;
            }
            if screen.max_blocks_in_mcu > MAX_BLOCKS_IN_MCU {
                panic!();
            }
            screen.mcu_row_size = DCTSIZE * screen.max_h_samp_factor;
            screen.mcu_col_size = DCTSIZE * screen.max_v_samp_factor;
            screen.mcus_per_row = jdiv_round_up(screen.width as usize, screen.mcu_row_size);
            screen.mcu_rows = jdiv_round_up(screen.height as usize, screen.mcu_col_size) as u16;
            screen.mcus = screen.mcus_per_row as u16 * screen.mcu_rows;

            if screen.downsample == RP_DOWNSAMPLE_CHECKER {
                let tl = GSP_SCREEN_WIDTH;
                let br = if is_top {
                    GSP_SCREEN_HEIGHT_TOP
                } else {
                    GSP_SCREEN_HEIGHT_BOTTOM
                };

                let mcu_l_v = tl / screen.mcu_col_size as u32;
                let mcu_l_r = tl % screen.mcu_col_size as u32;
                let mcu_l_w = (mcu_l_r > 0) as u32;

                let mcu_r_v = br / screen.mcu_col_size as u32;
                let mcu_r_r = br % screen.mcu_col_size as u32;
                let mcu_r_w = (mcu_r_r > 0) as u32;

                let checker = &mut screen.checker;

                let mut mcus = 0;
                for mcu_y_start in 0..screen.mcu_rows as u32 {
                    let params = &mut checker.mcu_row_params[mcu_y_start as usize];

                    let x_start = if mcu_y_start < mcu_l_v {
                        let y_end = (mcu_y_start + 1) * screen.mcu_col_size as u32;
                        tl - y_end
                    } else if mcu_y_start < mcu_l_v + mcu_l_w {
                        0
                    } else {
                        let y_start =
                            (mcu_y_start - mcu_l_v) * screen.mcu_col_size as u32 - mcu_l_r;
                        y_start
                    };
                    params.mcu_col_start = (x_start / screen.mcu_row_size as u32) as u16;

                    let x_end = if mcu_y_start < mcu_r_v {
                        let y_end = (mcu_y_start + 1) * screen.mcu_col_size as u32;
                        tl + y_end
                    } else if mcu_y_start < mcu_r_v + mcu_r_w {
                        tl + br
                    } else {
                        let y_start =
                            (mcu_y_start - mcu_r_v) * screen.mcu_col_size as u32 - mcu_r_r;
                        tl + (br - y_start)
                    };
                    params.mcu_col_end = jdiv_round_up(x_end as usize, screen.mcu_row_size) as u16;

                    mcus += params.mcu_col_end - params.mcu_col_start;
                }

                checker.mcus = mcus;
                const MCU_STEP: u16 = 8;
                checker.mcus_per_row = MCU_STEP;
                checker.mcu_rows =
                    jdiv_round_up(checker.mcus as usize, checker.mcus_per_row as usize) as u16;
            }

            *s.index_into_mut(&mut ret) = if delta_prog {
                let mut qf: [u16; NUM_QUANT_TBLS] = const_default();
                for ci in 0..MAX_COMPONENTS {
                    let comp = &comp_infos.infos[ci];
                    let mcu_we = comp.h_samp_exp;
                    let mcu_he = comp.v_samp_exp;

                    qf[comp.quant_tbl_no as usize] += 1 << mcu_we + mcu_he;
                }
                const QF_F: [f32; NUM_QUANT_TBLS] = [1.25f32, 2f32 / 3f32];
                let qf = {
                    let mut ret: [f32; NUM_QUANT_TBLS] = const_default();
                    for i in 0..NUM_QUANT_TBLS {
                        ret[i] = qf[i] as f32 * QF_F[i];
                    }
                    ret
                };
                screen.delta_q_params.qf = qf;
                let qt = {
                    let mut qt = 0f32;
                    for i in 0..NUM_QUANT_TBLS {
                        qt += screen.delta_q_params.qf[i];
                    }
                    qt
                };
                let q_step = (DELTA_Q_STEP * qt, DELTA_Q_STEP * (DCTSIZE2 - 1) as f32 * qt);
                let q_steps = q_step.0 + q_step.1;
                let q_steps_i = 1f32 / q_steps;
                screen.delta_q_params.q_steps_i = q_steps_i * SCALE_QD_I_F;

                screen.delta_q_params.m = (MIN_DCT_COMP_SIZE * screen.max_blocks_in_mcu) as f32;

                (screen.max_blocks_in_mcu, q_steps)
            } else {
                (0, 0f32)
            }
        }
        ret
    }
}

impl Jpeg {
    pub unsafe fn once(&mut self) {
        self.shared.once();
        self.shared_mut.once();
    }

    #[named]
    pub fn init(
        &mut self,
        quality: [u32; RP_SCREEN_COUNT as usize],
        core_count: CoreCount,
        hq: [u32; RP_SCREEN_COUNT as usize],
        downsample: [u32; RP_SCREEN_COUNT as usize],
        rel_stream: bool,
        delta_prog: bool,
    ) -> Option<()> {
        let shared_mut_params = self
            .shared
            .init(quality, rel_stream, delta_prog, core_count, hq, downsample);
        self.shared_mut.init(delta_prog, shared_mut_params);

        for w in WorkIndex::all() {
            let sem = self.shared.work_sem.get_mut(&w);
            if *sem > 0 {
                unsafe {
                    let _ = svcCloseHandle(*sem);
                }
                *sem = 0;
            }

            let res = unsafe { svcCreateSemaphore(sem, 0, core_count.get() as i32 - 1) };
            if res != 0 {
                ns_dbg_print!(create_semaphore_failed, c_str!("jpeg work_sem"), res);
                return None;
            }
            self.shared_mut
                .work_inited
                .get_mut(&w)
                .store(false, Ordering::Release);
            self.shared_mut
                .work_sem_count
                .get_mut(&w)
                .store(core_count.get() as u8, Ordering::Release);
        }
        for s in ScreenIndex::all() {
            let sem = self.shared.screen_sem.get_mut(&s);
            if *sem > 0 {
                unsafe {
                    let _ = svcCloseHandle(*sem);
                }
                *sem = 0;
            }

            let res = unsafe { svcCreateSemaphore(sem, 1, 1) };
            if res != 0 {
                ns_dbg_print!(create_semaphore_failed, c_str!("jpeg screen_sem"), res);
                return None;
            }

            self.shared_mut
                .screen_bool
                .get_mut(&s)
                .store(false, Ordering::Release);
            *self.shared_mut.last_restart_interval.get_mut(&s) = 0;
        }

        unsafe {
            ptr::write_bytes(
                self.shared_mut.dq_prev_coeffs_top.as_mut_ptr(),
                0,
                self.shared_mut.dq_prev_coeffs_top.len(),
            );

            ptr::write_bytes(
                self.shared_mut.dq_prev_coeffs_bot.as_mut_ptr(),
                0,
                self.shared_mut.dq_prev_coeffs_bot.len(),
            );
        }

        Some(())
    }
}
