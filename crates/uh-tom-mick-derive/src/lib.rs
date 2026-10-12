//! Additive Uh Tom Mick atomic daemon attributes. `uh_tom_mick` consumes
//! helper attributes on impl methods; it does not alter sockets or install routes.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::Parser, parse_macro_input, punctuated::Punctuated, Expr, FnArg, ImplItem, ImplItemFn,
    ItemImpl, ItemStruct, Lit, Meta, Pat, ReturnType, Token, Type,
};

fn parse_admission(
    args: proc_macro2::TokenStream,
    item: ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    if !matches!(&item.fields, syn::Fields::Unit)
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            &item,
            "atomic admission must annotate a nongeneric unit struct",
        ));
    }
    let args = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut limit = None;
    for arg in args {
        match arg {
            Meta::NameValue(v) if v.path.is_ident("max_active") => {
                if limit.is_some() {
                    return Err(syn::Error::new_spanned(v, "duplicate max_active"));
                }
                if let Expr::Lit(e) = &v.value {
                    if let Lit::Int(n) = &e.lit {
                        let value: u32 = n.base10_parse()?;
                        if value == 0 {
                            return Err(syn::Error::new_spanned(n, "max_active must be nonzero"));
                        }
                        limit = Some(value);
                        continue;
                    }
                }
                return Err(syn::Error::new_spanned(
                    v,
                    "max_active must be a positive u32 literal",
                ));
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "expected max_active = <positive u32>",
                ))
            }
        }
    }
    let limit = limit.ok_or_else(|| syn::Error::new_spanned(&item, "missing max_active"))?;
    for attr in &item.attrs {
        if !attr.path().is_ident("doc") {
            return Err(syn::Error::new_spanned(
                attr,
                "only documentation attributes are supported on an atomic admission marker",
            ));
        }
    }
    let name = &item.ident;
    let vis = &item.vis;
    let attrs = &item.attrs;
    Ok(quote! {
        #(#attrs)*
        #[derive(Debug)]
        #vis struct #name { inner: ::uh_tom_mick::AtomicAdmission }
        impl #name {
            pub fn new() -> Self {
                Self { inner: ::uh_tom_mick::AtomicAdmission::new(#limit)
                    .expect("attribute validates nonzero capacity") }
            }
            pub fn limit(&self) -> u32 { self.inner.limit() }
            pub fn snapshot(&self) -> ::uh_tom_mick::AdmissionSnapshot { self.inner.snapshot() }
            pub fn active(&self) -> u32 { self.inner.active() }
            pub fn is_stopping(&self) -> bool { self.inner.is_stopping() }
            pub fn is_drained(&self) -> bool { self.inner.is_drained() }
            pub fn drain_status(&self) -> ::uh_tom_mick::DrainStatus { self.inner.drain_status() }
            pub fn begin_shutdown(&self) -> u32 { self.inner.begin_shutdown() }
            pub fn try_enter(&self) -> Result<::uh_tom_mick::AdmissionGuard<'_>,
                ::uh_tom_mick::AdmissionError> { self.inner.try_enter() }
        }
        impl ::core::default::Default for #name { fn default() -> Self { Self::new() } }
    })
}

