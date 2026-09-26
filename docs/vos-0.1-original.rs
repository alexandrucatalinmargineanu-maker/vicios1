use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashSet}, env, error::Error, fs::{self, File}, io::{self, Read, Write}, path::{Component, Path, PathBuf}, process::Command};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const OS_VERSION: &str = "0.1.0";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Package { name: String, version: String, description: String, file: String, sha256: String }
#[derive(Serialize, Deserialize)]
struct Index { packages: Vec<Package> }
#[derive(Default, Serialize, Deserialize)]
struct Database { packages: BTreeMap<String, Installed> }
#[derive(Clone, Serialize, Deserialize)]
struct Installed { version: String, files: Vec<String> }
#[derive(Serialize, Deserialize)]
struct Config { index_url: String }
#[derive(Deserialize)]
struct Manifest { name: String, version: String, description: String }

fn root() -> PathBuf { env::var_os("VOS_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/")) }
fn inside(path: &str) -> PathBuf { root().join(path) }
fn require_root() -> Result<()> {
    if root() == Path::new("/") && !Command::new("id").arg("-u").output()?.stdout.starts_with(b"0") {
        return Err("comanda necesită sudo".into());
    }
    Ok(())
}
fn db_path() -> PathBuf { inside("var/lib/vos/installed.json") }
fn load_db() -> Result<Database> {
    let p = db_path(); if !p.exists() { return Ok(Database::default()); }
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn save_db(db: &Database) -> Result<()> {
    let p = db_path(); fs::create_dir_all(p.parent().ok_or("cale DB invalidă")?)?;
    let tmp = p.with_extension("json.tmp"); fs::write(&tmp, serde_json::to_vec_pretty(db)?)?;
    fs::rename(tmp, p)?; Ok(())
}
fn config() -> Result<Config> {
    Ok(serde_json::from_slice(&fs::read(inside("etc/vos/repos.json"))
        .map_err(|_| "lipsește /etc/vos/repos.json; vezi README")?)?)
}
fn get(url: &str) -> Result<Vec<u8>> {
    if url.starts_with("file://") { return Ok(fs::read(url.trim_start_matches("file://"))?); }
    if !url.starts_with("https://") { return Err("repository-ul trebuie să folosească HTTPS sau file:// pentru teste".into()); }
    let out = Command::new("curl").args(["--fail", "--location", "--silent", "--show-error", "--max-time", "60", "--proto", "=https", "--proto-redir", "=https", url]).output()?;
    if !out.status.success() { return Err(format!("descărcare eșuată: {url}").into()); }
    Ok(out.stdout)
}
fn index() -> Result<(String, Index)> {
    let url = config()?.index_url;
    let data = get(&url)?;
    let idx: Index = serde_json::from_slice(&data)?;
    let mut names = HashSet::new();
    for p in &idx.packages {
        check_name(&p.name)?;
        if !names.insert(&p.name) || p.version.is_empty() || p.sha256.len() != 64 || !p.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("index invalid: nume duplicat, versiune sau SHA-256 invalid".into());
        }
        if p.file.contains('/') || p.file.contains('\\') || !p.file.ends_with(".vpk") { return Err("nume de pachet .vpk invalid".into()); }
    }
    let base = url.rsplit_once('/').ok_or("URL index invalid")?.0.to_string();
    Ok((base, idx))
}
fn check_name(s: &str) -> Result<()> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)) || s == "." || s == ".." {
        return Err(format!("nume invalid: {s}").into());
    }
    Ok(())
}
fn safe_path(s: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for c in s.components() {
        match c { Component::Normal(x) => parts.push(x.to_str().ok_or("cale non-UTF8")?.to_owned()), _ => return Err("cale periculoasă în arhivă".into()) }
    }
    if parts.is_empty() { return Err("cale goală".into()); }
    Ok(parts.join("/"))
}
fn archive(bytes: &[u8]) -> Result<(Manifest, Vec<(String, Vec<u8>, u32)>)> {
    let decoder = GzDecoder::new(bytes);
    let mut ar = tar::Archive::new(decoder);
    let mut manifest = None; let mut files = Vec::new(); let mut seen = HashSet::new();
    for item in ar.entries()? {
        let mut e = item?; let name = safe_path(&e.path()?)?;
        if !seen.insert(name.clone()) { return Err("cale duplicată în arhivă".into()); }
        if e.header().entry_type().is_dir() { continue; }
        if !e.header().entry_type().is_file() { return Err("arhiva conține link sau tip de fișier neacceptat".into()); }
        let mut data = Vec::new(); e.read_to_end(&mut data)?;
        if name == "manifest.json" { manifest = Some(serde_json::from_slice(&data)?); }
        else if let Some(rel) = name.strip_prefix("files/") {
            if rel.is_empty() { return Err("cale goală".into()); }
            let mode = e.header().mode()? & 0o777;
            files.push((safe_path(Path::new(rel))?, data, mode));
        } else { return Err(format!("intrare neașteptată: {name}").into()); }
    }
    Ok((manifest.ok_or("manifest.json lipsește")?, files))
}
fn install(p: &Package, base: &str, db: &mut Database) -> Result<()> {
    let bytes = get(&format!("{base}/packages/{}", p.file))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != p.sha256.to_lowercase() { return Err(format!("SHA-256 diferit pentru {}", p.name).into()); }
    let (m, files) = archive(&bytes)?;
    if m.name != p.name || m.version != p.version { return Err("manifestul nu corespunde indexului".into()); }
    let previous = db.packages.get(&p.name).cloned();
    for (rel, _, _) in &files {
        if db.packages.iter().any(|(name, pkg)| name != &p.name && pkg.files.contains(rel)) { return Err(format!("fișier deja deținut de alt pachet: /{rel}").into()); }
        let dest = inside(rel);
        if dest.symlink_metadata().is_ok() && !previous.as_ref().is_some_and(|old| old.files.contains(rel)) { return Err(format!("fișier existent, fără proprietar VOS: /{rel}").into()); }
        // Refuzăm directoare simbolice din sistemul țintă.
        let mut parent = dest.parent();
        while let Some(dir) = parent { if dir == root() { break; } if dir.symlink_metadata().is_ok_and(|x| x.file_type().is_symlink()) { return Err("director simbolic în calea destinație".into()); } parent = dir.parent(); }
    }
    let names: Vec<String> = files.iter().map(|x| x.0.clone()).collect();
    for (rel, data, mode) in files {
        let path = inside(&rel); fs::create_dir_all(path.parent().ok_or("cale invalidă")?)?;
        let tmp = path.with_extension("vos-tmp"); fs::write(&tmp, data)?;
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&tmp, fs::Permissions::from_mode(mode & 0o755))?; }
        fs::rename(tmp, path)?;
    }
    if let Some(old) = previous { for f in old.files { if !names.contains(&f) { let _ = fs::remove_file(inside(&f)); } } }
    db.packages.insert(p.name.clone(), Installed { version: p.version.clone(), files: names }); save_db(db)?;
    println!("Instalat: {} {}", p.name, p.version); Ok(())
}
fn remove(name: &str) -> Result<()> {
    require_root()?; check_name(name)?; let mut db = load_db()?;
    let pkg = db.packages.remove(name).ok_or("pachetul nu este instalat")?;
    for f in pkg.files { fs::remove_file(inside(&f))?; }
    save_db(&db)?; println!("Eliminat: {name}"); Ok(())
}
fn run(prog: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(prog).args(args).status()?;
    if !status.success() { return Err(format!("{prog} a eșuat: {status}").into()); } Ok(())
}
fn doctor() -> Result<()> {
    require_root()?;
    let mut problems = 0;
    for path in ["etc/vos/repos.json", "var/lib/vos/installed.json"] {
        let p = inside(path); if p.exists() { println!("OK  /{path}"); } else { println!("WARN  lipsește /{path}"); problems += 1; }
    }
    match load_db() { Ok(db) => for (name, pkg) in db.packages {
        for f in pkg.files { if !inside(&f).exists() { println!("WARN  {name}: lipsește /{f}"); problems += 1; } }
    }, Err(e) => { println!("WARN  baza de date: {e}"); problems += 1; } }
    match index() { Ok((_, idx)) => println!("OK  repository accesibil ({} pachete)", idx.packages.len()), Err(e) => { println!("WARN  repository: {e}"); problems += 1; } }
    if root() == Path::new("/") {
        for (label, cmd, args) in [("spațiu", "df", vec!["-h", "/"]), ("rețea", "ip", vec!["-brief", "addr"]), ("servicii", "systemctl", vec!["--failed", "--no-pager"])] {
            println!("\n{label}:"); if run(cmd, &args).is_err() { problems += 1; }
        }
    }
    println!("\nDiagnostic: {problems} avertismente");
    if problems > 0 { return Err("diagnosticul a găsit probleme".into()); } Ok(())
}
fn help() { println!("VOS 0.1 | update | install NUME | remove NUME | search TEXT | info NUME | doctor | ip | ports | reboot | shutdown | version | firewall"); }
fn main() { if let Err(e) = cli() { eprintln!("vos: {e}"); std::process::exit(1); } }
fn cli() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("version") => println!("{OS_VERSION}"),
        Some("install") => { require_root()?; let name = args.get(1).ok_or("folosește: vos install NUME")?; check_name(name)?;
            let (base, idx) = index()?; let p = idx.packages.iter().find(|p| &p.name == name).ok_or("pachetul nu există în repository")?;
            install(p, &base, &mut load_db()?)?;
        }
        Some("update") => { require_root()?; let (base, idx) = index()?; let mut db = load_db()?; let mut changed = 0;
            for p in &idx.packages { if db.packages.get(&p.name).is_some_and(|x| x.version != p.version) { install(p, &base, &mut db)?; changed += 1; } }
            println!("Actualizări instalate: {changed}");
        }
        Some("remove") => remove(args.get(1).ok_or("folosește: vos remove NUME")?)?,
        Some("search") => { let q = args.get(1).ok_or("folosește: vos search TEXT")?.to_lowercase(); let (_, idx) = index()?;
            for p in idx.packages { if p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q) { println!("{} {} — {}", p.name, p.version, p.description); } }
        }
        Some("info") => { let name = args.get(1).ok_or("folosește: vos info NUME")?; let (_, idx) = index()?;
            let p = idx.packages.iter().find(|p| &p.name == name).ok_or("pachetul nu există")?;
            let installed = load_db()?.packages.get(name).map(|x| x.version.clone()).unwrap_or_else(|| "nu".into());
            println!("{} {}\n{}\nInstalat: {}\nFișier: {}\nSHA-256: {}", p.name, p.version, p.description, installed, p.file, p.sha256);
        }
        Some("doctor") => doctor()?,
        Some("ip") => run("ip", &["-brief", "address"] )?,
        Some("ports") => run("ss", &["-tulpen"] )?,
        Some("firewall") => { if Command::new("firewall-cmd").arg("--state").status().is_ok_and(|s| s.success()) { run("firewall-cmd", &["--list-all"])?; } else { run("nft", &["list", "ruleset"])?; } }
        Some("reboot") => { require_root()?; run("systemctl", &["reboot"])?; }
        Some("shutdown") => { require_root()?; run("systemctl", &["poweroff"])?; }
        _ => help(),
    }
    io::stdout().flush()?; Ok(())
}
