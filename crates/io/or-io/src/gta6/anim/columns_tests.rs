use super::*;
use crate::gta6::anim::metadata::Attribute;

fn hash(name: &str) -> u32 {
    name_hash32(name.as_bytes())
}

fn attribute(name: &str, value: AttributeValue) -> Attribute {
    Attribute {
        name: hash(name),
        value,
    }
}

/// An `interface` property whose `parameters` object holds a float, a Vector3, a Vector4 and an int.
fn properties() -> Vec<Record> {
    let parameters = vec![
        attribute("speed", AttributeValue::Float(1.0)),
        attribute("direction", AttributeValue::Vector3([0.0; 3])),
        attribute("blend", AttributeValue::Vector4([0.0; 4])),
        attribute("count", AttributeValue::Int(2)),
    ];
    let other = Record {
        name: hash("guid"),
        signature: 1,
        phases: None,
        attributes: vec![attribute("parameters", AttributeValue::Float(0.0))],
    };
    let interface = Record {
        name: hash("interface"),
        signature: 2,
        phases: None,
        attributes: vec![
            attribute("value", AttributeValue::Int(0)),
            attribute("parameters", AttributeValue::Object(parameters)),
        ],
    };
    vec![other, interface]
}

/// Where `name` falls among the parameters by name hash.
fn index(name: &str) -> u16 {
    let names = ["speed", "direction", "blend", "count"];
    let mut hashes: Vec<u32> = names.into_iter().map(hash).collect();
    hashes.sort();
    hashes
        .iter()
        .position(|h| *h == hash(name))
        .expect("listed") as u16
}

#[test]
fn a_vector_component_s_hash_continues_the_name_s_hash_by_a_dot_and_its_letter() {
    for (y, letter) in ["x", "y", "z", "w"].into_iter().enumerate() {
        assert_eq!(
            component_hash(hash("value"), y as u8),
            hash(&format!("value.{letter}"))
        );
    }
    assert_eq!(unfinalize(finalize_hash(0x1234_5678)), 0x1234_5678);
}

#[test]
fn a_column_names_a_parameter_by_name_hash_order_and_its_component() {
    let properties = properties();
    let speed = (hash("speed"), [index("speed"), 0]);
    assert_eq!(
        column_parameter(&properties, speed),
        Ok(ColumnParameter {
            parameter: hash("speed"),
            component: None,
        })
    );
    let z = (hash("direction.z"), [index("direction"), 2]);
    let read = column_parameter(&properties, z).expect("resolves");
    assert_eq!(read.component, Some(2));
    assert_eq!(
        read.label(|h| format!("{h:08X}")),
        format!("{:08X}.z", hash("direction"))
    );
    let w = (hash("blend.w"), [index("blend"), 3]);
    assert!(column_parameter(&properties, w).is_ok());
}

#[test]
fn a_column_off_its_parameter_component_or_hash_is_refused() {
    let properties = properties();
    let refused = [
        (hash("speed"), [index("speed"), 1]),
        (hash("direction.w"), [index("direction"), 3]),
        (hash("direction.x"), [index("direction"), 1]),
        (hash("speed"), [4, 0]),
        (hash("count"), [index("count"), 0]),
    ];
    for column in refused {
        assert!(column_parameter(&properties, column).is_err(), "{column:?}");
    }
    let no_interface = column_parameter(&properties[..1], (hash("speed"), [0, 0]));
    assert!(no_interface.is_err());
}

#[test]
fn a_database_whose_two_columns_name_one_component_is_refused() {
    let properties = properties();
    let column = (hash("speed"), [index("speed"), 0]);
    let database = |columns: Vec<Column>| {
        Typed::Rows(crate::gta6::anim::Database {
            keys: columns,
            tables: Vec::new(),
            grids: Vec::new(),
            lists: 0,
            bounds: [0.0; 8],
            hash: 0,
        })
    };
    assert_eq!(check(&properties, &database(vec![column])), Ok(()));
    assert!(check(&properties, &database(vec![column, column])).is_err());
    assert_eq!(check(&[], &Typed::Constants(Vec::new())), Ok(()));
}
