//! Pure-f64 flux stretches and RGB composites shared by `bake` (native) and
//! `viewer` (WASM). Ported 1:1 from the Python jellyscope; carries the golden
//! tests that pin the astronomy math against the original.
#![forbid(unsafe_code)]

pub mod composite;
pub mod stretch;
