// SPDX-License-Identifier: GPL-2.0

//! Network Qdisc abstraction.
//!
//! C headers: [`include/net/sch_generic.h`](srctree/include/net/sch_generic.h)
//!  [`include/linux/skbuff.h`](srctree/include/linux/skbuff.h)

use crate::{error::to_result, prelude::*, sync::aref::ARef, types::Opaque};
use core::marker::PhantomData;

/// A wrapper for the C [`struct sk_buff`].
///
/// # Invariants
///
/// All instances are valid skbs created by the C portion of the kernel.
///
/// Instances of this type are always refcounted, that is, a call to [`skb_get`] ensures
/// that the allocation remains valid at least until the matching call to [`consume_skb`].
///
/// [`struct sk_buff`]: srctree/include/linux/skbuff.h
/// [`skb_get`]: srctree/include/linux/skbuff.h
/// [`consume_skb`]: srctree/include/linux/skbuff.h
#[repr(transparent)]
pub struct SkBuff(Opaque<bindings::sk_buff>);

impl SkBuff {
    /// Casts a [`struct sk_buff`] from a raw pointer into a reference
    /// of the abstraction [`SkBuff`].
    ///
    /// # Safety
    ///
    /// For the duration of `'a`:
    /// - The pointer must point at a valid `sk_buff`.
    /// - The `sk_buff` must not be freed.
    ///
    /// [`struct sk_buff`]: srctree/include/linux/skbuff.h
    #[inline]
    unsafe fn from_raw<'a>(ptr: *mut bindings::sk_buff) -> &'a Self {
        // CAST: `Self` is a `repr(transparent)` wrapper around `bindings::sk_buff`.
        let ptr = ptr.cast::<Self>();
        // SAFETY: by the function requirements the pointer is valid and not freed
        // for the duration of `'a`.
        unsafe { &*ptr }
    }

    /// Returns a raw pointer to the `struct sk_buff`.
    #[inline]
    pub fn as_raw(&self) -> *mut bindings::sk_buff {
        self.0.get()
    }
}

// SAFETY: The type invariants guarantee that `SkBuff` is always refcounted.
unsafe impl crate::sync::aref::AlwaysRefCounted for SkBuff {
    #[inline]
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::skb_get(self.as_raw()) };
    }

    #[inline]
    unsafe fn dec_ref(obj: core::ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::consume_skb(obj.cast().as_ptr()) }
    }
}

/// An instance of a Qdisc.
///
/// Wraps the kernel's [`struct Qdisc`].
///
/// A [`Qdisc`] reference is created when a callback in [`QdiscOps`] is executed.
///
/// [`Qdisc`] accepts a generic type for the privdata field.
///
/// # Invariants
/// - While a [`Qdisc`] reference exists, the root lock is held; this means you are in
///   a context where all methods defined on this struct are safe to call.
/// - This struct always has an initialized `privdata` as `P`.(WIP)
///
/// [`struct Qdisc`]: srctree/include/net/sch_generic.h
#[repr(transparent)]
pub struct Qdisc<P>(Opaque<bindings::Qdisc>, PhantomData<P>);

