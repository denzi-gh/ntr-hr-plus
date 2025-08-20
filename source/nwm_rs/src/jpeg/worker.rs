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
    pub shared_mut: JpegSharedMutCell,
    pub bufs: &'a mut WorkerBufs,
    pub info: &'a CInfo,
    pub thread_index: ThreadIndex,
    pub huff_state: HuffState,
    pub last_dc_vals: LastDcVals,
}

pub type LastDcVals = [s16; MAX_COMPONENTS];

pub struct JpegSharedMutCell {
    pub cell: *mut JpegSharedMut,
}

impl<'a> JpegWorker<'a> {
    pub fn encode<F, G>(
        &'a mut self,
        dst: WorkerDst,
        src: &[u8],
        pre_progress: F,
        progress: G,
    ) -> Option<JpegDqRet>
    where
        F: FnMut(u32),
        G: FnMut(),
    {
        JpegEncode { worker: self, dst }.encode(src, pre_progress, progress)
    }
}

#[derive(ConstDefault, Clone, Copy)]
pub struct CInfo {
    pub is_top: bool,
    pub color_space: ColorSpace,
    pub restart_interval: u16,
    pub work_index: WorkIndex,
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
            shared_mut: JpegSharedMutCell {
                cell: &mut self.shared_mut,
            },
            bufs: thread_index.index_into_mut(&mut self.bufs),
            info: work_index.index_into_mut(&mut self.info),
            thread_index,
            huff_state: const_default(),
            last_dc_vals: const_default(),
        }
    }
}
