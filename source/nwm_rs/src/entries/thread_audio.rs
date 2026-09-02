use crate::*;

// NTR-HR+ game-audio capture (new-3ds only): read the dsp final mix through
// the physical+0x80000000 mirror and stream it as audio packets via rp_output.
// note: a thread entry must end via svcExitThread, the initial lr is null.

const DSP_REGION0: u32 = 0x1FF50000 + 0x80000000; // final-mix region 0 (mirror VA)
const DSP_REGION1: u32 = 0x1FF70000 + 0x80000000; // final-mix region 1 (mirror VA)
const DSP_FINAL_SAMPLES_OFF: u32 = 0xA80; // (0x8540 - 0x8000) DSP words * 2
const DSP_FRAME_COUNTER_OFF: u32 = 0x7FFE; // last u16 of the 0x8000-byte region

const AUDIO_SAMPLE_RATE: u32 = 32728; // dsp final-mix rate
const AUDIO_FRAME_SAMPLES: u32 = 160; // samples per dsp mix frame
const AUDIO_FRAME_BYTES: usize = 640; // 160 samples * 2 ch * 2 bytes (s16 LE)
const AUDIO_HDR_TYPE: u8 = 4; // hdr[2]: NTR-HR+ audio packet type
const AUDIO_FMT_PCM16: u8 = 0; // hdr[3]: payload format/version

// each packet carries the previous and current frame so one lost packet
// leaves no gap; hdr[0] is the newest frame's sequence number
const AUDIO_REDUNDANCY: usize = 2;
const AUDIO_PAYLOAD_BYTES: usize = AUDIO_REDUNDANCY * AUDIO_FRAME_BYTES;

// poll well under the ~4.888 ms dsp frame so no mix frame is missed
const AUDIO_POLL_NS: s64 = 1_500_000;

// bytes/s this stream adds to the radio; reserved from the video pacer qos so
// the viewer's bandwidth limit governs combined a/v instead of video alone
pub const AUDIO_QOS_BUDGET: u32 =
    (AUDIO_SAMPLE_RATE / AUDIO_FRAME_SAMPLES) * (DATA_HDR_SIZE as u32 + AUDIO_PAYLOAD_BYTES as u32);

// fresher region by frame_counter (wrap-safe)
fn newer_region(fc0: u16, fc1: u16) -> u32 {
    if fc1 != fc0 && fc1.wrapping_sub(fc0) < 0x8000 {
        DSP_REGION1
    } else {
        DSP_REGION0
    }
}

// rp_output needs NWM_HDR_SIZE bytes of headroom before packet_buf
const BUF_SIZE: usize = NWM_HDR_SIZE as usize + DATA_HDR_SIZE as usize + AUDIO_PAYLOAD_BYTES;

// allocated once: the bump pool has no free, but the thread entry reruns on
// every session restart, so allocating there leaks BUF_SIZE per reconnect
pub static mut AUDIO_BUF: *mut u8 = ptr::null_mut();

pub fn once_audio() {
    if let Some(b) = request_mem_from_pool::<BUF_SIZE>() {
        let buf = b.to_ptr() as *mut u8;
        let packet_buf = unsafe { buf.add(NWM_HDR_SIZE as usize) };
        unsafe {
            *packet_buf.add(1) = 0; // hdr[1]: flags (reserved)
            *packet_buf.add(2) = AUDIO_HDR_TYPE; // hdr[2]: audio type
            *packet_buf.add(3) = AUDIO_FMT_PCM16; // hdr[3]: format/version
        }
        unsafe { AUDIO_BUF = buf };
    }
}

pub extern "C" fn thread_audio(_: *mut c_void) {
    unsafe {
        __system_initSyscalls();
    }

    let buf = unsafe { AUDIO_BUF };
    if !buf.is_null() {
        let packet_buf = unsafe { buf.add(NWM_HDR_SIZE as usize) };
        // zeroed so the first packet's older slot is silence
        let payload = unsafe { packet_buf.add(DATA_HDR_SIZE as usize) };
        unsafe { ptr::write_bytes(payload, 0, AUDIO_PAYLOAD_BYTES) };
        let newest = unsafe { payload.add((AUDIO_REDUNDANCY - 1) * AUDIO_FRAME_BYTES) };

        let mut seq: u8 = 0;
        let mut last_fc: u16 = 0;
        let mut have_last = false;

        while !reset_threads() {
            let fc0 =
                unsafe { ptr::read_volatile((DSP_REGION0 + DSP_FRAME_COUNTER_OFF) as *const u16) };
            let fc1 =
                unsafe { ptr::read_volatile((DSP_REGION1 + DSP_FRAME_COUNTER_OFF) as *const u16) };
            let src = newer_region(fc0, fc1);
            let fc = if src == DSP_REGION1 { fc1 } else { fc0 };

            // skip if the mix hasn't advanced
            if have_last && fc == last_fc {
                unsafe { svcSleepThread(AUDIO_POLL_NS) };
                continue;
            }
            last_fc = fc;
            have_last = true;

            unsafe {
                // drop the oldest frame, read the new one into the last slot
                ptr::copy(
                    payload.add(AUDIO_FRAME_BYTES),
                    payload,
                    (AUDIO_REDUNDANCY - 1) * AUDIO_FRAME_BYTES,
                );
                ptr::copy_nonoverlapping(
                    (src + DSP_FINAL_SAMPLES_OFF) as *const u8,
                    newest,
                    AUDIO_FRAME_BYTES,
                );
                *packet_buf.add(0) = seq; // hdr[0]: newest frame's sequence number
                // sending before nwmSendPacket is set would be a null jump;
                // and stop once the session is dead so we don't blast forever
                if entries::thread_nwm::nwm_send_ready()
                    && entries::thread_nwm::nwm_session_alive()
                {
                    let _ = entries::thread_nwm::rp_output(
                        packet_buf,
                        DATA_HDR_SIZE as usize + AUDIO_PAYLOAD_BYTES,
                    );
                }
            }
            seq = seq.wrapping_add(1);

            unsafe { svcSleepThread(AUDIO_POLL_NS) };
        }
    }

    unsafe { svcExitThread() }
}
