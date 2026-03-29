//! Cached message size for two-pass serialization.
//!
//! Protobuf's length-delimited encoding requires knowing a sub-message's
//! serialized size before writing it. Without caching, computing sizes on
//! deeply nested messages is O(depth^2). `CachedSize` makes both passes O(n).
//!
//! Uses `AtomicU32` with `Relaxed` ordering. On all major platforms (x86,
//! ARM64, RISC-V), Relaxed loads/stores compile to the same instructions as
//! plain memory access -- the compiler barrier is free at runtime. The benefit
//! is that messages become `Sync`, enabling `Arc<Message>`.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::sync::atomic::{AtomicU32, Ordering};

/// Cached serialized size of a protobuf message.
///
/// This is embedded in every generated message struct. It is transparent
/// to equality and hashing -- two messages that differ only in cached size
/// are considered equal.
pub struct CachedSize {
    size: AtomicU32,
}

impl CachedSize {
    /// Create a new cached size initialized to zero.
    pub const fn new() -> Self {
        Self {
            size: AtomicU32::new(0),
        }
    }

    /// Get the cached size.
    pub fn get(&self) -> u32 {
        self.size.load(Ordering::Relaxed)
    }

    /// Set the cached size. Called during `compute_size()`.
    pub fn set(&self, size: u32) {
        self.size.store(size, Ordering::Relaxed);
    }
}

impl Default for CachedSize {
    fn default() -> Self {
        Self::new()
    }
}

// Clone resets to zero: the cloned message may diverge and needs recomputation.
impl Clone for CachedSize {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl fmt::Debug for CachedSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CachedSize").field(&self.get()).finish()
    }
}

// Cached size is NOT part of message identity.
impl PartialEq for CachedSize {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for CachedSize {}

// Cached size contributes nothing to the hash.
impl Hash for CachedSize {
    fn hash<H: Hasher>(&self, _state: &mut H) {}
}

// Compile-time proof that CachedSize is Send + Sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CachedSize>();
};

// Serde: CachedSize is always skipped in serialization.
#[cfg(feature = "json")]
impl serde::Serialize for CachedSize {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

#[cfg(feature = "json")]
impl<'de> serde::Deserialize<'de> for CachedSize {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(CachedSize::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_zero() {
        let cs = CachedSize::new();
        assert_eq!(cs.get(), 0);
    }

    #[test]
    fn set_and_get() {
        let cs = CachedSize::new();
        cs.set(42);
        assert_eq!(cs.get(), 42);
    }

    #[test]
    fn clone_resets_to_zero() {
        let cs = CachedSize::new();
        cs.set(100);
        let cloned = cs.clone();
        assert_eq!(cloned.get(), 0);
    }

    #[test]
    fn equality_ignores_cached_value() {
        let a = CachedSize::new();
        let b = CachedSize::new();
        a.set(10);
        b.set(20);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_is_stable_regardless_of_value() {
        use std::collections::hash_map::DefaultHasher;
        let a = CachedSize::new();
        let b = CachedSize::new();
        a.set(10);
        b.set(20);

        let hash = |cs: &CachedSize| {
            let mut h = DefaultHasher::new();
            cs.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&a), hash(&b));
    }
}
