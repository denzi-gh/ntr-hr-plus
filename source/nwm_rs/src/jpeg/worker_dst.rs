// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

#[derive(Copy, Clone)]
pub union WorkderDstUser {
    pub none_info: *const entries::thread_nwm::NwmThreadInfo,
    pub kcp_hdr: ArqRpHdr,
}

#[derive(Clone, ConstDefault)]
pub struct WorkerDst {
    pub blkn: u16,
    pub s: ScreenIndex,
    pub w: WorkIndex,
    pub dst: *mut u8,
    pub free_in_bytes: u16,
    pub user: WorkderDstUser,
    pub rel_stream: bool,
    pub delta_prog: bool,
}

impl WorkerDst {
    pub fn init(s: ScreenIndex, w: WorkIndex, dst: *mut u8, user: WorkderDstUser) -> Self {
        Self {
            blkn: 0,
            s,
            w,
            dst: dst as *mut u8,
            free_in_bytes: entries::thread_nwm::get_packet_data_size() as u16,
            user,
            rel_stream: entries::thread_nwm::get_reliable_stream()
                != entries::thread_nwm::ReliableStream::None,
            delta_prog: entries::thread_nwm::get_reliable_stream_delta_prog(),
        }
    }

    pub fn write_byte(&mut self, byte: u8) -> bool {
        if self.free_in_bytes == 0 {
            if !self.flush() {
                return false;
            }
        }
        unsafe { *self.dst = byte };
        self.dst = unsafe { self.dst.add(1) };
        self.free_in_bytes -= 1;

        true
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> bool {
        let mut src = bytes.as_ptr();
        let mut len = bytes.len() as u16;

        if self.free_in_bytes > 0 {
            if self.free_in_bytes < len {
                unsafe {
                    ptr::copy_nonoverlapping(src, self.dst, self.free_in_bytes as usize);
                }
                len -= self.free_in_bytes;
                src = unsafe { src.add(self.free_in_bytes as usize) };
                unsafe { self.dst = self.dst.add(self.free_in_bytes as usize) };
                self.free_in_bytes = 0;
                if !self.flush() {
                    return false;
                }
            }
        } else {
            if !self.flush() {
                return false;
            }
        }

        unsafe {
            ptr::copy_nonoverlapping(src, self.dst, len as usize);
        }

        self.free_in_bytes -= len;
        self.dst = unsafe { self.dst.add(len as usize) };

        true
    }

    fn flush(&mut self) -> bool {
        unsafe {
            self.dq_update_size(entries::thread_nwm::PACKET_DATA_SIZE_KCP as u32);
            self.blkn = 0;
            entries::thread_nwm::rp_send_buffer(self, false, self.rel_stream)
        }
    }

    pub fn term(&mut self) -> bool {
        unsafe {
            self.dq_update_size(
                entries::thread_nwm::PACKET_DATA_SIZE_KCP as u32 - self.free_in_bytes as u32,
            );
            self.blkn = 0;
            entries::thread_nwm::rp_send_buffer(self, true, self.rel_stream)
        }
    }

    pub unsafe fn advance_to(&mut self, dst: *mut u8) {
        self.free_in_bytes -= unsafe { dst.offset_from_unsigned(self.dst) } as u16;
        self.dst = dst;
    }

    fn dq_update_size(&mut self, size: u32) {
        if self.delta_prog {
            let comp_size = unsafe { (*JPEG).shared_mut.compressed_size.get_mut(&self.s) };
            entries::thread_nwm::rp_dq_update_size(comp_size, size, self.blkn)
        } else {
            entries::thread_nwm::rp_update_size(self.w, size)
        }
    }
}

#[derive(Copy, Clone, ConstDefault)]
pub struct ArqRpHdr {
    pub w: WorkIndex,
    pub t: ThreadIndex,
}

const _ARQ_RP_HDR_SIZE_ASSERT: () = {
    assert!(mem::size_of::<u16>() == ARQ_DATA_HDR_SIZE as usize);
};

const _ARQ_RP_W_SIZE_ASSERT: () = {
    assert!(WORK_COUNT - 1 <= ((1 << entries::work_thread::RP_KCP_HDR_W_NBITS) - 1));
};

// We store RP_CORE_COUNT_MAX as a special value to indicate term packet
const _ARQ_RP_T_SIZE_ASSERT: () = {
    assert!(RP_CORE_COUNT_MAX <= ((1 << entries::work_thread::RP_KCP_HDR_T_NBITS) - 1));
};

impl ArqRpHdr {
    pub unsafe fn write_hdr(&self, dst: *mut u8) {
        let hdr = (self.w.get() as u16) << (PID_NBITS + CID_NBITS)
            | (self.t.get() as u16)
                << (PID_NBITS + CID_NBITS + entries::work_thread::RP_KCP_HDR_W_NBITS);
        unsafe {
            ptr::copy_nonoverlapping(&hdr, dst as *mut _, 1);
        }
    }
}
