//! Process host concerns of the OpenBitFun GPUI shell: process CLI flags, the
//! self-update flow, diagnostics bundles, the single-instance lock, and
//! storage-root isolation.

pub mod cli;
pub mod diagnostics;
pub mod instance;
pub mod storage;
pub mod update;