impl<P> Qdisc<P> {
    /// Creates a new [`Qdisc`] reference from a raw pointer.
    ///
    /// # Safety
    ///
    /// For the duration of `'a`,
    /// - the pointer must point at a valid `struct Qdisc` and root lock of the Qdisc is held.
    /// - `privdata` must be initialized as `P`.
    unsafe fn from_raw<'a>(ptr: *mut bindings::Qdisc) -> &'a mut Self {
        // CAST: `Self` is a `repr(transparent)` wrapper around `bindings::Qdisc`.
        let ptr = ptr.cast::<Self>();
        // SAFETY: by safety requirements, the pointer is valid and the lock is held for the
        // duration of `'a`, so the access is exclusive
        unsafe { &mut *ptr }
    }
    /// WIP this is not ready yet.
    /// Gets `Qdisc` privdata and casts with generic P.
    pub fn get_qdisc_priv(&mut self) -> &mut P {
        let qdisc = self.0.get();
        // SAFETY: TODO
        unsafe { &mut *(*qdisc).privdata.as_mut_ptr().cast::<P>() }
    }
    /// Gets the number of packets in the queue.
    pub fn qlen(&self) -> u32 {
        let qdisc = self.0.get();
        // SAFETY: The struct invariant ensures the root lock is held,
        // so it's safe to access this field.
        unsafe { (*qdisc).q.qlen }
    }
    /// Gets the limit of the queue
    pub fn limit(&self) -> u32 {
        let qdisc = self.0.get();
        // SAFETY: The struct invariant ensures the root lock is held,
        // so it's safe to access this field.
        unsafe { (*qdisc).limit }
    }
    /// Drops skb: adds skb to `to_free` and updates stats. Returns NET_XMIT_DROP.
    /// TODO: to_free should also be safely abstracted.
    pub fn drop_skb(&mut self, skb: ARef<SkBuff>, to_free: *mut *mut bindings::sk_buff) -> u32 {
        let qdisc = self.0.get();
        let raw_skb: *mut bindings::sk_buff = ARef::into_raw(skb).cast().as_ptr();
        // SAFETY: The struct invariant ensures the root lock is held,
        // changes made by helper to qdisc are safe. `raw_skb` is valid by the `SkBuff` invariant.
        // The reference to the skb is handed to `to_free`.
        unsafe { bindings::qdisc_drop(raw_skb, qdisc, to_free) as u32 }
    }
    /// Enqueues skb at the tail of the queue. Returns NET_XMIT_SUCCESS.
    pub fn enqueue_tail(&mut self, skb: ARef<SkBuff>) -> u32 {
        let qdisc = self.0.get();
        let raw_skb: *mut bindings::sk_buff = ARef::into_raw(skb).cast().as_ptr();
        // SAFETY: The struct invariant ensures the root lock is held,
        // changes made by helper to qdisc are safe. `raw_skb` is valid by the `SkBuff` invariant.
        // The reference to the skb is handed to the queue now.
        unsafe { bindings::qdisc_enqueue_tail(raw_skb, qdisc) as u32 }
    }
    /// Dequeues skb at the head of the queue.
    pub fn dequeue_head(&mut self) -> Option<ARef<SkBuff>> {
        let qdisc = self.0.get();
        // SAFETY: The struct invariant ensures the root lock is held,
        // changes made by helper to qdisc are safe.
        let raw_skb = unsafe { bindings::qdisc_dequeue_head(qdisc) };
        core::ptr::NonNull::new(raw_skb.cast::<SkBuff>()).map(|skb| 

            // SAFETY: The reference was queued so we are sure the refcount was incremented at least
            // by one before. Dequeuing transferred the reference to us, so the skb won't be used
            // by the queue afterwards.
            unsafe { ARef::from_raw(skb) })
    }

    /// Returns a reference to the skb at the head of the queue, or `None` if it is empty.
    pub fn peek_head(&self) -> Option<&SkBuff>{
        let qdisc = self.0.get();
        // SAFETY: The struct invariant ensures the root lock is held,
        // read is safe.
        let raw_skb = unsafe { bindings::qdisc_peek_head(qdisc) };
        core::ptr::NonNull::new(raw_skb).map(|skb|
            // SAFETY: `skb` is non-null and valid: it is in the queue, and every skb in the queue
            // got there through `enqueue_tail`, which handed the queue a refcounted reference.
            // `dequeue_head` and `reset` need `&mut self`, so they cannot run while this
            // reference, which borrows `&self`, exists.
            unsafe { SkBuff::from_raw(skb.as_ptr()) })

    }

    /// Frees queue of qdisc.
    pub fn reset(&mut self) {
        let qdisc = self.0.get();
        // SAFETY: The struct invariant ensures the root lock is held,
        // changes made by helper to qdisc are safe.
        unsafe { bindings::qdisc_reset_queue(qdisc) };
    }
}

/// Operations for a Qdisc type.
///
/// This trait is used to create an [`QdiscOpsVTable`] with [`create_qdisc_ops`].
/// Note that all functions are called with the root lock being held by
/// the networking core (e.g. [`__dev_xmit_skb`]).
///
/// [`__dev_xmit_skb`]: srctree/net/core/dev.c
#[vtable]
pub trait QdiscOps {
    /// Type of per Qdisc instance `privdata`.
    ///
    /// Space is allocated by the C side and will be initialized on `init` of Qdisc.
    type PrivData;

    /// ID of the qdisc used to register it, `tc` uses it to reference the qdisc.
    ///
    /// Size at most `IFNAMSIZ`, including NUL terminator, enforced at compile time.
    const ID: &'static CStr;

    /// Called when an skb should be scheduled for transmission.
    ///
    /// Qdisc owns the skb, either it gets scheduled to be dropped (`to_free`)
    /// or it is enqueued successfully.
    /// `to_free` is a list of dropped packets that gets freed after the lock is released.
    /// Returns `NET_XMIT_SUCCESS` or `NET_XMIT_DROP`.
    fn enqueue(
        qdisc: &mut Qdisc<Self::PrivData>,
        skb: ARef<SkBuff>,
        to_free: *mut *mut bindings::sk_buff,
    ) -> u32;

    /// Called when the networking core wants the next packet to send to the driver.
    ///
    /// Ownership of the packet is returned to the networking core.
    /// Returns packet in the queue, or None, if nothing should be sent now.
    fn dequeue(qdisc: &mut Qdisc<Self::PrivData>) -> Option<ARef<SkBuff>>;

    /// Returns the next packet without removing it from the queue or None if empty.
    fn peek(qdisc: &mut Qdisc<Self::PrivData>) -> Option<&SkBuff>;

