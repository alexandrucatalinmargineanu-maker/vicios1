use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

pub const ABI: &str = "x86_64-vicios-gnu";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub name: String,
    pub version: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub abi: String,
    #[serde(default)]
    pub depends: Vec<Dependency>,
    #[serde(default)]
    pub essential: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub manifest: Manifest,
    pub file: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub format: u32,
    pub serial: u64,
    pub expires: u64,
    pub abi: String,
    pub packages: Vec<Package>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub index_url: String,
    pub public_key: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OwnedFile {
    pub sha256: String,
    pub mode: u32,
    pub link: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub files: BTreeMap<String, OwnedFile>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Database {
    pub packages: BTreeMap<String, Installed>,
    #[serde(default)]
    pub repo_serial: u64,
}
pub fn name(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-+._".contains(&b))
            && s != "."
            && s != "..",
        "invalid package name: {s}"
    );
    Ok(())
}
pub fn path(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty() && s.len() < 4096 && !s.contains('\\') && !s.contains('\0'),
        "invalid path"
    );
    ensure!(
        s.split('/').all(|p| !p.is_empty() && p != "." && p != ".."),
        "non-canonical path: {s}"
    );
    ensure!(
        Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "unsafe path: {s}"
    );
    Ok(())
}
pub fn payload_path(s: &str) -> Result<()> {
    path(s)?;
    ensure!(
        ["usr/", "etc/", "boot/", "opt/"]
            .iter()
            .any(|p| s.starts_with(p)),
        "payload must live in usr, etc, boot or opt: {s}"
    );
    ensure!(
        !s.starts_with("etc/vos/") && s != "etc/vos",
        "package cannot replace VOS trust configuration"
    );
    Ok(())
}
pub fn manifest(m: &Manifest) -> Result<()> {
    name(&m.name)?;
    semver::Version::parse(&m.version)?;
    ensure!(
        m.abi == ABI || m.abi == "any",
        "incompatible ABI: {}",
        m.abi
    );
    for d in &m.depends {
        name(&d.name)?;
        semver::VersionReq::parse(&d.version)?;
    }
    Ok(())
}
