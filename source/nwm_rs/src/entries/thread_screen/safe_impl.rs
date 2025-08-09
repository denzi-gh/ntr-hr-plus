use super::*;

fn do_screen(screen: Screen) -> Option<bool> {
    let conf = screen.config();

    let mut is_top = conf.priority_is_top;
    let mut busy_wait = false;
    let last_timing = get_system_tick();

    loop {
        if reset_threads() {
            return None;
        }

        if port_game_pid() == 0 {
            if conf.priority_factor != 0 {
                let frame_count = conf.frame_counts.get_mut(&is_top_index(is_top));

                if *frame_count >= conf.priority_factor {
                    *frame_count -= conf.priority_factor;
                    is_top = !is_top;
                } else {
                    *frame_count += 1;
                }
            }
            busy_wait = true;
            break;
        }

        if (get_system_tick().get() - last_timing.get()) as u32 >= conf.frame_timing_allowance {
            busy_wait = true;
            set_no_skip_frame(is_top);
            break;
        }

        if conf.priority_factor == 0 {
            if screen.screen_port_sync(is_top, true) {
                break;
            }
            continue;
        }

        let get_prio_scaled = |s| -> u32 {
            if s == conf.priority_is_top {
                1 << SCALE_BITS
            } else {
                conf.priority_factor_scaled
            }
        };

        let prio = [get_prio_scaled(true), get_prio_scaled(false)];

        let get_factor = |b| -> u32 {
            unchecked_div(
                (1 << SCALE_BITS) as u64 * *conf.frame_queues.get(&is_top_index(b)) as u64,
                prio[b as usize] as u64,
            ) as u32
        };
        let factor = [get_factor(true), get_factor(false)];

        if factor[true as usize] < (1 << SCALE_BITS) && factor[false as usize] < (1 << SCALE_BITS) {
            *conf.frame_queues.get_mut(&is_top_index(true)) += conf.priority_factor_scaled;
            *conf.frame_queues.get_mut(&is_top_index(false)) += conf.priority_factor_scaled;
        }

        is_top = if factor[is_top as usize] >= factor[!is_top as usize] {
            is_top
        } else {
            !is_top
        };

        let s = is_top;
        let mut try_dequeue = |b| -> bool {
            let frame_queue = conf.frame_queues.get_mut(&is_top_index(b));
            if *frame_queue >= prio[b as usize] {
                if screen.screen_port_sync(b, false) {
                    is_top = b;
                    *frame_queue -= prio[b as usize];
                    return true;
                }
            }
            false
        };

        if try_dequeue(s) {
            break;
        }

        if try_dequeue(!s) {
            break;
        }

        if let Some(s) = screen.screens_ports_sync() {
            is_top = s;
            let frame_queue = conf.frame_queues.get_mut(&is_top_index(is_top));
            if *frame_queue >= prio[is_top as usize] {
                *frame_queue -= prio[is_top as usize];
            } else {
                *frame_queue = 0;
            }
            break;
        }
    }

    if busy_wait {
        wait_for_vblank(is_top)
    }

    let screen_info = update_gpu_regs(is_top);
    if screen_info.fill & (1 << 24) > 0 {
        close_handles();
        return Some(false);
    }

    return Some(try_capture_screen(is_top, &screen_info));
}

pub fn thread_screen(impl_: Impl) -> Option<()> {
    let screen_ready = impl_.screen_ready_acquire()?;
    let work_done = screen_ready.work_done_acquire()?;

    loop {
        if do_screen(work_done.do_screen())? {
            break Some(());
        }
    }
}
