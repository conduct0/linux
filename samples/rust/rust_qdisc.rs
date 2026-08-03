// SPDX-License-Identifier: GPL-2.0

//! Rust qdisc sample.

use core::marker::PhantomData;

use kernel::bindings;
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

/// An instance of a Qdisc.
///
/// Wraps the kernel's [`struct Qdisc`].
///
/// A [`Qdisc`] instance is created when a callback in [`QdiscOps`] is executed. A Qdisc
/// executes [`QdiscOps`]'s methods during the callback.
///
/// [`Qdisc`] accepts a generic type for the privdata field.
///
/// # Invariants
/// TODO check locking and no lock mode
/// - Referencing a `Qdisc` using this struct asserts that you are in
///   a context where all methods defined on this struct are safe to call.
/// - This struct always has a valid `self.0.privdata`.
///
/// [`struct Qdisc`]: srctree/include/net/sch_generic.h
#[repr(transparent)]
pub struct Qdisc<P>(Opaque<bindings::Qdisc>, PhantomData<P>);

impl<P> Qdisc<P> {
    /// Creates a new [`Qdisc`] instance from a raw pointer.
    ///
    /// # Safety
    ///
    /// For the duration of `'a`,
    /// - the pointer must point at a valid `Qdisc`, and the caller
    ///   must be in a context where all methods defined on this struct
    ///   are safe to call.
    /// - `(*ptr).privdata` must be valid.
    unsafe fn from_raw<'a>(ptr: *mut bindings::Qdisc) -> &'a mut Self {
        // CAST: `Self` is a `repr(transparent)` wrapper around `bindings::Qdisc`.
        let ptr = ptr.cast::<Self>();
        // SAFETY: by the function requirements the pointer is valid and we have unique access for
        // the duration of `'a`.
        unsafe { &mut *ptr }
    }
    /// Gets `Qdisc` privdata and casts with generic P
    pub fn get_qdisc_priv(&mut self) -> &mut P {
        let qdisc = self.0.get();
        // SAFETY: privdata is allocated on the c side.
        // Upon initialization privdata is filled with type P
        unsafe { &mut *(*qdisc).privdata.as_mut_ptr().cast::<P>() }
    }
}

/// DOCS TODO
#[vtable]
pub trait QdiscOps {
    /// DOCS TODO
    type PrivData;

    /// DOCS TODO
    const ID: &'static CStr;

    fn enqueue(
        qdisc: &mut Qdisc<Self::PrivData>,
        skb: *mut bindings::sk_buff,
        to_free: *mut *mut bindings::sk_buff,
    ) -> Result;
    fn dequeue(qdisc: &mut Qdisc<Self::PrivData>) -> *mut bindings::sk_buff;
    fn peek(qdisc: &mut Qdisc<Self::PrivData>) -> *mut bindings::sk_buff;
    fn init(
        qdisc: &mut Qdisc<Self::PrivData>,
        arg: *mut bindings::nlattr,
        extack: *mut bindings::netlink_ext_ack,
    ) -> ffi::c_int;
    fn destroy(qdisc: &mut Qdisc<Self::PrivData>);
}

struct Adapter<T: QdiscOps>(PhantomData<T>);

impl<T: QdiscOps> Adapter<T> {
    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn enqueue_callback(
        skb: *mut bindings::sk_buff,
        sch: *mut bindings::Qdisc,
        to_free: *mut *mut bindings::sk_buff,
    ) -> ffi::c_int {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        match T::enqueue(qdisc, skb, to_free) {
            Ok(()) => 0,
            Err(e) => e.to_errno(),
        }
    }

    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn dequeue_callback(sch: *mut bindings::Qdisc) -> *mut bindings::sk_buff {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::dequeue(qdisc)
    }

    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn peek_callback(sch: *mut bindings::Qdisc) -> *mut bindings::sk_buff {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::peek(qdisc)
    }
    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn init(
        sch: *mut bindings::Qdisc,
        arg: *mut bindings::nlattr,
        extack: *mut bindings::netlink_ext_ack,
    ) -> ffi::c_int {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::init(qdisc, arg, extack)
    }
    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn destroy(arg1: *mut bindings::Qdisc) {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(arg1) };
        T::destroy(qdisc)
    }
}
#[repr(transparent)]
pub struct OperationsVTable(Opaque<bindings::Qdisc_ops>);

