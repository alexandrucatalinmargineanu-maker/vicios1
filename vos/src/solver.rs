//! One signed repository snapshot, one version per package. Cycles are allowed:
//! files are staged together and there are no per-package install scripts.
use crate::model::*;
use anyhow::{bail, ensure, Context, Result};
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub fn plan(names: &[String], index: &Index, db: &Database) -> Result<Vec<Package>> {
    let candidates: BTreeMap<&str, &Package> = index
        .packages
        .iter()
        .map(|p| (p.manifest.name.as_str(), p))
        .collect();
    let mut queue: VecDeque<String> = names.iter().cloned().collect();
    let mut selected = BTreeMap::<String, Package>::new();
    while let Some(n) = queue.pop_front() {
        if selected.contains_key(&n) {
            continue;
        }
        let p = candidates
            .get(n.as_str())
            .with_context(|| format!("package or dependency not in ViciOS repository: {n}"))?;
        if let Some(old) = db.packages.get(&n) {
            ensure!(
                Version::parse(&p.manifest.version)? >= Version::parse(&old.manifest.version)?,
                "downgrade refused: {n}"
            );
        }
        selected.insert(n.clone(), (*p).clone());
        for dep in &p.manifest.depends {
            let req = VersionReq::parse(&dep.version)?;
            let installed_ok = db.packages.get(&dep.name).is_some_and(|x| {
                Version::parse(&x.manifest.version).is_ok_and(|v| req.matches(&v))
            });
            if !installed_ok {
                queue.push_back(dep.name.clone());
            }
        }
    }
    let mut final_versions: BTreeMap<String, Manifest> = db
        .packages
        .iter()
        .map(|(n, p)| (n.clone(), p.manifest.clone()))
        .collect();
    final_versions.extend(
        selected
            .iter()
            .map(|(n, p)| (n.clone(), p.manifest.clone())),
    );
    validate(&final_versions)?;
    Ok(selected
        .into_values()
        .filter(|p| {
            db.packages
                .get(&p.manifest.name)
                .is_none_or(|i| i.manifest != p.manifest)
        })
        .collect())
}
pub fn validate(packages: &BTreeMap<String, Manifest>) -> Result<()> {
    for (n, p) in packages {
        for dep in &p.depends {
            let found = packages
                .get(&dep.name)
                .with_context(|| format!("{n} requires missing {}", dep.name))?;
            ensure!(
                VersionReq::parse(&dep.version)?.matches(&Version::parse(&found.version)?),
                "{n} requires {} {}, available {}",
                dep.name,
                dep.version,
                found.version
            );
        }
    }
    Ok(())
}
pub fn remove(names: &[String], db: &Database) -> Result<()> {
    let removing: BTreeSet<_> = names.iter().collect();
    for name in names {
        let p = db
            .packages
            .get(name)
            .with_context(|| format!("not installed: {name}"))?;
        if p.manifest.essential {
            bail!("essential package cannot be removed: {name}");
        }
    }
    let remaining = db
        .packages
        .iter()
        .filter(|(n, _)| !removing.contains(n))
        .map(|(n, p)| (n.clone(), p.manifest.clone()))
        .collect();
    validate(&remaining)
}
