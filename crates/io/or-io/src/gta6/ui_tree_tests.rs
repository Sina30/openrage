use super::*;

/// A document built record by record; a name is declared by the first record that uses it.
struct Doc {
    bytes: Vec<u8>,
    names: Vec<&'static str>,
    wide: bool,
}

impl Doc {
    fn new(wide: bool) -> Self {
        let mut bytes = b"RBF0".to_vec();
        if wide {
            bytes.extend(WIDE);
        }
        Self {
            bytes,
            names: Vec::new(),
            wide,
        }
    }

    fn record(&mut self, name: &'static str, kind: u8) -> &mut Self {
        let known = self.names.iter().position(|n| *n == name);
        let index = known.unwrap_or(self.names.len());
        self.bytes.extend([index as u8, kind]);
        if known.is_none() {
            self.names.push(name);
            self.bytes.extend((name.len() as u16).to_le_bytes());
            self.bytes.extend(name.as_bytes());
        }
        self
    }

    fn open(&mut self, name: &'static str, attributes: u16) -> &mut Self {
        self.record(name, 0x00);
        self.bytes.extend([0u8; 4]);
        self.bytes.extend(attributes.to_le_bytes());
        self
    }

    fn int(&mut self, name: &'static str, n: u64) -> &mut Self {
        self.record(name, 0x10);
        let width = if self.wide { 8 } else { 4 };
        self.bytes.extend(&n.to_le_bytes()[..width]);
        self
    }

    fn float(&mut self, name: &'static str, x: f32) -> &mut Self {
        self.record(name, 0x40);
        if self.wide {
            self.bytes.extend(f64::from(x).to_le_bytes());
        } else {
            self.bytes.extend(x.to_le_bytes());
        }
        self
    }

    fn flag(&mut self, name: &'static str, set: bool) -> &mut Self {
        self.record(name, if set { 0x20 } else { 0x30 })
    }

    fn string(&mut self, name: &'static str, s: &str) -> &mut Self {
        self.record(name, 0x60);
        self.bytes.extend((s.len() as u16).to_le_bytes());
        self.bytes.extend(s.as_bytes());
        self
    }

    fn text(&mut self, payload: &[u8]) -> &mut Self {
        self.bytes.extend([0xFD, 0xFF]);
        self.bytes.extend((payload.len() as u32).to_le_bytes());
        self.bytes.extend(payload);
        self
    }

    fn close(&mut self) -> &mut Self {
        self.bytes.extend([0xFF, 0xFF]);
        self
    }
}

/// A panel of two items: the first with an attribute, a text and a value after its child.
fn panel(wide: bool) -> Doc {
    let mut doc = Doc::new(wide);
    doc.open("Panel", 0);
    doc.open("Item", 2).string("Id", "first").int("Order", 7);
    doc.open("Label", 0).text(b"Bonjour\0").close();
    doc.float("Width", 1.5).flag("Shown", true).close();
    doc.open("Item", 0).flag("Shown", false).close();
    doc.close();
    doc
}

fn element(name: &str) -> Element {
    Element {
        name: name.into(),
        ..Element::default()
    }
}

#[test]
fn a_document_reads_as_elements_with_attributes_and_ordered_content() {
    for wide in [true, false] {
        let tree = read(&panel(wide).bytes).expect("read");
        assert_eq!(tree.wide, wide);
        let label = Element {
            content: vec![Node::Text(Text::Utf8("Bonjour".into()))],
            ..element("Label")
        };
        let first = Element {
            attributes: vec![
                ("Id".into(), Value::String("first".into())),
                ("Order".into(), Value::Integer(7)),
            ],
            content: vec![
                Node::Element(label),
                Node::Value("Width".into(), Value::Float(1.5)),
                Node::Value("Shown".into(), Value::Bool(true)),
            ],
            ..element("Item")
        };
        let second = Element {
            content: vec![Node::Value("Shown".into(), Value::Bool(false))],
            ..element("Item")
        };
        let want = Element {
            content: vec![Node::Element(first), Node::Element(second)],
            ..element("Panel")
        };
        assert_eq!(tree.root, want);
        let counts = Counts {
            elements: 4,
            attributes: 2,
            values: 3,
            texts: 1,
            depth: 3,
        };
        assert_eq!(tree.counts(), counts);
        let item = tree.root.children().next().expect("an item");
        let label = item.children().next().expect("a label");
        assert_eq!(label.texts().collect::<Vec<_>>(), ["Bonjour"]);
    }
}

