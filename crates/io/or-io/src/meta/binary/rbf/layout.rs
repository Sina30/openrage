use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RbfStructShape {
    pub x1: i16,
    pub x2: i16,
    pub attribute_count: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RbfDescriptor {
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct RbfLayout {
    pub descriptors: Vec<RbfDescriptor>,
    pub shapes: BTreeMap<String, RbfStructShape>,
    pub conflicts: Vec<String>,
    pub content_kinds: BTreeMap<String, String>,
    /// Whether GTA VI's marker `FB FF 01 00` follows the magic, after which a 0x10 and a 0x40
    /// value are 8 bytes.
    pub wide_values: bool,
    /// Each struct's shape by its name, in stream order, which a body with [`Self::wide_values`]
    /// or [`Self::conflicts`] writes back: its structs of one name state different shapes.
    pub occurrences: BTreeMap<String, Vec<RbfStructShape>>,
}

impl RbfLayout {
    pub fn record_shape(&mut self, name: &str, shape: RbfStructShape) {
        match self.shapes.get(name) {
            Some(existing) if *existing != shape => {
                if !self.conflicts.iter().any(|c| c == name) {
                    self.conflicts.push(name.to_string());
                }
            }
            Some(_) => {}
            None => {
                self.shapes.insert(name.to_string(), shape);
            }
        }
    }

    pub fn shape_for(&self, name: &str) -> RbfStructShape {
        self.shapes.get(name).copied().unwrap_or_default()
    }

    pub fn note_descriptor(&mut self, name: &str) {
        if !self.descriptors.iter().any(|d| d.name == name) {
            self.descriptors.push(RbfDescriptor {
                name: name.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_identical_shapes_are_not_conflicts() {
        let mut l = RbfLayout::default();
        let s = RbfStructShape {
            x1: 1,
            x2: 2,
            attribute_count: 3,
        };
        l.record_shape("Item", s);
        l.record_shape("Item", s);
        assert!(l.conflicts.is_empty());
        assert_eq!(l.shape_for("Item"), s);
    }

    #[test]
    fn differing_shapes_for_one_tag_are_flagged() {
        let mut l = RbfLayout::default();
        l.record_shape(
            "Item",
            RbfStructShape {
                x1: 1,
                x2: 2,
                attribute_count: 3,
            },
        );
        l.record_shape(
            "Item",
            RbfStructShape {
                x1: 9,
                x2: 2,
                attribute_count: 3,
            },
        );
        assert_eq!(l.conflicts, vec!["Item".to_string()]);
    }

    #[test]
    fn descriptors_keep_first_use_order_without_duplicates() {
        let mut l = RbfLayout::default();
        l.note_descriptor("a");
        l.note_descriptor("b");
        l.note_descriptor("a");
        assert_eq!(l.descriptors.len(), 2);
        assert_eq!(l.descriptors[0].name, "a");
        assert_eq!(l.descriptors[1].name, "b");
    }
}
