//! The owned form: a declaration written in domain types (`String`, `Vec<T>`,
//! `Option<Box<T>>`) yields the owned struct users edit, and the raw struct
//! whose layout, description and zero-copy view the raw generator makes.

use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote, ToTokens};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Error, Fields, GenericArgument, GenericParam, Ident, ItemEnum, ItemStruct, LitInt,
    Meta, PathArguments, Token, Type,
};

use super::{io, lit_int, parse_bits, parse_len, raw_record, unraw, Args, Bits, Len, SCALARS};

/// The error every generated conversion returns.
fn error() -> Tokens {
    quote!(::or_io::Error)
}

/// A variant's accessor stem: its name less the base's, in snake case
/// (`CrFrameFilterMultiWeight` of `CrFrameFilter` is `multi_weight`).
fn short_name(variant: &str, base: &str) -> String {
    let rest = match variant.strip_prefix(base) {
        Some(rest) if !rest.is_empty() => rest,
        _ => variant,
    };
    let chars: Vec<char> = rest.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let after_lower = chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit();
            let acronym_end = chars[i - 1].is_ascii_uppercase()
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if after_lower || acronym_end {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

const PASSTHROUGH: &[&str] = &[
    "Ptr",
    "PhysPtr",
    "MemberPtr",
    "Str",
    "Array",
    "Trailing",
    "Opaque",
];

fn args_of(seg: &syn::PathSegment) -> Vec<Type> {
    match &seg.arguments {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|g| match g {
                GenericArgument::Type(t) => Some(t.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A record's type parameters. An element parameter is declared bare and is
/// the owned type a record holds, so its raw half is over that type's raw
/// form. A target parameter is declared with its bound (`T: DrawableTarget`,
/// default and all) and is the same marker in both halves.
struct Params {
    all: Vec<Ident>,
    elements: Vec<Ident>,
}

/// Whether `ty` names one of `params` anywhere in it, bare.
fn mentions(ty: &Type, params: &[Ident]) -> bool {
    match ty {
        Type::Array(a) => mentions(&a.elem, params),
        Type::Path(p) => {
            (p.qself.is_none()
                && p.path.segments.len() == 1
                && params.contains(&p.path.segments[0].ident))
                || p.path
                    .segments
                    .iter()
                    .any(|s| args_of(s).iter().any(|t| mentions(t, params)))
        }
        _ => false,
    }
}

/// Whether `ty` holds the bare parameter `p` behind a pointer at one value,
/// which the raw half reaches as a record.
fn behind_one(ty: &Type, p: &Ident) -> bool {
    match ty {
        Type::Array(a) => behind_one(&a.elem, p),
        Type::Path(tp) => tp.path.segments.iter().any(|seg| {
            let args = args_of(seg);
            let single = [
                "Box",
                "Back",
                "Keyed",
                "Member",
                "OwnedOrKeyed",
                "NullIsEmpty",
            ]
            .contains(&seg.ident.to_string().as_str());
            (single && args.first().is_some_and(|a| is_bare(a, p)))
                || args.iter().any(|a| behind_one(a, p))
        }),
        _ => false,
    }
}

fn is_bare(ty: &Type, p: &Ident) -> bool {
    matches!(ty, Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident(p))
}

/// The trait bounding a generic owned record's parameter `index` (its raw half's `__{Name}Param{index}`).
fn param_trait(name: &Ident, index: usize) -> Ident {
    format_ident!("__{}Param{}", name, index)
}

/// Each parameter trait of another owned record that `ty` passes the bare
/// parameter `p` to, as `Vec<Chunk<T>>` passes `T` to `Chunk`'s first;
/// `own` names the record declaring `ty`, whose own trait is not its bound.
fn passed_to(ty: &Type, p: &Ident, own: &Ident, out: &mut Vec<syn::Path>) {
    match ty {
        Type::Array(a) => passed_to(&a.elem, p, own, out),
        Type::Path(tp) if tp.qself.is_none() => {
            let seg = tp.path.segments.last().expect("a path has a segment");
            let name = seg.ident.to_string();
            let args = args_of(seg);
            let wrapper = [
                "Option",
                "Box",
                "Vec",
                "Member",
                "Back",
                "Keyed",
                "Shared",
                "Ref",
                "String",
                "NullIsEmpty",
            ]
            .contains(&name.as_str())
                || SCALARS.contains(&name.as_str())
                || PASSTHROUGH.contains(&name.as_str());
            let itself = tp.path.segments.len() == 1 && (seg.ident == *own || name == "Self");
            if !wrapper && !itself {
                for (i, a) in args.iter().enumerate() {
                    if is_bare(a, p) {
                        let mut path = tp.path.clone();
                        let last = path.segments.last_mut().expect("a segment");
                        last.ident = param_trait(&seg.ident, i);
                        last.arguments = PathArguments::None;
                        if !out.contains(&path) {
                            out.push(path);
                        }
                    }
                }
            }
            for a in &args {
                passed_to(a, p, own, out);
            }
        }
        _ => {}
    }
}

/// The on-disk type a domain type is held as.
///
/// ⚠ A generic record over an element parameter is named by its raw half
/// (`__ChunkRaw<T>`), because inside a raw struct `T` is already raw.
fn raw_of(ty: &Type, params: &Params) -> syn::Result<Type> {
    let io = io();
    match ty {
        Type::Array(a) => {
            let elem = raw_of(&a.elem, params)?;
            let len = &a.len;
            Ok(syn::parse_quote!([#elem; #len]))
        }
        Type::Path(p) if p.qself.is_none() => {
            let seg = p.path.segments.last().expect("a path has a segment");
            let name = seg.ident.to_string();
            let args = args_of(seg);
            let one = |what: &str| -> syn::Result<Type> {
                args.first()
                    .cloned()
                    .ok_or_else(|| Error::new(ty.span(), format!("`{what}` takes a type")))
            };
            Ok(match name.as_str() {
                "String" => syn::parse_quote!(#io::Str),
                "Option" => {
                    let inner = one("Option")?;
                    let Type::Path(ip) = &inner else {
                        return Err(Error::new(
                            inner.span(),
                            "an optional field is `Option<Box<T>>`",
                        ));
                    };
                    let iseg = ip.path.segments.last().expect("a segment");
                    if iseg.ident != "Box" {
                        return Err(Error::new(
                            inner.span(),
                            "an optional field is `Option<Box<T>>`",
                        ));
                    }
                    let t = raw_of(&args_of(iseg)[0], params)?;
                    syn::parse_quote!(#io::Ptr<#t>)
                }
                "Box" => {
                    let t = raw_of(&one("Box")?, params)?;
                    syn::parse_quote!(#io::Ptr<#t>)
                }
                "Vec" => {
                    let t = raw_of(&one("Vec")?, params)?;
                    syn::parse_quote!(#io::Array<#t>)
                }
                "Member" => {
                    let t = raw_of(&one("Member")?, params)?;
                    syn::parse_quote!(#io::MemberPtr<#t>)
                }
                "Back" | "Keyed" | "OwnedOrKeyed" | "NullIsEmpty" => {
                    let t = raw_of(&one(&name)?, params)?;
                    syn::parse_quote!(#io::Ptr<#t>)
                }
                "Shared" | "Ref" => raw_of(&one(&name)?, params)?,
                n if SCALARS.contains(&n) || PASSTHROUGH.contains(&n) => ty.clone(),
                _ if p.path.segments.len() == 1
                    && args.is_empty()
                    && params.all.contains(&seg.ident) =>
                {
                    ty.clone()
                }
                _ if args.iter().any(|a| mentions(a, &params.elements)) => {
                    let raw_args = args
                        .iter()
                        .map(|a| raw_of(a, params))
                        .collect::<syn::Result<Vec<Type>>>()?;
                    let mut path = p.path.clone();
                    let last = path.segments.last_mut().expect("a segment");
                    last.ident = format_ident!("__{}Raw", last.ident);
                    last.arguments =
                        PathArguments::AngleBracketed(syn::parse_quote!(<#(#raw_args),*>));
                    Type::Path(syn::TypePath { qself: None, path })
                }
                _ => syn::parse_quote!(<#ty as #io::Owned>::Raw),
            })
        }
        other => Err(Error::new(
            other.span(),
            "a field's type is a path or an array",
        )),
    }
}

/// Whether a domain type spells an on-disk pointer anywhere in it.
fn holds_address(ty: &Type) -> bool {
    match ty {
        Type::Array(a) => holds_address(&a.elem),
        Type::Path(p) => p.path.segments.iter().any(|seg| {
            ["Ptr", "PhysPtr", "MemberPtr", "Str"].contains(&seg.ident.to_string().as_str())
                || args_of(seg).iter().any(holds_address)
        }),
        _ => false,
    }
}

fn vec_elem(ty: &Type) -> Option<Type> {
    wrapped(ty, "Vec")
}

/// `T` of a `name<T>`.
fn wrapped(ty: &Type, name: &str) -> Option<Type> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    (seg.ident == name)
        .then(|| args_of(seg).into_iter().next())
        .flatten()
}

/// `T` of a `Box<T>` or an `Option<Box<T>>`: a pointer at one record.
fn pointee(ty: &Type) -> Option<Type> {
    wrapped(ty, "Box").or_else(|| wrapped(ty, "Option").and_then(|o| wrapped(&o, "Box")))
}

/// Whether `ty` reads an alias of what another pointer owns as a copy: a pointer at one record, a `String`, or a `Vec` of either.
fn by_value(ty: &Type) -> bool {
    let one = |t: &Type| {
        pointee(t).is_some()
            || matches!(t, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "String"))
    };
    one(ty) || vec_elem(ty).is_some_and(|e| one(&e))
}

fn is_string(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "String"))
}

/// Whether `ty` is a `String` or an `Option<...>`, or a `Ref` to one: a pointer slot that reads as empty.
fn may_be_empty(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last()
        .is_some_and(|s| s.ident == "String" || s.ident == "Option"))
        || wrapped(ty, "Ref").is_some_and(|p| may_be_empty(&p))
}

/// What `#[shares]` names: a field of this record, or a path from one. A
/// path through a `Box<T>` or `Option<Box<T>>` field continues in the record
/// it points at; otherwise it runs through embedded records. `[N]` is element
/// `N` of a run of pointers there.
struct Aim {
    first: Ident,
    rest: Vec<Ident>,
    index: Option<usize>,
}

fn parse_aim(attr: &Attribute) -> syn::Result<Aim> {
    attr.parse_args_with(|input: syn::parse::ParseStream| {
        let first = syn::ext::IdentExt::parse_any(input)?;
        let mut rest = Vec::new();
        while input.parse::<Option<Token![.]>>()?.is_some() {
            rest.push(syn::ext::IdentExt::parse_any(input)?);
        }
        let index = if input.peek(syn::token::Bracket) {
            let content;
            syn::bracketed!(content in input);
            Some(content.parse::<LitInt>()?.base10_parse::<usize>()?)
        } else {
            None
        };
        Ok(Aim { first, rest, index })
    })
}

struct OwnedField {
    ident: Ident,
    ty: Type,
    raw_ty: Type,
    vis: syn::Visibility,
    attrs: Vec<Attribute>,
    raw_attrs: Vec<Attribute>,
    len: Option<(Len, Type)>,
    trailing: bool,
    /// `#[phys]`: a run in the graphics half.
    phys: bool,
    /// Private: declared and described, and kept in `__carry` rather than shown.
    hidden: bool,
    /// `#[shares(other)]`: a `Shared<V>` field, and the pointer whose allocation it may share.
    shares: Option<(Aim, Type)>,
    /// `#[init(expr)]`: what a private field saves as in a value built from scratch.
    init: Option<syn::Expr>,
    /// `#[derived(f)]`: a private field a save derives as `f(&self)`, `None` where nothing derives it.
    derived: Option<syn::Expr>,
    /// `#[codec(RawType)]`: a field whose allocation a codec computes; `raw_ty` is `RawType`.
    /// A private one is derived: opening only checks it, and a save lays out its default.
    codec: bool,
    /// `#[bits(...)]`: the named ranges of this field's bits.
    bits: Vec<Bits>,
    /// `#[select(...)]`: an inline union, and what selects its arm.
    select: Option<Vec<Cond>>,
    /// `#[keyed_by(key)]`: a pointer whose target this record's `key` keys.
    keyed_by: Option<Ident>,
    /// `#[refs]`: a private count a save sets to the number of pointers at the record.
    refs: bool,
}

/// One arm of a `#[select]`: `name = value => Arm`, or `_ => Arm` for every value no other names.
struct Cond {
    on: Option<(String, i128)>,
    arm: Ident,
    span: Span,
}

fn parse_select(attr: &Attribute) -> syn::Result<Vec<Cond>> {
    let conds = attr.parse_args_with(|input: syn::parse::ParseStream| {
        let mut out = Vec::new();
        while !input.is_empty() {
            let span = input.span();
            let on = if input.parse::<Option<Token![_]>>()?.is_some() {
                None
            } else {
                let name = syn::ext::IdentExt::parse_any(input)?;
                input.parse::<Token![=]>()?;
                let negative = input.parse::<Option<Token![-]>>()?.is_some();
                let value: i128 = input.parse::<LitInt>()?.base10_parse()?;
                Some((unraw(&name), if negative { -value } else { value }))
            };
            input.parse::<Token![=>]>()?;
            let arm: Ident = input.parse()?;
            out.push(Cond { on, arm, span });
            if input.parse::<Option<Token![,]>>()?.is_none() {
                break;
            }
        }
        Ok(out)
    })?;
    if conds.is_empty() {
        return Err(Error::new(attr.span(), "`#[select(name = N => Arm, ...)]`"));
    }
    if let Some(c) = conds.iter().rev().skip(1).find(|c| c.on.is_none()) {
        return Err(Error::new(
            c.span,
            "`_` selects what no other arm does, so it comes last",
        ));
    }
    Ok(conds)
}

/// The accessor stem of a union's arm, `__read_<arm>`.
fn arm_reader(arm: &Ident) -> Ident {
    format_ident!("__read_{}", short_name(&arm.to_string(), ""))
}

/// A `#[select]` resolved against its record: what reads each condition's
/// value from `raw`, and what writes it.
struct Selector {
    /// Each condition as `(test, stamp, arm)`; `None` for `_`.
    conds: Vec<(Option<(Tokens, Tokens)>, Ident)>,
    /// Each selecting name and what reads it, for a refusal.
    reads: Vec<(String, Tokens)>,
}

fn resolve_select(conds: &[Cond], fields: &[OwnedField], what: &str) -> syn::Result<Selector> {
    let io = io();
    let error = error();
    let mut out = Selector {
        conds: Vec::new(),
        reads: Vec::new(),
    };
    for c in conds {
        let Some((name, value)) = &c.on else {
            out.conds.push((None, c.arm.clone()));
            continue;
        };
        let range = fields
            .iter()
            .find_map(|f| f.bits.iter().find(|b| b.name == *name).map(|b| (f, b)));
        let (read, test, stamp) = match range {
            Some((f, b)) => {
                let ident = &f.ident;
                let (low, width) = (b.low, b.width);
                let mask = if width == 64 {
                    u64::MAX
                } else {
                    (1u64 << width) - 1
                };
                if *value < 0 || *value > i128::from(mask) {
                    return Err(Error::new(
                        c.span,
                        format!("`{name}` is {width} bits wide and cannot hold {value}"),
                    ));
                }
                let (value, placed) = (*value as u64, mask << low);
                let shifted = value << low;
                let read = quote!(((#io::TagValue::tag_bits({ raw.#ident }) >> #low) & #mask));
                let why = format!("`{}` cannot hold its arm's `{name}`", unraw(ident));
                let stamp = quote! {
                    let __bits = (#io::TagValue::tag_bits({ raw.#ident }) & !#placed) | #shifted;
                    raw.#ident = match ::core::convert::TryFrom::try_from(__bits) {
                        ::core::result::Result::Ok(v) => v,
                        ::core::result::Result::Err(_) => {
                            return ::core::result::Result::Err(#error::Invalid {
                                what: ::std::string::String::from(#what),
                                why: ::std::string::String::from(#why),
                            })
                        }
                    };
                };
                (read.clone(), quote!(#read == #value), stamp)
            }
            None => {
                let Some(f) = fields.iter().find(|f| unraw(&f.ident) == *name) else {
                    return Err(Error::new(
                        c.span,
                        format!("`{name}` is neither a field nor a bit range of this record"),
                    ));
                };
                let ident = &f.ident;
                let value = i64::try_from(*value)
                    .map_err(|_| Error::new(c.span, "a selecting value fits 64 bits"))?;
                let read = quote!(#io::TagValue::tag_value({ raw.#ident }));
                let why = format!("`{name}` cannot hold its arm's value");
                let stamp = quote! {
                    raw.#ident = match ::core::convert::TryFrom::try_from(#value) {
                        ::core::result::Result::Ok(v) => v,
                        ::core::result::Result::Err(_) => {
                            return ::core::result::Result::Err(#error::Invalid {
                                what: ::std::string::String::from(#what),
                                why: ::std::string::String::from(#why),
                            })
                        }
                    };
                };
                (read.clone(), quote!(#read == #value), stamp)
            }
        };
        if !out.reads.iter().any(|(n, _)| n == name) {
            out.reads.push((name.clone(), read));
        }
        out.conds.push((Some((test, stamp)), c.arm.clone()));
    }
    Ok(out)
}

impl Selector {
    /// The arm `raw` selects, as `Option<&'static str>`.
    fn chosen(&self) -> Tokens {
        let mut chain = quote!(::core::option::Option::None);
        for (cond, arm) in self.conds.iter().rev() {
            let name = arm.to_string();
            chain = match cond {
                Some((test, _)) => quote! {
                    if #test { ::core::option::Option::Some(#name) } else { #chain }
                },
                None => quote!(::core::option::Option::Some(#name)),
            };
        }
        chain
    }

    /// Every arm named, once each, in order.
    fn arms(&self) -> Vec<&Ident> {
        let mut arms: Vec<&Ident> = Vec::new();
        for (_, arm) in &self.conds {
            if !arms.contains(&arm) {
                arms.push(arm);
            }
        }
        arms
    }

    /// What `raw` holds in each selecting name, as one sentence.
    fn stated(&self) -> Tokens {
        let format = self
            .reads
            .iter()
            .map(|(name, _)| format!("`{name}` is {{:#x}}"))
            .collect::<Vec<_>>()
            .join(", ");
        let reads = self.reads.iter().map(|(_, r)| r);
        quote!(::std::format!(#format, #(#reads),*))
    }
}

/// The save-time code that makes the fields a `#[len]` names state the run's
/// new length, or refuses the save naming the run when they cannot.
///
/// ⚠ A `field`, `bytes` or `count` + `capacity` count is always written. The
/// other kinds are written only when the length they state is not the run's,
/// so a run left alone saves as it was read, and never for an empty run, which
/// saves as a null pointer whose count the tree's save keeps. A count an
/// earlier run wrote is only checked.
fn count_fixup(
    len: &Len,
    run_len: &Tokens,
    off: &Tokens,
    what: &str,
    written: &mut Vec<String>,
) -> Tokens {
    let io = io();
    let error = error();
    let refuse = |why: Tokens| {
        quote! {
            return ::core::result::Result::Err(#error::Invalid {
                what: ::std::string::String::from(#what),
                why: #why,
            })
        }
    };
    let said = quote!(__stated.map_or_else(
        || ::std::string::String::from("no length"),
        |s| s.to_string()
    ));
    let rd = |c: &str| {
        let c = super::field_ident(c);
        quote!(<u64 as ::core::convert::TryFrom<_>>::try_from({ raw.#c }).ok())
    };
    let put = |c: &str, v: Tokens| {
        let name = c;
        let c = super::field_ident(c);
        let why = refuse(quote!(::std::format!(
            "{} elements, and its count `{}` cannot hold {}",
            __n, #name, __v
        )));
        quote! {
            let __v: u64 = #v;
            raw.#c = match ::core::convert::TryFrom::try_from(__v) {
                ::core::result::Result::Ok(c) => c,
                ::core::result::Result::Err(_) => { #why }
            };
        }
    };
    let (writes, stated, assign): (Vec<&str>, Tokens, Tokens) = match len {
        Len::Field(c) | Len::Bytes(c) => (vec![c], rd(c), put(c, quote!(__n))),
        Len::CountCapacity(c, cap) => {
            let (a, b) = (put(c, quote!(__n)), put(cap, quote!(__n)));
            (vec![c, cap], rd(c), quote!(#a #b))
        }
        Len::Masked(c, m) | Len::MaskedOrMipChain(c, m) => {
            let r = rd(c);
            let why = refuse(quote!(::std::format!(
                "{} elements, and `{}` states a length only through the mask {:#x}",
                __n, #c, #m
            )));
            let set = put(c, quote!((#r.unwrap_or(0) & !#m) | __n));
            let masked = quote!(#r.map(|v| v & #m));
            // A zero under the mask states the mip chain, so a run of that length leaves the word alone.
            let (stated, assign) = match len {
                Len::MaskedOrMipChain(..) => {
                    let dims: Vec<Tokens> = super::MIP_CHAIN_FIELDS
                        .iter()
                        .map(|d| {
                            let d = rd(d);
                            quote!(#d.unwrap_or(0) as u32)
                        })
                        .collect();
                    let chain =
                        quote!(#io::__private::CountSource::mip_chain_len(#(#dims),*) as u64);
                    let not_a_byte_run = refuse(quote!(::std::format!(
                        "{} elements, and padding the mip chain to the mask's unit needs a run of bytes",
                        __n
                    )));
                    // The mask counts whole units, so a chain that is not one is padded with zeros to the next.
                    let assign = quote! {
                        let __n = if __n & !#m != 0 {
                            let __padded = __n.next_multiple_of(#m & #m.wrapping_neg());
                            if __n != #chain || __padded & !#m != 0 { #why }
                            if !#io::__private::pad_run(kids, #off, __padded as usize) { #not_a_byte_run }
                            __padded
                        } else {
                            __n
                        };
                        #set
                    };
                    let stated = quote!(match #masked {
                        ::core::option::Option::Some(0) => ::core::option::Option::Some(#chain),
                        stated => stated,
                    });
                    (stated, assign)
                }
                _ => (
                    masked,
                    quote! {
                        if __n & !#m != 0 { #why }
                        #set
                    },
                ),
            };
            (vec![c], stated, assign)
        }
        Len::Product(a, b) => {
            let (ra, rb) = (rd(a), rd(b));
            let why = refuse(quote!(::std::format!(
                "{} elements are not a whole number of `{}`, which is {}",
                __n, #b, __s
            )));
            let set = put(a, quote!(__n / __s));
            (
                vec![a],
                quote!(#ra.zip(#rb).and_then(|(a, b)| a.checked_mul(b))),
                quote! {
                    let __s = #rb.unwrap_or(0);
                    if __n.checked_rem(__s) != ::core::option::Option::Some(0) { #why }
                    #set
                },
            )
        }
        Len::PrefixedAboveOne(c) => {
            let r = rd(c);
            let why = refuse(quote!(::std::format!(
                "2 elements, and `{}` states a leading entry only above one, so it counts 0, 1, 3 or more",
                #c
            )));
            let set = put(c, quote!(if __n > 1 { __n - 1 } else { __n }));
            (
                vec![c],
                quote!(#r
                    .and_then(|c| usize::try_from(c).ok())
                    .map(|c| #io::__private::prefixed_above_one(c) as u64)),
                quote! {
                    if __n == 2 { #why }
                    #set
                },
            )
        }
        Len::Sum(parts) => {
            let reads: Vec<Tokens> = parts.iter().map(|p| rd(p)).collect();
            let sum = parts.join(" + ");
            let why = refuse(quote!(::std::format!(
                "{} elements, and `{}` states {}: a run of parts is resized by editing each part's count",
                __n, #sum, #said
            )));
            (
                vec![],
                quote!(::core::option::Option::Some(0u64) #(.zip(#reads).and_then(|(s, p)| s.checked_add(p)))*),
                why,
            )
        }
        Len::Fixed(k) => {
            let why = refuse(quote!(::std::format!(
                "{} elements, and the layout fixes its length at {}",
                __n, #k
            )));
            (vec![], quote!(::core::option::Option::Some(#k)), why)
        }
    };
    let shared = writes.iter().find(|c| written.iter().any(|w| w == *c));
    let first = shared.is_none() && !writes.is_empty();
    let always = matches!(len, Len::Field(_) | Len::Bytes(_) | Len::CountCapacity(..));
    let body = if first && always {
        assign
    } else {
        let mismatch = match shared {
            Some(c) => refuse(quote!(::std::format!(
                "{} elements, and `{}` also counts another run, which it states is {} long",
                __n, #c, #said
            ))),
            None => assign,
        };
        quote! {
            if __n != 0 && __stated != ::core::option::Option::Some(__n) { #mismatch }
        }
    };
    written.extend(writes.iter().map(|c| c.to_string()));
    quote! {
        {
            let __n = (#run_len) as u64;
            #[allow(unused_variables)]
            let __stated: ::core::option::Option<u64> = #stated;
            #body
        }
    }
}

pub fn owned_record(args: Args, item: ItemStruct) -> syn::Result<Tokens> {
    let io = io();
    let error = error();
    let mut params = Params {
        all: Vec::new(),
        elements: Vec::new(),
    };
    for param in &item.generics.params {
        match param {
            GenericParam::Type(t) => {
                params.all.push(t.ident.clone());
                if t.bounds.is_empty() && t.default.is_none() {
                    params.elements.push(t.ident.clone());
                }
            }
            other => {
                return Err(Error::new(
                    other.span(),
                    "a record's only parameters are types",
                ))
            }
        }
    }
    let Fields::Named(named) = &item.fields else {
        return Err(Error::new(item.span(), "a record has named fields"));
    };

    let mut fields = Vec::new();
    for field in &named.named {
        let ident = field.ident.clone().expect("named");
        let codec_field = field.attrs.iter().any(|a| a.path().is_ident("codec"));
        if !codec_field && super::mentions_param_by_value(&field.ty, &params.all) {
            return Err(Error::new(
                field.ty.span(),
                "a type parameter may appear only behind a pointer, so no offset depends on it",
            ));
        }
        let name = unraw(&ident);
        let unk = name.starts_with("unknown")
            || (name.starts_with("unk")
                && name[3..]
                    .chars()
                    .next()
                    .is_none_or(|c| c == '_' || c.is_ascii_digit()));
        if unk {
            return Err(Error::new(
                ident.span(),
                format!(
                    "`{name}` is not a name: every field takes the name the GTA source gives it"
                ),
            ));
        }
        let mut attrs = Vec::new();
        let mut raw_attrs = Vec::new();
        let mut len = None;
        let mut phys = false;
        let mut trailing = false;
        let mut shares = None;
        let mut init = None;
        let mut derived = None;
        let mut codec = None;
        let mut bits = Vec::new();
        let mut select = None;
        let mut keyed_by = None;
        let mut refs = false;
        for a in &field.attrs {
            if a.path().is_ident("codec") {
                codec = Some(a.parse_args::<Type>()?);
            } else if a.path().is_ident("bits") {
                bits.push(parse_bits(a)?);
                raw_attrs.push(a.clone());
            } else if a.path().is_ident("select") {
                select = Some(parse_select(a)?);
            } else if a.path().is_ident("keyed_by") {
                keyed_by = Some(a.parse_args::<Ident>()?);
            } else if a.path().is_ident("refs") {
                a.meta.require_path_only()?;
                refs = true;
            } else if a.path().is_ident("phys") {
                phys = true;
            } else if a.path().is_ident("init") {
                init = Some(a.parse_args::<syn::Expr>()?);
            } else if a.path().is_ident("derived") {
                derived = Some(a.parse_args::<syn::Expr>()?);
            } else if a.path().is_ident("shares") {
                shares = Some(parse_aim(a)?);
            } else if a.path().is_ident("trailing") {
                trailing = true;
            } else if a.path().is_ident("len") {
                len = Some(parse_len(a)?);
                raw_attrs.push(a.clone());
            } else if a.path().is_ident("at")
                || a.path().is_ident("note")
                || a.path().is_ident("pad_after")
            {
                raw_attrs.push(a.clone());
            } else {
                attrs.push(a.clone());
            }
        }
        let hidden = !matches!(field.vis, syn::Visibility::Public(_));
        let shared = wrapped(&field.ty, "Shared");
        let shares = match (shares, shared) {
            (Some(other), Some(inner)) if !hidden => Some((other, inner)),
            (None, None) => None,
            _ => return Err(Error::new(
                ident.span(),
                "a public `Shared<V>` field names the field it may share with `#[shares(field)]`",
            )),
        };
        if init.is_some() && !hidden {
            return Err(Error::new(
                ident.span(),
                "`#[init]` is for a private field: a public one states its own value",
            ));
        }
        if derived.is_some() && (!hidden || len.is_some() || codec.is_some() || refs) {
            return Err(Error::new(
                ident.span(),
                "`#[derived]` is for a private plain field: a save derives what it holds",
            ));
        }
        // A codec's `#[len]` is the description's: a private codec derives the run it counts.
        if hidden && len.is_some() && codec.is_none() {
            return Err(Error::new(
                ident.span(),
                "a run with `#[len]` is public: the count it names is what stays private",
            ));
        }
        let plain = shares.is_none() && len.is_none() && codec.is_none() && !phys && !trailing;
        if select.is_some() && (hidden || !plain || keyed_by.is_some() || refs) {
            return Err(Error::new(
                ident.span(),
                "a `#[select]` union is a public field of its own, embedded in its owner",
            ));
        }
        if let Some(key) = &keyed_by {
            let pointer =
                pointee(&field.ty).or_else(|| vec_elem(&field.ty).and_then(|e| pointee(&e)));
            if hidden || !plain || refs || pointer.is_none() {
                return Err(Error::new(
                    key.span(),
                    "`#[keyed_by(key)]` is on a public `Box<T>` or `Option<Box<T>>`, or a `Vec` of them",
                ));
            }
        }
        if refs {
            let count = matches!(&field.ty, Type::Path(p) if ["u8", "u16", "u32", "u64"]
                .iter()
                .any(|n| p.path.is_ident(n)));
            if !hidden || !count || !plain {
                return Err(Error::new(
                    ident.span(),
                    "`#[refs]` is a private unsigned count: an owned value shows no reference count",
                ));
            }
        }
        if let Some(raw) = codec {
            if shares.is_some() || phys || trailing || init.is_some() {
                return Err(Error::new(
                    ident.span(),
                    "a `#[codec]` field's codec alone lays out what it points at",
                ));
            }
            // A `#[len]` stays on the raw field for the description; the codec writes the count.
            fields.push(OwnedField {
                ident,
                ty: field.ty.clone(),
                raw_ty: raw,
                vis: field.vis.clone(),
                attrs,
                raw_attrs,
                len: None,
                trailing: false,
                phys: false,
                hidden,
                shares: None,
                init: None,
                derived: None,
                codec: true,
                bits,
                select: None,
                keyed_by: None,
                refs: false,
            });
            continue;
        }
        let domain = shares.as_ref().map_or(&field.ty, |(_, inner)| inner);
        if shares.is_some() && len.is_none() && vec_elem(domain).is_some() {
            return Err(Error::new(
                ident.span(),
                "a counted array carries its own count, so it cannot share another's allocation",
            ));
        }
        let (raw_ty, len) = match len {
            Some(len) => {
                let elem = vec_elem(domain).ok_or_else(|| {
                    Error::new(field.ty.span(), "a field with `#[len]` is a `Vec`")
                })?;
                let raw_elem = raw_of(&elem, &params)?;
                let raw: Type = if trailing {
                    syn::parse_quote!(#io::Trailing<#raw_elem>)
                } else if phys {
                    syn::parse_quote!(#io::PhysPtr<[#raw_elem]>)
                } else {
                    syn::parse_quote!(#io::Ptr<[#raw_elem]>)
                };
                (raw, Some((len, elem)))
            }
            None if phys || trailing => {
                return Err(Error::new(
                    ident.span(),
                    "`#[phys]` and `#[trailing]` are for a `Vec` with a `#[len]`",
                ))
            }
            None => (raw_of(&field.ty, &params)?, None),
        };
        fields.push(OwnedField {
            ident,
            ty: field.ty.clone(),
            raw_ty,
            vis: field.vis.clone(),
            attrs,
            raw_attrs,
            len,
            trailing,
            phys,
            hidden,
            shares,
            init,
            derived,
            codec: false,
            bits,
            select,
            keyed_by,
            refs,
        });
    }
    for f in &fields {
        if let Some((aim, _)) = &f.shares {
            let other = &aim.first;
            if !fields
                .iter()
                .any(|g| g.ident == *other && !g.hidden && g.shares.is_none())
            {
                return Err(Error::new(
                    other.span(),
                    format!(
                        "`{other}` is not a public pointer of this record for `{}` to share",
                        f.ident
                    ),
                ));
            }
        }
    }
    if let Some(key) = &args.key {
        if !fields.iter().any(|f| unraw(&f.ident) == *key && !f.hidden) {
            return Err(Error::new(
                Span::call_site(),
                format!("the key `{key}` is not a public field of this record"),
            ));
        }
    }
    for f in &fields {
        let Some(key) = &f.keyed_by else { continue };
        let Some(k) = fields.iter().find(|g| g.ident == *key && !g.hidden) else {
            return Err(Error::new(
                key.span(),
                format!("the key `{key}` is not a public field of this record"),
            ));
        };
        if vec_elem(&f.ty).is_some() != vec_elem(&k.ty).is_some() {
            return Err(Error::new(
                key.span(),
                "a `Vec` of pointers is keyed by a `Vec` of keys, element by element, and one pointer by one key",
            ));
        }
    }

    let name = &item.ident;
    let name_str = name.to_string();
    let raw_name = format_ident!("__{}Raw", name);
    let vis = &item.vis;
    let mut selectors = Vec::new();
    for f in &fields {
        if let Some(conds) = &f.select {
            let what = format!("{name_str}.{}", unraw(&f.ident));
            selectors.push((f.ident.clone(), resolve_select(conds, &fields, &what)?));
        }
    }
    let unions: Vec<Ident> = selectors.iter().map(|(i, _)| i.clone()).collect();

    // The raw struct, handed to the raw generator under the owned name.
    // Every field of the raw struct is public: a peek reads what the owned value keeps private.
    let raw_fields = fields.iter().map(|f| {
        let (attrs, ident, ty) = (&f.raw_attrs, &f.ident, &f.raw_ty);
        let vis = if unraw(ident).starts_with("_pad") {
            quote!()
        } else {
            quote!(pub)
        };
        quote!(#(#attrs)* #vis #ident: #ty)
    });
    // An element parameter a record reaches through a pointer at one value is a
    // record; one it holds only in runs may be a scalar (`AiSplitArray<u16>`).
    // One it passes to another owned record also takes that record's bound,
    // through a trait whose supertraits a bound on it implies.
    let mut param_traits = Vec::new();
    for (index, g) in item.generics.params.iter().enumerate() {
        let GenericParam::Type(t) = g else { continue };
        if !params.elements.contains(&t.ident) {
            continue;
        }
        let p = &t.ident;
        let own = if fields.iter().any(|f| behind_one(&f.ty, p)) {
            quote!(#io::Record)
        } else {
            quote!(#io::Plain + #io::Element + #io::Param)
        };
        let mut callees = Vec::new();
        for f in fields.iter().filter(|f| !f.codec) {
            passed_to(&f.ty, p, name, &mut callees);
        }
        let bound = param_trait(name, index);
        param_traits.push(quote! {
            #[doc(hidden)]
            #vis trait #bound: #own #(+ #callees)* {}
            impl<__P: #own #(+ #callees)*> #bound for __P {}
        });
    }
    let element_bound = |p: &Ident| -> Tokens {
        let index = item
            .generics
            .params
            .iter()
            .position(|g| matches!(g, GenericParam::Type(t) if t.ident == *p))
            .expect("a declared parameter");
        let bound = param_trait(name, index);
        quote!(#bound)
    };
    let param_decls = |element: &dyn Fn(&Ident) -> Tokens, defaults: bool| -> Vec<Tokens> {
        item.generics
            .params
            .iter()
            .filter_map(|g| match g {
                GenericParam::Type(t) if params.elements.contains(&t.ident) => {
                    Some(element(&t.ident))
                }
                GenericParam::Type(t) => {
                    let mut t = t.clone();
                    if !defaults {
                        t.eq_token = None;
                        t.default = None;
                    }
                    Some(t.into_token_stream())
                }
                _ => None,
            })
            .collect()
    };
    let raw_params = param_decls(
        &|p| {
            let bound = element_bound(p);
            quote!(#p: #bound)
        },
        true,
    );
    let raw_item: ItemStruct = syn::parse2(quote! {
        #[doc(hidden)]
        #vis struct #raw_name<#(#raw_params),*> { #(#raw_fields),* }
    })?;
    // A variant's raw type is named, not projected: a trait cannot be implemented for a projection.
    let raw_name_of = |ty: &Type| -> syn::Result<Type> {
        let Type::Path(p) = ty else {
            return Err(Error::new(ty.span(), "a variant is a record's path"));
        };
        let mut path = p.path.clone();
        let last = path.segments.last_mut().expect("a segment");
        last.ident = format_ident!("__{}Raw", last.ident);
        Ok(Type::Path(syn::TypePath { qself: None, path }))
    };
    let mut raw_args = args.clone();
    for (_, ty) in raw_args.variants.iter_mut() {
        *ty = raw_name_of(ty)?;
    }
    let public: Vec<(String, bool)> = fields
        .iter()
        .filter(|f| !f.hidden)
        .map(|f| (unraw(&f.ident), holds_address(&f.ty)))
        .collect();
    let raw_tokens = raw_record(
        raw_args,
        raw_item,
        Some(name_str.clone()),
        Some(public),
        &unions,
    )?;

    let owned_params = param_decls(&|p| quote!(#p: #io::Owned), false);
    let struct_params = param_decls(&|p| quote!(#p: #io::Owned), true);
    let where_raw: Vec<Tokens> = params
        .elements
        .iter()
        .map(|p| {
            let bound = element_bound(p);
            quote!(<#p as #io::Owned>::Raw: #bound)
        })
        .collect();
    let where_clause = if where_raw.is_empty() {
        Tokens::new()
    } else {
        quote!(where #(#where_raw),*)
    };
    let all = &params.all;
    let self_ty = quote!(#name<#(#all),*>);
    let raw_self_args: Vec<Tokens> = params
        .all
        .iter()
        .map(|p| {
            if params.elements.contains(p) {
                quote!(<#p as #io::Owned>::Raw)
            } else {
                quote!(#p)
            }
        })
        .collect();
    let raw_self = quote!(#raw_name<#(#raw_self_args),*>);
    let item_attrs = &item.attrs;
    let shown: Vec<&OwnedField> = fields.iter().filter(|f| !f.hidden).collect();
    let shown_idents = shown.iter().map(|f| &f.ident);
    let owned_fields = shown.iter().map(|f| {
        let (attrs, vis, ident, ty) = (&f.attrs, &f.vis, &f.ident, &f.ty);
        quote!(#(#attrs)* #vis #ident: #ty)
    });

    // The standard library's `Default` stops at arrays of 32, so an array is built element by element.
    fn default_of(ty: &Type) -> Tokens {
        match ty {
            Type::Array(a) => {
                let elem = default_of(&a.elem);
                quote!(::core::array::from_fn(|_| #elem))
            }
            _ => quote!(::core::default::Default::default()),
        }
    }

    let mut from = Vec::new();
    let mut to = Vec::new();
    let mut lends = Vec::new();
    let mut keep = Vec::new();
    let mut links = Vec::new();
    let mut fixups = Vec::new();
    let mut codecs = Vec::new();
    let mut leaf = Vec::new();
    // What each `#[derived]` field derives to once its record is open.
    let mut opened = Vec::new();
    // A count field the first run it counts writes; a later run sharing it only checks it.
    let mut written: Vec<String> = Vec::new();
    for f in &fields {
        let (ident, ty) = (&f.ident, &f.ty);
        let within = quote!(::core::mem::offset_of!(#raw_self, #ident));
        let off = quote!(at + #within);
        if f.codec {
            let open = quote! {
                <#ty as #io::Codec>::open(
                    &#io::Site::new::<#raw_self>(#io::Plain::as_bytes(raw), kids, at, #within),
                    rx,
                )?
            };
            let saved = if f.hidden {
                keep.push(quote!(let _: #ty = #open;));
                quote!(&<#ty as ::core::default::Default>::default())
            } else {
                from.push(quote!(#ident: #open));
                quote!(&self.#ident)
            };
            codecs.push(quote! {
                {
                    let mut __site =
                        #io::SiteMut::new::<#raw_self>(#io::Plain::as_bytes(&raw), kids, at, #within);
                    <#ty as #io::Codec>::save(#saved, &mut __site, cx)?;
                    raw = __site.finish()?;
                }
            });
            leaf.push(quote!(false));
            continue;
        }
        if f.hidden {
            let default = match &f.init {
                Some(init) => quote!(#init),
                None => default_of(ty),
            };
            keep.push(quote! {
                let __kept = <#ty as #io::Owned>::from_raw(&{ raw.#ident }, kids, #off, rx)?;
                #io::__private::hide(&mut __carry, #within, __kept);
            });
            match &f.derived {
                // Written with the other fields, so a count fixup after reads it.
                Some(derive) => {
                    let what = format!("{name_str}.{}", unraw(ident));
                    opened.push(quote! {
                        let __d: ::core::option::Option<#ty> = (#derive)(&__value);
                        __value.__carry.__keep_derived(#within, __d);
                    });
                    to.push(quote! {
                        raw.#ident = {
                            let __d: ::core::option::Option<#ty> = (#derive)(self);
                            let v: #ty = self.__carry.__derive(#within, __d, || #default, #what)?;
                            <#ty as #io::Owned>::to_raw(&v, kids, #off, cx)?
                        };
                    });
                }
                None => to.push(quote! {
                    raw.#ident = match #io::__private::hidden::<#ty>(&self.__carry, #within) {
                        ::core::option::Option::Some(v) => <#ty as #io::Owned>::to_raw(v, kids, #off, cx)?,
                        ::core::option::Option::None => {
                            let v: #ty = #default;
                            <#ty as #io::Owned>::to_raw(&v, kids, #off, cx)?
                        }
                    };
                }),
            }
            if f.refs {
                let raw_ty = &f.raw_ty;
                to.push(quote!(cx.__refs(#off, ::core::mem::size_of::<#raw_ty>());));
                // A run of plain records is written with no record entered, so each is its own element.
                leaf.push(quote!(false));
            } else {
                leaf.push(quote!(<#ty as #io::Owned>::LEAF));
            }
            if let Some(elem) = vec_elem(ty) {
                keep.push(quote!(__carry.keep_capacity(#within, &{ raw.#ident });));
                to.push(quote! {
                    let mut array = { raw.#ident };
                    self.__carry.apply_capacity::<#elem, _>(#within, #off, kids, &mut array, cx)?;
                    raw.#ident = array;
                });
            }
            continue;
        }
        let shared = f.shares.as_ref().map(|(aim, inner)| {
            let first = &aim.first;
            let index = match aim.index {
                Some(i) => quote!(::core::option::Option::Some(#i)),
                None => quote!(::core::option::Option::None),
            };
            let through = fields
                .iter()
                .find(|g| g.ident == *first)
                .is_some_and(|g| pointee(&g.ty).is_some());
            let along = if through && !aim.rest.is_empty() {
                let path = aim.rest.iter().map(unraw).collect::<Vec<_>>().join(".");
                quote!(#io::__private::Along {
                    at: at + ::core::mem::offset_of!(#raw_self, #first),
                    inner: ::core::option::Option::Some(#path),
                    index: #index,
                })
            } else {
                let rest = &aim.rest;
                quote!(#io::__private::Along {
                    at: at + ::core::mem::offset_of!(#raw_self, #first #(.#rest)*),
                    inner: ::core::option::Option::None,
                    index: #index,
                })
            };
            let plain = aim.rest.is_empty() && aim.index.is_none();
            // A reader takes a record's own pointers in declaration order, and
            // an embedded record's after them all; the one it reaches first owns.
            let order = |i: &Ident| fields.iter().position(|g| g.ident == *i);
            let reached_first =
                (!through && !aim.rest.is_empty()) || order(&f.ident) < order(first);
            (first, inner, along, plain, reached_first)
        });
        // A word that is not an address in the slot reads as the shared allocation and is written back, or repaired.
        let word = format_ident!("__word_{}", unraw(ident));
        let what = format!("{name_str}.{}", unraw(ident));
        if let Some((_, _, along, _, reached_first)) = &shared {
            lends.push(quote!(#io::__private::lend(rx, kids, #off, &#along);));
            keep.push(quote! {
                let #word = #io::__private::keep_word(&mut __carry, #within, kids, #off);
            });
            links.push(quote! {
                if let #io::Shared::Same = &self.#ident {
                    #io::share(kids, #off, &#along, #reached_first, cx)?;
                    #io::__private::put_stale_word(&self.__carry, #what, #within, #off, kids, cx);
                }
            });
        }
        match &f.len {
            Some((len, elem)) => {
                let read = quote!(#io::run_from_child::<#elem>(kids, #off, rx)?);
                let run_len = match &shared {
                    Some((other, _, along, plain, _)) => {
                        from.push(quote! {
                            #ident: if #io::shares(kids, #off, &#along) || #word {
                                #io::Shared::Same
                            } else {
                                #io::Shared::Own(#read)
                            }
                        });
                        to.push(quote! {
                            if let #io::Shared::Own(v) = &self.#ident {
                                #io::run_to_child::<#elem>(v, kids, #off, cx)?;
                            }
                        });
                        let same = if *plain {
                            quote!(self.#other.len())
                        } else {
                            quote!(#io::__private::shared_len::<#elem>(kids, &#along))
                        };
                        quote!(match &self.#ident {
                            #io::Shared::Same => #same,
                            #io::Shared::Own(v) => v.len(),
                        })
                    }
                    None if f.trailing => {
                        from.push(quote!(#ident: #read));
                        to.push(
                            quote!(#io::run_to_trailing::<#elem>(&self.#ident, kids, #off, cx)?;),
                        );
                        quote!(self.#ident.len())
                    }
                    // ⚠ A live pointer in the graphics half beside a length of zero is a payload the
                    // length does not state, so `run_from_child` refuses it rather than drop it.
                    None if f.phys => {
                        from.push(quote!(#ident: #read));
                        to.push(quote!(#io::run_to_child::<#elem>(&self.#ident, kids, #off, cx)?;));
                        quote!(self.#ident.len())
                    }
                    None => {
                        // A live pointer beside a length of zero reads as empty and is written back while it stays empty.
                        let live = format_ident!("__live_{}", unraw(ident));
                        let zero = match len {
                            Len::Field(c) => {
                                let c = super::field_ident(c);
                                quote!(<u64 as ::core::convert::TryFrom<_>>::try_from({ raw.#c }).ok() == ::core::option::Option::Some(0))
                            }
                            _ => quote!(false),
                        };
                        keep.push(quote! {
                            let #live = #io::__private::keep_empty_run(&mut __carry, #within, kids, #off, #zero, rx);
                        });
                        from.push(quote! {
                            #ident: if #live { ::std::vec::Vec::new() } else { #read }
                        });
                        let run = if by_value(ty) {
                            keep.push(quote! {
                                if !#live { #io::__private::keep_copies::<#ty>(&mut __carry, #within, kids, #off, rx)?; }
                            });
                            quote!(#io::__private::run_to_child_by_value::<#elem>(&self.#ident, &self.__carry, #within, kids, #off, cx)?;)
                        } else {
                            // A run aliasing one another pointer owns reads as a copy, saved as the alias while it equals its owner.
                            keep.push(quote! {
                                if !#live { #io::__private::keep_run_copy(&mut __carry, #within, kids, #off, rx)?; }
                            });
                            quote!(#io::__private::run_to_child_copied::<#elem>(&self.#ident, &self.__carry, #within, kids, #off, cx)?;)
                        };
                        to.push(quote! {
                            #run
                            #io::__private::put_empty_run::<#elem>(&self.__carry, #within, #off, kids, cx)?;
                        });
                        quote!(self.#ident.len())
                    }
                };
                leaf.push(quote!(false));
                let what = format!("{name_str}.{}", unraw(ident));
                fixups.push(count_fixup(len, &run_len, &off, &what, &mut written));
            }
            None => match &shared {
                Some((_, inner, along, _, _)) => {
                    from.push(quote! {
                        #ident: if #io::shares(kids, #off, &#along) || #word {
                            #io::Shared::Same
                        } else {
                            #io::Shared::Own(<#inner as #io::Owned>::from_raw(&{ raw.#ident }, kids, #off, rx)?)
                        }
                    });
                    to.push(quote! {
                        if let #io::Shared::Own(v) = &self.#ident {
                            raw.#ident = <#inner as #io::Owned>::to_raw(v, kids, #off, cx)?;
                        }
                    });
                    leaf.push(quote!(false));
                }
                None => {
                    let what = format!("{name_str}.{}", unraw(ident));
                    let selector = selectors.iter().find(|(i, _)| i == ident).map(|(_, s)| s);
                    let read =
                        quote!(<#ty as #io::Owned>::from_raw(&{ raw.#ident }, kids, #off, rx)?);
                    let write = quote!(<#ty as #io::Owned>::to_raw(&self.#ident, kids, #off, cx)?);
                    // An alias of what another pointer owns reads as a copy, saved as the alias while it equals its owner.
                    let plain = match vec_elem(ty) {
                        _ if by_value(ty) => quote! {
                            #io::__private::write_by_value::<#ty>(&self.#ident, &self.__carry, #within, kids, #off, cx)?
                        },
                        Some(elem) => quote! {
                            #io::__private::counted_run_copied::<#elem>(&self.#ident, &self.__carry, #within, kids, #off, cx)?
                        },
                        None => write.clone(),
                    };
                    let copies = match vec_elem(ty) {
                        _ if by_value(ty) => Some(
                            quote!(#io::__private::keep_copies::<#ty>(&mut __carry, #within, kids, #off, rx)?;),
                        ),
                        Some(_) => Some(
                            quote!(#io::__private::keep_run_copy(&mut __carry, #within, kids, #off, rx)?;),
                        ),
                        None => None,
                    };
                    match (selector, &f.keyed_by) {
                        (Some(s), _) => {
                            let chosen = s.chosen();
                            let stated = s.stated();
                            let arms = s.arms();
                            let names: Vec<String> = arms.iter().map(|a| a.to_string()).collect();
                            let readers: Vec<Ident> = arms.iter().map(|a| arm_reader(a)).collect();
                            from.push(quote! {
                                #ident: match #chosen {
                                    #( ::core::option::Option::Some(#names) =>
                                        <#ty>::#readers(&{ raw.#ident }, kids, #off, rx)?, )*
                                    _ => return ::core::result::Result::Err(#error::Invalid {
                                        what: ::std::string::String::from(#what),
                                        why: ::std::format!("{}, which selects no arm", #stated),
                                    }),
                                }
                            });
                            to.push(quote!(raw.#ident = #write;));
                            let stamps = s.conds.iter().filter_map(|(c, arm)| {
                                let (_, stamp) = c.as_ref()?;
                                Some((arm.to_string(), stamp))
                            });
                            let mut stamped: Vec<String> = Vec::new();
                            let mut stamp_arms = Vec::new();
                            for (arm, stamp) in stamps {
                                if !stamped.contains(&arm) {
                                    stamp_arms.push(quote!(#arm => { #stamp }));
                                    stamped.push(arm);
                                }
                            }
                            fixups.push(quote! {
                                {
                                    let __want: &'static str = <#ty>::__arm_name(&self.#ident);
                                    match __want {
                                        #(#stamp_arms)*
                                        _ => {}
                                    }
                                    let __got: ::core::option::Option<&'static str> = #chosen;
                                    if __got != ::core::option::Option::Some(__want) {
                                        return ::core::result::Result::Err(#error::Invalid {
                                            what: ::std::string::String::from(#what),
                                            why: ::std::format!(
                                                "the {} arm is chosen, and with {} the file reads it as {}",
                                                __want,
                                                #stated,
                                                __got.unwrap_or("no arm")
                                            ),
                                        });
                                    }
                                }
                            });
                        }
                        (None, Some(key)) if vec_elem(ty).is_some() => {
                            let key_ty = fields
                                .iter()
                                .find(|g| g.ident == *key)
                                .map(|g| &g.ty)
                                .expect("a key the checks above found");
                            let elem = vec_elem(ty);
                            keep.push(quote! {
                                {
                                    let __keys = <#key_ty as #io::Owned>::from_raw(
                                        &{ raw.#key },
                                        kids,
                                        at + ::core::mem::offset_of!(#raw_self, #key),
                                        rx,
                                    )?;
                                    rx.__own_keys(kids, #off, __keys.iter().map(#io::KeyWord::word));
                                }
                            });
                            // An entry is read by value only once what reached its target first is read.
                            match by_value(ty) {
                                true => from.push(quote! {
                                    #ident: {
                                        let __run = #read;
                                        #io::__private::keep_copies::<#ty>(&mut __carry, #within, kids, #off, rx)?;
                                        __run
                                    }
                                }),
                                false => from.push(quote!(#ident: #read)),
                            }
                            to.push(quote! {
                                raw.#ident = #io::__private::keyed_run_to::<#elem>(
                                    &self.#ident,
                                    self.#key.iter().map(#io::KeyWord::word),
                                    &self.__carry,
                                    #within,
                                    kids,
                                    #off,
                                    cx,
                                )?;
                            });
                        }
                        (None, Some(key)) => {
                            from.push(quote! {
                                #ident: {
                                    rx.__own_key(kids, #off, #io::KeyWord::word(&{ raw.#key }));
                                    #read
                                }
                            });
                            to.push(quote! {
                                raw.#ident = cx.__keyed_by(#io::KeyWord::word(&self.#key), || {
                                    <#ty as #io::Owned>::to_raw(&self.#ident, kids, #off, cx)
                                })?;
                            });
                        }
                        (None, None) if may_be_empty(ty) => {
                            // A word that is not an address reads as empty and is written back while it stays empty.
                            let word = format_ident!("__word_{}", unraw(ident));
                            keep.push(quote! {
                                let #word = #io::__private::keep_word(&mut __carry, #within, kids, #off);
                            });
                            keep.extend(copies);
                            from.push(quote! {
                                #ident: if #word { ::core::default::Default::default() } else { #read }
                            });
                            to.push(quote! {
                                raw.#ident = #plain;
                                #io::__private::put_word(&self.__carry, #within, #off, kids);
                            });
                            // A live pointer at an empty string reads as empty and is written back live while it stays empty.
                            if is_string(ty) {
                                keep.push(quote! {
                                    #io::__private::keep_empty_string(&mut __carry, #within, kids, #off);
                                });
                                to.push(quote! {
                                    #io::__private::put_empty_string(&self.__carry, #within, #off, kids);
                                });
                            }
                        }
                        (None, None) => {
                            keep.extend(copies);
                            from.push(quote!(#ident: #read));
                            to.push(quote!(raw.#ident = #plain;));
                        }
                    }
                    leaf.push(quote!(<#ty as #io::Owned>::LEAF));
                    if let Some(elem) = vec_elem(ty) {
                        keep.push(quote!(__carry.keep_capacity(#within, &{ raw.#ident });));
                        to.push(quote! {
                            let mut array = { raw.#ident };
                            self.__carry.apply_capacity::<#elem, _>(#within, #off, kids, &mut array, cx)?;
                            raw.#ident = array;
                        });
                    }
                }
            },
        }
    }
    let recompute = match &args.recompute {
        Some(path) => quote!(#path(self, &mut raw, kids, at, cx)?;),
        None => Tokens::new(),
    };
    let key = match &args.key {
        Some(key) => {
            let key = super::field_ident(key);
            quote! {
                fn key(&self) -> ::core::option::Option<u32> {
                    ::core::option::Option::Some(#io::KeyWord::word(&self.#key))
                }
                fn raw_key(raw: &Self::Raw) -> ::core::option::Option<u32> {
                    ::core::option::Option::Some(#io::KeyWord::word(&{ raw.#key }))
                }
            }
        }
        None => Tokens::new(),
    };

    let shown_tys: Vec<&Type> = shown.iter().map(|f| &f.ty).collect();
    let phantom = super::unused_params(&params.all, &shown_tys);
    let phantom_init = if phantom.is_empty() {
        Tokens::new()
    } else {
        quote!(__params: ::core::marker::PhantomData,)
    };
    let field_idents: Vec<&Ident> = shown.iter().map(|f| &f.ident).collect();
    let field_defaults: Vec<Tokens> = shown.iter().map(|f| default_of(&f.ty)).collect();
    let default_impl = quote! {
        impl<#(#owned_params),*> ::core::default::Default for #self_ty #where_clause {
            fn default() -> Self {
                Self {
                    #(#field_idents: #field_defaults,)*
                    #phantom_init
                    __carry: ::core::default::Default::default(),
                }
            }
        }
    };

    let value = quote!(::or_io::desc::Value);
    let any = match &args.tag {
        Some(_) => {
            let any_name = format_ident!("{}Any", name);
            let any_raw = format_ident!("__{}AnyRaw", name);
            // Several tags may select one layout, which is one variant.
            let mut vtys: Vec<&Type> = Vec::new();
            for (_, t) in &args.variants {
                let spelled = t.to_token_stream().to_string();
                if !vtys
                    .iter()
                    .any(|v| v.to_token_stream().to_string() == spelled)
                {
                    vtys.push(t);
                }
            }
            let vidents: Vec<Ident> = vtys
                .iter()
                .map(|t| match t {
                    Type::Path(p) => p.path.segments.last().expect("a segment").ident.clone(),
                    _ => format_ident!("Variant"),
                })
                .collect();
            let vraws: Vec<Type> = vtys
                .iter()
                .map(|t| raw_of(t, &params))
                .collect::<syn::Result<_>>()?;
            let doc =
                format!("A [`{name_str}`] behind a pointer, as whichever layout its tag selects.");
            let ext_name = format_ident!("{}AnyExt", name);
            let ext_doc = format!(
                "Each layout of a [`{any_name}`], reached in one call from the pointer that holds it."
            );
            let (as_ref, as_mut): (Vec<Ident>, Vec<Ident>) = vidents
                .iter()
                .map(|v| {
                    let short = short_name(&v.to_string(), &name_str);
                    (
                        format_ident!("as_{}", short),
                        format_ident!("as_{}_mut", short),
                    )
                })
                .unzip();
            let as_docs: Vec<String> = vidents
                .iter()
                .map(|v| {
                    format!("The [`{v}`] this holds; `None` for another layout or a null pointer.")
                })
                .collect();
            let any_self = quote!(#any_name<#(#all),*>);
            quote! {
                #[doc = #doc]
                #[derive(Debug, Clone, PartialEq)]
                #vis enum #any_name<#(#struct_params),*> #where_clause {
                    #( #vidents(#vtys), )*
                    /// A tag no declared variant takes.
                    Base(#self_ty),
                }

                impl<#(#owned_params),*> #any_self #where_clause {
                    #(
                        #[doc = #as_docs]
                        pub fn #as_ref(&self) -> ::core::option::Option<&#vtys> {
                            match self {
                                Self::#vidents(x) => ::core::option::Option::Some(x),
                                _ => ::core::option::Option::None,
                            }
                        }

                        #[doc = #as_docs]
                        pub fn #as_mut(&mut self) -> ::core::option::Option<&mut #vtys> {
                            match self {
                                Self::#vidents(x) => ::core::option::Option::Some(x),
                                _ => ::core::option::Option::None,
                            }
                        }
                    )*
                }

                #[doc = #ext_doc]
                #vis trait #ext_name<#(#owned_params),*> #where_clause {
                    #(
                        #[doc = #as_docs]
                        fn #as_ref(&self) -> ::core::option::Option<&#vtys>;
                        #[doc = #as_docs]
                        fn #as_mut(&mut self) -> ::core::option::Option<&mut #vtys>;
                    )*
                }

                impl<#(#owned_params),*> #ext_name<#(#all),*> for ::core::option::Option<::std::boxed::Box<#any_self>> #where_clause {
                    #(
                        fn #as_ref(&self) -> ::core::option::Option<&#vtys> {
                            self.as_deref().and_then(#any_name::#as_ref)
                        }
                        fn #as_mut(&mut self) -> ::core::option::Option<&mut #vtys> {
                            self.as_deref_mut().and_then(#any_name::#as_mut)
                        }
                    )*
                }

                #[doc(hidden)]
                #vis type #any_raw<#(#all),*> = #raw_name<#(#all),*>;

                impl<#(#owned_params),*> #io::Heap for #any_self #where_clause {
                    fn heap_bytes(&self) -> usize {
                        match self {
                            #( Self::#vidents(x) => #io::Heap::heap_bytes(x), )*
                            Self::Base(x) => #io::Heap::heap_bytes(x),
                        }
                    }
                }

                impl<#(#owned_params),*> #io::Owned for #any_self #where_clause {
                    type Raw = #raw_self;
                    const LEAF: bool = false;
                    fn from_raw(_: &Self::Raw, _: #io::Kids<'_>, at: usize, _: &#io::Rx<'_>) -> ::core::result::Result<Self, #error> {
                        ::core::result::Result::Err(#error::Invalid {
                            what: ::std::string::String::from(#name_str),
                            why: ::std::format!("a tagged base is reached through a pointer, not embedded at {at:#x}"),
                        })
                    }
                    fn to_raw(&self, _: &mut ::std::vec::Vec<(u32, #value)>, at: usize, _: &#io::Cx) -> ::core::result::Result<Self::Raw, #error> {
                        ::core::result::Result::Err(#error::Invalid {
                            what: ::std::string::String::from(#name_str),
                            why: ::std::format!("a tagged base is reached through a pointer, not embedded at {at:#x}"),
                        })
                    }
                    fn from_value(v: &#value, rx: &#io::Rx<'_>) -> ::core::result::Result<Self, #error> {
                        let found = v.type_name().unwrap_or_default().to_string();
                        #(
                            if found == <#vraws as #io::Described>::name() {
                                return ::core::result::Result::Ok(Self::#vidents(<#vtys as #io::Owned>::from_value(v, rx)?));
                            }
                        )*
                        ::core::result::Result::Ok(Self::Base(<#self_ty as #io::Owned>::from_value(v, rx)?))
                    }
                    fn to_element(&self, cx: &#io::Cx) -> ::core::result::Result<#value, #error> {
                        match self {
                            #( Self::#vidents(x) => #io::Owned::to_element(x, cx), )*
                            Self::Base(x) => #io::Owned::to_element(x, cx),
                        }
                    }
                    fn __anchor(&mut self, address: u64) {
                        match self {
                            #( Self::#vidents(x) => #io::Owned::__anchor(x, address), )*
                            Self::Base(x) => #io::Owned::__anchor(x, address),
                        }
                    }
                    fn __anchored(&self) -> u64 {
                        match self {
                            #( Self::#vidents(x) => #io::Owned::__anchored(x), )*
                            Self::Base(x) => #io::Owned::__anchored(x),
                        }
                    }
                }
            }
        }
        None => Tokens::new(),
    };

    Ok(quote! {
        #(#param_traits)*

        #raw_tokens

        #(#item_attrs)*
        #[derive(Debug, Clone, PartialEq)]
        #vis struct #name<#(#struct_params),*> #where_clause {
            #(#owned_fields,)*
            #phantom
            #[doc(hidden)]
            pub __carry: #io::Carry,
        }

        impl<#(#owned_params),*> #io::Owned for #self_ty #where_clause {
            type Raw = #raw_self;
            const LEAF: bool = true #(&& #leaf)*;

            fn from_raw(raw: &Self::Raw, kids: #io::Kids<'_>, at: usize, rx: &#io::Rx<'_>) -> ::core::result::Result<Self, #error> {
                let mut __carry = #io::Carry::default();
                #(#lends)*
                #(#keep)*
                #[allow(unused_mut)]
                let mut __value = Self {
                    #(#from,)*
                    #phantom_init
                    __carry,
                };
                #(#opened)*
                ::core::result::Result::Ok(__value)
            }

            fn to_raw(&self, kids: &mut ::std::vec::Vec<(u32, #value)>, at: usize, cx: &#io::Cx) -> ::core::result::Result<Self::Raw, #error> {
                let mut raw = <Self::Raw as #io::Plain>::zeroed();
                #(#to)*
                #(#links)*
                #(#fixups)*
                #(#codecs)*
                #recompute
                ::core::result::Result::Ok(raw)
            }

            fn to_element(&self, cx: &#io::Cx) -> ::core::result::Result<#value, #error> {
                let value = {
                    let _entered = cx.enter(::core::any::type_name::<Self>());
                    let mut kids = ::std::vec::Vec::new();
                    let raw = <Self as #io::Owned>::to_raw(self, &mut kids, 0, cx)?;
                    #io::record_value(cx, &<Self::Raw as #io::Described>::name(), #io::Plain::as_bytes(&raw), kids)?
                };
                cx.__settle(value)
            }

            #key

            fn __anchor(&mut self, address: u64) {
                self.__carry.__anchor(address)
            }

            fn __anchored(&self) -> u64 {
                self.__carry.__anchored()
            }

            fn run_to_value(items: &[Self], capacity: ::core::option::Option<usize>, cx: &#io::Cx) -> ::core::result::Result<#value, #error> {
                if <Self as #io::Owned>::LEAF {
                    #io::records_value(items, &<Self::Raw as #io::Described>::name(), capacity, cx)
                } else {
                    ::core::result::Result::Ok(#value::Array {
                        elements: items.iter().map(|i| #io::Owned::to_element(i, cx)).collect::<::core::result::Result<_, _>>()?,
                        capacity,
                    })
                }
            }
        }

        impl<#(#owned_params),*> #io::Heap for #self_ty #where_clause {
            fn heap_bytes(&self) -> usize {
                #io::Heap::heap_bytes(&self.__carry) #(+ #io::Heap::heap_bytes(&self.#shown_idents))*
            }
        }

        impl<#(#owned_params),*> #io::HasOrigin for #self_ty #where_clause {
            fn origin(&self) -> &#io::Origin { self.__carry.origin() }
            fn origin_mut(&mut self) -> &mut #io::Origin { self.__carry.origin_mut() }
        }

        #default_impl

        #any
    }
    .into_token_stream())
}

/// `#[record(union, size = N)]` on an enum of `Arm(Record)` variants: bytes
/// laid out as one of several pointer-free records of that size, which a
/// `#[select]` on the field holding them chooses between.
pub fn union_record(attr: Tokens, item: ItemEnum) -> syn::Result<Tokens> {
    let io = io();
    let error = error();
    let value = quote!(::or_io::desc::Value);
    let mut size = None;
    for meta in Punctuated::<Meta, Token![,]>::parse_terminated.parse2(attr)? {
        match &meta {
            Meta::Path(p) if p.is_ident("union") => {}
            Meta::NameValue(nv) if nv.path.is_ident("size") => size = Some(lit_int(&nv.value)?),
            other => return Err(Error::new(other.span(), "`#[record(union, size = N)]`")),
        }
    }
    let size =
        size.ok_or_else(|| Error::new(item.span(), "a union says its size: `size = N`"))? as usize;
    if !item.generics.params.is_empty() {
        return Err(Error::new(item.generics.span(), "a union is not generic"));
    }
    let mut arms = Vec::new();
    for v in &item.variants {
        let ty = match &v.fields {
            Fields::Unnamed(u) if u.unnamed.len() == 1 && v.discriminant.is_none() => {
                u.unnamed[0].ty.clone()
            }
            _ => {
                return Err(Error::new(
                    v.span(),
                    "a union's arm is `Arm(Record)`: one record, the size of the union",
                ))
            }
        };
        arms.push((v.ident.clone(), v.attrs.clone(), ty));
    }
    let Some((first, ..)) = arms.first() else {
        return Err(Error::new(item.span(), "a union has at least one arm"));
    };

    let name = &item.ident;
    let name_str = name.to_string();
    let raw_name = format_ident!("__{}Raw", name);
    let vis = &item.vis;
    let attrs = &item.attrs;
    let idents: Vec<&Ident> = arms.iter().map(|(i, ..)| i).collect();
    let vattrs: Vec<&Vec<Attribute>> = arms.iter().map(|(_, a, _)| a).collect();
    let tys: Vec<&Type> = arms.iter().map(|(.., t)| t).collect();
    let strs: Vec<String> = idents.iter().map(|i| i.to_string()).collect();
    let slots: Vec<Ident> = idents
        .iter()
        .map(|i| super::field_ident(&short_name(&i.to_string(), "")))
        .collect();
    let readers: Vec<Ident> = idents.iter().map(|i| arm_reader(i)).collect();
    let checks = arms.iter().map(|(arm, _, ty)| {
        let wide = format!("`{name_str}::{arm}` is not the union's {size:#x} bytes");
        let leaf = format!("`{name_str}::{arm}` holds a pointer, and a union's arms hold none");
        quote! {
            ::core::assert!(::core::mem::size_of::<<#ty as #io::Owned>::Raw>() == #size, #wide);
            ::core::assert!(<#ty as #io::Owned>::LEAF, #leaf);
        }
    });

    Ok(quote! {
        #(#attrs)*
        #[derive(Debug, Clone, PartialEq)]
        #vis enum #name {
            #( #(#vattrs)* #idents(#tys), )*
        }

        #[doc(hidden)]
        #[repr(C)]
        #[derive(Clone, Copy)]
        #vis union #raw_name {
            #( #slots: <#tys as #io::Owned>::Raw, )*
        }

        const _: () = { #(#checks)* };

        // SAFETY: every arm is `Plain` and exactly the union's size, so every
        // byte of the union is some arm's and any bit pattern is valid.
        unsafe impl #io::Plain for #raw_name {}

        impl #io::Field for #raw_name {
            fn repr(_: ::core::option::Option<#io::__private::CountSource>) -> #io::__private::FieldRepr {
                #io::__private::FieldRepr::Opaque { size: #size }
            }
        }

        impl ::core::fmt::Debug for #raw_name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_tuple(#name_str).field(&#io::Plain::as_bytes(self)).finish()
            }
        }

        impl ::core::default::Default for #name {
            fn default() -> Self {
                Self::#first(::core::default::Default::default())
            }
        }

        impl #io::Heap for #name {
            fn heap_bytes(&self) -> usize {
                match self {
                    #( Self::#idents(x) => #io::Heap::heap_bytes(x), )*
                }
            }
        }

        impl #io::Owned for #name {
            type Raw = #raw_name;
            const LEAF: bool = true;
            fn from_raw(_: &Self::Raw, _: #io::Kids<'_>, at: usize, _: &#io::Rx<'_>) -> ::core::result::Result<Self, #error> {
                ::core::result::Result::Err(#error::Invalid {
                    what: ::std::string::String::from(#name_str),
                    why: ::std::format!("the union at {at:#x} is read beside the field that selects its arm"),
                })
            }
            fn to_raw(&self, kids: &mut ::std::vec::Vec<(u32, #value)>, at: usize, cx: &#io::Cx) -> ::core::result::Result<Self::Raw, #error> {
                let mut raw = <Self::Raw as #io::Plain>::zeroed();
                match self {
                    #( Self::#idents(x) => raw.#slots = <#tys as #io::Owned>::to_raw(x, kids, at, cx)?, )*
                }
                ::core::result::Result::Ok(raw)
            }
            fn to_element(&self, _: &#io::Cx) -> ::core::result::Result<#value, #error> {
                ::core::result::Result::Err(#error::Invalid {
                    what: ::std::string::String::from(#name_str),
                    why: ::std::string::String::from("a union is held in its owner's bytes, not as an element"),
                })
            }
        }

        impl #name {
            #[doc(hidden)]
            pub fn __arm_name(&self) -> &'static str {
                match self {
                    #( Self::#idents(_) => #strs, )*
                }
            }

            #(
                #[doc(hidden)]
                pub fn #readers(raw: &#raw_name, kids: #io::Kids<'_>, at: usize, rx: &#io::Rx<'_>) -> ::core::result::Result<Self, #error> {
                    let arm = <<#tys as #io::Owned>::Raw as #io::Plain>::read_from(#io::Plain::as_bytes(raw))
                        .ok_or_else(|| #error::Invalid {
                            what: ::std::string::String::from(#name_str),
                            why: ::std::string::String::from("the union is shorter than its arm"),
                        })?;
                    ::core::result::Result::Ok(Self::#idents(<#tys as #io::Owned>::from_raw(&arm, kids, at, rx)?))
                }
            )*
        }
    })
}

#[cfg(test)]
mod tests {
    use super::short_name;

    #[test]
    fn a_variant_accessor_drops_the_base_name_and_goes_snake_case() {
        assert_eq!(
            short_name("CrFrameFilterMultiWeight", "CrFrameFilter"),
            "multi_weight"
        );
        assert_eq!(short_name("BoneBasic", "CrFrameFilter"), "bone_basic");
        assert_eq!(
            short_name("CrFrameFilter", "CrFrameFilter"),
            "cr_frame_filter"
        );
        assert_eq!(short_name("PhBoundBVH", "PhBound"), "bvh");
        assert_eq!(short_name("BVHNode2", "Ph"), "bvh_node2");
    }
}
