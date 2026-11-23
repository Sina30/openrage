//! `#[record]`: one declaration, and from it the `#[repr(C)]` struct read in
//! place, its view and record accessors, and its description.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote, ToTokens};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Error, Expr, ExprLit, Fields, GenericArgument, GenericParam, Ident, ItemEnum,
    ItemStruct, Lit, LitInt, LitStr, Meta, PathArguments, Token, Type,
};

/// Declare a record, or with `enum` an open enum, laid out exactly as the file holds it.
#[proc_macro_attribute]
pub fn record(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = Tokens::from(attr);
    let union = matches!(attr.clone().into_iter().next(), Some(proc_macro2::TokenTree::Ident(i)) if i == "union");
    let out = match syn::parse::<syn::Item>(item) {
        Ok(syn::Item::Struct(item)) => record_struct(attr, item),
        Ok(syn::Item::Enum(item)) if union => owned::union_record(attr, item),
        Ok(syn::Item::Enum(item)) => record_enum(attr, item),
        Ok(other) => Err(Error::new(
            other.span(),
            "`#[record]` declares a struct or an enum",
        )),
        Err(e) => Err(e),
    };
    out.unwrap_or_else(Error::into_compile_error).into()
}

mod owned;

/// A field's name as the description spells it: `r#type` is `type`.
fn unraw(ident: &Ident) -> String {
    syn::ext::IdentExt::unraw(ident).to_string()
}

/// The identifier for a field named `name`, raw where `name` is a keyword.
fn field_ident(name: &str) -> Ident {
    syn::parse_str::<Ident>(name).unwrap_or_else(|_| Ident::new_raw(name, Span::call_site()))
}

fn io() -> Tokens {
    quote!(::or_io::decl)
}

fn pad_type(n: u64) -> Type {
    let n = LitInt::new(&n.to_string(), Span::call_site());
    syn::parse_quote!([u8; #n])
}

fn lit_int(expr: &Expr) -> syn::Result<u64> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse(),
        other => Err(Error::new(other.span(), "expected an integer literal")),
    }
}

fn lit_str(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s.value()),
        Expr::Path(p) if p.path.get_ident().is_some() => {
            Ok(p.path.get_ident().unwrap().to_string())
        }
        other => Err(Error::new(other.span(), "expected a string")),
    }
}

#[derive(Clone)]
struct Args {
    size: u64,
    target: Option<String>,
    packed: Option<u64>,
    tag: Option<(String, Span)>,
    tag_bits: Option<(u32, u32)>,
    variants: Vec<(i64, Type)>,
    /// An owned record's save-time hook: `fn(&Self, &mut Raw<Self>, &mut [(u32, Value)], usize, &Cx)`.
    recompute: Option<syn::Path>,
    /// The owned field a `Keyed` reference finds this record by.
    key: Option<String>,
}

/// `variant(4 => Weighted)`: the tag value, and the record that value selects.
fn parse_variant(list: &syn::MetaList) -> syn::Result<(i64, Type)> {
    list.parse_args_with(|input: syn::parse::ParseStream| {
        let negative = input.parse::<Option<Token![-]>>()?.is_some();
        let value: i64 = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<Token![=>]>()?;
        let ty: Type = input.parse()?;
        Ok((if negative { -value } else { value }, ty))
    })
}

fn parse_struct_args(attr: Tokens) -> syn::Result<Args> {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(attr)?;
    let mut size = None;
    let mut target = None;
    let mut packed = None;
    let mut tag = None;
    let mut tag_bits = None;
    let mut variants = Vec::new();
    let mut recompute = None;
    let mut key = None;
    for meta in metas {
        match &meta {
            Meta::List(list) if list.path.is_ident("tag_bits") => {
                let ints =
                    list.parse_args_with(Punctuated::<LitInt, Token![,]>::parse_terminated)?;
                let ints: Vec<u32> = ints
                    .iter()
                    .map(|i| i.base10_parse())
                    .collect::<syn::Result<_>>()?;
                let [low, width] = ints[..] else {
                    return Err(Error::new(list.span(), "`tag_bits(low, width)`"));
                };
                if width == 0 || low + width > 64 {
                    return Err(Error::new(list.span(), "a tag's bits lie inside 64"));
                }
                tag_bits = Some((low, width));
            }
            Meta::NameValue(nv) if nv.path.is_ident("size") => size = Some(lit_int(&nv.value)?),
            Meta::NameValue(nv) if nv.path.is_ident("key") => key = Some(lit_str(&nv.value)?),
            Meta::NameValue(nv) if nv.path.is_ident("recompute") => match &nv.value {
                Expr::Path(p) => recompute = Some(p.path.clone()),
                other => return Err(Error::new(other.span(), "`recompute = path::to::function`")),
            },
            Meta::NameValue(nv) if nv.path.is_ident("target") => target = Some(lit_str(&nv.value)?),
            Meta::NameValue(nv) if nv.path.is_ident("tag") => {
                tag = Some((lit_str(&nv.value)?, nv.value.span()))
            }
            Meta::List(list) if list.path.is_ident("packed") => {
                packed = Some(list.parse_args::<LitInt>()?.base10_parse()?);
            }
            Meta::List(list) if list.path.is_ident("variant") => {
                let (value, ty) = parse_variant(list)?;
                if variants.iter().any(|(v, _)| *v == value) {
                    return Err(Error::new(
                        list.span(),
                        format!("two variants have the tag {value}"),
                    ));
                }
                variants.push((value, ty));
            }
            other => {
                return Err(Error::new(
                    other.span(),
                    "`#[record]` takes `size = N`, and optionally `target = gen8`, `packed(N)`, \
                     and `tag = \"field\"` with one `variant(N => Type)` per tag value",
                ))
            }
        }
    }
    let size = size.ok_or_else(|| Error::new(Span::call_site(), "`#[record]` needs `size = N`"))?;
    match (&tag, variants.is_empty()) {
        (Some((_, span)), true) => {
            return Err(Error::new(
                *span,
                "a tag selects a variant: declare at least one `variant(N => Type)`",
            ))
        }
        (None, false) => {
            return Err(Error::new(
                Span::call_site(),
                "a variant is selected by a tag: add `tag = \"field\"`",
            ))
        }
        _ => {}
    }
    if tag_bits.is_some() && tag.is_none() {
        return Err(Error::new(
            Span::call_site(),
            "`tag_bits` says which bits of the tag field are the tag: add `tag = \"field\"`",
        ));
    }
    Ok(Args {
        size,
        target,
        packed,
        tag,
        tag_bits,
        variants,
        recompute,
        key,
    })
}