const IFNAMSIZ_USIZE: usize = bindings::IFNAMSIZ as usize;

const fn parse_id<T: QdiscOps>() -> [u8; IFNAMSIZ_USIZE] {
    let id_bytes = T::ID.to_bytes_with_nul();
    assert!(
        id_bytes.len() <= IFNAMSIZ_USIZE,
        "QdiscOps ID too long for id field"
    );
    let mut parsed: [u8; IFNAMSIZ_USIZE] = [0; IFNAMSIZ_USIZE];
    let mut i = 0;
    while i < id_bytes.len() {
        parsed[i] = id_bytes[i];
        i += 1;
    }
    return parsed;
}

// SAFETY: `DriverVTable` doesn't expose any &self method to access internal data, so it's safe to
// share `&DriverVTable` across execution context boundaries.
unsafe impl Sync for OperationsVTable {}

/// Creates a [`DriverVTable`] instance from [`Driver`].
///
/// This is used by [`module_phy_driver`] macro to create a static array of `phy_driver`.
///
/// [`module_phy_driver`]: crate::module_phy_driver
pub const fn create_qdisc_ops<T: QdiscOps>() -> OperationsVTable {
    // INVARIANT: All the fields of `struct Qdisc_ops` are initialized properly.
    OperationsVTable(Opaque::new(bindings::Qdisc_ops {
        id: parse_id::<T>(),
        // TODO use  size_of::<T>()
        priv_size: 0,
        enqueue: Some(Adapter::<T>::enqueue_callback),
        dequeue: Some(Adapter::<T>::dequeue_callback),
        peek: Some(Adapter::<T>::peek_callback),
        next: core::ptr::null_mut(),
        cl_ops: core::ptr::null(),
        init: Some(Adapter::<T>::init),
        reset: None,
        destroy: Some(Adapter::<T>::destroy),
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
        static_flags: 0,
    }))
}

// SAMPLE--->
struct QdiscSample;
//
// static mut QDISC_OPS: bindings::Qdisc_ops = bindings::Qdisc_ops {
//     next: core::ptr::null_mut(),
//     cl_ops: core::ptr::null(),
//     id: *b"rust_qdisc\0\0\0\0\0\0",
//
//     priv_size: 0,
//     static_flags: 0,
//     enqueue: Some(enqueue),
//     dequeue: Some(dequeue),
//     peek: Some(peek),
//     init: None,
//     reset: None,
//     destroy: None,
//     change: None,
//     attach: None,
//     change_tx_queue_len: None,
//     change_real_num_tx: None,
//     dump: None,
//     dump_stats: None,
//     ingress_block_set: None,
//     egress_block_set: None,
//     ingress_block_get: None,
//     egress_block_get: None,
//     owner: core::ptr::null_mut(),
// };
// unsafe extern "C" fn enqueue(
//     skb: *mut bindings::sk_buff,
//     _: *mut Qdisc,
//     _: *mut *mut bindings::sk_buff,
// ) -> ffi::c_int {
//     unsafe {
//         QUEUE[0] = skb;
//         pr_info!("ENQUEUING");
//     }
//     return 0;
// }
//
// unsafe extern "C" fn dequeue(_: *mut Qdisc) -> *mut bindings::sk_buff {
//     pr_info!("DEQUEUE");
//     unsafe {
//         let skb = QUEUE[0];
//         QUEUE[0] = core::ptr::null_mut();
//         return skb;
//     }
// }
// unsafe extern "C" fn peek(_: *mut Qdisc) -> *mut bindings::sk_buff {
//     pr_info!("PEEK");
//     unsafe {
//         let skb = QUEUE[0];
//         return skb;
//     }
// }
//
// static mut QUEUE: [*mut bindings::sk_buff; 2usize] = [core::ptr::null_mut(), core::ptr::null_mut()];
//
#[vtable]
impl QdiscOps for QdiscSample {}
impl ::kernel::Module for QdiscSample {
    fn init(_: &'static ::kernel::ThisModule) -> Result<Self> {
        let qdisc_ops = create_qdisc_ops::<QdiscSample>();
        let res = kernel::error::to_result(unsafe {
            bindings::register_qdisc(core::ptr::addr_of_mut!(qdisc_ops.0.get()))
        });
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
