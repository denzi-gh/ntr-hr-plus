use super::*;

pub fn start_up(impl_: Impl, nwm_hdr: NwmHdr) {
    let protocol = nwm_hdr.protocol();
    let src_port = nwm_hdr.src_port();
    let dst_port = nwm_hdr.dst_port();

    let tcp_hit = protocol == 0x6 && src_port == utils::htons(NS_MENU_LISTEN_PORT as u16);
    let udp_hit = protocol == 0x11
        && src_port == utils::htons(NWM_INIT_SRC_PORT as u16)
        && dst_port == utils::htons(NWM_INIT_DST_PORT as u16);

    if tcp_hit || udp_hit {
        let saddr = nwm_hdr.src_addr();
        let daddr = nwm_hdr.dst_addr();

        if !impl_.inited() {
            impl_.set_nwm_hdr(&nwm_hdr);
            RP_CONFIG.dst_addr().store(daddr, Ordering::Release);
            impl_.set_remote_dst_addr(daddr);
            *impl_.src_addr() = saddr;

            impl_.init_main_thread();
        } else {
            let rp_config_daddr = RP_CONFIG.dst_addr();
            let rp_config_daddr_val = rp_config_daddr.load(Ordering::Acquire);
            let need_update = if ((tcp_hit && rp_config_daddr_val == 0) || udp_hit)
                && rp_config_daddr_val != daddr
            {
                rp_config_daddr.store(daddr, Ordering::Release);
                impl_.set_remote_dst_addr(daddr);
                true
            } else {
                false
            };
            let cached_saddr = impl_.src_addr();
            let need_update = if *cached_saddr != saddr {
                *cached_saddr = saddr;
                true
            } else {
                false
            } || need_update;

            if need_update {
                impl_.set_nwm_hdr(&nwm_hdr);
            }
        }
    }
}
