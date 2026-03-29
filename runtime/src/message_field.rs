//! Ergonomic wrapper for optional message fields.
//!
//! Protobuf optional/required message fields are typically `Option<Box<T>>`.
//! `MessageField<T>` wraps this with transparent deref to a static default
//! instance when unset, avoiding allocation for every `unwrap_or_default()`.

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::Deref;

/// A lazily-initialized static default instance for a message type.
///
/// # Safety
///
/// Implementors must ensure the returned reference points to a value in static
/// storage (e.g., via `Box::leak` or a `static` variable) that is never mutated
/// after publication.
pub unsafe trait DefaultInstance: Default {
    /// Returns a `&'static` reference to a default-valued instance.
    fn default_instance() -> &'static Self;
}

/// Wrapper for optional protobuf message fields.
///
/// Derefs to the contained value when set, or to a static default instance
/// when unset. This avoids heap allocation for reading unset fields.
///
/// ```ignore
/// // Generated code:
/// pub struct Outer {
///     pub inner: MessageField<Inner>,
/// }
///
/// // Usage -- no allocation if inner is unset:
/// println!("{}", msg.inner.some_field);
/// ```
#[derive(Clone)]
pub struct MessageField<T: DefaultInstance + 'static> {
    inner: Option<Box<T>>,
}

impl<T: DefaultInstance + 'static> MessageField<T> {
    /// Create an empty (unset) message field.
    pub const fn none() -> Self {
        Self { inner: None }
    }

    /// Create a set message field.
    pub fn some(value: T) -> Self {
        Self {
            inner: Some(Box::new(value)),
        }
    }

    /// Returns `true` if the field is set.
    pub fn is_set(&self) -> bool {
        self.inner.is_some()
    }

    /// Returns `true` if the field is unset.
    pub fn is_unset(&self) -> bool {
        self.inner.is_none()
    }

    /// Get a mutable reference, initializing with `Default` if unset.
    pub fn get_or_insert_default(&mut self) -> &mut T {
        self.inner.get_or_insert_with(|| Box::new(T::default()))
    }

    /// Set the field value.
    pub fn set(&mut self, value: T) {
        self.inner = Some(Box::new(value));
    }

    /// Clear the field (set to unset).
    pub fn clear(&mut self) {
        self.inner = None;
    }

    /// Take the value out, leaving the field unset.
    pub fn take(&mut self) -> Option<T> {
        self.inner.take().map(|b| *b)
    }

    /// Apply a function to the value, initializing if unset.
    pub fn modify<F: FnOnce(&mut T)>(&mut self, f: F) {
        f(self.get_or_insert_default());
    }

    /// Get the inner value, or an error if unset.
    pub fn ok_or<E>(self, err: E) -> Result<T, E> {
        match self.inner {
            Some(b) => Ok(*b),
            None => Err(err),
        }
    }

    /// Get a reference to the boxed value if set.
    pub fn as_option(&self) -> Option<&T> {
        self.inner.as_deref()
    }

    /// Get a mutable reference to the boxed value if set.
    pub fn as_option_mut(&mut self) -> Option<&mut T> {
        self.inner.as_deref_mut()
    }

    /// Convert into the inner Option<Box<T>>.
    pub fn into_inner(self) -> Option<Box<T>> {
        self.inner
    }
}

impl<T: DefaultInstance + 'static> Deref for MessageField<T> {
    type Target = T;

    fn deref(&self) -> &T {
        match &self.inner {
            Some(b) => b,
            None => T::default_instance(),
        }
    }
}

impl<T: DefaultInstance + 'static> Default for MessageField<T> {
    fn default() -> Self {
        Self::none()
    }
}

impl<T: DefaultInstance + PartialEq + 'static> PartialEq for MessageField<T> {
    fn eq(&self, other: &Self) -> bool {
        // An unset field equals a set-to-default field (protobuf semantics).
        let a: &T = self;
        let b: &T = other;
        a == b
    }
}

impl<T: DefaultInstance + Eq + 'static> Eq for MessageField<T> {}

impl<T: DefaultInstance + Hash + 'static> Hash for MessageField<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let val: &T = self;
        val.hash(state);
    }
}

impl<T: DefaultInstance + fmt::Debug + 'static> fmt::Debug for MessageField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            Some(b) => write!(f, "Set({:?})", b),
            None => write!(f, "Unset"),
        }
    }
}

impl<T: DefaultInstance + 'static> From<Option<T>> for MessageField<T> {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => Self::some(v),
            None => Self::none(),
        }
    }
}

impl<T: DefaultInstance + 'static> From<T> for MessageField<T> {
    fn from(value: T) -> Self {
        Self::some(value)
    }
}

// ---------------------------------------------------------------------------
// Serde support (behind "json" feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "json")]
impl<T: DefaultInstance + serde::Serialize + 'static> serde::Serialize for MessageField<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.inner {
            Some(v) => v.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

#[cfg(feature = "json")]
impl<'de, T: DefaultInstance + serde::Deserialize<'de> + 'static> serde::Deserialize<'de>
    for MessageField<T>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let opt = Option::<T>::deserialize(deserializer)?;
        Ok(match opt {
            Some(v) => MessageField::some(v),
            None => MessageField::none(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    #[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
    struct TestMsg {
        pub value: i32,
    }

    // Safety: OnceLock ensures static storage and single initialization.
    unsafe impl DefaultInstance for TestMsg {
        fn default_instance() -> &'static Self {
            static INSTANCE: OnceLock<TestMsg> = OnceLock::new();
            INSTANCE.get_or_init(TestMsg::default)
        }
    }

    #[test]
    fn unset_derefs_to_default() {
        let field: MessageField<TestMsg> = MessageField::none();
        assert!(field.is_unset());
        assert_eq!(field.value, 0); // derefs to default instance
    }

    #[test]
    fn set_derefs_to_value() {
        let field = MessageField::some(TestMsg { value: 42 });
        assert!(field.is_set());
        assert_eq!(field.value, 42);
    }

    #[test]
    fn get_or_insert_default_initializes() {
        let mut field: MessageField<TestMsg> = MessageField::none();
        assert!(field.is_unset());
        field.get_or_insert_default().value = 10;
        assert!(field.is_set());
        assert_eq!(field.value, 10);
    }

    #[test]
    fn modify_initializes_and_applies() {
        let mut field: MessageField<TestMsg> = MessageField::none();
        field.modify(|msg| msg.value = 99);
        assert!(field.is_set());
        assert_eq!(field.value, 99);
    }

    #[test]
    fn unset_equals_default() {
        let unset: MessageField<TestMsg> = MessageField::none();
        let default_set = MessageField::some(TestMsg::default());
        assert_eq!(unset, default_set);
    }

    #[test]
    fn take_returns_value_and_clears() {
        let mut field = MessageField::some(TestMsg { value: 7 });
        let taken = field.take();
        assert_eq!(taken, Some(TestMsg { value: 7 }));
        assert!(field.is_unset());
    }

    #[test]
    fn clear_unsets_field() {
        let mut field = MessageField::some(TestMsg { value: 5 });
        field.clear();
        assert!(field.is_unset());
    }
}
