//! The five BPMS-002 reference processes compile cleanly (parity with the BPMS-002 checker is in
//! tests/conformance.rs).
// Helpers outside #[test] fns may unwrap too (clippy.toml allows it inside tests).
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use procera_ir::Severity;

#[test]
fn reference_processes_compile_without_errors_or_warnings() {
    for name in REFERENCE {
        let out = offline(&reference(name));
        let noisy: Vec<_> = out.diagnostics.iter().filter(|d| d.severity != Severity::Info).collect();
        assert!(noisy.is_empty(), "{name}: {noisy:#?}");
        assert!(out.compiled.is_some(), "{name}");
    }
}
