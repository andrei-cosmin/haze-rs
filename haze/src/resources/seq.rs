//! `Seq<T>`: the ordered list of registered implementations of a trait.

use std::{
    any::type_name,
    fmt::{self, Debug, Formatter},
    ops::Deref,
    slice::Iter,
    sync::Arc,
};

/// A shared, ordered, read-only sequence of values, cheap to clone.
///
/// Built by [`Resources::collect`](crate::Resources::collect) from every
/// [`#[register]`](macro@crate::register)ed implementation of a trait, and
/// injected like any other resource. It dereferences to a slice, so `iter`,
/// `len`, indexing and `for` loops work directly. Cloning shares the same items.
///
/// # Examples
///
/// ```
/// use haze::Seq;
///
/// let numbers = Seq::from(vec![Box::new(1_u8), Box::new(2_u8)]);
/// let shared = numbers.clone();
/// assert_eq!(shared.len(), 2);
/// assert_eq!(*shared[1], 2);
/// ```
pub struct Seq<T: ?Sized>(Arc<[Box<T>]>);

impl<T: ?Sized> From<Vec<Box<T>>> for Seq<T> {
    fn from(items: Vec<Box<T>>) -> Self {
        Self(items.into())
    }
}

impl<'a, T: ?Sized> IntoIterator for &'a Seq<T> {
    type Item = &'a Box<T>;
    type IntoIter = Iter<'a, Box<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: ?Sized> Deref for Seq<T> {
    type Target = [Box<T>];

    #[inline]
    fn deref(&self) -> &[Box<T>] {
        &self.0
    }
}

impl<T: ?Sized> Clone for Seq<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: ?Sized> Debug for Seq<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Seq")
            .field("of", &type_name::<T>())
            .field("len", &self.0.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Display;

    use super::Seq;

    #[test]
    fn clones_share_the_same_items() {
        let first = Seq::from(vec![Box::new(String::from("a"))]);
        let second = first.clone();
        assert!(std::ptr::eq(first[0].as_ref(), second[0].as_ref()));
    }

    #[test]
    fn holds_trait_objects_in_order() {
        let items: Vec<Box<dyn Display>> = vec![Box::new(1), Box::new("two")];
        let seq = Seq::from(items);
        let mut rendered = Vec::new();
        for item in &seq {
            rendered.push(item.to_string());
        }
        assert_eq!(rendered, ["1", "two"]);
    }

    #[test]
    fn debug_shows_the_type_and_length() {
        let seq = Seq::from(vec![Box::new(5_u8)]);
        assert_eq!(format!("{seq:?}"), "Seq { of: \"u8\", len: 1, .. }");
    }
}
