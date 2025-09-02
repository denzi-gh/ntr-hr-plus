// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

pub enum EncodeBufferBase<'a, const N: usize> {
    Local(&'a [u8; N]),
    Dst,
}
pub struct EncodeBuffer<'a, 'b, 'c, const N: usize> {
    pub buf: *mut u8,
    pub base: EncodeBufferBase<'a, N>,
    pub state: &'b mut HuffState,
    pub dst: &'c mut WorkerDst,
    #[cfg(not(feature = "o3ds"))]
    pub rel_stream: bool,
}

impl<'a, 'b, 'c, const N: usize> EncodeBuffer<'a, 'b, 'c, N>
where
    'a: 'c,
{
    pub fn init<'d: 'a>(
        state: &'b mut HuffState,
        dst: &'a mut WorkerDst,
        buf: &'d mut [u8; N],
        #[cfg(not(feature = "o3ds"))] rel_stream: bool,
    ) -> Self {
        if dst.free_in_bytes < N as u16 {
            EncodeBuffer {
                buf: buf.as_mut_ptr(),
                base: EncodeBufferBase::Local(buf),
                state,
                dst,
                #[cfg(not(feature = "o3ds"))]
                rel_stream,
            }
        } else {
            EncodeBuffer {
                buf: dst.dst,
                base: EncodeBufferBase::Dst,
                state,
                dst,
                #[cfg(not(feature = "o3ds"))]
                rel_stream,
            }
        }
    }

    pub fn store(self) {
        match self.base {
            EncodeBufferBase::Local(buf) => {
                let len = unsafe { self.buf.offset_from_unsigned(buf.as_ptr()) };
                self.dst
                    .write_bytes(unsafe { slice::from_raw_parts(buf.as_ptr(), len) });
            }
            EncodeBufferBase::Dst => unsafe { self.dst.advance_to(self.buf) },
        }
    }

    pub unsafe fn emit_byte(&mut self, b: u8) {
        unsafe {
            #[cfg(not(feature = "o3ds"))]
            let rel_stream = self.rel_stream;
            #[cfg(feature = "o3ds")]
            let rel_stream = false;

            if rel_stream {
                *self.buf = b;
                self.buf = self.buf.add(1);
            } else {
                *self.buf = b;
                *(self.buf.add(1)) = 0;
                self.buf = self.buf.add(2 - (b < 0xFF) as usize);
            }
        }
    }

    unsafe fn flush(&mut self) {
        unsafe {
            #[cfg(not(feature = "o3ds"))]
            let rel_stream = self.rel_stream;
            #[cfg(feature = "o3ds")]
            let rel_stream = false;

            if !rel_stream && (self.state.c & 0x80808080 & !(self.state.c + 0x01010101) > 0) {
                self.emit_byte((self.state.c >> 24) as u8);
                self.emit_byte((self.state.c >> 16) as u8);
                self.emit_byte((self.state.c >> 8) as u8);
                self.emit_byte(self.state.c as u8);
            } else {
                *self.buf = (self.state.c >> 24) as u8;
                *self.buf.add(1) = (self.state.c >> 16) as u8;
                *self.buf.add(2) = (self.state.c >> 8) as u8;
                *self.buf.add(3) = (self.state.c) as u8;
                self.buf = self.buf.add(4);
            }
        }
    }

    unsafe fn put_and_flush(&mut self, code: u32, size: u8) {
        self.state.c = unsafe {
            core::intrinsics::unchecked_shl(self.state.c, size as isize + self.state.free_bits)
                | core::intrinsics::unchecked_shr(code, -self.state.free_bits)
        };
        unsafe {
            self.flush();
        }
        self.state.free_bits += BIT_BUF_SIZE as isize;
        self.state.c = code;
    }

    pub unsafe fn put_bits(&mut self, code: u32, size: u8) {
        self.state.free_bits -= size as isize;
        if self.state.free_bits < 0 {
            unsafe {
                self.put_and_flush(code, size);
            }
        } else {
            self.state.c = unsafe { core::intrinsics::unchecked_shl(self.state.c, size) } | code;
        }
    }

    pub unsafe fn put_code(&mut self, code: u32, size: u8, mut temp: i32, mut nbits: i32) {
        temp &= unsafe { core::intrinsics::unchecked_shl(1, nbits) } - 1;
        temp |= unsafe { core::intrinsics::unchecked_shl(code as i32, nbits) };
        nbits += size as i32;
        unsafe {
            self.put_bits(temp as u32, nbits as u8);
        }
    }
}
