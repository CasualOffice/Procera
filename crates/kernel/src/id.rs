//! Identifier generation. Every entity id is a UUIDv7 (time-ordered, index
//! friendly; docs/04-LLD.md conventions). Generation goes through `IdGen` so
//! tests can make ids deterministic.

use std::fmt::Debug;
use uuid::Uuid;

pub trait IdGen: Send + Sync + Debug {
    fn new_id(&self) -> Uuid;
}

/// Production generator: random UUIDv7.
#[derive(Debug, Default, Clone, Copy)]
pub struct UuidV7Gen;

impl IdGen for UuidV7Gen {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_version_7() {
        assert_eq!(UuidV7Gen.new_id().get_version_num(), 7);
    }

    #[test]
    fn ids_are_time_ordered() {
        let a = UuidV7Gen.new_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = UuidV7Gen.new_id();
        assert!(a < b);
    }
}
