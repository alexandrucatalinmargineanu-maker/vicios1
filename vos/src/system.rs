use crate::model::{self, Database};
use anyhow::{bail, ensure, Context as _, Result};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
};
pub struct Context {
    pub root: PathBuf,
}
impl Context {
    pub fn new(root: PathBuf) -> Result<Self> {
        ensure!(root.is_absolute(), "--root must be absolute");
        let resolved = root
            .canonicalize()
            .context("target root must already exist")?;
        Ok(Self { root: resolved })
    }
    pub fn host(&self) -> bool {
        self.root == Path::new("/")
    }
    pub fn state(&self) -> PathBuf {
        self.root.join("var/lib/vos")
    }
    pub fn writable(&self) -> Result<File> {
        ensure!(
            !self.host() || unsafe { libc::geteuid() } == 0,
            "sudo required"
        );
        self.safe("var/lib/vos/lock")?;
        fs::create_dir_all(self.state())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.state().join("lock"))?;
        lock.try_lock_exclusive()
            .context("another VOS transaction is running")?;
        Ok(lock)
    }
    pub fn database(&self) -> Result<Database> {
        let path = self.safe("var/lib/vos/installed.json")?;
        let db: Database = if path.exists() {
            serde_json::from_slice(&fs::read(path)?)?
        } else {
            Database::default()
        };
        for (n, p) in &db.packages {
            model::manifest(&p.manifest)?;
            ensure!(n == &p.manifest.name, "invalid DB key");
            for f in p.files.keys() {
                model::payload_path(f)?;
            }
        }
        Ok(db)
    }
    pub fn safe(&self, rel: &str) -> Result<PathBuf> {
        model::path(rel)?;
        let dest = self.root.join(rel);
        let mut cur = self.root.clone();
        let parts: Vec<_> = Path::new(rel).components().collect();
        for c in &parts[..parts.len() - 1] {
            cur.push(c);
            if let Ok(m) = fs::symlink_metadata(&cur) {
                ensure!(
                    m.is_dir() && !m.file_type().is_symlink(),
                    "unsafe parent: {}",
                    cur.display()
                );
            }
        }
        Ok(dest)
    }
    pub fn live_only(&self) -> Result<()> {
        ensure!(self.host(), "host operation refused with --root/VOS_ROOT");
        Ok(())
    }
}
pub fn command(prog: &str, args: &[&str]) -> Result<()> {
    ensure!(
        Command::new(prog)
            .args(args)
            .status()
            .with_context(|| format!("missing tool: {prog}"))?
            .success(),
        "{prog} failed"
    );
    Ok(())
}
pub fn root_required() -> Result<()> {
    ensure!(unsafe { libc::geteuid() } == 0, "sudo required");
    Ok(())
}
pub fn version(ctx: &Context) -> Result<()> {
    let data = fs::read_to_string(ctx.root.join("etc/os-release"))?;
    let id = data
        .lines()
        .find_map(|l| l.strip_prefix("ID="))
        .unwrap_or("")
        .trim_matches('"');
    ensure!(id == "vicios", "target is not ViciOS");
    let v = data
        .lines()
        .find_map(|l| l.strip_prefix("VERSION_ID="))
        .context("VERSION_ID missing")?
        .trim_matches('"');
    println!("{v}");
    Ok(())
}
pub fn firewall(args: &[String]) -> Result<()> {
    let refs: Vec<_> = args.iter().map(String::as_str).collect();
    match refs.as_slice() {
        [] | ["status"] => {
            if Command::new("firewall-cmd")
                .arg("--state")
                .output()
                .is_ok_and(|x| x.status.success())
            {
                command("firewall-cmd", &["--get-active-zones"])?;
                command("firewall-cmd", &["--list-all-zones"])
            } else {
                command("nft", &["list", "ruleset"])
            }
        }
        ["allow", port] | ["remove", port] => {
            root_required()?;
            let (n, proto) = port.split_once('/').context("use PORT/tcp or PORT/udp")?;
            ensure!(
                n.parse::<u16>()? > 0 && ["tcp", "udp"].contains(&proto),
                "invalid port"
            );
            let action = if refs[0] == "allow" {
                "--add-port"
            } else {
                "--remove-port"
            };
            command(
                "firewall-cmd",
                &["--permanent", &format!("{action}={port}")],
            )?;
            command("firewall-cmd", &["--reload"])
        }
        _ => bail!("firewall [status | allow PORT/tcp | remove PORT/tcp]"),
    }
}
