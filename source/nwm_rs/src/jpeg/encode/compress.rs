// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

impl<'a, 'b> JpegEncode<'a, 'b> {
    pub fn compress(
        &mut self,
        mcu_col_num: usize,
        prev: *mut JBlock,
        delta_cache: bool,
        rescale_prev: bool,
        rescale_prev_shr: bool,
    ) {
        let w = self.worker.info.work_index;
        let mut blkn = 0;
        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        let div_parts = &screen.divisors.divisors;
        let hss = screen.max_h_samp_factor == SAMP_FACTOR;
        let vss = screen.max_v_samp_factor == SAMP_FACTOR;

        let shared_mut = unsafe { &mut *self.worker.shared_mut.cell };

        let cache = shared_mut.delta_q_cache.get_mut(&w);
        let cache_next_i = shared_mut.delta_q_cache_next.get_mut(&w);
        let delta_q = *shared_mut.work_delta_q.get(&w) as usize;
        let delta_q0 = unsafe { self.worker.shared.delta_q0_tbls.get_unchecked(delta_q) };
        let mut delta_cache_start = 0;

        let _need_wait_for_nwm =
            self.worker.shared.delta_prog && self.worker.thread_index.get() == 0;

        let comp_infos = unsafe { &*screen.comp_infos };
        for ci in 0..MAX_COMPONENTS {
            let comp = &comp_infos.infos[ci];

            let div_shifts = if self.worker.shared.delta_prog {
                unsafe {
                    self.worker
                        .shared
                        .div_delta_q_shifts
                        .get_unchecked(delta_q)
                        .get_unchecked(comp.quant_tbl_no as usize)
                }
            } else {
                unsafe { screen.div_shifts.get_unchecked(comp.quant_tbl_no as usize) }
            };

            let rp_shifts = unsafe {
                shared_mut
                    .rp_shifts
                    .get(&w)
                    .get_unchecked(comp.quant_tbl_no as usize)
            };

            let mcu_width = comp.h_samp_factor;
            let mcu_height = comp.v_samp_factor;

            let mcu_sample_width = mcu_width as u16 * DCTSIZE as u16;
            let xpos = mcu_col_num as u16 * mcu_sample_width;
            let mut ypos = 0;

            for _ in 0..mcu_height {
                let mut xpos = xpos;
                for _ in 0..mcu_width {
                    let mut cache_hit = false;
                    let output = unsafe { self.worker.bufs.mcu.get_unchecked_mut(blkn as usize) };
                    let prev = if self.worker.shared.delta_prog {
                        unsafe { prev.add(blkn as usize) }
                    } else {
                        ptr::null_mut()
                    };

                    if delta_cache {
                        for qi in cache_next_i[ci]..DELTA_Q_CACHE_COUNTS[ci] {
                            let delta_cache_i = delta_cache_start + qi;
                            let cache = unsafe { cache.get_unchecked_mut(delta_cache_i as usize) };

                            if cache.ypos > ypos {
                                break;
                            }

                            if cache.ypos == ypos && cache.xpos > xpos {
                                break;
                            }

                            if cache.xpos == xpos && cache.ypos == ypos {
                                if delta_q == DELTA_Q_COUNT as usize - 1 {
                                    *output = cache.cache;
                                    unsafe { *prev = cache.next };
                                } else {
                                    let delta_q0 = unsafe {
                                        delta_q0.get_unchecked(comp.quant_tbl_no as usize)
                                    };
                                    for i in 0..DCTSIZE2 {
                                        unsafe {
                                            let (off_prev, off_diff) = if rescale_prev
                                                && rescale_prev_shr
                                            {
                                                let mask =
                                                    core::intrinsics::unchecked_shl(1, delta_q0[i])
                                                        - 1;
                                                let off_next = (cache.next[i] < 0) as JCoef
                                                    & ((cache.next[i] & mask) > 0) as JCoef;
                                                let off_prev = ((*prev)[i] < 0) as JCoef
                                                    & (((*prev)[i] & mask) > 0) as JCoef;
                                                let off_diff = (((*prev)[i] & mask)
                                                    > (cache.next[i] & mask))
                                                    as JCoef;
                                                (off_next, off_next - off_prev + off_diff)
                                            } else {
                                                let mask =
                                                    core::intrinsics::unchecked_shl(1, delta_q0[i])
                                                        - 1;
                                                let off_next = (cache.next[i] < 0) as JCoef
                                                    & ((cache.next[i] & mask) > 0) as JCoef;
                                                (off_next, off_next)
                                            };
                                            (*prev)[i] = core::intrinsics::unchecked_shr(
                                                cache.next[i],
                                                delta_q0[i],
                                            ) + off_prev;
                                            output[i] = core::intrinsics::unchecked_shr(
                                                cache.cache[i],
                                                delta_q0[i],
                                            ) + off_diff;
                                        }
                                    }
                                }
                                cache_next_i[ci] = qi + 1;
                                cache_hit = true;
                                break;
                            }
                        }
                    }

                    if !cache_hit {
                        unsafe {
                            forward_dct(
                                self.worker.shared.delta_prog,
                                true,
                                rescale_prev,
                                rescale_prev_shr,
                                screen.downsample,
                                &self.worker.bufs.prep,
                                ci,
                                hss,
                                vss,
                                output,
                                ypos,
                                xpos,
                                div_parts.get_unchecked(comp.quant_tbl_no as usize),
                                div_shifts,
                                prev,
                                rp_shifts,
                                ptr::null_mut(),
                            );
                        }
                    }

                    xpos += DCTSIZE as u16;
                    blkn += 1;
                }
                ypos += DCTSIZE as u16;
            }

            delta_cache_start += DELTA_Q_CACHE_COUNTS[ci];
        }
    }

