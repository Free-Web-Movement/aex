//! Procedural macros for the aex web framework.
//!
//! Provides the `#[aex::routes]` attribute for declaring routes on methods
//! inside an `impl` block and mounting them with a single instance.
//!
//! ## Middleware resolution
//!
//! Middleware array entries (`[auth]`, `[self.auth]`, `[logger!()]`) are
//! classified by the macro:
//!
//! | 写法              | 分类         | 查找顺序                                        |
//! |-------------------|--------------|-------------------------------------------------|
//! | `self.auth`       | SelfMethod   | 强制实例方法（`&self`，返回 `bool`）            |
//! | `auth`            | BareIdent    | 级联：self 方法 → 关联函数 → 全局函数           |
//! | `logger!()` 等    | Expr         | 经 `IntoExecutor` 转换                          |
//!
//! - `auth` 裸标识符：先在 impl 块中找同名 **实例方法**（`&self`，`→ bool`），
//!   再找同名 **关联函数**（无 self，`→ bool`），最后 fallback 为 **全局函数**
//!   （仅支持同步；异步全局函数需用 `_async!(|ctx| auth(ctx).await)` 包装）。
//! - `self.auth`：强制走实例方法路径，找不到或签名不匹配则编译报错。
//! - 其他表达式：直接经 `IntoExecutor` 转换为 `Arc<Executor>`。
//!
//! ## Example
//!
//! ```rust,ignore
//! use aex::connection::context::Context;
//! use aex::http::router::Router;
//!
//! struct Class {
//!     name: String,
//!     api_key: String,
//! }
//!
//! #[aex::routes]
//! impl Class {
//!     // `self.auth` → explicit instance method
//!     // `audit` → bare ident: try self.auth (no, different name) → assoc fn → global
//!     #[get(["/", "/profile"], [self.auth])]
//!     fn profile(&self, ctx: &mut Context) {
//!         ctx.text(&self.name);
//!     }
//!
//!     #[post("/resources", [self.auth])]
//!     async fn create(&self, ctx: &mut Context) {
//!         ctx.text("created");
//!     }
//!
//!     // instance method — must return bool, called via `self.auth`
//!     fn auth(&self, ctx: &mut Context) -> bool {
//!         ctx.header("x-api-key").is_some_and(|k| k == self.api_key)
//!     }
//! }
//!
//! // global fn — callable as bare `[require_key]`
//! fn require_key(ctx: &mut Context) -> bool {
//!     ctx.req().query("token") == Some("secret")
//! }
//!
//! let mut router = Router::default();
//! let instance = Class { name: "aex".into(), api_key: "secret".into() };
//! router.push(instance); // mounts all routes + middleware for this instance
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Expr, ImplItem, ImplItemFn, ItemImpl, Lit, ReturnType, Type, parse_macro_input,
    punctuated::Punctuated, token::Comma,
};

const HTTP_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("delete", "DELETE"),
    ("patch", "PATCH"),
    ("options", "OPTIONS"),
    ("head", "HEAD"),
    ("all", "*"),
];

enum ReceiverKind {
    None,
    Ref,
    Invalid,
}

/// Middleware resolution strategy.
///
/// - `BareIdent`: cascade lookup — self method → associated fn → global fn
/// - `SelfMethod`: explicit `self.method` — must be a `&self` method on the impl
/// - `Expr`: any other expression, passed through `IntoExecutor`
enum MiddlewareSpec {
    BareIdent(syn::Ident),
    SelfMethod(syn::Ident),
    Expr(Expr),
}

struct RouteDecl {
    http_method: &'static str,
    paths: Vec<Expr>,
    middlewares: Vec<MiddlewareSpec>,
    fn_name: syn::Ident,
    is_async: bool,
    returns_bool: bool,
    receiver: ReceiverKind,
}

fn receiver_kind(f: &ImplItemFn) -> ReceiverKind {
    match f.sig.receiver() {
        None => ReceiverKind::None,
        Some(r) if r.mutability.is_some() || r.reference.is_none() => ReceiverKind::Invalid,
        Some(_) => ReceiverKind::Ref,
    }
}