    /// Frees all queued packets, resets `PrivData` state.
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
        // SAFETY: By the safety requirement of this function, `sch` is valid `struct Qdisc`.
        // `static_flags` does not contain `TCQ_F_NOLOCK`, so the root lock is held during calls.
        // TODO: privdata is not init yet
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        // SAFETY: The networking core never enqueues a null `skb`. It holds a reference to it,
        // so the refcount is nonzero, and it transfers that reference to the qdisc on enqueue.
        let skb = unsafe {ARef::from_raw(core::ptr::NonNull::new_unchecked(skb.cast()))};
        T::enqueue(qdisc, skb, to_free) as c_int
    }

    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn dequeue_callback(sch: *mut bindings::Qdisc) -> *mut bindings::sk_buff{
        // SAFETY: By the safety requirement of this function, `sch` is valid `struct Qdisc`.
        // `static_flags` does not contain `TCQ_F_NOLOCK`, so the root lock is held during calls.
        // TODO: privdata is not init yet
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::dequeue(qdisc).map_or(core::ptr::null_mut(), |skb| ARef::into_raw(skb).cast().as_ptr())
    }

    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn peek_callback(sch: *mut bindings::Qdisc) -> *mut bindings::sk_buff{
        // SAFETY: By the safety requirement of this function, `sch` is valid `struct Qdisc`.
        // `static_flags` does not contain `TCQ_F_NOLOCK`, so the root lock is held during calls.
        // TODO: privdata is not init yet
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::peek(qdisc).map_or(core::ptr::null_mut(), |skb| skb.as_raw())
    }

    /// # Safety
    ///
    /// `sch` must be passed by the corresponding callback in `Qdisc_ops`.
    unsafe extern "C" fn reset_callback(sch: *mut bindings::Qdisc) {
        // SAFETY: By the safety requirement of this function, `sch` is valid `struct Qdisc`.
        // `static_flags` does not contain `TCQ_F_NOLOCK`, so the root lock is held during calls.
        // TODO: privdata is not init yet
        let qdisc = unsafe { Qdisc::<T::PrivData>::from_raw(sch) };
        T::reset(qdisc);
    }
}
/// Wraps the kernel's [`struct Qdisc_ops`].
///
/// Created with [`create_qdisc_ops`] and registered by [`Registration::register`].
///
/// [`struct Qdisc_ops`]: srctree/include/net/sch_generic.h
#[repr(transparent)]
pub struct QdiscOpsVTable(Opaque<bindings::Qdisc_ops>);

// SAFETY: `QdiscOpsVTable` doesn't expose any &self method to access internal data, so it's safe
// to share `&QdiscOpsVTable` across execution context boundaries.
unsafe impl Sync for QdiscOpsVTable {}

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
    parsed
}

/// Creates a [`QdiscOpsVTable`] instance from [`QdiscOps`].
///
/// Must be used to initialize a `static` so the `ID` length check runs at compile time.
pub const fn create_qdisc_ops<T: QdiscOps>() -> QdiscOpsVTable {
    // INVARIANT: All the fields of `struct Qdisc_ops` are initialized properly.
    QdiscOpsVTable(Opaque::new(bindings::Qdisc_ops {
        id: parse_id::<T>(),
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
        // INVARIANT: `TCQ_F_NOLOCK` not set, networking core uses root lock of qdisc during
        // callbacks. This upholds the invariant of qdisc.
        static_flags: 0,
    }))
}
/// Registration of a [`QdiscOpsVTable`] with the kernel.
///
/// The qdisc is unregistered when this is dropped.
///
/// # Invariants
///
/// `qdisc_ops` is registered with the kernel.
pub struct Registration {
    qdisc_ops: Pin<&'static mut QdiscOpsVTable>,
}

// SAFETY: The only action allowed in a `Registration` instance is dropping it, which is safe to do
// from any thread because `unregister_qdisc` can be called from any thread context.
unsafe impl Send for Registration {}

impl Registration {
    /// Registers a QdiscOps.
    pub fn register(
        module: &'static ThisModule,
        qdisc_ops: Pin<&'static mut QdiscOpsVTable>,
    ) -> Result<Self> {
        // SAFETY: `qdisc_ops` is uniquely owned and has not been registered yet,
        // so nothing else can be accessing it.
        unsafe { (*qdisc_ops.0.get()).owner = module.as_ptr() };
        // SAFETY: `qdisc_ops` points at a valid, pinned `struct Qdisc_ops` with `'static` lifetime.
        to_result(unsafe { bindings::register_qdisc(qdisc_ops.0.get()) })?;
        // INVARIANT: `register_qdisc` succeeded, so `qdisc_ops` is registered.
        Ok(Registration { qdisc_ops })
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: By the type invariant, `qdisc_ops` is registered, so it may be unregistered.
        unsafe { bindings::unregister_qdisc(self.qdisc_ops.0.get()) };
    }
}
