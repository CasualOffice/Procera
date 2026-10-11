//! Normal form, canonical bytes and checksum (docs/11 §2, ADR-0003).
//!
//! Normal form: every edge carries `on`, nodes and edges are ordered by id, variables by name,
//! SLAs by id, and `meta.compilerVersion` names this compiler. Array order inside a node (branches,
//! candidates, escalations) is meaningful and kept. Canonical bytes are RFC 8785 (JCS) JSON;
//! the checksum is the lowercase hex SHA-256 of those bytes.

use crate::model::IrDocument;
use sha2::{Digest, Sha256};

/// Version of the compiler's output format. Bump it whenever normalisation changes, because the
/// version is part of the hashed document.
pub const COMPILER_VERSION: &str = "0.1.0";

/// Returns the normal form of `doc` (idempotent).
pub fn normalize(doc: &IrDocument) -> IrDocument {
    let mut ir = doc.clone();
    ir.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    ir.edges.sort_by(|a, b| a.id.cmp(&b.id));
    ir.variables.sort_by(|a, b| a.name.cmp(&b.name));
    ir.sla.sort_by(|a, b| a.id.cmp(&b.id));
    ir.meta.compiler_version = Some(COMPILER_VERSION.to_owned());
    ir
}

/// RFC 8785 canonical JSON of `doc` as it stands (call [`normalize`] first for the IR form).
pub fn canonical_bytes(doc: &IrDocument) -> Result<Vec<u8>, serde_json::Error> {
    serde_jcs::to_vec(doc)
}

/// Lowercase hex SHA-256 of canonical bytes.
pub fn checksum_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