/// Middleware entry classification:
///
/// 1. `self.method` (Expr::Field, base = self) → **SelfMethod** — explicit
///    instance method, must be `&self` in this impl.
/// 2. Bare single-segment path (`auth`) → **BareIdent** — cascade lookup:
///    self method → associated fn → global fn.
/// 3. Anything else → **Expr** — ordinary value via `IntoExecutor`.
fn classify_middleware(e: &Expr) -> MiddlewareSpec {
    // `self.auth` → explicit instance method
    if let Expr::Field(field) = e {
        if let Expr::Path(base) = &*field.base {
            if base.qself.is_none() && base.path.is_ident("self") {
                if let syn::Member::Named(ident) = &field.member {
                    return MiddlewareSpec::SelfMethod(ident.clone());
                }
            }
        }
    }
    // bare ident → cascade: self method → assoc fn → global fn
    if let Expr::Path(p) = e {
        let single = p.qself.is_none()
            && p.path.leading_colon.is_none()
            && p.path.segments.len() == 1
            && p.path.segments[0].arguments.is_none();
        if single {
            return MiddlewareSpec::BareIdent(p.path.segments[0].ident.clone());
        }
    }
    MiddlewareSpec::Expr(e.clone())
}

fn is_route_attr(attr: &Attribute) -> bool {
    HTTP_METHODS
        .iter()
        .any(|(name, _)| attr.path().is_ident(name))
}

fn parse_route_decl(f: &ImplItemFn) -> syn::Result<RouteDecl> {
    let method = HTTP_METHODS
        .iter()
        .find(|(name, _)| f.attrs.iter().any(|a| a.path().is_ident(name)))
        .map(|(_, m)| *m)
        .ok_or_else(|| syn::Error::new_spanned(&f.sig.ident, "missing route attribute"))?;

    let attr = f
        .attrs
        .iter()
        .find(|a| HTTP_METHODS.iter().any(|(name, _)| a.path().is_ident(name)))
        .unwrap();

    let args: Punctuated<Expr, Comma> = attr.parse_args_with(Punctuated::parse_terminated)?;
    if args.is_empty() {
        return Err(syn::Error::new_spanned(
            &f.sig.ident,
            "route attribute requires at least one path",
        ));
    }

    let first = &args[0];
    let paths = match first {
        Expr::Lit(l) if matches!(l.lit, Lit::Str(_)) => vec![first.clone()],
        Expr::Array(arr) => {
            let mut v = Vec::new();
            for e in &arr.elems {
                if !matches!(e, Expr::Lit(l) if matches!(l.lit, Lit::Str(_))) {
                    return Err(syn::Error::new_spanned(e, "path must be a string literal"));
                }
                v.push(e.clone());
            }
            v
        }
        other => {
            return Err(syn::Error::new_spanned(
                other,
                "first argument must be a path string or an array of path strings",
            ));
        }
    };

    let middlewares = match args.get(1) {
        None => Vec::new(),
        Some(Expr::Array(arr)) => arr.elems.iter().map(classify_middleware).collect(),
        Some(other) => vec![classify_middleware(other)],
    };

    let returns_bool = matches!(
        &f.sig.output,
        ReturnType::Type(_, ty) if matches!(ty.as_ref(), Type::Path(p) if p.path.is_ident("bool"))
    );

    let receiver = receiver_kind(f);

    Ok(RouteDecl {
        http_method: method,
        paths,
        middlewares,
        fn_name: f.sig.ident.clone(),
        is_async: f.sig.asyncness.is_some(),
        returns_bool,
        receiver,
    })
}

