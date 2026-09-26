use crate::{model::*, repo::hash};
use anyhow::{ensure, Context, Result};
use flate2::read::GzDecoder;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path},
};
pub struct Payload {
    pub bytes: Vec<u8>,
    pub record: OwnedFile,
}
pub struct Unpacked {
    pub files: BTreeMap<String, Payload>,
}
pub fn unpack(bytes: &[u8], expected: &Manifest) -> Result<Unpacked> {
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let mut found = None;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let s = entry.path()?.to_str().context("non UTF-8 path")?.to_owned();
        path(&s)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        ensure!(
            entry.size() <= 512 * 1024 * 1024,
            "individual file too large"
        );
        total += entry.size();
        ensure!(
            total <= 2 * 1024 * 1024 * 1024 && files.len() < 200_000,
            "archive size limit exceeded"
        );
        if s == "manifest.json" {
            ensure!(
                kind.is_file() && found.is_none() && entry.size() < 1024 * 1024,
                "invalid manifest entry"
            );
            let mut b = vec![];
            entry.read_to_end(&mut b)?;
            found = Some(serde_json::from_slice::<Manifest>(&b)?);
            continue;
        }
        let rel = s.strip_prefix("files/").context("unknown archive entry")?;
        payload_path(rel)?;
        ensure!(!files.contains_key(rel), "duplicate archive path");
        let mode = entry.header().mode()?;
        ensure!(mode & !0o777 == 0, "special permission bits not supported");
        let (bytes, link) = if kind.is_file() {
            let mut b = vec![];
            entry.read_to_end(&mut b)?;
            (b, None)
        } else {
            ensure!(
                kind.is_symlink(),
                "hardlinks/devices/FIFOs are not supported"
            );
            let target = entry
                .link_name()?
                .context("missing symlink target")?
                .to_str()
                .context("non UTF8 symlink")?
                .to_owned();
            ensure!(
                !target.is_empty() && !Path::new(&target).is_absolute(),
                "absolute symlink not supported"
            );
            let mut depth = Path::new(rel).parent().unwrap().components().count() as i32;
            for c in Path::new(&target).components() {
                match c {
                    Component::ParentDir => depth -= 1,
                    Component::Normal(_) => depth += 1,
                    Component::CurDir => (),
                    _ => anyhow::bail!("invalid symlink"),
                };
                ensure!(depth >= 0, "symlink escapes root");
            }
            (target.as_bytes().to_vec(), Some(target))
        };
        let record = OwnedFile {
            sha256: hash(&bytes),
            mode,
            link,
        };
        files.insert(rel.to_owned(), Payload { bytes, record });
    }
    ensure!(
        found.as_ref() == Some(expected),
        "manifest differs from signed index"
    );
    for key in files.keys() {
        let mut p = Path::new(key).parent();
        while let Some(parent) = p {
            ensure!(
                !files.contains_key(parent.to_str().unwrap_or("")),
                "archive path nested below a file/symlink"
            );
            p = parent.parent();
        }
    }
    Ok(Unpacked { files })
}
