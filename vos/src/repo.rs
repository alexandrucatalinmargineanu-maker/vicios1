use crate::{model::*, system::Context};
use anyhow::{bail, ensure, Context as _, Result};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_INDEX: u64 = 16 * 1024 * 1024;
pub const MAX_PACKAGE: u64 = 1024 * 1024 * 1024;
pub fn hash(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
pub fn fetch(url: &str, limit: u64) -> Result<Vec<u8>> {
    let file = tempfile::NamedTempFile::new()?;
    if let Some(path) = url.strip_prefix("file://") {
        let mut f = fs::File::open(path)?.take(limit + 1);
        let mut b = vec![];
        f.read_to_end(&mut b)?;
        ensure!(b.len() as u64 <= limit, "download too large");
        return Ok(b);
    }
    ensure!(
        url.starts_with("https://") && !url.contains('\n'),
        "only HTTPS or local file:// repositories supported"
    );
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "300",
            "--max-filesize",
            &limit.to_string(),
            "--output",
        ])
        .arg(file.path())
        .arg(url)
        .status()
        .context("curl is required")?;
    ensure!(status.success(), "download failed: {url}");
    ensure!(
        file.as_file().metadata()?.len() <= limit,
        "download too large"
    );
    Ok(fs::read(file.path())?)
}
pub fn load(ctx: &Context) -> Result<(String, Index)> {
    let cfg: Config = serde_json::from_slice(
        &fs::read(ctx.root.join("etc/vos/repos.json")).context("missing etc/vos/repos.json")?,
    )?;
    let bytes = fetch(&cfg.index_url, MAX_INDEX)?;
    let signature = fetch(&format!("{}.sig", cfg.index_url), 1024)?;
    let key_bytes: [u8; 32] = hex::decode(cfg.public_key.trim())?
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    let key = VerifyingKey::from_bytes(&key_bytes)?;
    let sig = Signature::from_slice(&hex::decode(std::str::from_utf8(&signature)?.trim())?)?;
    key.verify_strict(&bytes, &sig)
        .context("repository signature invalid")?;
    let index: Index = serde_json::from_slice(&bytes)?;
    ensure!(
        index.format == 2 && index.abi == ABI,
        "unsupported index format or ABI"
    );
    ensure!(
        index.expires > SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        "repository index expired"
    );
    ensure!(
        index.serial >= ctx.database()?.repo_serial,
        "repository rollback detected"
    );
    let mut seen = BTreeSet::new();
    for p in &index.packages {
        manifest(&p.manifest)?;
        ensure!(
            seen.insert(p.manifest.name.clone()),
            "multiple candidates per name are not supported"
        );
        name(&p.file)?;
        ensure!(p.file.ends_with(".vpk"), "invalid archive filename");
        ensure!(
            p.sha256.len() == 64 && hex::decode(&p.sha256).is_ok(),
            "bad SHA256"
        );
        ensure!(p.size > 0 && p.size <= MAX_PACKAGE, "package too large");
    }
    let base = cfg
        .index_url
        .rsplit_once('/')
        .context("invalid index URL")?
        .0
        .to_owned();
    Ok((base, index))
}
pub fn package(base: &str, p: &Package) -> Result<Vec<u8>> {
    let data = fetch(&format!("{base}/packages/{}", p.file), p.size)?;
    if data.len() as u64 != p.size || hash(&data) != p.sha256 {
        bail!("package integrity check failed: {}", p.manifest.name);
    }
    Ok(data)
}
