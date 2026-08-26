// SPDX-License-Identifier: GPL-2.0

//! Rust qdisc sample.

use crate::{error::to_result, prelude::*, types::Opaque};
use core::marker::PhantomData;

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
    /// DOCS TODO
    pub fn qlen(&self) -> u32 {
        let qdisc = self.0.get();
        // SAFETY: The struct invariant ensures that we may access
        // this field without additional synchronization.
        unsafe { (*qdisc).q.qlen }
    }
    /// DOCS TODO
    pub fn limit(&self) -> u32 {
        let qdisc = self.0.get();
        // For now will hardcode. should be init in the init func.
        unsafe { (*qdisc).limit }
    }
    /// DOCS TODO
    pub fn drop(&self, skb: *mut bindings::sk_buff, to_free: *mut *mut bindings::sk_buff) -> u32 {
        let qdisc = self.0.get();
        unsafe { return bindings::qdisc_drop(skb, qdisc, to_free) as u32 }
    }
    /// DOCS TODO
    pub fn enqueue_tail(&self, skb: *mut bindings::sk_buff) -> u32 {
        let qdisc = self.0.get();
        unsafe { return bindings::qdisc_enqueue_tail(skb, qdisc) as u32 }
    }
    /// DOCS TODO
    pub fn dequeue_head(&self) -> *mut bindings::sk_buff {
        let qdisc = self.0.get();
        unsafe { return bindings::qdisc_dequeue_head(qdisc) }
    }
    /// DOCS TODO
    pub fn peek_head(&self) -> *mut bindings::sk_buff {
        let qdisc = self.0.get();
        unsafe { return bindings::qdisc_peek_head(qdisc) }
    }
    /// DOCS TODO
    pub fn reset(&self) {
        let qdisc = self.0.get();
        unsafe { bindings::qdisc_reset_queue(qdisc) }
    }
}

/// DOCS TODO
#[vtable]
pub trait QdiscOps {
    /// DOCS TODO
    type PrivData;

    /// DOCS TODO
    const ID: &'static CStr;

    /// DOCS TODO
    fn enqueue(
        qdisc: &mut Qdisc<Self::PrivData>,
        skb: *mut bindings::sk_buff,
        to_free: *mut *mut bindings::sk_buff,
    ) -> u32;
    /// DOCS TODO
    fn dequeue(qdisc: &mut Qdisc<Self::PrivData>) -> *mut bindings::sk_buff;
    /// DOCS TODO
    fn peek(qdisc: &mut Qdisc<Self::PrivData>) -> *mut bindings::sk_buff;
    /// DOCS TODO
    fn reset(qdisc: &mut Qdisc<Self::PrivData>);
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
    ) -> c_int {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::enqueue(qdisc, skb, to_free) as c_int
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
    unsafe extern "C" fn reset_callback(sch: *mut bindings::Qdisc) {
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::reset(qdisc);
    }
}
/// TODO doc
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
        priv_size: size_of::<T::PrivData>() as i32,
        enqueue: Some(Adapter::<T>::enqueue_callback),
        dequeue: Some(Adapter::<T>::dequeue_callback),
        peek: Some(Adapter::<T>::peek_callback),
        next: core::ptr::null_mut(),
        cl_ops: core::ptr::null(),
        init: None,
        reset: Some(Adapter::<T>::reset_callback),
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
        static_flags: 0,
    }))
}
/// Registration structure for Qdisc_ops
///
/// Registers [`OperationsVTable`] instances with the kernel. They will be unregistered when
/// dropped.
/// # Invariants
///
///
pub struct Registration {
    qdisc_ops: Pin<&'static mut OperationsVTable>,
}

// SAFETY: The only action allowed in a `Registration` instance is dropping it, which is safe to do
// from any thread because `unregister_qdisc` can be called from any thread context.
unsafe impl Send for Registration {}

impl Registration {
    /// Registers a QdiscOps.
    pub fn register(
        module: &'static ThisModule,
        qdisc_ops: Pin<&'static mut OperationsVTable>,
    ) -> Result<Self> {
        // SAFETY: `qdisc_ops` is uniquely owned and has not been registered yet,
        // so nothing else can be accessing it.
        unsafe { (*qdisc_ops.0.get()).owner = module.as_ptr() };
        let res = to_result(unsafe { bindings::register_qdisc(qdisc_ops.0.get()) });
        if res.is_ok() {
            pr_info!("Success 123");
            Ok(Registration { qdisc_ops })
        } else {
            pr_err!("DID NOT WORK123");
            return Err(res.err().unwrap());
        }
    }
}

/// TODO
impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: The type invariants guarantee that `self.drivers` is valid.
        // So it's just an FFI call.
        unsafe { bindings::unregister_qdisc(self.qdisc_ops.0.get()) };
    }
}
