// SPDX-License-Identifier: GPL-2.0

//! Rust qdisc sample.

use kernel::bindings;
use kernel::net::qdisc::create_qdisc_ops;
use kernel::net::qdisc::Qdisc;
use kernel::net::qdisc::QdiscOps;
use kernel::net::qdisc::QdiscOpsVTable;
use kernel::net::qdisc::Registration;
use kernel::net::qdisc::SkBuff;
use kernel::prelude::*;
use kernel::sync::aref::ARef;

struct QdiscSample {
    _reg: Registration,
}
module! {
    type: QdiscSample,
    name: "rust_qdisc",
    authors: ["Rust for Linux Contributors"],
    description: "Rust qdisc abstraction",
    license: "GPL",
    params:{},
}

#[vtable]
impl QdiscOps for QdiscSample {
    const ID: &'static CStr = c"rust_qdisc";

    type PrivData = ();

    fn init(qdisc: &mut Qdisc<Self::PrivData>) -> Result {
        pr_info!("123 INIT");
        qdisc.set_limit(1000);
        Ok(())
    }

    fn enqueue(
        qdisc: &mut Qdisc<Self::PrivData>,
        skb: ARef<SkBuff>,
        to_free: *mut *mut bindings::sk_buff,
    ) -> u32 {
        if qdisc.qlen() < qdisc.limit() {
            return qdisc.enqueue_tail(skb);
        }

        qdisc.drop_skb(skb, to_free)
    }

    fn dequeue(qdisc: &mut Qdisc<Self::PrivData>) -> Option<ARef<SkBuff>> {
        qdisc.dequeue_head()
    }

    fn peek(qdisc: &mut Qdisc<Self::PrivData>) -> Option<&SkBuff> {
        qdisc.peek_head()
    }

    fn reset(qdisc: &mut Qdisc<Self::PrivData>) {
        pr_info!("123 RESET");
        qdisc.reset();
    }
}

const _: () = {
    static mut QDISC_OPS: QdiscOpsVTable = create_qdisc_ops::<QdiscSample>();
    impl ::kernel::Module for QdiscSample {
        fn init(module: &'static ::kernel::ThisModule) -> Result<Self> {
            // SAFETY: `init` is called only once. `Registration` is the only one that owns the
            // `qdisc_ops`.
            let qdisc_ops = unsafe { &mut *(&raw mut QDISC_OPS) };
            let reg = Registration::register(module, core::pin::Pin::static_mut(qdisc_ops))?;
            Ok(QdiscSample { _reg: reg })
        }
    }
};