/// `#[bits(name, low, width)]`, or with a note: a named range of a field's bits.
#[derive(Clone)]
struct Bits {
    name: String,
    low: u32,
    width: u32,
    note: Option<String>,
    span: Span,
}

fn parse_bits(attr: &Attribute) -> syn::Result<Bits> {
    attr.parse_args_with(|input: syn::parse::ParseStream| {
        let name = syn::ext::IdentExt::parse_any(input)?;
        input.parse::<Token![,]>()?;
        let low: u32 = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<Token![,]>()?;
        let width: u32 = input.parse::<LitInt>()?.base10_parse()?;
        let note = if input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
            Some(input.parse::<LitStr>()?.value())
        } else {
            None
        };
        if width == 0 || low.checked_add(width).is_none_or(|end| end > 64) {
            return Err(Error::new(
                attr.span(),
                "a bit range is at least one bit wide and lies inside 64",
            ));
        }
        Ok(Bits {
            name: unraw(&name),
            low,
            width,
            note,
            span: attr.span(),
        })
    })
}

enum Len {
    Field(String),
    CountCapacity(String, String),
    Masked(String, u64),
    MaskedOrMipChain(String, u64),
    Product(String, String),
    Fixed(u64),
    Bytes(String),
    PrefixedAboveOne(String),
    Sum(Vec<String>),
}

fn parse_len(attr: &Attribute) -> syn::Result<Len> {
    let metas = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let mut get = std::collections::BTreeMap::new();
    for meta in &metas {
        let Meta::NameValue(nv) = meta else {
            return Err(Error::new(
                meta.span(),
                "`#[len(...)]` takes `key = value` pairs",
            ));
        };
        let key = nv
            .path
            .get_ident()
            .map(Ident::to_string)
            .unwrap_or_default();
        get.insert(key, nv.value.clone());
    }
    let s = |k: &str| get.get(k).map(lit_str).transpose();
    Ok(if let Some(v) = get.get("fixed") {
        Len::Fixed(lit_int(v)?)
    } else if let Some(sum) = s("sum")? {
        let parts: Vec<String> = sum.split('+').map(|p| p.trim().to_string()).collect();
        if parts.len() < 2 || parts.iter().any(String::is_empty) {
            return Err(Error::new(
                attr.span(),
                "`sum = \"a + b\"` names two fields or more",
            ));
        }
        Len::Sum(parts)
    } else if let Some(product) = s("product")? {
        let (a, b) = product
            .split_once('*')
            .ok_or_else(|| Error::new(attr.span(), "`product = \"count * stride\"`"))?;
        Len::Product(a.trim().to_string(), b.trim().to_string())
    } else if let Some(masked) = s("masked")? {
        let mask = get
            .get("mask")
            .ok_or_else(|| Error::new(attr.span(), "`masked` needs `mask = 0x...`"))?;
        Len::Masked(masked, lit_int(mask)?)
    } else if let Some(masked) = s("masked_or_mip_chain")? {
        let mask = get
            .get("mask")
            .ok_or_else(|| Error::new(attr.span(), "`masked_or_mip_chain` needs `mask = 0x...`"))?;
        Len::MaskedOrMipChain(masked, lit_int(mask)?)
    } else if let Some(count) = s("count")? {
        let capacity =
            s("capacity")?.ok_or_else(|| Error::new(attr.span(), "`count` needs `capacity`"))?;
        Len::CountCapacity(count, capacity)
    } else if let Some(f) = s("field")? {
        Len::Field(f)
    } else if let Some(f) = s("bytes")? {
        Len::Bytes(f)
    } else if let Some(f) = s("prefixed_above_one")? {
        Len::PrefixedAboveOne(f)
    } else {
        return Err(Error::new(
            attr.span(),
            "`#[len]` takes `field`, `count` + `capacity`, `masked` + `mask`, `masked_or_mip_chain` + `mask`, `product`, `sum`, `fixed`, `bytes` or `prefixed_above_one`",
        ));
    })
}

impl Len {
    fn fields(&self) -> Vec<&str> {
        match self {
            Len::Field(f) | Len::Bytes(f) | Len::PrefixedAboveOne(f) | Len::Masked(f, _) => vec![f],
            Len::CountCapacity(c, cap) => vec![c, cap],
            Len::Product(a, b) => vec![a, b],
            Len::Sum(parts) => parts.iter().map(String::as_str).collect(),
            Len::MaskedOrMipChain(f, _) => std::iter::once(f.as_str())
                .chain(MIP_CHAIN_FIELDS)
                .collect(),
            Len::Fixed(_) => vec![],
        }
    }

