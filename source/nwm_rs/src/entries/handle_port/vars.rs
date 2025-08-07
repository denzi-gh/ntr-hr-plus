use super::*;

#[no_mangle]
extern "C" fn handlePortCmd(
    cmd_id: u32,
    norm_params_count: u32,
    trans_params_size: u32,
    cmd_buf1: *const u32,
) {
}
