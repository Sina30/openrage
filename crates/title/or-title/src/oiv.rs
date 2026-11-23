//! An `.oiv` package: the zip OpenIV installs, with an `assembly.xml` that says what goes where.
//!
//! ⚠ **Written from the 2.2 shape of the package as the author knows it, and read back by this
//! module alone.** No OpenIV was available to install what it writes, so the one claim here is
//! that `Writer` and `Reader` agree on the zip and `Package::to_xml` and `Package::parse` on the
//! manifest. It authors one command, add, which is all a build's plan holds; it reads no package
//! it did not write, and refuses a manifest version other than 2.2 by name.

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::dlc::xml::{decode_attr, escape_xml, local_name, resolve_general_ref};

/// The manifest, at the root of the zip.
pub const ASSEMBLY: &str = "assembly.xml";
/// The folder of the zip that the files an add names are under.
pub const CONTENT: &str = "content";
/// The only manifest version this module writes and reads.
pub const MANIFEST_VERSION: &str = "2.2";
/// The title a package says it installs into.
pub const TARGET: &str = "Five";

#[derive(Debug, thiserror::Error)]
pub enum OivError {
    #[error("the package's zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the manifest: {0}")]
    Manifest(String),
}

/// What a package says about itself and the files it adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub major: u16,
    pub minor: u16,
    pub author: String,
    pub description: String,
    /// In the order they are written, which is the order of the paths they come from.
    pub adds: Vec<Add>,
}

/// One file the package puts in the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Add {
    /// The archives it goes into, outermost first, each as a path inside the one around it.
    pub archives: Vec<String>,
    /// Where the file is in the package, under [`CONTENT`].
    pub source: String,
    /// Where it lands: inside the innermost archive, or in the game folder when there is none.
    pub destination: String,
}

impl Add {
    /// The add of a file at `path` in the game's tree: an archive for each name on the way that ends in `.rpf`.
    ///
    /// ⚠ The last name is the file and never an archive of the chain, so a `dlc.rpf` is added whole.
    pub fn of(path: &str) -> Self {
        let names: Vec<&str> = path.split('/').collect();
        let (file, folders) = names.split_last().expect("a split has a first part");
        let mut archives: Vec<String> = Vec::new();
        let mut from = 0;
        for (n, name) in folders.iter().enumerate() {
            if name.to_ascii_lowercase().ends_with(".rpf") {
                archives.push(folders[from..=n].join("/"));
                from = n + 1;
            }
        }
        let rest = folders[from..].iter().chain(std::iter::once(file));
        Self {
            archives,
            source: path.to_string(),
            destination: rest.copied().collect::<Vec<_>>().join("/"),
        }
    }
}

