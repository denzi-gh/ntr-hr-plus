// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

#[derive(Copy, Clone)]
pub enum WorkderDstUser {
    NoneInfo(*const entries::thread_nwm::NwmThreadInfo),
    KcpHdr(ArqRpHdr),
}

#[derive(Clone, ConstDefault)]
pub struct WorkerDst {
    pub blkn: u16,
    pub s: ScreenIndex,
    pub w: WorkIndex,
    pub dst: *mut u8,
    pub free_in_bytes: u16,
    pub user: WorkderDstUser,
}

impl WorkerDst {
    pub fn write_byte<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self, byte: u8) -> bool {
        if self.free_in_bytes == 0 {
            if !self.flush::<REL_STREAM, DELTA_Q>() {
                return false;
            }
        }
        unsafe { *self.dst = byte };
        self.dst = unsafe { self.dst.add(1) };
        self.free_in_bytes -= 1;

        true
    }

    pub fn write_bytes<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self, bytes: &[u8]) -> bool {
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
                if !self.flush::<REL_STREAM, DELTA_Q>() {
                    return false;
                }
            }
        } else {
            if !self.flush::<REL_STREAM, DELTA_Q>() {
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

    fn flush<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self) -> bool {
        unsafe {
            self.dq_update_size::<DELTA_Q>(entries::thread_nwm::PACKET_DATA_SIZE_KCP as u32);
            self.blkn = 0;
            entries::thread_nwm::rp_send_buffer::<REL_STREAM>(self, false)
        }
    }

    pub fn term<const REL_STREAM: bool, const DELTA_Q: bool>(&mut self) -> bool {
        unsafe {
            self.dq_update_size::<DELTA_Q>(
                entries::thread_nwm::PACKET_DATA_SIZE_KCP as u32 - self.free_in_bytes as u32,
            );
            self.blkn = 0;
            entries::thread_nwm::rp_send_buffer::<REL_STREAM>(self, true)
        }
    }

    pub unsafe fn advance_to(&mut self, dst: *mut u8) {
        self.free_in_bytes -= unsafe { dst.offset_from_unsigned(self.dst) } as u16;
        self.dst = dst;
    }

    fn dq_update_size<const DLETA_Q: bool>(&mut self, size: u32) {
        if DLETA_Q {
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
