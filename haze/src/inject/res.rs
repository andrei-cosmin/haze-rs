//! `Res<T>`: the server-function argument that receives a resource.

#[cfg(feature = "server")]
use std::future::ready;
use std::ops::{Deref, DerefMut};

#[cfg(feature = "server")]
use dioxus_fullstack::{
    HttpError,
    axum_core::extract::{FromRequestParts, OptionalFromRequestParts},
    http::request::Parts,
};

#[cfg(feature = "server")]
use crate::inject::request_parts::RequestPartsExt;

/// A server function argument that receives a clone of a resource.
///
/// `Res<T>` is a plain wrapper, shaped like axum's `Extension<T>`, that
/// dereferences to the resource it holds; a derived [`Pack`](trait@crate::Pack)
/// is received the same way, as `Res<Pack>`. A handle of
/// [`#[server]`](macro@crate::server) written `T` is handed to Dioxus as
/// `Res<T>` and opened again, so the body sees `T`.
///
/// With the `server` feature it is an axum extractor that works like
/// `Extension<T>`: it looks up the value inserted as exactly `T` in the
/// [`Resources`](crate::Resources) attached by [`serve`](fn@crate::serve). A
/// missing value rejects the call with a `500 Internal Server Error` naming
/// `T`. Use `Option<Res<T>>` to receive `None` when `T` is absent from an
/// attached registry. Both forms reject the call if the request has no
/// [`Resources`](crate::Resources).
///
/// # Examples
///
/// ```rust,ignore
/// #[server(clicks: Res<Clicks>)]
/// async fn bump() -> Result<u64> {
///     Ok(clicks.bump())
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct Res<T>(pub T);

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
impl<T, S> FromRequestParts<S> for Res<T>
where
    T: Clone + Send + Sync + 'static,
    S: Send + Sync,
{
    type Rejection = HttpError;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        ready(parts.resource().map(Self))
    }
}

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
impl<T, S> OptionalFromRequestParts<S> for Res<T>
where
    T: Clone + Send + Sync + 'static,
    S: Send + Sync,
{
    type Rejection = HttpError;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Option<Self>, Self::Rejection>> + Send {
        ready(parts.optional_resource().map(|value| value.map(Self)))
    }
}

impl<T> Deref for Res<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Res<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Res;

    #[test]
    fn debug_shows_the_value() {
        let res = Res(5_u8);
        assert_eq!(format!("{res:?}"), "Res(5)");
        assert_eq!(*res, 5);
    }

    #[test]
    fn deref_mut_reaches_the_value() {
        let mut res = Res(String::from("a"));
        res.push('b');
        assert_eq!(res.0, "ab");
    }
}

#[cfg(all(test, feature = "server"))]
mod extract_tests {
    use std::sync::Arc;

    use dioxus_fullstack::{
        axum_core::extract::{FromRequestParts, OptionalFromRequestParts},
        http::{Request, StatusCode, request::Parts},
    };

    use super::Res;
    use crate::Resources;

    trait Greeter: Send + Sync {
        fn greet(&self) -> String;
    }

    struct English;

    impl Greeter for English {
        fn greet(&self) -> String {
            String::from("hello")
        }
    }

    struct Incoming;

    impl Incoming {
        fn with(resources: Resources) -> Parts {
            let mut parts = Request::new(()).into_parts().0;
            parts.extensions.insert(resources);
            parts
        }
    }

    #[tokio::test]
    async fn injects_a_clone_of_the_inserted_value() {
        let mut resources = Resources::new();
        resources.insert(String::from("storage"));
        let mut parts = Incoming::with(resources);
        let Res(storage) =
            <Res<String> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert_eq!(storage, "storage");
    }

    #[tokio::test]
    async fn injects_a_trait_object_behind_an_arc() {
        let mut resources = Resources::new();
        resources.insert::<Arc<dyn Greeter>>(Arc::new(English));
        let mut parts = Incoming::with(resources);
        let greeter =
            <Res<Arc<dyn Greeter>> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert_eq!(greeter.greet(), "hello");
    }

    #[tokio::test]
    async fn an_injected_arc_is_the_same_instance() {
        let original = Arc::new(7_u32);
        let mut resources = Resources::new();
        resources.insert(original.clone());
        let mut parts = Incoming::with(resources);
        let Res(injected) =
            <Res<Arc<u32>> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert!(Arc::ptr_eq(&original, &injected));
    }

    #[tokio::test]
    async fn a_missing_type_is_rejected_with_a_server_error() {
        let mut parts = Incoming::with(Resources::new());
        let error = <Res<u32> as FromRequestParts<()>>::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn an_optional_inject_is_none_when_missing() {
        let mut parts = Incoming::with(Resources::new());
        let found = <Res<u32> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn an_optional_inject_is_some_when_inserted() {
        let mut resources = Resources::new();
        resources.insert(9_u32);
        let mut parts = Incoming::with(resources);
        let found = <Res<u32> as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(found.map(|Res(value)| value), Some(9));
    }
}