impl Package {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            major: 1,
            minor: 0,
            author: String::new(),
            description: String::new(),
            adds: Vec::new(),
        }
    }

    /// The manifest, `assembly.xml`.
    pub fn to_xml(&self) -> String {
        let mut tree = Archive::default();
        for add in &self.adds {
            let mut at = &mut tree;
            for archive in &add.archives {
                at = at.inside.entry(archive.clone()).or_default();
            }
            at.adds.push(add);
        }
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<package version=\"{MANIFEST_VERSION}\" target=\"{TARGET}\">\n"
        ));
        out.push_str("  <metadata>\n");
        out.push_str(&format!("    <name>{}</name>\n", escape_xml(&self.name)));
        out.push_str("    <version>\n");
        out.push_str(&format!("      <major>{}</major>\n", self.major));
        out.push_str(&format!("      <minor>{}</minor>\n", self.minor));
        out.push_str("    </version>\n");
        out.push_str("    <author>\n");
        out.push_str(&format!(
            "      <displayName>{}</displayName>\n",
            escape_xml(&self.author)
        ));
        out.push_str("    </author>\n");
        out.push_str(&format!(
            "    <description>{}</description>\n",
            escape_xml(&self.description)
        ));
        out.push_str("  </metadata>\n  <content>\n");
        tree.write(&mut out, 2);
        out.push_str("  </content>\n</package>\n");
        out
    }

    /// The package a manifest describes; the version other than 2.2 is refused by name.
    pub fn parse(xml: &[u8]) -> Result<Self, OivError> {
        let fail = |why: String| OivError::Manifest(why);
        let mut reader = XmlReader::from_reader(xml);
        let mut buf = Vec::new();
        let mut path: Vec<String> = Vec::new();
        let mut text = String::new();
        let mut package = Self::new("");
        let mut archives: Vec<String> = Vec::new();
        let mut source: Option<String> = None;
        let mut version_seen = false;
        loop {
            match reader
                .read_event_into(&mut buf)
                .map_err(|e| fail(e.to_string()))?
            {
                Event::Start(e) => {
                    let name = local_name(e.name());
                    let attr = |key: &str| {
                        e.attributes().flatten().find_map(|a| {
                            (a.key.as_ref() == key.as_bytes()).then(|| decode_attr(&a.value))
                        })
                    };
                    match name.as_str() {
                        "package" => {
                            let version = attr("version").unwrap_or_default();
                            if version != MANIFEST_VERSION {
                                return Err(fail(format!(
                                    "version {version:?} is not one this reads: only {MANIFEST_VERSION}"
                                )));
                            }
                            version_seen = true;
                        }
                        "archive" => archives.push(
                            attr("path")
                                .ok_or_else(|| fail("an archive names no path".to_string()))?,
                        ),
                        "add" => {
                            source = Some(
                                attr("source")
                                    .ok_or_else(|| fail("an add names no source".to_string()))?,
                            )
                        }
                        _ => {}
                    }
                    path.push(name);
                    text.clear();
                }
                Event::Text(t) => text.push_str(&String::from_utf8_lossy(t.as_ref())),
                // CDATA is literal by definition, and the usual spelling of a description.
                Event::CData(t) => text.push_str(&String::from_utf8_lossy(t.as_ref())),
                Event::GeneralRef(r) => text.push_str(&resolve_general_ref(&r)),
                Event::End(_) => {
                    let name = path.pop().unwrap_or_default();
                    let under = path.last().map(String::as_str).unwrap_or_default();
                    match (under, name.as_str()) {
                        ("metadata", "name") => package.name = std::mem::take(&mut text),
                        ("metadata", "description") => {
                            package.description = std::mem::take(&mut text)
                        }
                        ("author", "displayName") => package.author = std::mem::take(&mut text),
                        ("version", "major") => package.major = number(&text, &fail)?,
                        ("version", "minor") => package.minor = number(&text, &fail)?,
                        (_, "archive") => {
                            archives.pop();
                        }
                        (_, "add") => {
                            let source = source
                                .take()
                                .ok_or_else(|| fail("an add names no source".to_string()))?;
                            package.adds.push(Add {
                                archives: archives.clone(),
                                source,
                                destination: std::mem::take(&mut text),
                            });
                        }
                        _ => {}
                    }
                    text.clear();
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
        if !version_seen {
            return Err(fail("there is no package element".to_string()));
        }
        Ok(package)
    }
}

fn number(text: &str, fail: &impl Fn(String) -> OivError) -> Result<u16, OivError> {
    text.trim()
        .parse()
        .map_err(|_| fail(format!("{text:?} is not a version number")))
}

/// The adds that land in one archive, and the archives inside it.
#[derive(Default)]
struct Archive<'a> {
    adds: Vec<&'a Add>,
    inside: BTreeMap<String, Archive<'a>>,
}

impl Archive<'_> {
    fn write(&self, out: &mut String, depth: usize) {
        let pad = "  ".repeat(depth);
        for add in &self.adds {
            out.push_str(&format!(
                "{pad}<add source=\"{}\">{}</add>\n",
                escape_xml(&add.source),
                escape_xml(&add.destination)
            ));
        }
        for (path, archive) in &self.inside {
            out.push_str(&format!(
                "{pad}<archive path=\"{}\" createIfNotExist=\"True\" type=\"RPF7\">\n",
                escape_xml(path)
            ));
            archive.write(out, depth + 1);
            out.push_str(&format!("{pad}</archive>\n"));
        }
    }
}

/// Writes a package's zip: the manifest first, then each file under [`CONTENT`].
///
/// ⚠ Stored and not deflated, with the time the zip format starts at, so two packages of one
/// plan are byte-identical on any machine and any compressor; a resource is deflated inside
/// itself, so little is lost.
pub struct Writer<W: Write + Seek> {
    zip: ZipWriter<W>,
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
}

impl<W: Write + Seek> Writer<W> {
    /// Start a package whose manifest is the text `manifest`, which [`Package::to_xml`] writes.
    pub fn new(out: W, manifest: &str) -> Result<Self, OivError> {
        let mut zip = ZipWriter::new(out);
        zip.start_file(ASSEMBLY, options())?;
        zip.write_all(manifest.as_bytes())?;
        Ok(Self { zip })
    }

    /// Add the `size` bytes of `body` as the file `source` under [`CONTENT`]; how many were written.
    ///
    /// ⚠ `size` decides only whether the entry is written in the zip64 form, which a file of 4 GiB
    /// or more needs, so a body that is longer than it says is refused by the zip.
    pub fn add(&mut self, source: &str, size: u64, body: &mut impl Read) -> Result<u64, OivError> {
        let large = size >= u64::from(u32::MAX);
        self.zip
            .start_file(format!("{CONTENT}/{source}"), options().large_file(large))?;
        Ok(std::io::copy(body, &mut self.zip)?)
    }

    pub fn finish(self) -> Result<W, OivError> {
        Ok(self.zip.finish()?)
    }
}

