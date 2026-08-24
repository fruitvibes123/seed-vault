//! Standing ALLOC-FREEDOM gate for dragonfruit's firmware verify path. See
//! `Cargo.toml` for the full rationale.
//!
//! This is a `#![no_std]` STATICLIB with NO `#[global_allocator]`. The verification
//! suite builds it for `thumbv8m.main-none-eabihf`; the link SUCCEEDS only if nothing
//! reachable from `verify_bundle` uses `alloc` (otherwise the linker errors
//! "no global memory allocator found but one is required"). A LIBRARY (rlib) build
//! cannot catch this — an rlib links without an allocator even when a dep pulls in
//! `alloc`; only a final artifact (staticlib) forces the allocator to resolve.
//!
//! (No `#![forbid(unsafe_code)]` here: the one export uses `#[unsafe(no_mangle)]`
//! to anchor the verify path against dead-code elimination, and `forbid` would
//! reject that attribute. The crate runs no code and holds no `unsafe` blocks —
//! the security core `dragonfruit` keeps its own `forbid`.)
#![no_std]

use core::panic::PanicInfo;
use dragonfruit::{
    verify_bundle_no_window, verify_bundle_over_bytes, BundleFile, Purpose, BUNDLE_FILE_LEN,
};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

                                                                                    
                                                                           
                                                                                 
                          
static BUNDLE: [u8; BUNDLE_FILE_LEN] = [0u8; BUNDLE_FILE_LEN];
static ROOT: [u8; 32] = [0u8; 32];
static ARTIFACT: [u8; 1] = [0u8; 1];
                                                                                   
                                                                             
                                                                                    
static ARTIFACT_HASH: [u8; 32] = [0u8; 32];

/// Anchors the full no_std verify path into this no-allocator staticlib so the link
/// fails on alloc-creep. Exported (`#[no_mangle]`/`extern "C"`) so neither it nor its
/// callees are dead-code-eliminated; it is never invoked at runtime.
///
/// Drives BOTH public verify entries: the clocked
/// `verify_bundle_over_bytes` AND the clock-free `verify_bundle_no_window`. They are
/// DISTINCT call-graph entry points — the no-clock loci (S1/S2) use the windowless one —
/// so anchoring only the clocked sibling would leave alloc-creep reachable ONLY from the
/// windowless entry undetected (a smaller vacuity of the same class). Both results feed the return
/// so neither call is dead-code-eliminated.
#[unsafe(no_mangle)]
pub extern "C" fn dragonfruit_noalloc_gate_probe() -> i32 {
    let bf = match BundleFile::from_bytes(&BUNDLE) {
        Ok(bf) => bf,
        Err(_) => return 2,
    };
                                                                   
    let clocked = match verify_bundle_over_bytes(&bf.as_bundle(), &ROOT, 0, &ARTIFACT, Purpose::Img)
    {
        Ok(_) => 0,
        Err(_) => 1,
    };
                                                                                            
                                                                                             
                                     
    let windowless =
        match verify_bundle_no_window(&bf.as_bundle(), &ROOT, &ARTIFACT_HASH, Purpose::Img, 0) {
            Ok(_) => 0,
            Err(_) => 1,
        };
    clocked + (windowless << 1)
}
