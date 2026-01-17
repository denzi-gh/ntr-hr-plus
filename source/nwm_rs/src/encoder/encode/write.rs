// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

impl<'a, 'b> JpegEncode<'a, 'b> {
    pub fn write_headers(&mut self) {
        /* File header */
        self.write_marker(M_SOI);
        self.write_jfif_app0();

        /* Frame header */
        for i in 0..NUM_QUANT_TBLS {
            self.write_dqt(i as usize);
        }
        self.write_sof(M_SOF0);

        /* Scan header */
        for i in 0..NUM_HUFF_TBLS {
            self.write_dht(i as usize, false);
            self.write_dht(i as usize, true);
        }
        #[cfg(not(feature = "o3ds"))]
        if self.worker.shared.core_count.get() > 1 {
            self.write_dri();
        }
        self.write_sos();
    }

    pub fn write_marker(&mut self, mark: u8)
    /* Emit a marker code */
    {
        self.write_byte(0xFF);
        self.write_byte(mark);
    }

    pub fn write_byte(&mut self, value: u8) {
        self.dst.write_byte(value);
    }

    pub fn write_2bytes(&mut self, value: u16)
    /* Emit a 2-byte integer; these are always MSB first in JPEG files */
    {
        self.write_byte(((value >> 8) & 0xFF) as u8);
        self.write_byte((value & 0xFF) as u8);
    }

    pub fn write_jfif_app0(&mut self)
    /* Emit a JFIF-compliant APP0 marker */
    {
        /*
         * Length of APP0 block       (2 bytes)
         * Block ID                   (4 bytes - ASCII "JFIF")
         * Zero byte                  (1 byte to terminate the ID string)
         * Version Major, Minor       (2 bytes - major first)
         * Units                      (1 byte - 0x00 = none, 0x01 = inch, 0x02 = cm)
         * Xdpu                       (2 bytes - dots per unit horizontal)
         * Ydpu                       (2 bytes - dots per unit vertical)
         * Thumbnail X size           (1 byte)
         * Thumbnail Y size           (1 byte)
         */

        self.write_marker(M_APP0);

        self.write_2bytes(2 + 4 + 1 + 2 + 1 + 2 + 2 + 1 + 1); /* length */

        self.write_byte(0x4A); /* Identifier: ASCII "JFIF" */
        self.write_byte(0x46);
        self.write_byte(0x49);
        self.write_byte(0x46);
        self.write_byte(0);
        self.write_byte(1); /* Version fields */
        self.write_byte(1);
        self.write_byte(0); /* Pixel size information */
        self.write_2bytes(1);
        self.write_2bytes(1);
        self.write_byte(0); /* No thumbnail image */
        self.write_byte(0);
    }

    pub fn write_dqt(&mut self, index: usize)
    /* Emit a DQT marker */
    /* Returns the precision used (0 = 8bits, 1 = 16bits) for baseline checking */
    {
        let s = is_top_index(self.worker.info.is_top);
        let screen = s.index_into(&self.worker.jpeg_shared.screens);
        let qtbl = &screen.quant_tbls.quant_tbls[index];

        self.write_marker(M_DQT);
        self.write_2bytes((DCTSIZE2 + 1 + 2) as u16);
        self.write_byte(index as u8);
        for i in 0..DCTSIZE2 {
            /* The table entries must be emitted in zigzag order. */
            let qval =
                *unsafe { qtbl.quant_val.get_unchecked(JPEG_NATURAL_ORDER[i] as usize) } as u8;
            self.write_byte(qval);
        }
    }

    pub fn write_dht(&mut self, mut index: usize, is_ac: bool) {
        let tbl = if is_ac {
            &self.worker.jpeg_shared.jpeg_tbls.huff_tbls.ac_huff_tbls[index]
        } else {
            &self.worker.jpeg_shared.jpeg_tbls.huff_tbls.dc_huff_tbls[index]
        };
        if is_ac {
            index |= 0x10; /* output index has AC bit set */
        }

        self.write_marker(M_DHT);

        let mut length = 0 as u16;
        for i in 1..=16 as usize {
            length += tbl.bits[i] as u16;
        }

        self.write_2bytes((length + 2 + 1 + 16) as u16);
        self.write_byte(index as u8);

        for i in 1..=16 as usize {
            self.write_byte(tbl.bits[i]);
        }

        for i in 0..length as u8 {
            self.write_byte(tbl.huff_vals[i as usize]);
        }
    }

