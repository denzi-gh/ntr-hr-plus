use super::*;

pub struct NwmHdr<'a>(&'a [u8; Self::N]);

impl NwmHdr<'_> {
    pub const N: usize = NWM_HDR_SIZE as usize;

    pub fn protocol(&self) -> u8 {
        self.0[0x17 + 0x8]
    }

    pub fn src_port(&self) -> u16 {
        unsafe { *(&self.0[0x22 + 0x8] as *const u8 as *const u16) }
    }

    pub fn dst_port(&self) -> u16 {
        unsafe { *(&self.0[0x22 + 0xa] as *const u8 as *const u16) }
    }

    pub fn src_addr(&self) -> u32 {
        unsafe { (&self.0[0x1a + 0x8] as *const u8 as *const u32).read_unaligned() }
    }

    pub fn dst_addr(&self) -> u32 {
        unsafe { (&self.0[0x1e + 0x8] as *const u8 as *const u32).read_unaligned() }
    }
}

pub struct Impl(());

static mut INITED: bool = false;
static mut SRC_ADDR: u32 = 0;

impl Impl {
    pub fn init_main_thread(&self) {
        if unsafe { INITED } {
            return;
        }

        // if we fail, do not try to restart to avoid locking things up and leaking memory
        unsafe { INITED = true };

        create_thread_from_pool::<{ RP_THREAD_STACK_SIZE as usize }>(
            unsafe { &mut entries::thread_main::THREAD_MAIN_HANDLE },
            Some(entries::thread_main::encode_thread_main),
            0,
            RP_THREAD_PRIO_DEFAULT as s32,
            RP_CORE_ID_MAIN,
        );
    }

    pub fn inited(&self) -> bool {
        unsafe { INITED }
    }

    pub fn src_addr(&self) -> &mut u32 {
        unsafe { &mut SRC_ADDR }
    }

    pub fn set_nwm_hdr(&self, v: &NwmHdr) {
        unsafe {
            ptr::copy_nonoverlapping(
                v.0.as_ptr(),
                (*entries::thread_nwm::get_current_nwm_hdr()).as_mut_ptr(),
                NwmHdr::N,
            )
        }
    }

    #[named]
    pub fn set_remote_dst_addr(&self, mut v: u32) {
        let res = unsafe {
            copyRemoteMemory(
                HOME_PROCESS_HANDLE,
                ptr::addr_of_mut!((*config_consts::RP_CONFIG).dstAddr) as *mut c_void,
                CUR_PROCESS_HANDLE,
                ptr::addr_of_mut!(v) as *mut c_void,
                mem::size_of::<u32>() as u32,
            ) as i32
        };
        if res != 0 {
            ns_dbg_print!(failed, c_str!("Copy remote memory"), res);
        }
    }

    #[named]
    fn open_home_process() -> bool {
        if unsafe { HOME_PROCESS_HANDLE } != 0 {
            return true;
        }
        let res = unsafe {
            svcOpenProcess(
                &mut HOME_PROCESS_HANDLE,
                (*config_consts::NTR_CONFIG).HomeMenuPid,
            )
        };
        if res != 0 {
            ns_dbg_print!(failed, c_str!("Open process"), res);
            false
        } else {
            true
        }
    }

    fn init() -> Option<Self> {
        if Self::open_home_process() {
            Some(Self(()))
        } else {
            None
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rpStartup(buf: *const u8) {
    if let Some(impl_) = Impl::init() {
        let buf = unsafe { mem::transmute(buf) };
        let buf = NwmHdr(buf);
        safe_impl::start_up(impl_, buf)
    }
}

pub static mut HOME_PROCESS_HANDLE: Handle = const_default();