/// Reads a package's zip back: its manifest and each file it holds.
pub struct PackageReader<R: Read + Seek> {
    zip: ZipArchive<R>,
    /// The manifest as the zip holds it, byte for byte.
    pub manifest: Vec<u8>,
    pub package: Package,
}

impl<R: Read + Seek> PackageReader<R> {
    pub fn new(input: R) -> Result<Self, OivError> {
        let mut zip = ZipArchive::new(input)?;
        let mut manifest = Vec::new();
        zip.by_name(ASSEMBLY)
            .map_err(|_| OivError::Manifest(format!("the zip holds no {ASSEMBLY}")))?
            .read_to_end(&mut manifest)?;
        let package = Package::parse(&manifest)?;
        Ok(Self {
            zip,
            manifest,
            package,
        })
    }

    /// The names of every entry of the zip, the manifest included.
    pub fn entries(&self) -> Vec<String> {
        self.zip.file_names().map(str::to_string).collect()
    }

    /// The bytes of the file `source` under [`CONTENT`].
    pub fn read(&mut self, source: &str) -> Result<Vec<u8>, OivError> {
        let mut body = Vec::new();
        self.zip
            .by_name(&format!("{CONTENT}/{source}"))?
            .read_to_end(&mut body)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn package() -> Package {
        let mut package = Package::new("My <Pack> & more");
        package.author = "Someone".into();
        package.description = "Two \"files\"".into();
        package.major = 2;
        package.minor = 7;
        // In the order a manifest lists them: the files of the game folder, then each archive in turn.
        package.adds = [
            "readme.txt",
            "update/update.rpf/x64/dlcpacks/mypack/dlc.rpf",
            "x64e.rpf/levels/gta5/a.ytd",
        ]
        .iter()
        .map(|path| Add::of(path))
        .collect();
        package
    }

    #[test]
    fn a_path_through_an_archives_name_is_added_inside_that_archive() {
        let flat = Add::of("scripts/readme.txt");
        assert_eq!(flat.archives, Vec::<String>::new());
        assert_eq!(flat.destination, "scripts/readme.txt");

        let inside = Add::of("update/update.rpf/x64/dlc.rpf/inner/a.ytd");
        assert_eq!(inside.archives, ["update/update.rpf", "x64/dlc.rpf"]);
        assert_eq!(inside.destination, "inner/a.ytd");
        assert_eq!(inside.source, "update/update.rpf/x64/dlc.rpf/inner/a.ytd");

        let whole = Add::of("update/x64/dlcpacks/mypack/dlc.rpf");
        assert_eq!(
            whole.archives,
            Vec::<String>::new(),
            "an archive added whole is a file"
        );
        assert_eq!(whole.destination, "update/x64/dlcpacks/mypack/dlc.rpf");
    }

    #[test]
    fn a_manifest_reads_back_as_the_package_that_wrote_it() {
        let xml = package().to_xml();
        assert!(
            xml.contains("<package version=\"2.2\" target=\"Five\">"),
            "{xml}"
        );
        assert!(xml.contains("My &lt;Pack&gt; &amp; more"), "{xml}");
        assert_eq!(Package::parse(xml.as_bytes()).unwrap(), package());
        assert_eq!(package().to_xml(), xml, "writing is deterministic");
    }

    #[test]
    fn a_manifest_of_another_version_is_refused_by_name() {
        let xml = package()
            .to_xml()
            .replace("version=\"2.2\"", "version=\"2.1\"");
        let why = Package::parse(xml.as_bytes()).unwrap_err().to_string();
        assert!(why.contains("2.1") && why.contains("2.2"), "{why}");
        assert!(Package::parse(b"<not-a-package/>").is_err());
    }

    #[test]
    fn a_zip_reads_back_with_its_manifest_and_each_file_and_is_byte_identical_twice() {
        let write = || {
            let mut writer = Writer::new(Cursor::new(Vec::new()), &package().to_xml()).unwrap();
            writer
                .add("readme.txt", 5, &mut Cursor::new(b"hello".to_vec()))
                .unwrap();
            writer
                .add(
                    "x64e.rpf/levels/gta5/a.ytd",
                    3,
                    &mut Cursor::new(vec![1, 2, 3]),
                )
                .unwrap();
            writer.finish().unwrap().into_inner()
        };
        let bytes = write();
        assert_eq!(bytes, write());

        let mut reader = PackageReader::new(Cursor::new(bytes)).unwrap();
        assert_eq!(reader.package, package());
        assert_eq!(
            reader.entries(),
            [
                ASSEMBLY,
                "content/readme.txt",
                "content/x64e.rpf/levels/gta5/a.ytd"
            ]
        );
        assert_eq!(reader.read("readme.txt").unwrap(), b"hello");
        assert_eq!(
            reader.read("x64e.rpf/levels/gta5/a.ytd").unwrap(),
            [1, 2, 3]
        );
        assert!(reader.read("missing.txt").is_err());
    }
}
