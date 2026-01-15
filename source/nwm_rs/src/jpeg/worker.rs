// Contains code modified from github.com/libjpeg-turbo/libjpeg-turbo for use in NTR-HR
// See LICENSE-libjpeg-turbo.md at the project root for license details

use super::*;

pub type BitBufType = u32;

#[derive(ConstDefault)]
pub struct HuffState {
    pub c: BitBufType,
    pub free_bits: isize,
}

pub const BIT_BUF_SIZE: usize = mem::size_of::<BitBufType>() * 8;

#[derive(ConstDefault)]
pub struct JpegWorker<'a> {
    pub shared: &'a JpegShared,
    #[cfg(not(feature = "o3ds"))]
    pub shared_mut: JpegSharedMutCell,
    pub bufs: &'a mut WorkerBufs,
    pub info: &'a CInfo,
    #[cfg(not(feature = "mem3"))]
    pub thread_index: ThreadIndex,
    pub huff_state: HuffState,
    pub last_dc_vals: LastDcVals,
}

pub type LastDcVals = [s16; MAX_COMPONENTS];

#[cfg(not(feature = "o3ds"))]
pub struct JpegSharedMutCell {
    pub cell: *mut JpegSharedMut,
}

#[cfg(not(feature = "o3ds"))]
impl<'a> JpegWorker<'a> {
    pub fn encode<F>(&'a mut self, dst: WorkerDst, src: &[u8], pre_progress: F) -> Option<JpegDqRet>
    where
        F: FnMut(),
    {
        JpegEncode { worker: self, dst }.encode::<_, F>(src, pre_progress)
    }
}

#[cfg(all(feature = "o3ds", not(feature = "mem3")))]
impl<'a> JpegWorker<'a> {
    pub fn encode(
        &'a mut self,
        dst: WorkerDst,
        #[cfg(not(feature = "mem3"))] src: &[u8],
    ) -> Option<JpegDqRet> {
        JpegEncode { worker: self, dst }.encode::<fn() -> (), fn() -> ()>(src)
    }
}

#[cfg(all(feature = "o3ds", feature = "mem3"))]
impl<'a> JpegWorker<'a> {
    pub fn encode<G>(
        &'a mut self,
        dst: WorkerDst,
        #[cfg(feature = "mem3")] src: *const u8,
        #[cfg(feature = "mem3")] pitch: u32,
        progress: G,
    ) -> Option<JpegDqRet>
    where
        G: FnMut(),
    {
        JpegEncode { worker: self, dst }.encode::<G, _>(src, pitch, progress)
    }
}

#[derive(ConstDefault, Clone, Copy)]
pub struct CInfo {
    pub is_top: bool,
    pub color_space: ColorSpace,
    #[cfg(not(feature = "mem3"))]
    pub restart_interval: u16,
    pub work_index: WorkIndex,
    #[cfg(not(feature = "o3ds"))]
    pub core_count: CoreCount,
    pub even_odd: bool,
}

impl Jpeg {
    pub fn set_info(&mut self, info: CInfo) {
        *info.work_index.index_into_mut(&mut self.info) = info;
    }

    pub unsafe fn get_worker<'a>(
        &'a mut self,
        work_index: WorkIndex,
        thread_index: ThreadIndex,
    ) -> JpegWorker<'a> {
        JpegWorker {
            shared: &self.shared,
            #[cfg(not(feature = "o3ds"))]
            shared_mut: JpegSharedMutCell {
                cell: &mut self.shared_mut,
            },
            bufs: thread_index.index_into_mut(&mut self.bufs),
            info: work_index.index_into_mut(&mut self.info),
            #[cfg(not(feature = "mem3"))]
            thread_index,
            huff_state: const_default(),
            last_dc_vals: const_default(),
        }
    }

    pub fn worker_dst(
        &self,
        #[cfg(not(feature = "o3ds"))] s: ScreenIndex,
        #[cfg(not(feature = "o3ds"))] w: WorkIndex,
        dst: *mut u8,
        #[cfg(not(feature = "o3ds"))] user: WorkderDstUser,
    ) -> WorkerDst {
        WorkerDst {
            blkn: 0,
            #[cfg(not(feature = "o3ds"))]
            s,
            #[cfg(not(feature = "o3ds"))]
            w,
            dst: dst as *mut u8,
            #[cfg(not(feature = "o3ds"))]
            free_in_bytes: entries::thread_nwm::get_packet_data_size() as u16,
            #[cfg(feature = "o3ds")]
            free_in_bytes: RP_CB_PACKET_SIZE as u16,
            #[cfg(not(feature = "o3ds"))]
            user,
            #[cfg(not(feature = "o3ds"))]
            rel_stream: self.shared.rel_stream,
            #[cfg(not(feature = "o3ds"))]
            delta_prog: self.shared.delta_prog,
            #[cfg(not(feature = "o3ds"))]
            even_odd: w.index_into(&self.info).even_odd,
            #[cfg(feature = "o3ds")]
            even_odd: WorkIndex::init(0).index_into(&self.info).even_odd,
        }
    }
}
