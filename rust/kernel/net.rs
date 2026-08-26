// SPDX-License-Identifier: GPL-2.0

//! Networking.

#[cfg(CONFIG_RUST_PHYLIB_ABSTRACTIONS)]
pub mod phy;

#[cfg(CONFIG_RUST_QDISC_ABSTRACTIONS)]
pub mod qdisc;
