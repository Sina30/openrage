use crate::desc::{FieldRepr, Pack, Pointee};

pub fn audit(pack: &Pack) -> Vec<String> {
    let mut problems = Vec::new();

    if pack.types.is_empty() {
        problems.push(
            "the pack describes no types at all - is the crate defining them actually linked?"
                .to_string(),
        );
    }

    let mut seen_names: Vec<&str> = Vec::new();
    let described: Vec<&str> = pack.types.iter().map(|t| t.name.as_str()).collect();

    for ty in &pack.types {
        if seen_names.contains(&ty.name.as_str()) {
            problems.push(format!("`{}` is described twice", ty.name));
        } else {
            seen_names.push(&ty.name);
        }

        // Whether the description fits together: no field past the end of its
        // own type, no two fields over the same byte. `bind` runs the same check
        // at load time, for a type whose `T__Raw` mirrors a different target
        // and so has no `offset_of!` to be compared with.
        problems.extend(ty.incoherences(|name| pack.type_by_name(name).map(|t| t.size)));

        let mut seen_fields: Vec<&str> = Vec::new();
        for field in &ty.fields {
            if seen_fields.contains(&field.name.as_str()) {
                problems.push(format!("`{}.{}` is described twice", ty.name, field.name));
            } else {
                seen_fields.push(&field.name);
            }
        }

        if let Some(selector) = &ty.selector {
            match ty.field_by_name(&selector.tag) {
                None => problems.push(format!(
                    "`{}` is tagged by `{}`, which it does not have",
                    ty.name, selector.tag
                )),
                Some(tag) => {
                    use crate::desc::{FieldRepr, Scalar};
                    let float = matches!(tag.repr, FieldRepr::Scalar(Scalar::F32 | Scalar::F64));
                    let readable = match &tag.repr {
                        FieldRepr::Scalar(Scalar::Int(_)) | FieldRepr::Enum { .. } => true,
                        _ => float && selector.tag_bits.is_some(),
                    };
                    if !readable {
                        problems.push(if float {
                            format!(
                                "`{}` is tagged by `{}`, which is a float and does not say \
                                 which of its bits are the tag",
                                ty.name, selector.tag
                            )
                        } else {
                            format!(
                                "`{}` is tagged by `{}`, which cannot be read as one: a tag \
                                 has to be an integer, an enum, or a float with a bit range",
                                ty.name, selector.tag
                            )
                        });
                    }

                    if let Some(range) = &selector.tag_bits {
                        let holds = tag.repr.inline_size().map(|b| (b * 8) as u32).unwrap_or(0);
                        if range.width == 0 {
                            problems.push(format!(
                                "`{}` takes its tag from zero bits of `{}`",
                                ty.name, selector.tag
                            ));
                        }
                        if holds > 0 && range.end() > holds {
                            problems.push(format!(
                                "`{}` takes its tag from bits {}..{} of `{}`, past the \
                                 field's {holds}",
                                ty.name,
                                range.low,
                                range.end(),
                                selector.tag
                            ));
                        }
                    }
                }
            }

            let mut seen_tags: Vec<i64> = Vec::new();
            for (value, variant) in &selector.variants {
                if !pack.types.iter().any(|t| &t.name == variant) {
                    problems.push(format!(
                        "`{}` selects `{variant}` for {value}, which the pack does not describe",
                        ty.name
                    ));
                }
                if seen_tags.contains(value) {
                    problems.push(format!("`{}` maps {value} to more than one type", ty.name));
                } else {
                    seen_tags.push(*value);
                }
            }
        }

        let trailing: Vec<&crate::desc::PackField> =
            ty.fields.iter().filter(|f| f.repr.is_trailing()).collect();
        if trailing.len() > 1 {
            problems.push(format!(
                "`{}` declares {} trailing runs — {} — and they would all begin at {:#x}",
                ty.name,
                trailing.len(),
                trailing
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                ty.size
            ));
        }

        if !trailing.is_empty() {
            for other in &pack.types {
                for field in &other.fields {
                    let by_value = match &field.repr {
                        FieldRepr::Inline { type_name }
                        | FieldRepr::InlineArray { type_name, .. } => Some(("embeds", type_name)),
                        FieldRepr::Ptr(Pointee::Many {
                            type_name: Some(t), ..
                        })
                        | FieldRepr::AtArray(Pointee::Many {
                            type_name: Some(t), ..
                        })
                        | FieldRepr::Trailing {
                            type_name: Some(t), ..
                        } => Some(("lays out a run of", t)),
                        _ => None,
                    };
                    if let Some((how, name)) = by_value {
                        if name == &ty.name {
                            problems.push(format!(
                                "`{}.{}` {how} `{}`, which trails a run of its own — only a \
                                 position where the size is still open can hold one, so reach \
                                 it through a pointer",
                                other.name, field.name, ty.name
                            ));
                        }
                    }
                }
            }
        }

        for field in &ty.fields {
            if field.repr.is_trailing() {
                if field.offset != ty.size {
                    problems.push(format!(
                        "`{}.{}` trails the struct and is at {:#x}, where `{}` is {:#x} bytes; \
                         a trailing run begins exactly where its header ends",
                        ty.name, field.name, field.offset, ty.name, ty.size
                    ));
                }
            } else if field.offset >= ty.size {
                problems.push(format!(
                    "`{}.{}` is at {:#x}, outside the type's {:#x} bytes",
                    ty.name, field.name, field.offset, ty.size
                ));
            }

            if let FieldRepr::Trailing {
                count,
                element_size,
                type_name,
            } = &field.repr
            {
                use crate::desc::CountSource;
                if matches!(count, CountSource::Inline | CountSource::Unknown) {
                    problems.push(format!(
                        "`{}.{}` trails the struct and states no count; there is no header \
                         beside it to carry one, so a reader has nothing to go on",
                        ty.name, field.name
                    ));
                }
                if type_name.is_none() && element_size == &Some(0) {
                    problems.push(format!(
                        "`{}.{}` trails the struct with a zero-byte stride, which is a run of \
                         no length whatever its count says",
                        ty.name, field.name
                    ));
                }
            }

            if let Some(count) = field.repr.count_field() {
                if !ty.fields.iter().any(|f| f.name == count) {
                    problems.push(format!(
                        "`{}.{}` takes its count from `{count}`, which `{}` does not have",
                        ty.name, field.name, ty.name
                    ));
                }
            }

            if let Some(capacity) = field.repr.capacity_field() {
                if !ty.fields.iter().any(|f| f.name == capacity) {
                    problems.push(format!(
                        "`{}.{}` takes its capacity from `{capacity}`, which `{}` does not have",
                        ty.name, field.name, ty.name
                    ));
                }
                if Some(capacity) == field.repr.count_field() {
                    problems.push(format!(
                        "`{}.{}` names `{capacity}` as both its count and its capacity; \
                         if they really are one field, drop `capacity_field`",
                        ty.name, field.name
                    ));
                }
            }

            if let Some(stride) = field.repr.stride_field() {
                if !ty.fields.iter().any(|f| f.name == stride) {
                    problems.push(format!(
                        "`{}.{}` multiplies its count by `{stride}`, which `{}` does not have",
                        ty.name, field.name, ty.name
                    ));
                }
                if Some(stride) == field.repr.count_field() {
                    problems.push(format!(
                        "`{}.{}` names `{stride}` as both its count and its stride, so its \
                         length is that field squared, which is not a length anything states",
                        ty.name, field.name
                    ));
                }
                let in_bytes = matches!(&field.repr, FieldRepr::Ptr(Pointee::Bytes { .. }))
                    || matches!(
                        &field.repr,
                        FieldRepr::Trailing {
                            type_name: None,
                            ..
                        }
                    );
                if !in_bytes {
                    problems.push(format!(
                        "`{}.{}` states its length as `{} * {stride}` and is not a run of \
                         bytes; a product is a byte count, and a run whose elements the \
                         pack describes already counts them",
                        ty.name,
                        field.name,
                        field.repr.count_field().unwrap_or("?")
                    ));
                }
            }

            if field.repr.describes_its_target() && field.space.is_none() {
                problems.push(format!(
                    "`{}.{}` is a pointer that does not say which heap it points into; \
                     declare it `as_physical_ptr` if it is graphics memory",
                    ty.name, field.name
                ));
            }

            if field.is_bit_packed() {
                let holds = field
                    .repr
                    .inline_size()
                    .map(|bytes| (bytes * 8) as u32)
                    .unwrap_or(0);

                for (i, range) in field.bits.iter().enumerate() {
                    if range.width == 0 {
                        problems.push(format!(
                            "`{}.{}` declares `{}` as zero bits wide",
                            ty.name, field.name, range.name
                        ));
                    }
                    if holds > 0 && range.end() > holds {
                        problems.push(format!(
                            "`{}.{}` puts `{}` at bits {}..{}, past the field's {holds}",
                            ty.name,
                            field.name,
                            range.name,
                            range.low,
                            range.end()
                        ));
                    }
                    for other in &field.bits[i + 1..] {
                        if range.mask() & other.mask() != 0 {
                            problems.push(format!(
                                "`{}.{}` overlaps `{}` and `{}` over the same bits",
                                ty.name, field.name, range.name, other.name
                            ));
                        }
                    }
                }
            }

            if let (Some(count), Some(mask)) = (field.repr.count_field(), field.repr.count_mask()) {
                if mask == 0 {
                    problems.push(format!(
                        "`{}.{}` masks its count from `{count}` with zero, which is no count",
                        ty.name, field.name
                    ));
                } else {
                    let run = mask >> mask.trailing_zeros();
                    if run & run.wrapping_add(1) != 0 {
                        problems.push(format!(
                            "`{}.{}` masks its count from `{count}` with {mask:#x}, which is not \
                             a contiguous run of bits",
                            ty.name, field.name
                        ));
                    }
                }
            }

            if !field.repr.is_indirect() {
                if let Some(space) = field.space {
                    problems.push(format!(
                        "`{}.{}` claims {space:?} storage and is not a pointer",
                        ty.name, field.name
                    ));
                }
            }

            if let Some(Pointee::Member { type_name } | Pointee::Members { type_name, .. }) =
                field.repr.pointee()
            {
                let embedded_somewhere = pack.types.iter().any(|t| {
                    t.fields.iter().any(|f| {
                        matches!(&f.repr,
                            FieldRepr::Inline { type_name: e } | FieldRepr::InlineArray { type_name: e, .. }
                                if e == type_name)
                    })
                });
                if !embedded_somewhere {
                    problems.push(format!(
                        "`{}.{}` points into its owner at a `{type_name}`, which no type in \
                         this pack embeds, so no record could hold what it points at",
                        ty.name, field.name
                    ));
                }
            }

            // ⚠ Graphics memory holds payloads, not addresses: across every
            // writer in the workspace, in both generations, the physical heap
            // holds vertex, index and pixel bytes. A table of pointers built
            // there resolves in OpenRage and is rejected by the platform's
            // memory manager.
            if matches!(
                field.repr.pointee(),
                Some(Pointee::ManyPtr { .. } | Pointee::Members { .. })
            ) && field.is_physical()
            {
                problems.push(format!(
                    "`{}.{}` is a table of pointers declared in graphics memory; the \
                     physical heap holds payloads, not addresses",
                    ty.name, field.name
                ));
            }

            if let Some(target) = field.repr.referenced_type() {
                if !described.contains(&target) {
                    problems.push(format!(
                        "`{}.{}` points at `{target}`, which this pack does not describe",
                        ty.name, field.name
                    ));
                }
            }

            if let Some(target) = field.repr.referenced_enum() {
                if pack.enum_by_name(target).is_none() {
                    problems.push(format!(
                        "`{}.{}` is a `{target}`, which this pack does not describe",
                        ty.name, field.name
                    ));
                }
            }
        }
    }

    let mut seen_enums: Vec<&str> = Vec::new();
    for enumeration in &pack.enums {
        if seen_enums.contains(&enumeration.name.as_str()) {
            problems.push(format!("enum `{}` is described twice", enumeration.name));
        } else {
            seen_enums.push(&enumeration.name);
        }

        if enumeration.variants.is_empty() {
            problems.push(format!("enum `{}` has no variants", enumeration.name));
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desc::{AddressSpace, FieldRepr, IntWidth, PackField, PackType, Pointee, Scalar};

    fn ty(name: &str, fields: Vec<PackField>) -> PackType {
        PackType {
            name: name.into(),
            size: 0x20,
            fields,
            selector: None,
        }
    }

    fn field(name: &str, offset: usize) -> PackField {
        PackField {
            name: name.into(),
            offset,
            repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
            space: None,
            bits: Vec::new(),
            note: None,
        }
    }

    fn pointer(name: &str, offset: usize, repr: FieldRepr) -> PackField {
        PackField {
            name: name.into(),
            offset,
            repr,
            space: Some(AddressSpace::Virtual),
            bits: Vec::new(),
            note: None,
        }
    }

    #[test]
    fn an_empty_pack_is_reported_as_probably_a_linking_mistake() {
        let pack = Pack::new("t", "g");
        assert!(audit(&pack)[0].contains("no types at all"));
    }

    #[test]
    fn a_type_described_twice_is_reported() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty("GrmGeometry", vec![]), ty("GrmGeometry", vec![])];

        assert!(audit(&pack).iter().any(|p| p.contains("described twice")));
    }

    #[test]
    fn a_field_outside_its_type_is_reported() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty("GrmGeometry", vec![field("vertex_count", 0x40)])];

        assert!(audit(&pack)
            .iter()
            .any(|p| p.contains("outside the type's")));
    }

    #[test]
    fn a_type_openrage_has_never_heard_of_is_not_a_problem() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty("LightAttributes", vec![field("falloff", 0)])];

        assert!(audit(&pack).is_empty(), "{:?}", audit(&pack));
    }

    #[test]
    fn a_count_field_that_does_not_exist_is_reported() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty(
            "GrmGeometry",
            vec![pointer(
                "bone_indices",
                0,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: None,
                    count: CountSourceShim::field("bone_index_count"),
                    element_size: Some(2),
                }),
            )],
        )];

        assert!(
            audit(&pack)
                .iter()
                .any(|p| p.contains("takes its count from `bone_index_count`")),
            "{:?}",
            audit(&pack)
        );
    }

    #[test]
    fn a_capacity_field_that_does_not_exist_is_reported() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty(
            "PhBoundComposite",
            vec![
                pointer(
                    "bounds",
                    0,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: None,
                        count: CountSourceShim::count_and_capacity("num_bounds", "max_num_bounds"),
                        element_size: Some(8),
                    }),
                ),
                field("num_bounds", 8),
            ],
        )];

        assert!(
            audit(&pack)
                .iter()
                .any(|p| p.contains("takes its capacity from `max_num_bounds`")),
            "{:?}",
            audit(&pack)
        );
    }

    #[test]
    fn one_field_cannot_be_both_the_count_and_the_capacity() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty(
            "PhBoundComposite",
            vec![
                pointer(
                    "bounds",
                    0,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: None,
                        count: CountSourceShim::count_and_capacity("num_bounds", "num_bounds"),
                        element_size: Some(8),
                    }),
                ),
                field("num_bounds", 8),
            ],
        )];

        assert!(
            audit(&pack)
                .iter()
                .any(|p| p.contains("as both its count and its capacity")),
            "{:?}",
            audit(&pack)
        );
    }

    #[test]
    fn a_pointer_into_an_undescribed_type_is_reported() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![ty(
            "GrmGeometry",
            vec![pointer(
                "vertex_buffer",
                0,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "VertexBuffer".into(),
                }),
            )],
        )];

        assert!(
            audit(&pack).iter().any(|p| p.contains("does not describe")),
            "{:?}",
            audit(&pack)
        );
    }

    #[test]
    fn a_clean_pack_audits_clean() {
        let mut pack = Pack::new("t", "g");
        pack.types = vec![
            ty(
                "Geometry",
                vec![
                    field("vertices_count", 0),
                    pointer(
                        "vertex_buffer",
                        8,
                        FieldRepr::Ptr(Pointee::One {
                            type_name: "VertexBuffer".into(),
                        }),
                    ),
                ],
            ),
            ty("VertexBuffer", vec![field("vertices_count", 0)]),
        ];

        assert!(audit(&pack).is_empty(), "{:?}", audit(&pack));
    }

    #[test]
    fn a_member_pointer_at_a_type_nothing_embeds_is_reported() {
        let members = |type_name: &str| {
            pointer(
                "props",
                0,
                FieldRepr::AtArray(Pointee::Members {
                    type_name: type_name.into(),
                    count: CountSourceShim::field("count"),
                }),
            )
        };
        let mut pack = Pack::new("t", "g");
        pack.types = vec![
            ty("Behaviour", vec![members("Prop"), field("count", 8)]),
            ty("Prop", vec![field("x", 0)]),
        ];
        assert!(
            audit(&pack)
                .iter()
                .any(|p| p.contains("points into its owner at a `Prop`")),
            "{:?}",
            audit(&pack)
        );

        pack.types[0].fields.push(PackField {
            repr: FieldRepr::Inline {
                type_name: "Prop".into(),
            },
            ..field("prop", 0x10)
        });
        assert!(
            !audit(&pack)
                .iter()
                .any(|p| p.contains("points into its owner")),
            "{:?}",
            audit(&pack)
        );
    }

    struct CountSourceShim;
    impl CountSourceShim {
        fn field(name: &str) -> crate::desc::CountSource {
            crate::desc::CountSource::Field(name.to_string())
        }

        fn count_and_capacity(count: &str, capacity: &str) -> crate::desc::CountSource {
            crate::desc::CountSource::CountAndCapacity {
                count: count.to_string(),
                capacity: capacity.to_string(),
            }
        }
    }
}
