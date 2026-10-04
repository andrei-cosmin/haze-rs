//! A hasher that uses a `TypeId`'s own bits, as `http::Extensions` does.

use std::hash::Hasher;

/// Keeps the 64 bits a `TypeId` writes instead of hashing them again, the way
/// `http::Extensions` does: they are already a high-quality hash from the compiler.
#[derive(Default)]
pub(crate) struct IdHasher(u64);

impl Hasher for IdHasher {
    fn write(&mut self, _: &[u8]) {
        unreachable!("TypeId hashes with write_u64");
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}
