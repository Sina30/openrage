//! The list of rules a person or a script is given is the one the code holds, and says each rule's limit and whether a reason sets it aside.

use or_wardrobe::rules::{export, export_text, read_export, Exported};
use or_wardrobe::RULES;

#[test]
fn the_list_written_out_reads_back_as_rules_row_for_row() {
    let written = export();
    let rows = read_export(&written).expect("what is exported reads back");
    let want: Vec<Exported> = RULES.iter().map(Exported::from).collect();
    assert_eq!(rows, want);
    assert!(
        rows.len() >= 30,
        "the list is the whole of RULES, not a part of it"
    );
}

#[test]
fn a_list_that_lacks_a_field_or_names_another_format_does_not_read() {
    let mut written = export();
    written["rules"][0]
        .as_object_mut()
        .unwrap()
        .remove("severity");
    assert!(read_export(&written).is_err());
    let mut other = export();
    other["format"] = "something-else".into();
    assert!(read_export(&other).is_err());
}

#[test]
fn a_rule_with_a_limit_says_where_it_stands_and_in_what() {
    let rows = read_export(&export()).unwrap();
    let size = rows.iter().find(|r| r.code == "texture-size").unwrap();
    assert_eq!(
        size.threshold
            .as_ref()
            .map(|t| (t.default, t.unit.as_str())),
        Some((2048, "pixels"))
    );
    let missing = rows.iter().find(|r| r.code == "model-missing").unwrap();
    assert!(missing.threshold.is_none() && !missing.waivable);

    let text = export_text();
    for rule in RULES {
        assert!(
            text.lines().any(|l| l.starts_with(rule.code)),
            "{} is a line of the text",
            rule.code
        );
    }
    let line = text
        .lines()
        .find(|l| l.starts_with("texture-size"))
        .unwrap();
    assert!(
        line.contains("2048 pixels") && line.contains("warning"),
        "{line}"
    );
}