    #[cfg(not(feature = "o3ds"))]
    pub fn write_dri(&mut self) {
        self.write_marker(M_DRI);
        self.write_2bytes(4); /* fixed length */
        self.write_2bytes(self.worker.info.restart_interval);
    }

    pub fn write_sos(&mut self) {
        self.write_marker(M_SOS);

        self.write_2bytes((2 * MAX_COMPONENTS + 2 + 1 + 3) as u16); /* length */

        self.write_byte(MAX_COMPONENTS as u8);

        let infos = unsafe {
            &(*is_top_index(self.worker.info.is_top)
                .index_into(&self.worker.jpeg_shared.screens)
                .comp_infos)
                .infos
        };
        for i in 0..MAX_COMPONENTS {
            let comp = &infos[i];
            self.write_byte(comp.component_id);

            /* We emit 0 for unused field(s); this is recommended by the P&M text
             * but does not seem to be specified in the standard.
             */

            /* DC needs no table for refinement scan */
            let td = comp.dc_tbl_no;
            /* AC needs no table when not present */
            let ta = comp.ac_tbl_no;

            self.write_byte((td << 4) + ta);
        }

        self.write_byte(0);
        self.write_byte((DCTSIZE2 - 1) as u8);
        self.write_byte(0);
    }

    pub fn write_sof(&mut self, code: u8) {
        self.write_marker(code);

        self.write_2bytes((3 * MAX_COMPONENTS + 2 + 5 + 1) as u16); /* length */

        self.write_byte(8);

        let s = is_top_index(self.worker.info.is_top).index_into(&self.worker.shared.screens);
        self.write_2bytes(s.height);
        self.write_2bytes(s.width);

        self.write_byte(MAX_COMPONENTS as u8);

        for info in unsafe {
            &(*is_top_index(self.worker.info.is_top)
                .index_into(&self.worker.jpeg_shared.screens)
                .comp_infos)
                .infos
        } {
            self.write_byte(info.component_id);
            self.write_byte((info.h_samp_factor << 4) + info.v_samp_factor);
            self.write_byte(info.quant_tbl_no);
        }
    }

    #[cfg(not(feature = "o3ds"))]
    pub fn write_rst(&mut self) {
        self.write_marker(M_RST0 + self.worker.thread_index.get() as u8);
    }

    pub fn write_trailer(&mut self) {
        self.write_marker(M_EOI);
    }

    pub fn write_term(&mut self) {
        self.dst.term();
    }

    pub fn reset_mcu(&mut self) {
        self.worker.huff_state = const_default();
        self.worker.huff_state.free_bits = BIT_BUF_SIZE as isize;
        self.worker.last_dc_vals = const_default();
    }

    pub fn flush_mcu(&mut self) {
        let mut put_bits = BIT_BUF_SIZE as isize - self.worker.huff_state.free_bits;

        let mut localbuf: [u8; mem::size_of::<BitBufType>() * 4] = const_default();
        let put_buffer = self.worker.huff_state.c;
        let mut buf = EncodeBuffer::<_>::init(
            &mut self.worker.huff_state,
            &mut self.dst,
            &mut localbuf,
            #[cfg(not(feature = "o3ds"))]
            self.worker.shared.rel_stream,
        );

        while put_bits >= 8 {
            put_bits -= 8;
            let temp = unsafe { core::intrinsics::unchecked_shr(put_buffer, put_bits) };
            unsafe { buf.emit_byte(temp as u8) }
        }
        if put_bits > 0 {
            /* fill partial byte with ones */
            let temp = (put_buffer << (8 - put_bits))
                | unsafe { core::intrinsics::unchecked_shr(0xFF, put_bits) };
            unsafe { buf.emit_byte(temp as u8) }
        }

        buf.store();
    }
}
