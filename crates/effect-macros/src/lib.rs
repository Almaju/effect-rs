//! Procedural macros for the [`effect`] crate.
//!
//! - [`Newtype`] — derive for opaque single-field tuple structs.
//! - [`Brand`]   — derive a validated `try_new` constructor for a
//!   newtype, given a `Refinement` impl.
//! - [`Schema`]  — derive parse/encode/json_schema for named-field
//!   structs, treating `Option<T>` fields as optional.
//!
//! All macros are re-exported from `effect`; users should depend on
//! `effect`, not on `effect-macros` directly.

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

/// Derive a `try_new` smart constructor for a newtype, given a
/// [`Refinement`](https://docs.rs/effect/latest/effect/refinement/trait.Refinement.html)
/// impl that validates the inner value.
///
/// ```ignore
/// use effect::{Brand, Newtype, Refinement};
///
/// pub struct EmailRefiner;
/// impl Refinement<String> for EmailRefiner {
///     type Error = String;
///     fn check(value: &String) -> Result<(), Self::Error> {
///         if !value.contains('@') {
///             return Err("missing '@'".into());
///         }
///         Ok(())
///     }
/// }
///
/// #[derive(Debug, Clone, PartialEq, Eq, Newtype, Brand)]
/// #[brand(refine_with = EmailRefiner)]
/// pub struct Email(String);
///
/// let ok  = Email::try_new("a@b".to_string());
/// let bad = Email::try_new("nope".to_string());  // Err
/// ```
///
/// Compile errors if `#[brand(refine_with = …)]` is missing.
#[proc_macro_derive(Brand, attributes(brand))]
pub fn derive_brand(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_brand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_brand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let inner = extract_inner(input)?;
    let refiner = extract_refiner(input)?;

    Ok(quote! {
        impl #name {
            /// Validated constructor — checks the inner value against
            /// the refinement before wrapping.
            pub fn try_new(
                value: #inner,
            ) -> ::core::result::Result<
                Self,
                <#refiner as ::effect::Refinement<#inner>>::Error,
            > {
                <#refiner as ::effect::Refinement<#inner>>::check(&value)?;
                ::core::result::Result::Ok(Self(value))
            }
        }
    })
}

fn extract_refiner(input: &DeriveInput) -> syn::Result<syn::Path> {
    let mut refiner: Option<syn::Path> = None;
    for attr in &input.attrs {
        if attr.path().is_ident("brand") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("refine_with") {
                    refiner = Some(meta.value()?.parse()?);
                    Ok(())
                } else {
                    Err(meta.error("unknown #[brand(...)] key; expected `refine_with`"))
                }
            })?;
        }
    }
    refiner.ok_or_else(|| {
        syn::Error::new_spanned(
            input,
            "#[derive(Brand)] requires #[brand(refine_with = SomeRefiner)]",
        )
    })
}

// ── #[derive(Schema)] ─────────────────────────────────────────────

/// Derive macro for `Schema` on named-field structs.
///
/// Generated impl:
/// - `parse_json`: requires an object; each field is parsed, errors
///   carry the field name via `SchemaError::at_field`.
/// - `encode_json`: emits a JSON object with all fields (`None`
///   encoded as `null`).
/// - `json_schema`: emits a JSON Schema `object` with `properties` and
///   `required` (Option fields are excluded from `required`).
///
/// `Option<T>` fields are treated as optional in the input — a missing
/// key parses as `None` (in addition to `null`).
///
/// ```ignore
/// use effect::Schema;
///
/// #[derive(Schema)]
/// pub struct User {
///     pub name: String,
///     pub age: u32,
///     pub nickname: Option<String>,
/// }
/// ```
#[proc_macro_derive(Schema)]
pub fn derive_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_schema(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_schema(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    let Data::Struct(s) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "#[derive(Schema)] supports structs only (enums and unions are TODO)",
        ));
    };
    let Fields::Named(fields) = &s.fields else {
        return Err(syn::Error::new_spanned(
            input,
            "#[derive(Schema)] requires a struct with named fields",
        ));
    };

    let mut parse_arms = Vec::new();
    let mut encode_inserts = Vec::new();
    let mut property_inserts = Vec::new();
    let mut required_lits = Vec::new();

    for field in &fields.named {
        let ident = field
            .ident
            .as_ref()
            .expect("named field has an identifier");
        let ty = &field.ty;
        let name_str = ident.to_string();
        let is_opt = is_option_type(ty);

        if is_opt {
            parse_arms.push(quote::quote! {
                #ident: match __obj.get(#name_str) {
                    ::core::option::Option::Some(__v) => {
                        <#ty as ::effect_schema::Schema>::parse_json(__v)
                            .map_err(|__e| ::effect_schema::SchemaError::at_field(#name_str, __e))?
                    }
                    ::core::option::Option::None => ::core::option::Option::None,
                },
            });
        } else {
            parse_arms.push(quote::quote! {
                #ident: {
                    let __v = __obj.get(#name_str).ok_or_else(
                        || ::effect_schema::SchemaError::missing_field(#name_str)
                    )?;
                    <#ty as ::effect_schema::Schema>::parse_json(__v)
                        .map_err(|__e| ::effect_schema::SchemaError::at_field(#name_str, __e))?
                },
            });
            required_lits.push(quote::quote! {
                ::effect_schema::serde_json::Value::String(#name_str.to_string())
            });
        }

        encode_inserts.push(quote::quote! {
            __obj.insert(
                #name_str.to_string(),
                <#ty as ::effect_schema::Schema>::encode_json(&self.#ident),
            );
        });

        property_inserts.push(quote::quote! {
            __props.insert(
                #name_str.to_string(),
                <#ty as ::effect_schema::Schema>::json_schema(),
            );
        });
    }

    Ok(quote::quote! {
        impl ::effect_schema::Schema for #name {
            fn parse_json(
                __input: &::effect_schema::serde_json::Value,
            ) -> ::core::result::Result<Self, ::effect_schema::SchemaError> {
                let __obj = __input.as_object().ok_or_else(
                    || ::effect_schema::SchemaError::type_mismatch("object", __input)
                )?;
                ::core::result::Result::Ok(Self {
                    #(#parse_arms)*
                })
            }

            fn encode_json(&self) -> ::effect_schema::serde_json::Value {
                let mut __obj = ::effect_schema::serde_json::Map::new();
                #(#encode_inserts)*
                ::effect_schema::serde_json::Value::Object(__obj)
            }

            fn json_schema() -> ::effect_schema::serde_json::Value {
                let mut __props = ::effect_schema::serde_json::Map::new();
                #(#property_inserts)*
                ::effect_schema::serde_json::Value::Object({
                    let mut __spec = ::effect_schema::serde_json::Map::new();
                    __spec.insert("type".to_string(), ::effect_schema::serde_json::Value::String("object".to_string()));
                    __spec.insert("properties".to_string(), ::effect_schema::serde_json::Value::Object(__props));
                    __spec.insert(
                        "required".to_string(),
                        ::effect_schema::serde_json::Value::Array(vec![#(#required_lits),*]),
                    );
                    __spec
                })
            }
        }
    })
}

/// Recognize `Option<T>` (or any path ending in `Option`). Doesn't
/// handle `Option<T>` aliased as another name.
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(p) = ty {
        if let Some(last) = p.path.segments.last() {
            return last.ident == "Option";
        }
    }
    false
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