    fn source(&self) -> Tokens {
        let p = quote!(::or_io::decl::__private::CountSource);
        match self {
            Len::Field(f) | Len::Bytes(f) => quote!(#p::Field(#f.to_string())),
            Len::CountCapacity(c, cap) => {
                quote!(#p::CountAndCapacity { count: #c.to_string(), capacity: #cap.to_string() })
            }
            Len::Masked(f, m) => quote!(#p::MaskedField { field: #f.to_string(), mask: #m }),
            Len::MaskedOrMipChain(f, m) => {
                quote!(#p::MaskedOrMipChain { field: #f.to_string(), mask: #m })
            }
            Len::Product(a, b) => {
                quote!(#p::Product { count: #a.to_string(), stride: #b.to_string() })
            }
            Len::Fixed(n) => {
                let n = *n as usize;
                quote!(#p::Fixed(#n))
            }
            Len::PrefixedAboveOne(f) => quote!(#p::PrefixedAboveOne { count: #f.to_string() }),
            Len::Sum(parts) => {
                quote!(#p::Sum { fields: ::std::vec![#(#parts.to_string()),*] })
            }
        }
    }

    fn value(&self) -> Tokens {
        let io = io();
        let read = |f: &str| {
            let f = Ident::new(f, Span::call_site());
            quote!(#io::Len::len(&{ self.0.get().#f }))
        };
        match self {
            Len::Field(f) | Len::Bytes(f) | Len::CountCapacity(f, _) => read(f),
            Len::Masked(f, m) => {
                let r = read(f);
                quote!(((#r as u64) & #m) as usize)
            }
            Len::Product(a, b) => {
                let (a, b) = (read(a), read(b));
                quote!(#a.saturating_mul(#b))
            }
            Len::Fixed(n) => {
                let n = *n as usize;
                quote!(#n)
            }
            Len::PrefixedAboveOne(f) => {
                let r = read(f);
                quote!(#io::__private::prefixed_above_one(#r))
            }
            Len::Sum(parts) => {
                let reads: Vec<Tokens> = parts.iter().map(|p| read(p)).collect();
                quote!(0usize #(.saturating_add(#reads))*)
            }
            Len::MaskedOrMipChain(f, m) => {
                let r = read(f);
                let dims: Vec<Tokens> = MIP_CHAIN_FIELDS.iter().map(|d| read(d)).collect();
                quote!(match ((#r as u64) & #m) as usize {
                    0 => #io::__private::CountSource::mip_chain_len(#(#dims as u32),*),
                    stated => stated,
                })
            }
        }
    }
}

/// ⚠ `or_io::desc::MIP_CHAIN_FIELDS`, in its order: this crate cannot import it.
const MIP_CHAIN_FIELDS: [&str; 7] = [
    "format",
    "width",
    "height",
    "depth",
    "mip_count",
    "cut_mip_levels",
    "layer_count",
];

/// What a field's type makes of it, read from its spelling.
enum Kind {
    Plain,
    One(Type),
    Str,
    Run(Type),
    Array(Type),
    Trailing(Type),
    Member(Type),
    Embed(Type),
}

const SCALARS: &[&str] = &[
    "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "Bool8", "Hash32", "Count",
    "Open", "Vec2", "Vec3", "Vec4",
];

fn first_arg(seg: &syn::PathSegment) -> Option<Type> {
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

fn kind_of(ty: &Type) -> Kind {
    let Type::Path(path) = ty else {
        return Kind::Plain;
    };
    let Some(seg) = path.path.segments.last() else {
        return Kind::Plain;
    };
    let name = seg.ident.to_string();
    if SCALARS.contains(&name.as_str()) {
        return Kind::Plain;
    }
    match name.as_str() {
        "Str" => Kind::Str,
        "Ptr" | "PhysPtr" => match first_arg(seg) {
            Some(Type::Slice(s)) => Kind::Run(*s.elem),
            Some(Type::Path(p)) if p.path.segments.last().is_some_and(|s| s.ident == "CStr") => {
                Kind::Str
            }
            Some(Type::Path(p)) if p.path.segments.last().is_some_and(|s| s.ident == "Opaque") => {
                Kind::Plain
            }
            Some(t) => Kind::One(t),
            None => Kind::Plain,
        },
        "MemberPtr" => first_arg(seg).map_or(Kind::Plain, Kind::Member),
        "Array" => first_arg(seg).map_or(Kind::Plain, Kind::Array),
        "Trailing" => first_arg(seg).map_or(Kind::Plain, Kind::Trailing),
        _ => Kind::Embed(ty.clone()),
    }
}

/// Whether `ty` is spelled `Hash32`.
fn is_hash32(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Hash32"))
}

/// A bare type parameter may stand for a scalar, so its run has no record accessor.
fn looks_like_record(ty: &Type, params: &[Ident]) -> bool {
    match ty {
        Type::Path(p)
            if p.path.segments.len() == 1 && params.contains(&p.path.segments[0].ident) =>
        {
            false
        }
        Type::Path(p) => p.path.segments.last().is_some_and(|s| {
            !SCALARS.contains(&s.ident.to_string().as_str())
                && !["Ptr", "PhysPtr", "MemberPtr", "Str"].contains(&s.ident.to_string().as_str())
        }),
        _ => false,
    }
}

/// Whether `ty` spells the identifier `p` anywhere in it.
fn names(ty: &Type, p: &Ident) -> bool {
    fn walk(tokens: Tokens, p: &Ident) -> bool {
        tokens.into_iter().any(|t| match t {
            proc_macro2::TokenTree::Ident(i) => i == *p,
            proc_macro2::TokenTree::Group(g) => walk(g.stream(), p),
            _ => false,
        })
    }
    walk(ty.to_token_stream(), p)
}

/// A zero-sized field using each parameter no field type names, as a tagged
/// base's target is named only by its variants; empty when every one is named.
fn unused_params(params: &[Ident], tys: &[&Type]) -> Tokens {
    let unused: Vec<&Ident> = params
        .iter()
        .filter(|p| !tys.iter().any(|t| names(t, p)))
        .collect();
    if unused.is_empty() {
        return Tokens::new();
    }
    quote! {
        #[doc(hidden)]
        pub __params: ::core::marker::PhantomData<fn() -> (#(#unused,)*)>,
    }
}

fn mentions_param_by_value(ty: &Type, params: &[Ident]) -> bool {
    match ty {
        Type::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
            let seg = &p.path.segments[0];
            params.contains(&seg.ident) && seg.arguments.is_empty()
        }
        Type::Path(p) => p
            .path
            .segments
            .first()
            .is_some_and(|s| params.contains(&s.ident)),
        Type::Array(a) => mentions_param_by_value(&a.elem, params),
        _ => false,
    }
}

struct FieldDecl {
    ident: Ident,
    ty: Type,
    /// An optional cross-check: the offset is always the one the layout computes.
    at: Option<u64>,
    len: Option<Len>,
    note: Option<String>,
    bits: Vec<Bits>,
    public: bool,
    pad: bool,
    unknown: bool,
}

fn record_struct(attr: Tokens, item: ItemStruct) -> syn::Result<Tokens> {
    owned::owned_record(parse_struct_args(attr)?, item)
}

/// `owned_public`: for the raw half of an owned declaration, the fields the
/// owned struct shows and whether each holds an address. `unions`: its fields
/// holding an inline union, which a view leaves as bytes.
fn raw_record(
    args: Args,
    mut item: ItemStruct,
    desc_name: Option<String>,
    owned_public: Option<Vec<(String, bool)>>,
    unions: &[Ident],
) -> syn::Result<Tokens> {
    let io = io();
    if let Some(repr) = item.attrs.iter().find(|a| a.path().is_ident("repr")) {
        return Err(Error::new(
            repr.span(),
            "`#[record]` chooses the representation; remove `#[repr]`",
        ));
    }

    let mut params = Vec::new();
    for param in &item.generics.params {
        match param {
            GenericParam::Type(t) => params.push(t.ident.clone()),
            other => {
                return Err(Error::new(
                    other.span(),
                    "a record's only parameters are types",
                ))
            }
        }
    }

    let Fields::Named(named) = &mut item.fields else {
        return Err(Error::new(item.span(), "a record has named fields"));
    };

    let mut fields = Vec::new();
    let mut last_at: Option<u64> = None;
    // Explicit padding a field declares after itself, inserted as a private field.
    let mut padded: Vec<(usize, Ident, u64)> = Vec::new();
    for (index, field) in named.named.iter_mut().enumerate() {
        let ident = field.ident.clone().expect("named");
        let mut at = None;
        let mut len = None;
        let mut note = None;
        let mut pad_after = None;
        let mut bits: Vec<Bits> = Vec::new();
        let mut kept = Vec::new();
        for a in field.attrs.drain(..) {
            if a.path().is_ident("at") {
                at = Some(a.parse_args::<LitInt>()?.base10_parse::<u64>()?);
            } else if a.path().is_ident("bits") {
                let range = parse_bits(&a)?;
                if let Some(other) = bits
                    .iter()
                    .find(|b| b.low < range.low + range.width && range.low < b.low + b.width)
                {
                    return Err(Error::new(
                        range.span,
                        format!("`{}` overlaps `{}`", range.name, other.name),
                    ));
                }
                bits.push(range);
            } else if a.path().is_ident("len") {
                len = Some(parse_len(&a)?);
            } else if a.path().is_ident("note") {
                note = Some(a.parse_args::<LitStr>()?.value());
            } else if a.path().is_ident("pad_after") {
                pad_after = Some(a.parse_args::<LitInt>()?.base10_parse::<u64>()?);
            } else {
                kept.push(a);
            }
        }
        field.attrs = kept;
        if let (Some(at), Some(prev)) = (at, last_at) {
            if at <= prev {
                return Err(Error::new(
                    ident.span(),
                    format!("`{ident}` is declared at {at:#x}, not after the field before it at {prev:#x}"),
                ));
            }
        }
        if at.is_some() {
            last_at = at;
        }
        if let Some(n) = pad_after {
            padded.push((index, ident.clone(), n));
        }
        let name = unraw(&ident);
        let pad = name.starts_with("_pad");
        let unknown = name.starts_with("unknown_");
        let public = matches!(field.vis, syn::Visibility::Public(_));
        if (pad || unknown) && public {
            return Err(Error::new(
                ident.span(),
                format!(
                    "`{ident}` is carried, not claimed: an unknown or padding field stays private"
                ),
            ));
        }
        if mentions_param_by_value(&field.ty, &params) {
            return Err(Error::new(
                field.ty.span(),
                "a type parameter may appear only behind a pointer, so no offset depends on it",
            ));
        }
        match (kind_of(&field.ty), &len) {
            (Kind::Run(_) | Kind::Trailing(_), None) => {
                return Err(Error::new(
                    ident.span(),
                    format!("`{ident}` is a run: say where its length is with `#[len(...)]`"),
                ))
            }
            (Kind::Array(_), Some(_)) => {
                return Err(Error::new(
                    ident.span(),
                    format!("`{ident}` is an `Array`, which carries its own count"),
                ))
            }
            _ => {}
        }
        fields.push(FieldDecl {
            ident: ident.clone(),
            ty: field.ty.clone(),
            at,
            len,
            note,
            bits,
            public,
            pad,
            unknown,
        });
        if let Some(n) = pad_after {
            fields.push(FieldDecl {
                ident: format_ident!("_pad_after_{}", ident),
                ty: pad_type(n),
                at: None,
                len: None,
                note: None,
                bits: Vec::new(),
                public: false,
                pad: true,
                unknown: false,
            });
        }
    }
    // The padding each `#[pad_after(N)]` declares, as a private field right after its owner.
    for (index, ident, n) in padded.into_iter().rev() {
        let pad_ident = format_ident!("_pad_after_{}", ident);
        let ty = pad_type(n);
        let field: syn::Field = syn::parse_quote!(#pad_ident: #ty);
        let mut rebuilt: Punctuated<syn::Field, Token![,]> = Punctuated::new();
        for (i, f) in std::mem::take(&mut named.named).into_iter().enumerate() {
            rebuilt.push(f);
            if i == index {
                rebuilt.push(field.clone());
            }
        }
        named.named = rebuilt;
    }

    let mut ranges: Vec<&Bits> = Vec::new();
    for b in fields.iter().flat_map(|f| &f.bits) {
        if ranges.iter().any(|r| r.name == b.name) {
            return Err(Error::new(
                b.span,
                format!("two bit ranges of this record are named `{}`", b.name),
            ));
        }
        ranges.push(b);
    }

    for f in &fields {
        if let Some(len) = &f.len {
            for name in len.fields() {
                if !fields.iter().any(|g| g.ident == name) {
                    return Err(Error::new(
                        f.ident.span(),
                        format!(
                            "`{}`'s length names `{name}`, which this record does not declare",
                            f.ident
                        ),
                    ));
                }
            }
        }
    }

    if let Some((tag, span)) = &args.tag {
        if !fields.iter().any(|f| unraw(&f.ident) == tag.as_str()) {
            return Err(Error::new(
                *span,
                format!("the tag `{tag}` is not a field of this record"),
            ));
        }
    }

    let name = &item.ident;
    let name_str = desc_name.unwrap_or_else(|| name.to_string());
    let size = args.size as usize;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    let self_ty = quote!(#name #ty_generics);
    let static_bounds: Vec<Tokens> = params.iter().map(|p| quote!(#p: 'static)).collect();
    let field_tys: Vec<&Type> = fields.iter().map(|f| &f.ty).collect();

    let mut where_plain = where_clause
        .map(|w| w.predicates.to_token_stream())
        .unwrap_or_default();
    if !where_plain.is_empty() {
        where_plain.extend(quote!(,));
    }
    let preds = quote! { #where_plain #(#static_bounds,)* };

    // Layout proof. Offsets are the ones `repr(C)` computes from field order and
    // natural alignment; what is proved is that no byte is padding the compiler
    // chose — every field ends where the next begins, the last where the size
    // does — and that an optional `#[at]` agrees with the computed offset.
    let mut checks = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        let ident = &f.ident;
        let ty = &f.ty;
        for b in &f.bits {
            let end = (b.low + b.width) as usize;
            let msg = format!(
                "`{name_str}.{ident}`'s bit range `{}` ends at bit {end}, past the field's width",
                b.name
            );
            checks.push(quote! {
                ::core::assert!(#end <= ::core::mem::size_of::<#ty>() * 8, #msg);
            });
        }
        if let Some(at) = f.at {
            let at = at as usize;
            let msg = format!(
                "`{name_str}.{ident}` is declared at {at:#x} and the layout puts it elsewhere"
            );
            checks.push(quote! {
                ::core::assert!(::core::mem::offset_of!(Self, #ident) == #at, #msg);
            });
        }
        let (next, next_desc) = match fields.get(i + 1) {
            Some(n) => {
                let n_ident = &n.ident;
                (
                    quote!(::core::mem::offset_of!(Self, #n_ident)),
                    format!("`{name_str}.{}` begins", n.ident),
                )
            }
            None => (quote!(#size), format!("the declared size {size:#x} ends")),
        };
        let msg = format!(
            "`{name_str}.{ident}` does not end where {next_desc}: the compiler would pad here, so \
             declare the engine's padding field or `#[pad_after(N)]`, or fix the size"
        );
        checks.push(quote! {
            ::core::assert!(
                ::core::mem::offset_of!(Self, #ident) + ::core::mem::size_of::<#ty>() == #next,
                #msg
            );
        });
    }
    let msg = format!("`{name_str}` is not the declared {size:#x} bytes");
    checks.push(quote! { ::core::assert!(::core::mem::size_of::<Self>() == #size, #msg); });
    if fields.is_empty() {
        return Err(Error::new(
            name.span(),
            "a record declares at least one field",
        ));
    }

    let repr = match args.packed {
        Some(n) => {
            let n = LitInt::new(&n.to_string(), Span::call_site());
            quote!(#[repr(C, packed(#n))])
        }
        None => quote!(#[repr(C)]),
    };

    let target = match &args.target {
        Some(t) => quote!(::core::option::Option::Some(#t)),
        None => quote!(::core::option::Option::None),
    };

    let type_name = if params.is_empty() {
        quote!(::std::string::String::from(#name_str))
    } else {
        quote! {{
            let parts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(<#params as #io::Param>::param_name()),*]
                .into_iter().flatten().collect();
            if parts.is_empty() {
                ::std::string::String::from(#name_str)
            } else {
                ::std::format!("{}<{}>", #name_str, parts.join(", "))
            }
        }}
    };

    // Declared padding is described as padding, so every byte of the record
    // is accounted for and the writer still writes it as zero.
    let padding_fields: Vec<Tokens> = fields
        .iter()
        .filter(|f| unraw(&f.ident).starts_with("_pad_after_"))
        .map(|f| {
            let ident = &f.ident;
            let fname = unraw(ident);
            let ty = &f.ty;
            quote! {
                #io::__private::PackField {
                    name: ::std::string::String::from(#fname),
                    offset: ::core::mem::offset_of!(Self, #ident),
                    repr: #io::__private::FieldRepr::Padding { size: ::core::mem::size_of::<#ty>() },
                    space: ::core::option::Option::None,
                    bits: ::std::vec::Vec::new(),
                    note: ::core::option::Option::None,
                }
            }
        })
        .collect();
    let pack_fields: Vec<Tokens> = fields
        .iter()
        .filter(|f| !f.pad)
        .map(|f| {
            let ident = &f.ident;
            let fname = unraw(ident);
            let ty = &f.ty;
            let len = match &f.len {
                Some(l) => {
                    let s = l.source();
                    quote!(::core::option::Option::Some(#s))
                }
                None => quote!(::core::option::Option::None),
            };
            let repr = quote!(<#ty as #io::Field>::repr(#len));
            let note = match &f.note {
                Some(n) => quote!(::core::option::Option::Some(::std::string::String::from(#n))),
                None => quote!(::core::option::Option::None),
            };
            let bits = f.bits.iter().map(|b| {
                let (bname, low, width) = (&b.name, b.low, b.width);
                let note = match &b.note {
                    Some(n) => {
                        quote!(::core::option::Option::Some(::std::string::String::from(#n)))
                    }
                    None => quote!(::core::option::Option::None),
                };
                quote! {
                    #io::__private::BitRange {
                        name: ::std::string::String::from(#bname),
                        low: #low,
                        width: #width,
                        note: #note,
                    }
                }
            });
            quote! {
                #io::__private::PackField {
                    name: ::std::string::String::from(#fname),
                    offset: ::core::mem::offset_of!(Self, #ident),
                    repr: #repr,
                    space: <#ty as #io::Field>::space(),
                    bits: ::std::vec![#(#bits),*],
                    note: #note,
                }
            }
        })
        .collect();

    let mut view_fns = Vec::new();
    let mut record_fns = Vec::new();
    for f in fields
        .iter()
        .filter(|f| f.public && !f.pad && !f.unknown && !unions.contains(&f.ident))
    {
        let ident = &f.ident;
        let fname = unraw(ident);
        let mut_ident = format_ident!("{}_mut", ident);
        let raw = quote!({ self.0.get().#ident }.raw());
        match kind_of(&f.ty) {
            Kind::Plain => {}
            Kind::One(t) => {
                view_fns.push(quote! {
                    pub fn #ident(&self) -> #io::Result<::core::option::Option<#io::View<'a, #t>>> {
                        self.0.__one::<#t>(#fname, #raw)
                    }
                });
                record_fns.push(quote! {
                    pub fn #mut_ident(&mut self) -> #io::Result<::core::option::Option<#io::RecordMut<'_, #t>>> {
                        self.0.__one::<#t>(#fname)
                    }
                });
            }
            Kind::Str => view_fns.push(quote! {
                pub fn #ident(&self) -> #io::Result<::core::option::Option<&'a ::core::ffi::CStr>> {
                    self.0.__str(#fname, #raw)
                }
            }),
            Kind::Member(t) => view_fns.push(quote! {
                pub fn #ident(&self) -> #io::Result<::core::option::Option<#io::View<'a, #t>>> {
                    self.0.__member::<#t>(#fname, #raw)
                }
            }),
            Kind::Run(e) => {
                let n = f.len.as_ref().expect("checked").value();
                view_fns.push(quote! {
                    pub fn #ident(&self) -> #io::Result<#io::Run<'a, #e>> {
                        let n: usize = #n;
                        self.0.__run::<#e>(#fname, #raw, n)
                    }
                });
                if looks_like_record(&e, &params) {
                    record_fns.push(quote! {
                        pub fn #mut_ident(&mut self) -> #io::Result<#io::RunMut<'_, #e>> {
                            self.0.__run::<#e>(#fname)
                        }
                    });
                }
            }
            Kind::Array(e) => {
                view_fns.push(quote! {
                    pub fn #ident(&self) -> #io::Result<#io::Run<'a, #e>> {
                        let array = { self.0.get().#ident };
                        self.0.__run::<#e>(#fname, array.raw(), array.count())
                    }
                });
                if looks_like_record(&e, &params) {
                    record_fns.push(quote! {
                        pub fn #mut_ident(&mut self) -> #io::Result<#io::RunMut<'_, #e>> {
                            self.0.__run::<#e>(#fname)
                        }
                    });
                }
            }
            Kind::Trailing(e) => {
                let n = f.len.as_ref().expect("checked").value();
                view_fns.push(quote! {
                    pub fn #ident(&self) -> #io::Result<#io::Run<'a, #e>> {
                        let n: usize = #n;
                        self.0.__trailing::<#e>(#fname, n)
                    }
                });
                if looks_like_record(&e, &params) {
                    record_fns.push(quote! {
                        pub fn #mut_ident(&mut self) -> #io::Result<#io::RunMut<'_, #e>> {
                            self.0.__run::<#e>(#fname)
                        }
                    });
                }
            }
            Kind::Embed(t) if args.packed.is_none() => view_fns.push(quote! {
                pub fn #ident(&self) -> #io::View<'a, #t> {
                    self.0.__embed(&self.0.get().#ident, ::core::mem::offset_of!(#self_ty, #ident))
                }
            }),
            Kind::Embed(_) => {}
        }
    }

    let fields_idents: Vec<&Ident> = fields.iter().map(|f| &f.ident).collect();
    let mut view_generics = item.generics.clone();
    view_generics.params.insert(0, syn::parse_quote!('a));
    let (view_impl, view_ty, _) = view_generics.split_for_impl();
    let mut record_generics = item.generics.clone();
    record_generics.params.insert(0, syn::parse_quote!('r));
    let (record_impl, record_ty, _) = record_generics.split_for_impl();
    let access = format_ident!("__{}Access", name);
    let edit = format_ident!("__{}Edit", name);
    let access_params = &view_generics.params;
    let edit_params = &record_generics.params;
    let gat_view_ty = quote!(#access<'a, #(#params),*>);
    let gat_edit_ty = quote!(#edit<'r, #(#params),*>);

    let vis = &item.vis;
    let attrs = &item.attrs;
    let generics = &item.generics;
    let fields_tokens = named.named.iter().map(|f| {
        let attrs = &f.attrs;
        let vis = &f.vis;
        let ident = &f.ident;
        let ty = &f.ty;
        quote!(#(#attrs)* #vis #ident: #ty)
    });
    let where_tokens = &generics.where_clause;
    let phantom = unused_params(&params, &field_tys);

    let eager = if params.is_empty() {
        quote! { const _: () = <#name as #io::Described>::__LAYOUT; }
    } else {
        Tokens::new()
    };

    let variant_tys: Vec<&Type> = args.variants.iter().map(|(_, t)| t).collect();
    let variant_tags: Vec<i64> = args.variants.iter().map(|(v, _)| *v).collect();
    let mut grouped: Vec<(&Type, Vec<i64>)> = Vec::new();
    for (value, ty) in &args.variants {
        match grouped
            .iter_mut()
            .find(|(t, _)| t.to_token_stream().to_string() == ty.to_token_stream().to_string())
        {
            Some((_, tags)) => tags.push(*value),
            None => grouped.push((ty, vec![*value])),
        }
    }
    let impl_tys: Vec<&Type> = grouped.iter().map(|(t, _)| *t).collect();
    let impl_tags: Vec<&Vec<i64>> = grouped.iter().map(|(_, tags)| tags).collect();
    let (selector, tagged) = match &args.tag {
        Some((tag, _)) => {
            let tag_ident = field_ident(tag);
            let (bits, read) = match args.tag_bits {
                Some((low, width)) => {
                    let mask: u64 = if width == 64 {
                        u64::MAX
                    } else {
                        (1u64 << width) - 1
                    };
                    (
                        quote! {
                            ::core::option::Option::Some(#io::__private::BitRange {
                                name: ::std::string::String::from(#tag),
                                low: #low,
                                width: #width,
                                note: ::core::option::Option::None,
                            })
                        },
                        quote! {
                            ((#io::TagValue::tag_bits({ self.#tag_ident }) >> #low) & #mask) as i64
                        },
                    )
                }
                None => (
                    quote!(::core::option::Option::None),
                    quote!(#io::TagValue::tag_value({ self.#tag_ident })),
                ),
            };
            (
                quote! {
                    ::core::option::Option::Some(#io::__private::PackSelector {
                        tag: ::std::string::String::from(#tag),
                        tag_bits: #bits,
                        variants: ::std::vec![#((#variant_tags, <#variant_tys as #io::Described>::name())),*],
                    })
                },
                quote! {
                    impl #impl_generics #io::Tagged for #self_ty where #preds {
                        fn tag(&self) -> i64 { #read }
                    }
                    #(
                        impl #impl_generics #io::VariantOf<#self_ty> for #impl_tys where #preds {
                            const TAGS: &'static [i64] = &[#(#impl_tags),*];
                        }
                    )*
                },
            )
        }
        None => (quote!(::core::option::Option::None), Tokens::new()),
    };

    let hash_fields: Vec<String> = (fields.iter())
        .filter(|f| !f.pad && is_hash32(&f.ty))
        .map(|f| unraw(&f.ident))
        .collect();
    let owned_public = match owned_public {
        Some(fields) => {
            let (names, addresses): (Vec<String>, Vec<bool>) = fields.into_iter().unzip();
            quote! {
                const __OWNED_PUBLIC: ::core::option::Option<&'static [(&'static str, bool)]> =
                    ::core::option::Option::Some(&[#((#names, #addresses)),*]);
            }
        }
        None => Tokens::new(),
    };

    Ok(quote! {
        #(#attrs)*
        #repr
        #vis struct #name #generics #where_tokens {
            #(#fields_tokens,)*
            #phantom
        }

        impl #impl_generics ::core::clone::Clone for #self_ty where #preds #(#field_tys: ::core::marker::Copy,)* {
            fn clone(&self) -> Self { *self }
        }
        impl #impl_generics ::core::marker::Copy for #self_ty where #preds #(#field_tys: ::core::marker::Copy,)* {}

        // SAFETY: every field is `Plain`, and `__LAYOUT` proves at compile
        // time that the fields tile the declared size with no gap.
        unsafe impl #impl_generics #io::Plain for #self_ty where #preds #(#field_tys: #io::Plain,)* {}

        impl #impl_generics #io::Described for #self_ty where #preds {
            const __LAYOUT: () = { #(#checks)* };
            fn name() -> ::std::string::String { #type_name }
            fn target() -> ::core::option::Option<&'static str> { #target }
            fn describe() -> #io::__private::PackType {
                let () = <Self as #io::Described>::__LAYOUT;
                #io::__private::PackType {
                    name: <Self as #io::Described>::name(),
                    size: #size,
                    fields: {
                        let mut fields = ::std::vec![#(#pack_fields,)* #(#padding_fields,)*];
                        fields.sort_by_key(|f| f.offset);
                        fields
                    },
                    selector: #selector,
                }
            }
            fn reach(reach: &mut #io::Reach) {
                #(<#field_tys as #io::Field>::reach(reach);)*
                #(reach.visit::<#variant_tys>();)*
            }
            const __HASH_FIELDS: &'static [&'static str] = &[#(#hash_fields),*];
            #owned_public
        }

        #eager

        #tagged

        impl #impl_generics #io::Record for #self_ty where #preds {
            type Access<'a> = #gat_view_ty;
            type Edit<'r> = #gat_edit_ty;
        }

        impl #impl_generics #io::Field for #self_ty where #preds {
            fn repr(_: ::core::option::Option<#io::__private::CountSource>) -> #io::__private::FieldRepr {
                #io::__private::FieldRepr::Inline { type_name: <Self as #io::Described>::name() }
            }
            fn reach(reach: &mut #io::Reach) { reach.visit::<Self>() }
        }

        impl #impl_generics #io::Element for #self_ty where #preds {
            fn elem() -> #io::Elem { #io::Elem::Type(<Self as #io::Described>::name()) }
            fn reach(reach: &mut #io::Reach) { reach.visit::<Self>() }
        }

        impl #impl_generics #io::PointsTo for #self_ty where #preds {
            fn pointee(_: ::core::option::Option<#io::__private::CountSource>) -> #io::__private::Pointee {
                #io::__private::Pointee::One { type_name: <Self as #io::Described>::name() }
            }
            fn reach(reach: &mut #io::Reach) { reach.visit::<Self>() }
        }

        impl #impl_generics #io::Param for #self_ty where #preds {
            fn param_name() -> ::core::option::Option<::std::string::String> {
                ::core::option::Option::Some(<Self as #io::Described>::name())
            }
        }

        impl #impl_generics ::core::fmt::Debug for #self_ty where #preds #(#field_tys: ::core::fmt::Debug + ::core::marker::Copy,)* {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(#name_str)
                    #(.field(::core::stringify!(#fields_idents), &{ self.#fields_idents }))*
                    .finish()
            }
        }

        #[doc(hidden)]
        #vis struct #access<#access_params>(#io::RawView<'a, #self_ty>) where #preds;

        impl #view_impl ::core::clone::Clone for #access #view_ty where #preds {
            fn clone(&self) -> Self { *self }
        }
        impl #view_impl ::core::marker::Copy for #access #view_ty where #preds {}

        impl #view_impl ::core::ops::Deref for #access #view_ty where #preds #self_ty: #io::Record {
            type Target = #self_ty;
            #[inline]
            fn deref(&self) -> &#self_ty { self.0.get() }
        }

        impl #view_impl #io::Access<'a, #self_ty> for #access #view_ty where #preds #self_ty: #io::Record {
            fn wrap(raw: #io::RawView<'a, #self_ty>) -> Self { Self(raw) }
            fn raw(&self) -> &#io::RawView<'a, #self_ty> { &self.0 }
        }

        impl #view_impl #access #view_ty where #preds #self_ty: #io::Record {
            #(#view_fns)*
        }

        #[doc(hidden)]
        #vis struct #edit<#edit_params>(#io::RawRecord<'r, #self_ty>) where #preds #self_ty: #io::Plain;

        impl #record_impl ::core::ops::Deref for #edit #record_ty where #preds #self_ty: #io::Record {
            type Target = #self_ty;
            fn deref(&self) -> &#self_ty { self.0.get() }
        }

        impl #record_impl ::core::ops::DerefMut for #edit #record_ty where #preds #self_ty: #io::Record {
            fn deref_mut(&mut self) -> &mut #self_ty { self.0.get_mut() }
        }

        impl #record_impl #io::Edit<'r, #self_ty> for #edit #record_ty where #preds #self_ty: #io::Record {
            fn wrap(raw: #io::RawRecord<'r, #self_ty>) -> Self { Self(raw) }
            fn raw_mut(&mut self) -> &mut #io::RawRecord<'r, #self_ty> { &mut self.0 }
        }

        impl #record_impl #edit #record_ty where #preds #self_ty: #io::Record {
            #(#record_fns)*
        }
    })
}

fn record_enum(attr: Tokens, item: ItemEnum) -> syn::Result<Tokens> {
    let io = io();
    let mut tokens = attr.into_iter().peekable();
    let is_enum = matches!(tokens.peek(), Some(proc_macro2::TokenTree::Ident(i)) if i == "enum");
    if is_enum {
        tokens.next();
        if matches!(tokens.peek(), Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == ',') {
            tokens.next();
        }
    }
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(tokens.collect())?;
    let mut width = None;
    for meta in metas {
        match &meta {
            Meta::NameValue(nv) if nv.path.is_ident("width") => width = Some(lit_str(&nv.value)?),
            other => return Err(Error::new(other.span(), "`#[record(enum, width = u8)]`")),
        }
    }
    if !is_enum {
        return Err(Error::new(
            item.span(),
            "an enum is declared with `#[record(enum, width = ...)]`",
        ));
    }
    let width =
        width.ok_or_else(|| Error::new(item.span(), "an enum says its width: `width = u8`"))?;
    let (raw, int_width) = match width.as_str() {
        "u8" => (quote!(u8), quote!(U8)),
        "u16" => (quote!(u16), quote!(U16)),
        "u32" => (quote!(u32), quote!(U32)),
        "u64" => (quote!(u64), quote!(U64)),
        "i8" => (quote!(i8), quote!(I8)),
        "i16" => (quote!(i16), quote!(I16)),
        "i32" => (quote!(i32), quote!(I32)),
        "i64" => (quote!(i64), quote!(I64)),
        _ => {
            return Err(Error::new(
                item.span(),
                "an enum's width is an integer type",
            ))
        }
    };

    let name = &item.ident;
    let name_str = name.to_string();
    let mut variants = Vec::new();
    for v in &item.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(Error::new(
                v.span(),
                "an enum's variants are values, with no fields",
            ));
        }
        let Some((_, value)) = &v.discriminant else {
            return Err(Error::new(v.span(), "every variant states its value"));
        };
        let value = match value {
            Expr::Lit(ExprLit {
                lit: Lit::Int(i), ..
            }) => i.base10_parse::<i128>()?,
            Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => -(lit_int(&u.expr)? as i128),
            other => {
                return Err(Error::new(
                    other.span(),
                    "a variant's value is an integer literal",
                ))
            }
        };
        // ⚠ Pack data: the first doc line that says something is what an inspector shows beside the value.
        let note = v.attrs.iter().find_map(|a| match &a.meta {
            Meta::NameValue(nv) if nv.path.is_ident("doc") => lit_str(&nv.value)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            _ => None,
        });
        if variants
            .iter()
            .any(|(_, _, other, _): &(Ident, Vec<Attribute>, i128, Option<String>)| *other == value)
        {
            return Err(Error::new(
                v.span(),
                format!("two variants have the value {value}"),
            ));
        }
        variants.push((v.ident.clone(), v.attrs.clone(), value, note));
    }
    if variants.iter().any(|(i, ..)| i == "Other") {
        return Err(Error::new(
            item.span(),
            "`Other` is the variant for values no variant names",
        ));
    }

    let vis = &item.vis;
    let attrs = &item.attrs;
    let idents: Vec<&Ident> = variants.iter().map(|(i, ..)| i).collect();
    let vattrs: Vec<&Vec<Attribute>> = variants.iter().map(|(_, a, ..)| a).collect();
    let values: Vec<Tokens> = variants
        .iter()
        .map(|(_, _, v, _)| {
            let lit = LitInt::new(&format!("{v}{width}"), Span::call_site());
            quote!(#lit)
        })
        .collect();
    let pack_variants: Vec<Tokens> = variants
        .iter()
        .map(|(i, _, v, note)| {
            let n = i.to_string();
            let v = *v as i64;
            let note = match note {
                Some(s) => quote!(::core::option::Option::Some(::std::string::String::from(#s))),
                None => quote!(::core::option::Option::None),
            };
            quote!(#io::__private::PackVariant { name: ::std::string::String::from(#n), value: #v, note: #note })
        })
        .collect();

    Ok(quote! {
        #(#attrs)*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        #vis enum #name {
            #( #(#vattrs)* #idents, )*
            /// A value no variant names, kept as the number it is.
            Other(#raw),
        }

        impl #io::OpenEnum for #name {
            type Raw = #raw;
            const WIDTH: #io::__private::IntWidth = #io::__private::IntWidth::#int_width;
            fn from_raw(raw: #raw) -> Self {
                match raw {
                    #( #values => Self::#idents, )*
                    other => Self::Other(other),
                }
            }
            fn to_raw(self) -> #raw {
                match self {
                    #( Self::#idents => #values, )*
                    Self::Other(raw) => raw,
                }
            }
            fn name() -> &'static str { #name_str }
            fn describe() -> #io::__private::PackEnum {
                #io::__private::PackEnum {
                    name: ::std::string::String::from(#name_str),
                    width: #io::__private::IntWidth::#int_width,
                    variants: ::std::vec![#(#pack_variants),*],
                }
            }
        }

        impl ::core::fmt::Display for #name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let raw = <Self as #io::OpenEnum>::to_raw(*self);
                match self {
                    #( Self::#idents => ::core::write!(f, "{} ({})", ::core::stringify!(#idents), raw), )*
                    Self::Other(_) => ::core::write!(f, "{}", raw),
                }
            }
        }
    })
}
