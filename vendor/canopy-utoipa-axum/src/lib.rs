#![cfg_attr(doc_cfg, feature(doc_cfg))]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

//! Utoipa axum brings `utoipa` and `axum` closer together by the way of providing an ergonomic API that is extending on
//! the `axum` API. It gives a natural way to register handlers known to `axum` and also simultaneously generates OpenAPI
//! specification from the handlers.
//!
//! ## Crate features
//!
//! - **`debug`**: Implement debug traits for types.
//!
//! ## Install
//!
//! Add dependency declaration to `Cargo.toml`.
//!
//! ```toml
//! [dependencies]
//! utoipa-axum = "0.2"
//! ```
//!
//! ## Examples
//!
//! _**Use [`OpenApiRouter`][router] to collect handlers with _`#[utoipa::path]`_ macro to compose service and form OpenAPI spec.**_
//!
//! ```rust
//! # use axum::Json;
//! # use utoipa::openapi::OpenApi;
//! # use canopy_utoipa_axum::{routes, PathItemExt, router::OpenApiRouter};
//!  #[derive(utoipa::ToSchema, serde::Serialize)]
//!  struct User {
//!      id: i32,
//!  }
//!
//!  #[utoipa::path(get, path = "/user", responses((status = OK, body = User)))]
//!  async fn get_user() -> Json<User> {
//!     Json(User { id: 1 })
//!  }
//!  
//!  let (router, api): (axum::Router, OpenApi) = OpenApiRouter::new()
//!      .routes(routes!(get_user))
//!      .split_for_parts();
//! ```
//!
//! [router]: router/struct.OpenApiRouter.html

pub mod router;

use axum::routing::MethodFilter;
use utoipa::openapi::HttpMethod;

/// Extends [`utoipa::openapi::path::PathItem`] by providing conversion methods to convert this
/// path item type to a [`axum::routing::MethodFilter`].
pub trait PathItemExt {
    /// Convert this path item type to a [`axum::routing::MethodFilter`].
    ///
    /// Method filter is used with handler registration on [`axum::routing::MethodRouter`].
    fn to_method_filter(&self) -> MethodFilter;
}

impl PathItemExt for HttpMethod {
    fn to_method_filter(&self) -> MethodFilter {
        match self {
            HttpMethod::Get => MethodFilter::GET,
            HttpMethod::Put => MethodFilter::PUT,
            HttpMethod::Post => MethodFilter::POST,
            HttpMethod::Head => MethodFilter::HEAD,
            HttpMethod::Patch => MethodFilter::PATCH,
            HttpMethod::Trace => MethodFilter::TRACE,
            HttpMethod::Delete => MethodFilter::DELETE,
            HttpMethod::Options => MethodFilter::OPTIONS,
        }
    }
}

/// re-export paste so users do not need to add the dependency.
#[doc(hidden)]
pub use paste::paste;

