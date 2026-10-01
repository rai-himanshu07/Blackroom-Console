//! Wire protocol and blocking client of `remote-emergencyd`. It lives in its own crate because the
//! daemon depends on `remote-hostd` (stop marker), so hostd cannot depend on the daemon crate.
#![forbid(unsafe_code)]

pub mod client;
pub mod proto;
