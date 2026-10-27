//! Experimental subset. Expansion sources are copied unchanged from RustCrypto.
#![forbid(unsafe_code)]

mod expand_msg;
pub use elliptic_curve_modern::{Error, Result};

pub mod hash2curve {
    pub use crate::expand_msg::{ExpandMsg, Expander};
    pub use crate::expand_msg::xmd::{ExpandMsgXmd, ExpanderXmd};
    pub use crate::expand_msg::xof::ExpandMsgXof;
}
