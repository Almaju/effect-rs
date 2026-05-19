//! Procedural macros for the [`effect`] crate.
//!
//! Currently exposes:
//! - [`Newtype`] — derive for opaque single-field tuple structs.
//!
//! All macros are re-exported from `effect` itself; users should depend
//! on `effect`, not on `effect-macros` directly.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type, parse_macro_input};

/// Derive macro for opaque newtype tuple structs.
///
/// Generates:
/// - `impl From<Inner> for Self`
/// - `impl From<Self> for Inner`
/// - `impl AsRef<Inner> for Self`
/// - inherent `new(inner)` and `into_inner(self)`
///
/// Does **not** generate `Display`, `Deref`, `Debug`, `Hash`, `Clone`,
/// or `PartialEq` — those stay user-controlled. Add them with normal
/// `#[derive(...)]` if you want them.
///
/// ```ignore
/// use effect::Newtype;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Newtype)]
/// pub struct UserId(u64);
///
/// let id: UserId = UserId::new(42);
/// let raw: u64 = id.into_inner();
/// let from: UserId = 1u64.into();
/// ```
///
/// Compile errors are produced for non-tuple-structs and tuple structs
/// with more than one field.
#[proc_macro_derive(Newtype)]
pub fn derive_newtype(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_newtype(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_newtype(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let inner = extract_inner(input)?;

    Ok(quote! {
        impl ::core::convert::From<#inner> for #name {
            #[inline]
            fn from(value: #inner) -> Self { Self(value) }
        }

        impl ::core::convert::From<#name> for #inner {
            #[inline]
            fn from(value: #name) -> Self { value.0 }
        }

        impl ::core::convert::AsRef<#inner> for #name {
            #[inline]
            fn as_ref(&self) -> &#inner { &self.0 }
        }

        impl #name {
            /// Wrap an inner value.
            #[inline]
            pub fn new(value: #inner) -> Self { Self(value) }

            /// Unwrap the inner value, consuming the newtype.
            #[inline]
            pub fn into_inner(self) -> #inner { self.0 }
        }
    })
}

fn extract_inner(input: &DeriveInput) -> syn::Result<&Type> {
    match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Unnamed(f) if f.unnamed.len() == 1 => Ok(&f.unnamed[0].ty),
            Fields::Unnamed(_) => Err(syn::Error::new_spanned(
                input,
                "#[derive(Newtype)] requires a tuple struct with exactly one field",
            )),
            Fields::Named(_) => Err(syn::Error::new_spanned(
                input,
                "#[derive(Newtype)] requires a tuple struct, not a named-field struct",
            )),
            Fields::Unit => Err(syn::Error::new_spanned(
                input,
                "#[derive(Newtype)] cannot be applied to a unit struct",
            )),
        },
        Data::Enum(_) => Err(syn::Error::new_spanned(
            input,
            "#[derive(Newtype)] cannot be applied to enums",
        )),
        Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "#[derive(Newtype)] cannot be applied to unions",
        )),
    }
}