#[proc_macro_attribute]
pub fn atomic_admission(args: TokenStream, item: TokenStream) -> TokenStream {
    let original = parse_macro_input!(item as ItemStruct);
    match parse_admission(args.into(), original) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn track_field(attribute: &syn::Attribute) -> syn::Result<syn::Ident> {
    let args = attribute.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    if args.len() != 1 {
        return Err(syn::Error::new_spanned(
            attribute,
            "expected admission = field_name",
        ));
    }
    match &args[0] {
        Meta::NameValue(v) if v.path.is_ident("admission") => match &v.value {
            Expr::Path(p) if p.path.get_ident().is_some() => {
                Ok(p.path.get_ident().unwrap().clone())
            }
            _ => Err(syn::Error::new_spanned(
                &v.value,
                "admission must be a direct self field name",
            )),
        },
        other => Err(syn::Error::new_spanned(
            other,
            "expected admission = field_name",
        )),
    }
}

fn tracked_wrapper(
    method: &ImplItemFn,
    field: &syn::Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "tracked methods must be async",
        ));
    }
    if !method.sig.generics.params.is_empty()
        || method.sig.generics.where_clause.is_some()
        || method.sig.constness.is_some()
        || method.sig.unsafety.is_some()
        || method.sig.abi.is_some()
        || method.sig.variadic.is_some()
    {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "tracked methods must use nongeneric, safe, async methods",
        ));
    }
    let mut inputs = method.sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_none() => (),
        _ => {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "tracked method requires &self receiver",
            ))
        }
    }
    let mut parameters = Vec::new();
    let mut call_args = Vec::new();
    for input in inputs {
        match input {
            FnArg::Typed(t) => match t.pat.as_ref() {
                Pat::Ident(i) if i.by_ref.is_none() && i.subpat.is_none() => {
                    parameters.push(t.clone());
                    call_args.push(i.ident.clone());
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &t.pat,
                        "tracked parameters must be simple identifiers",
                    ))
                }
            },
            _ => return Err(syn::Error::new_spanned(input, "unexpected receiver")),
        }
    }
    let return_type: Type = match &method.sig.output {
        ReturnType::Default => syn::parse_quote! { () },
        ReturnType::Type(_, ty) => (*ty.as_ref()).clone(),
    };
    let vis = &method.vis;
    let name = &method.sig.ident;
    let wrapper = format_ident!("__uh_tom_mick_dispatch_{}", name);
    Ok(quote! {
        #[doc(hidden)]
        #vis async fn #wrapper(&self, #(#parameters),*) -> ::core::result::Result<#return_type,
            ::uh_tom_mick::AdmissionError> {
            let _uh_tom_mick_admission_guard = self.#field.try_enter()?;
            ::core::result::Result::Ok(self.#name(#(#call_args),*).await)
        }
    })
}
fn expand_impl(
    args: proc_macro2::TokenStream,
    mut implementation: ItemImpl,
) -> syn::Result<proc_macro2::TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "uh_tom_mick takes no arguments",
        ));
    }
    let mut wrappers = Vec::new();
    for member in &mut implementation.items {
        let ImplItem::Fn(method) = member else {
            continue;
        };
        let mut field = None;
        for attr in &method.attrs {
            if attr.path().is_ident("uh_tom_mick_track") {
                if field.is_some() {
                    return Err(syn::Error::new_spanned(attr, "duplicate uh_tom_mick_track"));
                }
                field = Some(track_field(attr)?);
            }
        }
        if let Some(field) = field {
            if implementation.trait_.is_some() {
                return Err(syn::Error::new_spanned(
                    &method.sig,
                    "cannot generate dispatch methods within a trait impl",
                ));
            }
            wrappers.push(tracked_wrapper(method, &field)?);
            method
                .attrs
                .retain(|a| !a.path().is_ident("uh_tom_mick_track"));
        }
    }
    implementation.items.extend(
        wrappers
            .into_iter()
            .map(|t| syn::parse2::<ImplItemFn>(t).map(ImplItem::Fn))
            .collect::<syn::Result<Vec<_>>>()?,
    );
    Ok(quote! { #implementation })
}
#[proc_macro_attribute]
pub fn uh_tom_mick(args: TokenStream, item: TokenStream) -> TokenStream {
    let original = parse_macro_input!(item as ItemImpl);
    match expand_impl(args.into(), original) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Alias for [`uh_tom_mick`].
#[proc_macro_attribute]
pub fn atomic_daemon(args: TokenStream, item: TokenStream) -> TokenStream {
    let original = parse_macro_input!(item as ItemImpl);
    match expand_impl(args.into(), original) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// Generate a typed checked counter. This is additive and never rewrites
/// arbitrary native AtomicU64 / Mutex / Arc fields elsewhere in the daemon.
fn parse_counter(
    args: proc_macro2::TokenStream,
    item: ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    if !matches!(&item.fields, syn::Fields::Unit)
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            &item,
            "atomic counter requires a nongeneric unit struct",
        ));
    }
    for a in &item.attrs {
        if !a.path().is_ident("doc") {
            return Err(syn::Error::new_spanned(
                a,
                "atomic counter only supports documentation attributes",
            ));
        }
    }
    let parsed = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
    let mut initial = None;
    for value in parsed {
        match value {
            Meta::NameValue(ref v) if v.path.is_ident("initial") => {
                if initial.is_some() {
                    return Err(syn::Error::new_spanned(v, "duplicate initial"));
                }
                match &v.value {
                    Expr::Lit(l) => match &l.lit {
                        Lit::Int(number) => {
                            initial = Some(number.base10_parse::<u64>()?);
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                v,
                                "initial requires a u64 literal",
                            ))
                        }
                    },
                    _ => return Err(syn::Error::new_spanned(v, "initial requires a u64 literal")),
                }
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "expected initial = <u64 literal>",
                ))
            }
        }
    }
    let initial = initial.unwrap_or(0);
    let name = &item.ident;
    let vis = &item.vis;
    let docs = &item.attrs;
    Ok(quote! {
        #(#docs)*
        #[derive(Debug)]
        #vis struct #name { inner: ::uh_tom_mick::AtomicCounter }
        impl #name {
            pub fn new() -> Self { Self { inner: ::uh_tom_mick::AtomicCounter::new(#initial) } }
            pub fn with_initial(initial: u64) -> Self { Self { inner: ::uh_tom_mick::AtomicCounter::new(initial) } }
            pub fn load(&self) -> u64 { self.inner.load() }
            pub fn add(&self, amount: u64) -> ::core::result::Result<u64, ::uh_tom_mick::CounterError> { self.inner.add(amount) }
            pub fn increment(&self) -> ::core::result::Result<u64, ::uh_tom_mick::CounterError> { self.inner.increment() }
            pub fn subtract(&self, amount: u64) -> ::core::result::Result<u64, ::uh_tom_mick::CounterError> { self.inner.subtract(amount) }
            pub fn decrement(&self) -> ::core::result::Result<u64, ::uh_tom_mick::CounterError> { self.inner.decrement() }
            pub fn reset(&self) -> u64 { self.inner.reset() }
        }
        impl ::core::default::Default for #name { fn default() -> Self { Self::new() } }
    })
}
#[proc_macro_attribute]
pub fn atomic_counter(args: TokenStream, item: TokenStream) -> TokenStream {
    let original = parse_macro_input!(item as ItemStruct);
    match parse_counter(args.into(), original) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// A unit-variant enum is the compile-time source of truth for named flag bits.
/// Generated Atomic<Name> delegates to the shared flag-set core implementation.
fn parse_flags(
    args: proc_macro2::TokenStream,
    item: syn::ItemEnum,
) -> syn::Result<proc_macro2::TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "atomic flags takes no arguments",
        ));
    }
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item,
            "atomic flags enum must be nongeneric",
        ));
    }
    for a in &item.attrs {
        if !a.path().is_ident("doc") {
            return Err(syn::Error::new_spanned(
                a,
                "atomic flags enum only supports documentation attributes",
            ));
        }
    }
    let n = item.variants.len();
    if n == 0 || n > 64 {
        return Err(syn::Error::new_spanned(
            &item,
            "atomic flags require 1 to 64 unit variants",
        ));
    }
    let mut variants = Vec::new();
    let mut arms = Vec::new();
    for (index, variant) in item.variants.iter().enumerate() {
        if !matches!(&variant.fields, syn::Fields::Unit) || variant.discriminant.is_some() {
            return Err(syn::Error::new_spanned(
                variant,
                "flags must have unit variants without explicit discriminants",
            ));
        }
        for attr in &variant.attrs {
            if !attr.path().is_ident("doc") {
                return Err(syn::Error::new_spanned(
                    attr,
                    "flags variants only support documentation attributes",
                ));
            }
        }
        let ident = &variant.ident;
        let docs = &variant.attrs;
        variants.push(quote! { #(#docs)* #ident });
        arms.push(quote! { Self::#ident => 1u64 << #index });
    }
    let known: u64 = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
    let name = &item.ident;
    let vis = &item.vis;
    let docs = &item.attrs;
    let atomic_name = format_ident!("Atomic{}", name);
    Ok(quote! {
        #(#docs)*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #vis enum #name { #(#variants,)* }
        impl #name {
            pub const fn bit(self) -> u64 { match self { #(#arms,)* } }
        }
        #[derive(Debug)]
        #vis struct #atomic_name { inner: ::uh_tom_mick::AtomicFlagSet }
        impl #atomic_name {
            pub const KNOWN_MASK: u64 = #known;
            pub fn new() -> Self { Self::with_bits(0).expect("generated schema is nonempty") }
            pub fn with_bits(bits: u64) -> ::core::result::Result<Self, ::uh_tom_mick::FlagSetError> {
                Ok(Self { inner: ::uh_tom_mick::AtomicFlagSet::new(Self::KNOWN_MASK, bits)? })
            }
            pub fn bits(&self) -> u64 { self.inner.bits() }
            pub fn is_set(&self, flag: #name) -> bool { self.bits() & flag.bit() != 0 }
            /// Returns whether the flag was previously enabled.
            pub fn enable(&self, flag: #name) -> bool {
                self.inner.set_bits(flag.bit()).expect("enum bit must be known") & flag.bit() != 0
            }
            /// Returns whether the flag was previously enabled.
            pub fn disable(&self, flag: #name) -> bool {
                self.inner.clear_bits(flag.bit()).expect("enum bit must be known") & flag.bit() != 0
            }
            /// Returns whether the flag is enabled AFTER toggling.
            pub fn toggle(&self, flag: #name) -> bool {
                self.inner.toggle_bits(flag.bit()).expect("enum bit must be known") & flag.bit() == 0
            }
            pub fn all(&self, flags: &[#name]) -> bool {
                let mask = flags.iter().fold(0u64, |mask, flag| mask | flag.bit());
                mask == 0 || self.inner.contains_all(mask).expect("enum mask must be known")
            }
            pub fn any(&self, flags: &[#name]) -> bool {
                let mask = flags.iter().fold(0u64, |mask, flag| mask | flag.bit());
                mask != 0 && self.inner.contains_any(mask).expect("enum mask must be known")
            }
            /// Atomic whole-word transition for protocols needing related flags together.
            /// Invalid raw values are rejected before any atomic operation.
            pub fn compare_exchange(&self, expected: u64, desired: u64)
                -> ::core::result::Result<::core::result::Result<u64, u64>, ::uh_tom_mick::FlagSetError>
            { self.inner.compare_exchange(expected, desired) }
        }
        impl ::core::default::Default for #atomic_name { fn default() -> Self { Self::new() } }
    })
}
#[proc_macro_attribute]
pub fn atomic_flags(args: TokenStream, item: TokenStream) -> TokenStream {
    let original = parse_macro_input!(item as syn::ItemEnum);
    match parse_flags(args.into(), original) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    fn check_ok(src: proc_macro2::TokenStream) {
        let result = syn::parse2::<ItemImpl>(src).unwrap();
        assert!(expand_impl(quote!(), result).is_ok());
    }
    fn check_bad(src: proc_macro2::TokenStream, expected: &str) {
        let result = syn::parse2::<ItemImpl>(src).unwrap();
        let error = expand_impl(quote!(), result).unwrap_err().to_string();
        assert!(
            error.contains(expected),
            "{error:?} did not contain {expected:?}"
        );
    }
    #[test]
    fn parses_one_tracked_method() {
        check_ok(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn call(&self, x: u32) -> u32 { x } }),
        );
    }
    #[test]
    fn parses_two_tracked_methods() {
        check_ok(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a(&self) {} #[uh_tom_mick_track(admission = gate)] async fn b(&self, x: u32) { let _ = x; } }),
        );
    }
    #[test]
    fn preserves_untracked_methods() {
        check_ok(quote!(impl S { fn a(&self) {} }));
    }
    #[test]
    fn rejects_sync_method() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] fn a(&self) {} }),
            "must be async",
        );
    }
    #[test]
    fn rejects_owned_self() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a(self) {} }),
            "requires &self",
        );
    }
    #[test]
    fn rejects_mut_self() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a(&mut self) {} }),
            "requires &self",
        );
    }
    #[test]
    fn rejects_generic() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a<T>(&self, _: T) {} }),
            "nongeneric",
        );
    }
    #[test]
    fn rejects_wildcard() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a(&self, _: u32) {} }),
            "simple identifiers",
        );
    }
    #[test]
    fn rejects_invalid_track_key() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(foo = gate)] async fn a(&self) {} }),
            "expected admission",
        );
    }
    #[test]
    fn rejects_multiple_track_params() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate, other = foo)] async fn a(&self) {} }),
            "expected admission",
        );
    }
    #[test]
    fn rejects_duplicate_track() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] #[uh_tom_mick_track(admission = gate)] async fn a(&self) {} }),
            "duplicate",
        );
    }
    #[test]
    fn rejects_trait_impl() {
        check_bad(
            quote!(impl Thing for S { #[uh_tom_mick_track(admission = gate)] async fn a(&self) {} }),
            "trait impl",
        );
    }
    #[test]
    fn rejects_args_on_parent() {
        let item: ItemImpl = syn::parse2(quote!(impl S {})).unwrap();
        assert!(expand_impl(quote!(unused), item)
            .unwrap_err()
            .to_string()
            .contains("takes no arguments"));
    }
    #[test]
    fn preserves_original_body_and_emits_adapter() {
        let item: ItemImpl = syn::parse2(quote!(impl S { #[uh_tom_mick_track(admission = gate)] pub async fn read(&self, request: u32) -> u32 { request + 5 } })).unwrap();
        let tokens = expand_impl(quote!(), item).unwrap().to_string();
        assert!(tokens.contains("request + 5"));
        assert!(tokens.contains("__uh_tom_mick_dispatch_read"));
        assert!(tokens.contains("self . gate . try_enter"));
        assert!(!tokens.contains("# [uh_tom_mick_track"));
    }
    #[test]
    fn admission_rejects_zero() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 0), item).is_err());
    }
    #[test]
    fn admission_accepts_max_u32() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 4294967295), item).is_ok());
    }
    #[test]
    fn admission_rejects_duplicate_limit() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 1, max_active = 2), item).is_err());
    }
    #[test]
    fn admission_rejects_missing_limit() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(), item).is_err());
    }
    #[test]
    fn admission_rejects_unknown_param() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(foo = 4), item).is_err());
    }
    #[test]
    fn admission_rejects_named_struct() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A {
                x: u32,
            }
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 4), item).is_err());
    }
    #[test]
    fn admission_rejects_tuple_struct() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A(u32);
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 4), item).is_err());
    }
    #[test]
    fn admission_rejects_fractional_max() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 1.5), item).is_err());
    }
    #[test]
    fn admission_rejects_string_max() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = "4"), item).is_err());
    }
    #[test]
    fn admission_rejects_negative_max() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = -1), item).is_err());
    }
    #[test]
    fn admission_rejects_overflow_max() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 4294967296), item).is_err());
    }
    #[test]
    fn admission_accepts_one() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 1), item).is_ok());
    }
    #[test]
    fn admission_rejects_derive_attribute() {
        let item: ItemStruct = syn::parse2(quote!(
            #[derive(Default)]
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 1), item).is_err());
    }
    #[test]
    fn admission_accepts_docs() {
        let item: ItemStruct = syn::parse2(quote!(
            #[doc = "A gate"]
            struct A;
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 2), item).is_ok());
    }
    #[test]
    fn rejects_no_receiver() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a() {} }),
            "requires &self",
        );
    }
    #[test]
    fn rejects_destructured_input() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async fn a(&self, (x,y): (u32,u32)) {} }),
            "simple identifiers",
        );
    }
    #[test]
    fn rejects_expression_admission() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = self.gate)] async fn a(&self) {} }),
            "direct self field",
        );
    }
    #[test]
    fn rejects_unsafe_method() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] async unsafe fn a(&self) {} }),
            "nongeneric",
        );
    }
    #[test]
    fn rejects_const_method() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] const fn a(&self) {} }),
            "must be async",
        );
    }
    #[test]
    fn rejects_track_without_args() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track] async fn a(&self) {} }),
            "expected",
        );
    }
    #[test]
    fn rejects_field_literal() {
        check_bad(
            quote!(impl S { #[uh_tom_mick_track(admission = "gate")] async fn a(&self) {} }),
            "direct self field",
        );
    }
    #[test]
    fn respects_original_public_visibility() {
        let item: ItemImpl = syn::parse2(
            quote!(impl S { #[uh_tom_mick_track(admission = gate)] pub async fn f(&self) {} }),
        )
        .unwrap();
        let expanded = expand_impl(quote!(), item).unwrap().to_string();
        assert!(expanded.contains("pub async fn __uh_tom_mick_dispatch_f"));
    }
    #[test]
    fn admission_rejects_generic_struct() {
        let item: ItemStruct = syn::parse2(quote!(
            struct A<T>(T);
        ))
        .unwrap();
        assert!(parse_admission(quote!(max_active = 4), item).is_err());
    }
    #[test]
    fn counter_defaults_to_zero() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(), t)
            .unwrap()
            .to_string()
            .contains("AtomicCounter"));
    }
    #[test]
    fn counter_custom_initial() {
        let t: ItemStruct = syn::parse2(quote!(
            pub struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(initial = 42), t)
            .unwrap()
            .to_string()
            .contains("42"));
    }
    #[test]
    fn counter_max_initial() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(initial = 18446744073709551615), t).is_ok());
    }
    #[test]
    fn counter_rejects_duplicate() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(initial = 1, initial = 2), t).is_err());
    }
    #[test]
    fn counter_rejects_negative() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(initial = -1), t).is_err());
    }
    #[test]
    fn counter_rejects_float() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(initial = 1.5), t).is_err());
    }
    #[test]
    fn counter_rejects_unknown_arg() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(foo = 1), t).is_err());
    }
    #[test]
    fn counter_rejects_nonunit() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count(u64);
        ))
        .unwrap();
        assert!(parse_counter(quote!(), t).is_err());
    }
    #[test]
    fn counter_rejects_generic() {
        let t: ItemStruct = syn::parse2(quote!(
            struct Count<T>(T);
        ))
        .unwrap();
        assert!(parse_counter(quote!(), t).is_err());
    }
    #[test]
    fn counter_rejects_derive_conflict() {
        let t: ItemStruct = syn::parse2(quote!(
            #[derive(Default)]
            struct Count;
        ))
        .unwrap();
        assert!(parse_counter(quote!(), t).is_err());
    }
    #[test]
    fn flags_generates_named_bits() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            pub enum F {
                First,
                Second,
                Third,
            }
        ))
        .unwrap();
        let v = parse_flags(quote!(), t).unwrap().to_string();
        assert!(v.contains("AtomicF"));
        assert!(v.contains("KNOWN_MASK"));
    }
    #[test]
    fn flags_accepts_64() {
        let ids: Vec<_> = (0usize..64).map(|i| format_ident!("Flag{}", i)).collect();
        let t: syn::ItemEnum = syn::parse2(quote!(enum Flags { #(#ids,)* })).unwrap();
        assert!(parse_flags(quote!(), t)
            .unwrap()
            .to_string()
            .contains("18446744073709551615"));
    }
    #[test]
    fn flags_rejects_empty() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            enum F {}
        ))
        .unwrap();
        assert!(parse_flags(quote!(), t).is_err());
    }
    #[test]
    fn flags_rejects_65() {
        let ids: Vec<_> = (0usize..65).map(|i| format_ident!("Flag{}", i)).collect();
        let t: syn::ItemEnum = syn::parse2(quote!(enum Flags { #(#ids,)* })).unwrap();
        assert!(parse_flags(quote!(), t).is_err());
    }
    #[test]
    fn flags_rejects_args() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            enum F {
                Ready,
            }
        ))
        .unwrap();
        assert!(parse_flags(quote!(unknown), t).is_err());
    }
    #[test]
    fn flags_rejects_payload() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            enum F {
                Ready(u8),
            }
        ))
        .unwrap();
        assert!(parse_flags(quote!(), t).is_err());
    }
    #[test]
    fn flags_rejects_discriminant() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            enum F {
                Ready = 3,
            }
        ))
        .unwrap();
        assert!(parse_flags(quote!(), t).is_err());
    }
    #[test]
    fn flags_rejects_derive_conflict() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            #[derive(Copy, Clone)]
            enum F {
                Ready,
            }
        ))
        .unwrap();
        assert!(parse_flags(quote!(), t).is_err());
    }
    #[test]
    fn flags_rejects_generic() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            enum F<T> {
                Ready(T),
            }
        ))
        .unwrap();
        assert!(parse_flags(quote!(), t).is_err());
    }
    #[test]
    fn flags_preserves_docs() {
        let t: syn::ItemEnum = syn::parse2(quote!(
            #[doc = "schema"]
            enum F {
                #[doc = "flag"]
                Ready,
            }
        ))
        .unwrap();
        let x = parse_flags(quote!(), t).unwrap().to_string();
        assert!(x.contains("schema") && x.contains("flag"));
    }
}
