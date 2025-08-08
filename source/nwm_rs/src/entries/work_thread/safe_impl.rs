use super::*;

pub fn send_frame(impl_: Impl) -> Option<()> {
    let work = match impl_.work_ready_acquire() {
        Ok(work) => {
            let mut work = work;
            loop {
                let blit_init = work.init_bctx();
                let sync = blit_init.sync();
                work = match sync.skip_frame() {
                    Ok(frame) => break frame.frame_release(),
                    Err(skip_frame) => skip_frame.skip_frame_release()?,
                };
            }
        }
        Err(work_other) => work_other.acquire(),
    }?;
    None
}
