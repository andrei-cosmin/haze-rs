//! Injecting resources into server functions and building structs from them.

mod build;
pub(crate) mod error_kind;
pub(crate) mod installer;
mod later;
pub(crate) mod need;
pub(crate) mod obtain;
mod pack;
pub(crate) mod provider;
pub(crate) mod registration;
#[cfg(feature = "server")]
mod request_parts;
mod res;

pub use build::Build;
pub use later::Later;
pub use pack::Pack;
pub use res::Res;