#[test]
fn a_wide_integer_is_eight_bytes_and_a_narrow_one_four() {
    let big = 0x1_0000_0002u64;
    let value = |wide: bool| {
        let mut doc = Doc::new(wide);
        doc.open("Root", 0).int("N", big).close();
        match read(&doc.bytes).expect("read").root.content.remove(0) {
            Node::Value(_, value) => value,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(value(true), Value::Integer(big));
    assert_eq!(value(false), Value::Integer(2));
}

#[test]
fn a_name_is_reused_with_another_type() {
    let mut doc = Doc::new(true);
    doc.open("Root", 0).int("Value", 3).string("Value", "three");
    doc.open("Value", 0).close().close();
    let root = read(&doc.bytes).expect("read").root;
    let kinds: Vec<&str> = (root.content.iter())
        .map(|node| match node {
            Node::Value(name, value) => {
                assert_eq!(name, "Value");
                value.kind()
            }
            Node::Element(e) => {
                assert_eq!(e.name, "Value");
                "element"
            }
            Node::Text(_) => "text",
        })
        .collect();
    assert_eq!(kinds, ["integer", "string", "element"]);
}

#[test]
fn three_floats_read_narrow_and_are_refused_wide() {
    let floats = |wide: bool| {
        let mut doc = Doc::new(wide);
        doc.open("Root", 0).record("At", 0x50);
        for x in [1.0f32, -2.0, 0.5] {
            doc.bytes.extend(x.to_le_bytes());
        }
        doc.close();
        read(&doc.bytes)
    };
    let root = floats(false).expect("read").root;
    let want = Node::Value("At".into(), Value::Floats([1.0, -2.0, 0.5]));
    assert_eq!(root.content, [want]);
    assert!(matches!(floats(true), Err(UiTreeError::WideFloats { .. })));
}

#[test]
fn a_text_that_is_not_utf8_before_one_nul_is_kept_as_its_bytes() {
    let payloads: [&[u8]; 4] = [b"no end", b"two\0ends\0", b"\xFF\xFE\0", b""];
    for payload in payloads {
        let mut doc = Doc::new(true);
        doc.open("Root", 0).text(payload).close();
        let root = read(&doc.bytes).expect("read").root;
        assert_eq!(root.content, [Node::Text(Text::Bytes(payload.to_vec()))]);
        assert_eq!(root.texts().count(), 0);
    }
}

#[test]
fn a_record_that_leaves_the_rules_is_refused() {
    use UiTreeError as E;
    let intact = panel(true).bytes;
    assert!(read(&intact).is_ok());

    assert_eq!(read(b"RBM0\0\0\0\0"), Err(E::NotRbf));
    let mut marker = intact.clone();
    marker[6] = 2;
    assert_eq!(read(&marker), Err(E::Marker([2, 0])));

    let mut trailing = intact.clone();
    trailing.push(0);
    assert!(matches!(read(&trailing), Err(E::Trailing { left: 1, .. })));
    let cut = &intact[..intact.len() - 2];
    assert!(matches!(read(cut), Err(E::Truncated { .. })));

    // The root's header is the record at 8: two bytes, a name's length, the name, then its words.
    let words = 8 + 2 + 2 + "Panel".len();
    let mut nonzero = intact.clone();
    nonzero[words + 2] = 1;
    let refused = read(&nonzero);
    assert_eq!(
        refused,
        Err(E::ElementWords {
            at: 8,
            words: [0, 1]
        })
    );

    let mut doc = Doc::new(true);
    doc.open("Root", 0);
    doc.bytes.extend([5, 0x10]);
    assert!(matches!(
        read(&doc.bytes),
        Err(E::Descriptor {
            index: 5,
            named: 1,
            ..
        })
    ));

    let mut doc = Doc::new(true);
    doc.open("Root", 0).record("Odd", 0x70).close();
    assert!(matches!(
        read(&doc.bytes),
        Err(E::RecordType { kind: 0x70, .. })
    ));

    // A value, a text and a close each need an open element.
    let mut value = Doc::new(true);
    value.int("N", 1);
    let mut text = Doc::new(true);
    text.text(b"x\0");
    let mut close = Doc::new(true);
    close.close();
    for doc in [value, text, close] {
        assert!(matches!(read(&doc.bytes), Err(E::Outside { .. })));
    }

    let mut doc = Doc::new(true);
    doc.open("Root", 0).record("Name", 0x60);
    doc.bytes.extend([2, 0, 0xC3, 0x28]);
    doc.close();
    assert!(matches!(read(&doc.bytes), Err(E::NotUtf8 { .. })));
}

#[test]
fn declared_attributes_lead_their_element() {
    use UiTreeError as E;
    // One value, then a child, a text or the close, where two attributes are declared.
    let mut child = Doc::new(true);
    child.open("Root", 2).int("A", 1).open("Child", 0);
    let mut text = Doc::new(true);
    text.open("Root", 2).int("A", 1).text(b"x\0");
    let mut close = Doc::new(true);
    close.open("Root", 2).int("A", 1).close();
    for doc in [child, text, close] {
        let refused = read(&doc.bytes);
        assert!(
            matches!(
                refused,
                Err(E::Attributes {
                    declared: 2,
                    found: 1,
                    ..
                })
            ),
            "{refused:?}"
        );
    }
}

#[test]
fn an_element_opens_no_deeper_than_the_bound() {
    let nested = |depth: usize| {
        let mut doc = Doc::new(true);
        for _ in 0..depth {
            doc.open("E", 0);
        }
        for _ in 0..depth {
            doc.close();
        }
        read(&doc.bytes)
    };
    assert_eq!(nested(MAX_DEPTH).expect("read").counts().depth, MAX_DEPTH);
    let refused = nested(MAX_DEPTH + 1);
    assert!(matches!(refused, Err(UiTreeError::Depth { .. })));
}
