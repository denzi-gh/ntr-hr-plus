#![no_std]
#![allow(internal_features)]
#![allow(incomplete_features)]
#![feature(atomic_from_mut)]
#![feature(core_intrinsics)]
#![feature(const_trait_impl)]
#![feature(generic_const_exprs)]
#![feature(generic_const_items)]
#![feature(adt_const_params)]
#![feature(inherent_associated_types)]
#![feature(trivial_bounds)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(stmt_expr_attributes)]
#![feature(iter_array_chunks)]
#![feature(array_ptr_get)]
#![feature(more_float_constants)]
#![feature(strict_provenance_atomic_ptr)]
#![allow(static_mut_refs)]

use dbg::*;
use fix::*;
use utils::*;
use vars::*;
use ::libc::*;
use const_default::{ConstDefault, const_default};
use core::ops::*;
use core::panic::PanicInfo;
use core::sync::atomic::*;
use core::{
    cmp,
    marker::{ConstParamTy, PhantomData},
    mem, ptr, slice,
};
use ctru::*;
use function_name::named;
use oorandom::Rand32;

#[allow(unused)]
#[allow(clippy::all)]
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod ctru {
    use crate::ConstDefault;
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[macro_use]
mod dbg;
mod entries;
mod fix;
mod jpeg;
mod utils;
mod vars;

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    unsafe {
        if let Some(location) = panic_info.location() {
            panicHandle(
                location.file().as_ptr() as *mut _,
                location.file().len() as i32,
                location.line() as i32,
                location.column() as i32,
            )
        } else {
            showMsgRaw(c_str!("Panic!"));
        }

        loop {
            sleep_thread(THREAD_WAIT_NS);
        }
    }
}
