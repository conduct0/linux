// SPDX-License-Identifier: GPL-2.0

//! Rust qdisc sample.

use kernel::bindings;
use kernel::net::qdisc::create_qdisc_ops;
use kernel::net::qdisc::OperationsVTable;
use kernel::net::qdisc::Qdisc;
use kernel::net::qdisc::QdiscOps;
use kernel::net::qdisc::Registration;
use kernel::prelude::*;

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

    fn enqueue(
        qdisc: &mut Qdisc<Self::PrivData>,
        skb: *mut bindings::sk_buff,
        to_free: *mut *mut bindings::sk_buff,
    ) -> u32 {
        pr_info!("123 ENQUEUING");
        const LIMIT: u32 = 1000;
        if qdisc.qlen() < LIMIT {
            return qdisc.enqueue_tail(skb);
        }

        return qdisc.drop(skb, to_free);
    }
    fn dequeue(qdisc: &mut Qdisc<Self::PrivData>) -> *mut bindings::sk_buff {
        pr_info!("123 DE-EQUEUING");
        return qdisc.dequeue_head();
    }
    fn peek(qdisc: &mut Qdisc<Self::PrivData>) -> *mut bindings::sk_buff {
        pr_info!("123 PEEK");
        return qdisc.peek_head();
    }
    fn reset(qdisc: &mut Qdisc<Self::PrivData>) {
        pr_info!("123 RESET");
        return qdisc.reset();
    }
}

const _: () = {
    static mut QDISC_OPS: OperationsVTable = create_qdisc_ops::<QdiscSample>();
    impl ::kernel::Module for QdiscSample {
        fn init(module: &'static ::kernel::ThisModule) -> Result<Self> {
            let qdisc = unsafe { &mut *(&raw mut QDISC_OPS) };
            let reg = Registration::register(module, core::pin::Pin::static_mut(qdisc))?;
            Ok(QdiscSample { _reg: reg })
        }
    }
};
