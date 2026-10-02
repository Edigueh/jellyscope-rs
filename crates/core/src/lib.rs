//! Pure-f64 flux stretches and RGB composites used by the WASM viewer. Ported
//! 1:1 from the Python jellyscope; golden tests pin the math against the
//! original.
#![forbid(unsafe_code)]

pub mod composite;
pub mod stretch;
