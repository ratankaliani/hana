#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![no_std]

use alloy_rlp as _;
use bytes as _;

extern crate alloc;

#[macro_use]
extern crate tracing;

mod traits;
pub use traits::CelestiaProvider;

mod source;
pub use source::CelestiaDASource;

mod celestia;
pub use celestia::CelestiaDADataSource;
