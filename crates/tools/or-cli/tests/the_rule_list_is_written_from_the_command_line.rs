//! `openrage-cli wardrobe rules` writes the list of rules the code holds, whole, and says no to what it does not take.

use std::process::ExitCode;

fn wardrobe(args: &[&str]) -> ExitCode {
    let mut all = vec!["wardrobe".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    or_cli::run(&all)
}

#[test]
fn the_list_a_script_is_given_is_the_rules_row_for_row_and_a_file_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("rules.json");
    let text = dir.path().join("rules.txt");

    assert_eq!(
        wardrobe(&["rules", "--json", "--out", json.to_str().unwrap()]),
        ExitCode::SUCCESS
    );
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).expect("it is JSON");
    let rows = or_wardrobe::rules::read_export(&document).expect("it is the list of rules");
    let want: Vec<_> = or_wardrobe::RULES
        .iter()
        .map(or_wardrobe::rules::Exported::from)
        .collect();
    assert_eq!(rows, want);

    assert_eq!(
        wardrobe(&["rules", "--out", text.to_str().unwrap()]),
        ExitCode::SUCCESS
    );
    let lines = std::fs::read_to_string(&text).unwrap();
    for rule in or_wardrobe::RULES {
        assert!(
            lines.lines().any(|l| l.starts_with(rule.code)),
            "{}",
            rule.code
        );
    }

    assert_ne!(
        wardrobe(&["rules", "a-project.openrage-proj"]),
        ExitCode::SUCCESS
    );
    assert_ne!(wardrobe(&["rules", "--nonsense"]), ExitCode::SUCCESS);
}