/// Collect axum handlers annotated with [`utoipa::path`] to [`router::UtoipaMethodRouter`].
///
/// [`routes`] macro will return [`router::UtoipaMethodRouter`] which contains an
/// [`axum::routing::MethodRouter`] and currently registered paths. The output of this macro is
/// meant to be used together with [`router::OpenApiRouter`] which combines the paths and axum
/// routers to a single entity.
///
/// Only handlers collected with [`routes`] macro will get registered to the OpenApi.
///
/// # Panics
///
/// Routes registered via [`routes`] macro or via `axum::routing::*` operations are bound to same
/// rules where only one one HTTP method can can be registered once per call. This means that the
/// following will produce runtime panic from axum code.
///
/// ```rust,no_run
/// # use canopy_utoipa_axum::{routes, router::UtoipaMethodRouter};
/// # use utoipa::path;
///  #[utoipa::path(get, path = "/search")]
///  async fn search_user() {}
///
///  #[utoipa::path(get, path = "")]
///  async fn get_user() {}
///
///  let _: UtoipaMethodRouter = routes!(get_user, search_user);
/// ```
/// Since the _`axum`_ does not support method filter for `CONNECT` requests, using this macro with
/// handler having request method type `CONNECT` `#[utoipa::path(connect, path = "")]` will panic at
/// runtime.
///
/// # Examples
///
/// _**Create new `OpenApiRouter` with `get_user` and `post_user` paths.**_
/// ```rust
/// # use canopy_utoipa_axum::{routes, router::{OpenApiRouter, UtoipaMethodRouter}};
/// # use utoipa::path;
///  #[utoipa::path(get, path = "")]
///  async fn get_user() {}
///
///  #[utoipa::path(post, path = "")]
///  async fn post_user() {}
///
///  let _: OpenApiRouter = OpenApiRouter::new().routes(routes!(get_user, post_user));
/// ```
#[macro_export]
macro_rules! routes {
    ( $handler:path $(, $tail:path)* $(,)? ) => {
        {
            use $crate::PathItemExt;
            let mut paths = utoipa::openapi::path::Paths::new();
            let mut schemas = Vec::<(String, utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>)>::new();
            let (path, item, types) = $crate::routes!(@resolve_types $handler : schemas);
            #[allow(unused_mut)]
            let mut method_router = types.iter().by_ref().fold(axum::routing::MethodRouter::new(), |router, path_type| {
                router.on(path_type.to_method_filter(), $handler)
            });
            paths.add_path_operation(&path, types, item);
            $( method_router = $crate::routes!( schemas: method_router: paths: $tail ); )*
            (schemas, paths, method_router)
        }
    };
    ( $schemas:tt: $router:ident: $paths:ident: $handler:path $(, $tail:tt)* ) => {
        {
            let (path, item, types) = $crate::routes!(@resolve_types $handler : $schemas);
            let router = types.iter().by_ref().fold($router, |router, path_type| {
                router.on(path_type.to_method_filter(), $handler)
            });
            $paths.add_path_operation(&path, types, item);
            router
        }
    };
    ( @resolve_types $handler:path : $schemas:tt ) => {
        {
            $crate::paste! {
                let path = $crate::routes!( @path [path()] of $handler );
                let mut operation = $crate::routes!( @path [operation()] of $handler );
                let types = $crate::routes!( @path [methods()] of $handler );
                let tags = $crate::routes!( @path [tags()] of $handler );
                $crate::routes!( @path [schemas(&mut $schemas)] of $handler );
                if !tags.is_empty() {
                    let operation_tags = operation.tags.get_or_insert(Vec::new());
                    operation_tags.extend(tags.iter().map(ToString::to_string));
                }
                (path, operation, types)
            }
        }
    };
    ( @path $op:tt of $part:ident $( :: $tt:tt )* ) => {
        $crate::routes!( $op : [ $part $( $tt )*] )
    };
    ( $op:tt : [ $first:tt $( $rest:tt )* ] $( $rev:tt )* ) => {
        $crate::routes!( $op : [ $( $rest )* ] $first $( $rev)* )
    };
    ( $op:tt : [] $first:tt $( $rest:tt )* ) => {
        $crate::routes!( @inverse $op : $first $( $rest )* )
    };
    ( @inverse $op:tt : $tt:tt $( $rest:tt )* ) => {
        $crate::routes!( @rev $op : $tt [$($rest)*] )
    };
    ( @rev $op:tt : $tt:tt [ $first:tt $( $rest:tt)* ] $( $reversed:tt )* ) => {
        $crate::routes!( @rev $op : $tt [ $( $rest )* ] $first $( $reversed )* )
    };
    ( @rev [$op:ident $( $args:tt )* ] : $handler:tt [] $($tt:tt)* ) => {
        {
            #[allow(unused_imports)]
            use utoipa::{Path, __dev::{Tags, SchemaReferences}};
            $crate::paste! {
                $( $tt :: )* [<__path_ $handler>]::$op $( $args )*
            }
        }
    };
    ( ) => {};
}

// CANOPY FORK: upstream's inline `#[cfg(test)] mod tests` (insta snapshot
// tests of the routes! macro) was removed to avoid vendoring its heavy
// dev-dependencies. The fork's own behaviour is covered by
// `tests/schema_collision.rs`.
