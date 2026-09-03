//! Building a `.gem` in memory, so the tests need no tracked binary.
//!
//! What it writes is the real format: an uncompressed tar of `metadata.gz`,
//! `data.tar.gz` and `checksums.yaml.gz`, with the digests filled in. The
//! package reader is therefore tested against bytes it did not produce
//! itself.

use std::io::Write as _;

use sha2::{Digest as _, Sha256};

/// A serialized `Gem::Specification`, as `metadata.gz` holds it.
pub fn metadata_yaml(name: &str, version: &str, platform: &str) -> String {
    format!(
        "--- !ruby/object:Gem::Specification\n\
         name: {name}\n\
         version: !ruby/object:Gem::Version\n  \
           version: {version}\n\
         platform: {platform}\n\
         authors:\n- Someone\n\
         require_paths:\n- lib\n\
         extensions: []\n\
         dependencies:\n\
         - !ruby/object:Gem::Dependency\n  \
           name: racc\n  \
           requirement: !ruby/object:Gem::Requirement\n    \
             requirements:\n    \
             - - \"~>\"\n      \
               - !ruby/object:Gem::Version\n        \
                 version: '1.4'\n  \
           type: :runtime\n\
         - !ruby/object:Gem::Dependency\n  \
           name: rspec\n  \
           requirement: !ruby/object:Gem::Requirement\n    \
             requirements:\n    \
             - - \">=\"\n      \
               - !ruby/object:Gem::Version\n        \
                 version: '0'\n  \
           type: :development\n\
         summary: a test gem\n"
    )
}

/// A `.gem`: `metadata` as its `metadata.gz`, `files` as its `data.tar.gz`.
pub fn gem_bytes(metadata: &str, files: &[(&str, &str)]) -> Vec<u8> {
    gem_from_data_tar(metadata, &tar_of(files))
}

/// A `.gem` holding ONE entry whose path is written straight into the tar
/// header. `tar::Builder` refuses to write a `..` path, so the header is
/// patched afterwards -- which is how a hostile gem would arrive anyway.
pub fn gem_with_raw_entry_path(metadata: &str, path: &str, body: &str) -> Vec<u8> {
    assert!(path.len() <= 100, "a tar name field holds 100 bytes");
    let mut tar = tar_of(&["x".repeat(path.len()).as_str()].map(|n| (n, body)));
    // The name is the first 100 bytes of the 512-byte header.
    tar[..path.len()].copy_from_slice(path.as_bytes());
    fix_checksum(&mut tar[..512]);
    gem_from_data_tar(metadata, &tar)
}

/// The header checksum is the sum of all 512 bytes with the checksum field
/// itself read as spaces.
fn fix_checksum(header: &mut [u8]) {
    header[148..156].fill(b' ');
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

fn gem_from_data_tar(metadata: &str, data_tar: &[u8]) -> Vec<u8> {
    let data = gzip(data_tar);
    let metadata = gzip(metadata.as_bytes());
    let checksums = gzip(
        format!(
            "---\nSHA256:\n  metadata.gz: {}\n  data.tar.gz: {}\n",
            hex(&metadata),
            hex(&data)
        )
        .as_bytes(),
    );
    tar_of_bytes(&[
        ("metadata.gz", &metadata),
        ("data.tar.gz", &data),
        ("checksums.yaml.gz", &checksums),
    ])
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("writing to a Vec");
    encoder.finish().expect("finishing a gzip stream")
}

fn tar_of(files: &[(&str, &str)]) -> Vec<u8> {
    let members: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(name, body)| (*name, body.as_bytes().to_vec()))
        .collect();
    let refs: Vec<(&str, &[u8])> = members.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    tar_of_bytes(&refs)
}

pub fn tar_of_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, body) in members {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *body)
            .expect("writing to a Vec");
    }
    builder.into_inner().expect("finishing a tar")
}
