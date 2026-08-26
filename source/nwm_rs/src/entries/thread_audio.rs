use crate::*;

// NTR-HR+ game-audio capture (new-3ds only): read the dsp final mix through
// the physical+0x80000000 mirror and stream it as audio packets via rp_output.
// note: a thread entry must end via svcExitThread, the initial lr is null.

const DSP_REGION0: u32 = 0x1FF50000 + 0x80000000; // final-mix region 0 (mirror VA)
const DSP_REGION1: u32 = 0x1FF70000 + 0x80000000; // final-mix region 1 (mirror VA)
const DSP_FINAL_SAMPLES_OFF: u32 = 0xA80; // (0x8540 - 0x8000) DSP words * 2
const DSP_FRAME_COUNTER_OFF: u32 = 0x7FFE; // last u16 of the 0x8000-byte region

const AUDIO_FRAME_BYTES: usize = 640; // 160 samples * 2 ch * 2 bytes (s16 LE)
const AUDIO_HDR_TYPE: u8 = 4; // hdr[2]: NTR-HR+ audio packet type
const AUDIO_FMT_PCM16: u8 = 0; // hdr[3]: payload format/version

// poll at half the ~4.888 ms dsp frame, de-dup on frame_counter
const AUDIO_POLL_NS: s64 = 2_444_000;

// fresher region by frame_counter (wrap-safe)
fn newer_region(fc0: u16, fc1: u16) -> u32 {
    if fc1 != fc0 && fc1.wrapping_sub(fc0) < 0x8000 {
        DSP_REGION1
    } else {
        DSP_REGION0
    }
}

pub extern "C" fn thread_audio(_: *mut c_void) {
    unsafe {
        __system_initSyscalls();
    }

    // rp_output needs NWM_HDR_SIZE bytes of headroom before packet_buf
    const BUF_SIZE: usize = NWM_HDR_SIZE as usize + DATA_HDR_SIZE as usize + AUDIO_FRAME_BYTES;
    if let Some(b) = request_mem_from_pool::<BUF_SIZE>() {
        let buf = b.to_ptr() as *mut u8;
        let packet_buf = unsafe { buf.add(NWM_HDR_SIZE as usize) };
        unsafe {
            *packet_buf.add(1) = 0; // hdr[1]: flags (reserved)
            *packet_buf.add(2) = AUDIO_HDR_TYPE; // hdr[2]: audio type
            *packet_buf.add(3) = AUDIO_FMT_PCM16; // hdr[3]: format/version
        }
        let pcm = unsafe { packet_buf.add(DATA_HDR_SIZE as usize) };

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
                *packet_buf.add(0) = seq; // hdr[0]: sequence number
                ptr::copy_nonoverlapping(
                    (src + DSP_FINAL_SAMPLES_OFF) as *const u8,
                    pcm,
                    AUDIO_FRAME_BYTES,
                );
                // sending before nwmSendPacket is set would be a null jump
                if entries::thread_nwm::nwm_send_ready() {
                    let _ = entries::thread_nwm::rp_output(
                        packet_buf,
                        DATA_HDR_SIZE as usize + AUDIO_FRAME_BYTES,
                    );
                }
            }
            seq = seq.wrapping_add(1);

            unsafe { svcSleepThread(AUDIO_POLL_NS) };
        }
    }

    unsafe { svcExitThread() }
}
