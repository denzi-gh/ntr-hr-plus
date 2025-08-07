use super::*;

#[export_name = "rp_max_qos"]
static mut max_qos: u32 = const_default();

#[export_name = "rp_current_qos"]
static mut current_qos: u32 = const_default();

#[no_mangle]
extern "C" fn rp_set_qos(qos: u32) {}

#[no_mangle]
extern "C" fn rp_udp_output(buf: *mut u8, len: s32, tick: *mut u32, kcp: *mut ikcpcb) -> s32 {
    0
}

#[no_mangle]
extern "C" fn nsControlRecv(fd: c_int) -> c_int {
    0
}

#[no_mangle]
extern "C" fn ikcp_seg_data_buf_malloc() -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
extern "C" fn ikcp_seg_data_buf_free(dst: *const ::libc::c_char) {}

#[no_mangle]
extern "C" fn rp_seg_data_buf_free(data_buf: *const ::libc::c_char) {}

pub extern "C" fn kcp_thread_nwm(_: *mut c_void) {}

pub extern "C" fn thread_nwm(_: *mut c_void) {}
