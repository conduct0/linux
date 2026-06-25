// SPDX-License-Identifier: GPL-2.0

//! Rust qdisc sample.

use core::marker::PhantomData;

use kernel::bindings::{register_qdisc, sk_buff, Qdisc, Qdisc_ops};
use kernel::ffi;
use kernel::prelude::*;
use kernel::types::Opaque;

module! {
    type: QdiscSample,
    name: "rust_qdisc",
    authors: ["Rust for Linux Contributors"],
    description: "Rust qdisc abstraction",
    license: "GPL",
    params:{},
}

#[repr(transparent)]
pub struct QdiscR<T>(Opaque<Qdisc>, PhantomData<T>);
impl<T> QdiscR<T> {
    unsafe fn from_raw<'a>(ptr: *mut Qdisc) -> &'a mut Self {
        // CAST: `Self` is a `repr(transparent)` wrapper around `bindings::Qdisc`.
        let ptr = ptr.cast::<Self>();
        // SAFETY: by the function requirements the pointer is valid and we have unique access for
        // the duration of `'a`.
        unsafe { &mut *ptr }
    }
    pub fn get_qdisc_priv(&mut self) -> &mut T {
        let qdisc = self.0.get();
        return unsafe { &mut *(*qdisc).privdata.as_mut_ptr().cast::<T>() };
    }
}

struct QdiscSample;

static mut QDISC_OPS: Qdisc_ops = Qdisc_ops {
    next: core::ptr::null_mut(),
    cl_ops: core::ptr::null(),
    id: *b"rust_qdisc\0\0\0\0\0\0",

    priv_size: 0,
    static_flags: 0,
    enqueue: Some(enqueue),
    dequeue: Some(dequeue),
    peek: Some(peek),
    init: None,
    reset: None,
    destroy: None,
    change: None,
    attach: None,
    change_tx_queue_len: None,
    change_real_num_tx: None,
    dump: None,
    dump_stats: None,
    ingress_block_set: None,
    egress_block_set: None,
    ingress_block_get: None,
    egress_block_get: None,
    owner: core::ptr::null_mut(),
};
unsafe extern "C" fn enqueue(skb: *mut sk_buff, _: *mut Qdisc, _: *mut *mut sk_buff) -> ffi::c_int {
    unsafe {
        QUEUE[0] = skb;
        pr_info!("ENQUEUING");
    }
    return 0;
}

unsafe extern "C" fn dequeue(_: *mut Qdisc) -> *mut sk_buff {
    pr_info!("DEQUEUE");
    unsafe {
        let skb = QUEUE[0];
        QUEUE[0] = core::ptr::null_mut();
        return skb;
    }
}
unsafe extern "C" fn peek(_: *mut Qdisc) -> *mut sk_buff {
    pr_info!("PEEK");
    unsafe {
        let skb = QUEUE[0];
        return skb;
    }
}

static mut QUEUE: [*mut sk_buff; 2usize] = [core::ptr::null_mut(), core::ptr::null_mut()];

impl ::kernel::Module for QdiscSample {
    fn init(_: &'static ::kernel::ThisModule) -> Result<Self> {
        let res =
            kernel::error::to_result(unsafe { register_qdisc(core::ptr::addr_of_mut!(QDISC_OPS)) });
        match res {
            Ok(_) => pr_info!("Success 123"),
            _ => pr_info!("Did not work 123"),
        }
        Ok(QdiscSample {})
    }
}

impl Drop for QdiscSample {
    fn drop(&mut self) {
        pr_info!("Rust Qdisc is working ");
    }
}
