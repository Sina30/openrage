use super::*;

fn item(file: &str, kind: &str) -> String {
    format!("<Item>\n <filename>{file}</filename>\n <fileType>{kind}</fileType>\n</Item>\n")
}

#[test]
fn a_change_set_registers_a_language_per_text_archive_in_its_order() {
    let xml = format!(
        "<Set>\n<id>made_up</id>\n<fileList>\n{}{}{}{}</fileList>\n</Set>",
        item("$(target)/data/lang/northern_final.rpf", TEXT_FILE_TYPE),
        item("$(target)/data/other/thing.rpf", "RPF_FILE"),
        item("lang\\Eastern.rpf", TEXT_FILE_TYPE),
        item("$(target)/data/lang/southern_final.rpf", TEXT_FILE_TYPE),
    );
    assert_eq!(registered(&xml), ["northern", "eastern", "southern"]);
    assert!(registered("<Set><fileList/></Set>").is_empty());
    // An item cut short states no type.
    assert!(registered("<Item><filename>lang/a_final.rpf</filename>").is_empty());
}

#[test]
fn a_raw_deflate_body_counts_by_its_content() {
    let find = |content| Rpf9ScanFind {
        offset: 0,
        kind: Rpf9BodyKind::RawDeflate { content },
        span: 0,
        stored_len: None,
        inflated_len: None,
        container: None,
    };
    assert_eq!(Scanned::of(&find(Rpf9Content::Pso)), Scanned::Beside);
    assert_eq!(
        Scanned::of(&find(Rpf9Content::TextTable)),
        Scanned::TextTable
    );
    assert_eq!(Scanned::of(&find(Rpf9Content::Rbf)), Scanned::Other);
}

#[test]
fn the_archives_of_tables_alone_are_the_languages_in_offset_order() {
    use Scanned::{Archive, Beside, Other, TextTable};
    let finds = [
        (0x0, Archive),
        (0x100, Other),
        // Two language archives, the second with its metadata and names beside its tables.
        (0x1000, Archive),
        (0x1100, TextTable),
        (0x1200, TextTable),
        (0x2000, Archive),
        (0x2100, Beside),
        (0x2200, TextTable),
        (0x2300, Beside),
        // An archive of a table and another body, and one of no table.
        (0x3000, Archive),
        (0x3100, TextTable),
        (0x3200, Other),
        (0x4000, Archive),
        (0x4100, Beside),
        (0x5000, Archive),
        (0x5100, TextTable),
    ];
    let archives = LanguageArchives::of(finds);
    assert_eq!(archives.len(), 3);
    let ordinals = [
        0x100,
        0x1000,
        0x1200,
        0x1FFF,
        0x2200,
        0x3100,
        0x4100,
        0x5100,
        u64::MAX - 1,
    ];
    let got: Vec<Option<usize>> = ordinals.iter().map(|at| archives.ordinal(*at)).collect();
    let want = [
        None,
        Some(0),
        Some(0),
        Some(0),
        Some(1),
        None,
        None,
        Some(2),
        Some(2),
    ];
    assert_eq!(got, want);

    let three: Vec<String> = ["northern", "eastern", "southern"]
        .map(String::from)
        .to_vec();
    assert_eq!(archives.language(0x2200, &three), Some("eastern"));
    assert_eq!(archives.language(0x5100, &three), Some("southern"));
    assert_eq!(archives.language(0x3100, &three), None);
    // A file that holds another count of language archives than are registered names none.
    assert_eq!(archives.language(0x2200, &three[..2]), None);
    assert!(LanguageArchives::of([(0x10, TextTable)]).is_empty());
}
