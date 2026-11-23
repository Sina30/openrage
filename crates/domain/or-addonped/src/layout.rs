//! Where the files of a ped land inside the resource.
//!
//! A file named for the ped (`my_ped.yft`, `my_ped.ytd`) streams beside the others, and any other file
//! of the ped (`head_000_r.ydd`) streams in a folder of the ped's name, which is where its component
//! files are found. A file whose place is another says so (`at`).

use crate::model::PedFile;

/// The folder FiveM streams from.
pub const STREAM: &str = "stream";

/// The last name of a path with `/`.
fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The name of a file without its extensions: what a file named for a ped starts with.
fn stem(path: &str) -> &str {
    let name = leaf(path);
    name.split_once('.').map_or(name, |(stem, _)| stem)
}

/// Where `file` of the ped called `ped` lands inside the resource, with `/`.
pub fn resource_path(ped: &str, file: &PedFile) -> String {
    if let Some(at) = &file.at {
        return at.clone();
    }
    let name = leaf(&file.path);
    if stem(&file.path).eq_ignore_ascii_case(ped) {
        format!("{STREAM}/{name}")
    } else {
        format!("{STREAM}/{ped}/{name}")
    }
}

/// Where the data file lands: at the resource's root, under its own name.
pub fn meta_path(source: &str) -> String {
    leaf(source).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, at: Option<&str>) -> PedFile {
        PedFile {
            path: path.into(),
            at: at.map(Into::into),
        }
    }

    #[test]
    fn a_file_named_for_the_ped_streams_beside_the_others_and_any_other_in_the_peds_folder() {
        assert_eq!(
            resource_path("my_ped", &file("src/my_ped.yft", None)),
            "stream/my_ped.yft"
        );
        assert_eq!(
            resource_path("my_ped", &file("My_Ped.ytd", None)),
            "stream/My_Ped.ytd"
        );
        assert_eq!(
            resource_path("my_ped", &file("parts/head_000_r.ydd", None)),
            "stream/my_ped/head_000_r.ydd"
        );
        assert_eq!(
            resource_path("my_ped", &file("my_ped_p.ydd", None)),
            "stream/my_ped/my_ped_p.ydd",
            "a name that only starts with the ped's is not the ped's"
        );
        assert_eq!(
            resource_path("my_ped", &file("a.ydd", Some("stream/elsewhere/a.ydd"))),
            "stream/elsewhere/a.ydd"
        );
        assert_eq!(meta_path("data/peds.meta"), "peds.meta");
    }
}
