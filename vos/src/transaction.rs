//! Persist backups BEFORE mutation. On a failed transaction or next invocation,
//! replay the backup journal. State and payload share one recovery boundary.
use crate::{
    archive::{Payload, Unpacked},
    model::*,
    repo::hash,
    system::Context,
};
use anyhow::{ensure, Context as _, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
};
#[derive(Serialize, Deserialize)]
struct Backup {
    path: String,
    mode: u32,
    link: Option<String>,
    file: Option<String>,
    existed: bool,
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("invalid destination")?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    sync_dir(parent)
}
pub fn actual(path: &Path) -> Result<String> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() {
        Ok(hash(fs::read_link(path)?.as_os_str().as_encoded_bytes()))
    } else {
        ensure!(m.is_file(), "not a regular file");
        Ok(hash(&fs::read(path)?))
    }
}
pub fn recover(ctx: &Context) -> Result<()> {
    let journal = ctx.state().join("pending");
    if !journal.exists() {
        return Ok(());
    }
    let backups: Vec<Backup> = serde_json::from_slice(&fs::read(journal.join("journal.json"))?)?;
    for b in &backups {
        let dest = ctx.safe(&b.path)?;
        if b.existed {
            if let Some(target) = &b.link {
                if fs::symlink_metadata(&dest).is_ok() {
                    fs::remove_file(&dest)?;
                }
                symlink(target, &dest)?;
            } else {
                let f = b.file.as_deref().context("backup missing")?;
                crate::model::name(f)?;
                atomic(&dest, &fs::read(journal.join(f))?, b.mode)?;
            }
        } else if fs::symlink_metadata(&dest).is_ok() {
            fs::remove_file(&dest)?;
        }
    }
    fs::remove_dir_all(&journal)?;
    sync_dir(&ctx.state())?;
    eprintln!("Recovered interrupted VOS transaction");
    Ok(())
}
pub fn install(
    ctx: &Context,
    db: &Database,
    packages: Vec<(Package, Unpacked)>,
    serial: u64,
) -> Result<()> {
    let mut next = db.clone();
    next.repo_serial = serial;
    let mut writes = BTreeMap::<String, Payload>::new();
    let mut deletes = BTreeSet::new();
    for (p, unpacked) in packages {
        let name = &p.manifest.name;
        if let Some(old) = db.packages.get(name) {
            deletes.extend(old.files.keys().cloned());
        }
        let mut owned = BTreeMap::new();
        for (rel, payload) in unpacked.files {
            ensure!(
                !writes.contains_key(&rel),
                "file collision across new packages: {rel}"
            );
            for (other, installed) in &db.packages {
                ensure!(
                    other == name || !installed.files.contains_key(&rel),
                    "file owned by {other}: {rel}"
                );
            }
            if fs::symlink_metadata(ctx.safe(&rel)?).is_ok() {
                ensure!(
                    db.packages
                        .get(name)
                        .is_some_and(|p| p.files.contains_key(&rel)),
                    "unmanaged file collision: {rel}"
                );
            }
            owned.insert(rel.clone(), payload.record.clone());
            writes.insert(rel, payload);
        }
        next.packages.insert(
            name.clone(),
            Installed {
                manifest: p.manifest,
                files: owned,
            },
        );
    }
    for rel in writes.keys() {
        deletes.remove(rel);
    }
    apply(ctx, db, &next, writes, deletes)
}
pub fn remove(ctx: &Context, db: &Database, names: &[String]) -> Result<()> {
    let mut next = db.clone();
    let mut deletes = BTreeSet::new();
    for name in names {
        if let Some(p) = next.packages.remove(name) {
            deletes.extend(p.files.into_keys());
        }
    }
    apply(ctx, db, &next, BTreeMap::new(), deletes)
}
fn apply(
    ctx: &Context,
    old: &Database,
    next: &Database,
    writes: BTreeMap<String, Payload>,
    deletes: BTreeSet<String>,
) -> Result<()> {
    let paths: BTreeSet<String> = writes
        .keys()
        .cloned()
        .chain(deletes.iter().cloned())
        .collect();
    // Reject modified configuration files; preserve the user's changes intact.
    for p in &paths {
        let dest = ctx.safe(p)?;
        if let Ok(m) = fs::symlink_metadata(&dest) {
            ensure!(
                m.is_file() || m.file_type().is_symlink(),
                "cannot replace directory: {p}"
            );
        }
        if p.starts_with("etc/") {
            for pkg in old.packages.values() {
                if let Some(record) = pkg.files.get(p) {
                    ensure!(
                        actual(&dest).is_ok_and(|v| v == record.sha256),
                        "locally modified configuration: {p}; merge it before upgrading/removing"
                    );
                }
            }
        }
        let mut parent = Path::new(p).parent();
        while let Some(par) = parent {
            ensure!(
                !writes.contains_key(par.to_str().unwrap_or("")),
                "file used as directory"
            );
            parent = par.parent();
        }
    }
    let mut paths = paths;
    paths.insert("var/lib/vos/installed.json".into());
    let staged = tempfile::Builder::new()
        .prefix("backup-")
        .tempdir_in(ctx.state())?;
    let mut backups = vec![];
    for (i, p) in paths.iter().enumerate() {
        let dest = ctx.safe(p)?;
        let meta = fs::symlink_metadata(&dest).ok();
        let mut b = Backup {
            path: p.clone(),
            mode: 0o644,
            link: None,
            file: None,
            existed: meta.is_some(),
        };
        if let Some(m) = meta {
            b.mode = m.permissions().mode() & 0o777;
            if m.file_type().is_symlink() {
                b.link = Some(
                    fs::read_link(&dest)?
                        .to_str()
                        .context("non UTF8 link")?
                        .into(),
                );
            } else {
                ensure!(m.is_file(), "invalid target file");
                let name = i.to_string();
                let target = staged.path().join(&name);
                fs::copy(&dest, &target)?;
                File::open(target)?.sync_all()?;
                b.file = Some(name);
            }
        }
        backups.push(b);
    }
    atomic(
        &staged.path().join("journal.json"),
        &serde_json::to_vec(&backups)?,
        0o600,
    )?;
    sync_dir(staged.path())?;
    let pending = ctx.state().join("pending");
    fs::rename(staged.path(), &pending)?;
    sync_dir(&ctx.state())?;
    let result = (|| -> Result<()> {
        for (p, payload) in &writes {
            let dest = ctx.safe(p)?;
            fs::create_dir_all(dest.parent().unwrap())?;
            if let Some(link) = &payload.record.link {
                let tempdir = tempfile::tempdir_in(dest.parent().unwrap())?;
                let temp = tempdir.path().join("link");
                symlink(link, &temp)?;
                fs::rename(&temp, &dest)?;
                sync_dir(dest.parent().unwrap())?;
            } else {
                atomic(&dest, &payload.bytes, payload.record.mode)?;
            }
        }
        for p in &deletes {
            let dest = ctx.safe(p)?;
            if fs::symlink_metadata(&dest).is_ok() {
                fs::remove_file(&dest)?;
                sync_dir(dest.parent().unwrap())?;
            }
        }
        atomic(
            &ctx.state().join("installed.json"),
            &serde_json::to_vec_pretty(next)?,
            0o644,
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        recover(ctx).context("automatic rollback failed; pending journal retained")?;
        return Err(e);
    }
    let committed = ctx.state().join("committed");
    if committed.exists() {
        fs::remove_dir_all(&committed)?;
    }
    fs::rename(&pending, &committed)?;
    sync_dir(&ctx.state())?;
    fs::remove_dir_all(committed)?;
    Ok(())
}
