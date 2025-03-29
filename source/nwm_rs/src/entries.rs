mod handle_port;
mod start_up;
mod thread_aux;
mod thread_main;
mod thread_nwm;
mod thread_screen;
mod work_thread;

pub use thread_nwm::{
    get_min_send_interval_ns, get_min_send_interval_tick, get_next_send_tick,
    get_reliable_stream_delta_prog, get_reliable_stream_method, nwm_is_waiting,
    packet_data_size_kcp, rp_delta_q_qos, rp_dq_update_size, rp_send_buffer, NwmInfo,
    ReliableStreamMethod,
};
// pub use thread_screen::{get_no_skip_frame, FRAME_TIMING_FACTOR_DQ};
pub use work_thread::{get_frame_time, reset_threads, set_reset_threads_ar};
