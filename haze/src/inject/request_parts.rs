//! Reading resources from a request, behind `Res<T>`.

use std::any::type_name;

use dioxus_fullstack::{
    HttpError,
    http::{StatusCode, request::Parts},
};

use crate::Resources;

/// Reads resources from the [`Resources`] attached to a request, the way
/// axum-core's `RequestPartsExt` adds extraction to `Parts`.
pub(crate) trait RequestPartsExt {
    /// The value inserted as `T`, or a `500` naming `T`.
    fn resource<T: Clone + Send + Sync + 'static>(&self) -> Result<T, HttpError>;

    /// The value inserted as `T`, or `None`; a `500` only if no registry is attached.
    fn optional_resource<T: Clone + Send + Sync + 'static>(&self) -> Result<Option<T>, HttpError>;
}

impl RequestPartsExt for Parts {
    fn resource<T: Clone + Send + Sync + 'static>(&self) -> Result<T, HttpError> {
        self.optional_resource::<T>()?.ok_or_else(|| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{} was never inserted", type_name::<T>()),
            )
        })
    }

    fn optional_resource<T: Clone + Send + Sync + 'static>(&self) -> Result<Option<T>, HttpError> {
        let Some(resources) = self.extensions.get::<Resources>() else {
            return Err(HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "{} was requested, but no haze Resources are attached to this request; start the router with haze::serve or layer Extension(resources), and call the server function inside a request",
                    type_name::<T>()
                ),
            ));
        };
        Ok(resources.get::<T>())
    }
}

#[cfg(test)]
mod tests {
    use dioxus_fullstack::http::{Request, StatusCode, request::Parts};

    use super::RequestPartsExt;
    use crate::Resources;

    struct Incoming;

    impl Incoming {
        fn with(resources: Resources) -> Parts {
            let mut parts = Self::bare();
            parts.extensions.insert(resources);
            parts
        }

        fn bare() -> Parts {
            Request::new(()).into_parts().0
        }
    }

    #[test]
    fn finds_an_inserted_value() {
        let mut resources = Resources::new();
        resources.insert(String::from("storage"));
        assert_eq!(
            Incoming::with(resources).resource::<String>().unwrap(),
            "storage"
        );
    }

    #[test]
    fn a_missing_type_is_rejected_by_name() {
        let error = Incoming::with(Resources::new())
            .resource::<String>()
            .unwrap_err();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            error.message.as_deref(),
            Some("alloc::string::String was never inserted")
        );
    }

    #[test]
    fn a_missing_optional_type_is_none() {
        assert_eq!(
            Incoming::with(Resources::new())
                .optional_resource::<String>()
                .unwrap(),
            None
        );
    }

    #[test]
    fn a_server_without_resources_names_the_type_and_the_fix() {
        let error = Incoming::bare().optional_resource::<u32>().unwrap_err();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            error.message.as_deref(),
            Some(
                "u32 was requested, but no haze Resources are attached to this request; start the router with haze::serve or layer Extension(resources), and call the server function inside a request"
            )
        );
    }
}