    pub fn encode_mcu(&mut self) {
        let mut blkn = 0;

        let is_top = self.worker.info.is_top;
        let screen = is_top_index(is_top).index_into(&self.worker.shared.screens);
        let comp_infos = unsafe { &*screen.comp_infos };

        for ci in 0..MAX_COMPONENTS {
            let comp = &comp_infos.infos[ci];
            let mcu_width = comp.h_samp_factor;
            let mcu_height = comp.v_samp_factor;

            let dc_tbl = if self.worker.shared.delta_prog {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .dq_entropy_tbls
                        .dc_derived_tbls
                        .get_unchecked(comp.dc_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .entropy_tbls
                        .dc_derived_tbls
                        .get_unchecked(comp.dc_tbl_no as usize)
                }
            };
            let ac_tbl = if self.worker.shared.delta_prog {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .dq_entropy_tbls
                        .ac_derived_tbls
                        .get_unchecked(comp.ac_tbl_no as usize)
                }
            } else {
                unsafe {
                    self.worker
                        .shared
                        .jpeg_tbls
                        .entropy_tbls
                        .ac_derived_tbls
                        .get_unchecked(comp.ac_tbl_no as usize)
                }
            };

            for _ in 0..mcu_height {
                for _ in 0..mcu_width {
                    let last_dc_val = self.worker.last_dc_vals[ci];
                    let dst = &mut self.dst;
                    let state = &mut self.worker.huff_state;
                    let block = unsafe { self.worker.bufs.mcu.get_unchecked(blkn) };
                    self.worker.last_dc_vals[ci] =
                        Self::encode_one_block(dst, state, block, last_dc_val, dc_tbl, ac_tbl);

                    blkn += 1;
                }
            }
        }
    }

    pub fn encode_one_block(
        dst: &mut WorkerDst,
        state: &mut HuffState,
        block: &[i16; DCTSIZE2],
        last_dc_val: i16,
        dc_derived_tbl: &DerivedTbl,
        ac_derived_tbl: &DerivedTbl,
    ) -> i16 {
        dst.blkn += 1;

        /* Although it is exceedingly rare, it is possible for a Huffman-encoded
         * coefficient block to be larger than the 128-byte unencoded block.  For each
         * of the 64 coefficients, PUT_BITS is invoked twice, and each invocation can
         * theoretically store 16 bits (for a maximum of 2048 bits or 256 bytes per
         * encoded block.)  If, for instance, one artificially sets the AC
         * coefficients to alternating values of 32767 and -32768 (using the JPEG
         * scanning order-- 1, 8, 16, etc.), then this will produce an encoded block
         * larger than 200 bytes.
         */
        const BUFSIZE: usize = DCTSIZE2 * 8;

        let mut localbuf: [u8; BUFSIZE] = const_default();
        let mut buf = EncodeBuffer::init(state, dst, &mut localbuf, dst.rel_stream);

        let (val1, bits, b0) = {
            let val = block[0] as i32 - last_dc_val as i32;
            let sign1 = val >> (i32::BITS as u8 - 1);
            let val1 = val + sign1;
            let abs = val1 ^ sign1;
            (val1, jpeg_nbits_nonzero(abs) as i32, block[0])
        };

        unsafe {
            buf.put_code(
                *dc_derived_tbl.ehufco.get_unchecked(bits as usize),
                *dc_derived_tbl.ehufsi.get_unchecked(bits as usize),
                val1,
                bits,
            )
        };

        let mut r = 0;

        for jpeg_natural_order_of_k in JPEG_NATURAL_ORDER.into_iter().skip(1) {
            let val = *unsafe { block.get_unchecked(jpeg_natural_order_of_k as usize) } as i32;
            if val == 0 {
                r += 16;
            } else {
                let (val1, bits) = {
                    let sign1 = val >> (core::mem::size_of_val(&val) * 8 - 1);
                    let val1 = val + sign1;
                    let abs = val1 ^ sign1;
                    (val1, jpeg_nbits_nonzero(abs) as i32)
                };

                while r >= 16 * 16 {
                    r -= 16 * 16;
                    unsafe {
                        buf.put_bits(ac_derived_tbl.ehufco[0xf0], ac_derived_tbl.ehufsi[0xf0])
                    };
                }
                r += bits;
                unsafe {
                    buf.put_code(
                        *ac_derived_tbl.ehufco.get_unchecked(r as usize),
                        *ac_derived_tbl.ehufsi.get_unchecked(r as usize),
                        val1,
                        bits,
                    )
                };
                r = 0;
            }
        }

        if r > 0 {
            unsafe { buf.put_bits(ac_derived_tbl.ehufco[0], ac_derived_tbl.ehufsi[0]) };
        }

        buf.store();

        b0
    }
}