/// Declares routes on an `impl` block and generates `AexRoutes` for it.
///
/// The generated handlers and object middlewares capture the mounted instance,
/// so `&self` methods can use the instance's state. Mount with
/// `Router::push(instance)`:
///
/// ```rust,ignore
/// #[aex::routes]
/// impl Class {
///     #[get(["/", "/profile"], [auth])]
///     fn profile(&self, ctx: &mut Context) { ... }
///
///     // object middleware: bare identifier, runs with `&self`
///     fn auth(&self, ctx: &mut Context) -> bool { ... }
/// }
/// let instance = Class { name: "aex".into() };
/// router.push(instance);
/// ```
#[proc_macro_attribute]
pub fn routes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut imp = parse_macro_input!(item as ItemImpl);
    imp.attrs.retain(|a| !a.path().is_ident("routes"));

    let mut decls = Vec::new();
    let mut errors: Vec<syn::Error> = Vec::new();

    for item in &mut imp.items {
        if let ImplItem::Fn(f) = item {
            if f.attrs.iter().any(is_route_attr) {
                match parse_route_decl(f) {
                    Ok(d) => decls.push(d),
                    Err(e) => errors.push(e),
                }
            }
            f.attrs.retain(|a| !is_route_attr(a));
        }
    }

    let mut methods: Vec<(syn::Ident, bool, bool, ReceiverKind)> = Vec::new();
    for item in &imp.items {
        if let ImplItem::Fn(f) = item {
            let is_async = f.sig.asyncness.is_some();
            let returns_bool = matches!(
                &f.sig.output,
                ReturnType::Type(_, ty) if matches!(ty.as_ref(), Type::Path(p) if p.path.is_ident("bool"))
            );
            methods.push((
                f.sig.ident.clone(),
                is_async,
                returns_bool,
                receiver_kind(f),
            ));
        }
    }

    if let Some(d) = decls
        .iter()
        .find(|d| matches!(d.receiver, ReceiverKind::Invalid))
    {
        errors.push(syn::Error::new_spanned(
            &d.fn_name,
            "route handlers must take `&self` or no receiver, not `&mut self`/`self`",
        ));
    }

    for d in &decls {
        for m in &d.middlewares {
            match m {
                MiddlewareSpec::BareIdent(ident) => {
                    // Cascade: if found in impl, validate; if not → free fn (compile error if absent)
                    if let Some((_, _, returns_bool, receiver)) =
                        methods.iter().find(|(n, _, _, _)| n == ident)
                    {
                        if matches!(receiver, ReceiverKind::Invalid) {
                            errors.push(syn::Error::new_spanned(
                                ident,
                                "middleware must take `&self` or no receiver, not `&mut self`/`self`",
                            ));
                        }
                        if !returns_bool {
                            errors.push(syn::Error::new_spanned(
                                ident,
                                "middleware must return `bool` (true = pass, false = block)",
                            ));
                        }
                    }
                    // Not found → free fn, compile error will surface if absent
                }
                MiddlewareSpec::SelfMethod(ident) => {
                    // Must be a `&self` method returning bool
                    match methods.iter().find(|(n, _, _, _)| n == ident) {
                        Some((_, _, true, ReceiverKind::Ref)) => {}
                        Some((_, _, false, ReceiverKind::Ref)) => {
                            errors.push(syn::Error::new_spanned(
                                ident,
                                "`self.method` middleware must return `bool`",
                            ));
                        }
                        Some((_, _, _, _)) => {
                            errors.push(syn::Error::new_spanned(
                                ident,
                                "`self.method` middleware must be a `&self` method (not `&mut self`, not associated)",
                            ));
                        }
                        None => {
                            errors.push(syn::Error::new_spanned(
                                ident,
                                "middleware method not found in this impl block",
                            ));
                        }
                    }
                }
                MiddlewareSpec::Expr(_) => {}
            }
        }
    }

    if !errors.is_empty() {
        let combined = errors
            .into_iter()
            .reduce(|mut acc, e| {
                acc.combine(e);
                acc
            })
            .unwrap();
        return combined.into_compile_error().into();
    }

    let generics = &imp.generics;
    let self_ty = &imp.self_ty;
    let where_clause = &imp.generics.where_clause;

    let mut stmts: Vec<TokenStream2> = Vec::new();
    for d in &decls {
        let method = d.http_method;
        let fn_name = &d.fn_name;

        for path in &d.paths {
            let mws = if d.middlewares.is_empty() {
                quote!(None)
            } else {
                let mws = d.middlewares.iter().map(|m| match m {
                    MiddlewareSpec::BareIdent(ident) => {
                        match methods.iter().find(|(n, _, _, _)| n == ident) {
                            // self method (async)
                            Some((_, true, _, ReceiverKind::Ref)) => quote! {
                                ::std::sync::Arc::new({
                                    let this = ::std::sync::Arc::clone(&this);
                                    move |ctx: &mut ::aex::connection::context::Context| {
                                        let this = ::std::sync::Arc::clone(&this);
                                        Box::pin(async move { this.#ident(ctx).await })
                                    }
                                })
                            },
                            // self method (sync)
                            Some((_, false, _, ReceiverKind::Ref)) => quote! {
                                ::aex::http::types::IntoExecutor::into_executor({
                                    let this = ::std::sync::Arc::clone(&this);
                                    move |ctx: &mut ::aex::connection::context::Context| this.#ident(ctx)
                                })
                            },
                            // associated fn (async, no self)
                            Some((_, true, _, ReceiverKind::None)) => quote! {
                                ::std::sync::Arc::new({
                                    move |ctx: &mut ::aex::connection::context::Context| {
                                        Box::pin(async move { Self::#ident(ctx).await })
                                    }
                                })
                            },
                            // associated fn (sync, no self)
                            Some((_, false, _, ReceiverKind::None)) => quote! {
                                ::aex::http::types::IntoExecutor::into_executor({
                                    move |ctx: &mut ::aex::connection::context::Context| Self::#ident(ctx)
                                })
                            },
                            // free function (not in impl block)
                            None => {
                                quote!(::aex::http::types::IntoExecutor::into_executor(#ident))
                            }
                            _ => unreachable!(),
                        }
                    }
                    MiddlewareSpec::SelfMethod(ident) => {
                        match methods.iter().find(|(n, _, _, _)| n == ident) {
                            Some((_, true, _, _)) => quote! {
                                ::std::sync::Arc::new({
                                    let this = ::std::sync::Arc::clone(&this);
                                    move |ctx: &mut ::aex::connection::context::Context| {
                                        let this = ::std::sync::Arc::clone(&this);
                                        Box::pin(async move { this.#ident(ctx).await })
                                    }
                                })
                            },
                            Some((_, false, _, _)) => quote! {
                                ::aex::http::types::IntoExecutor::into_executor({
                                    let this = ::std::sync::Arc::clone(&this);
                                    move |ctx: &mut ::aex::connection::context::Context| this.#ident(ctx)
                                })
                            },
                            None => unreachable!("validated above"),
                        }
                    }
                    MiddlewareSpec::Expr(e) => {
                        quote!(::aex::http::types::IntoExecutor::into_executor(#e))
                    }
                });
                quote!(Some(vec![#(#mws),*]))
            };

            let executor = match (&d.receiver, d.is_async) {
                (ReceiverKind::Ref, true) => {
                    let body = if d.returns_bool {
                        quote!(this.#fn_name(ctx).await)
                    } else {
                        quote!(this.#fn_name(ctx).await; true)
                    };
                    quote! {
                        {
                            let this = ::std::sync::Arc::clone(&this);
                            ::std::sync::Arc::new(move |ctx: &mut ::aex::connection::context::Context| {
                                let this = ::std::sync::Arc::clone(&this);
                                Box::pin(async move { #body })
                            })
                        }
                    }
                }
                (ReceiverKind::Ref, false) => {
                    quote! {
                        {
                            let this = ::std::sync::Arc::clone(&this);
                            ::std::sync::Arc::new(move |ctx: &mut ::aex::connection::context::Context| {
                                ::aex::http::types::HandlerOutput::into_boxed(this.#fn_name(ctx), ctx)
                            })
                        }
                    }
                }
                (ReceiverKind::None, true) => {
                    let body = if d.returns_bool {
                        quote!(Self::#fn_name(ctx).await)
                    } else {
                        quote!(Self::#fn_name(ctx).await; true)
                    };
                    quote!(::aex::_async!(move |ctx| { #body }))
                }
                (ReceiverKind::None, false) => {
                    quote! {
                        ::std::sync::Arc::new(move |ctx: &mut ::aex::connection::context::Context| {
                            ::aex::http::types::HandlerOutput::into_boxed(Self::#fn_name(ctx), ctx)
                        })
                    }
                }
                (ReceiverKind::Invalid, _) => unreachable!(),
            };

            stmts.push(quote! {
                router.insert(
                    #path,
                    Some(#method),
                    #executor,
                    #mws,
                );
            });
        }
    }

    let output = quote! {
        #imp

        impl #generics ::aex::http::router::AexRoutes for #self_ty #where_clause {
            fn __aex_register(router: &mut ::aex::http::router::Router, this: ::std::sync::Arc<Self>) {
                #(#stmts)*
            }
        }
    };
    output.into()
}
