mod archive;
mod model;
mod repo;
mod solver;
mod system;
mod transaction;
use anyhow::{ensure, Result};
use clap::{Parser, Subcommand};
use std::{fs, path::PathBuf};
use system::Context;
#[derive(Parser)]
#[command(name = "vos", version, about = "ViciOS package manager")]
struct Cli {
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[arg(long, global = true)]
    dry_run: bool,
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    Install {
        #[arg(required = true)]
        packages: Vec<String>,
    },
    Remove {
        #[arg(required = true)]
        packages: Vec<String>,
    },
    Update,
    Search {
        query: String,
    },
    Info {
        package: String,
    },
    Doctor,
    Ip,
    Ports,
    Reboot,
    Shutdown,
    Version,
    Firewall {
        args: Vec<String>,
    },
    /// Replays an interrupted transaction; performed automatically before mutations.
    Recover,
}
fn main() {
    if let Err(e) = run() {
        eprintln!("vos: {e:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = cli
        .root
        .or_else(|| std::env::var_os("VOS_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"));
    let ctx = Context::new(root)?;
    match cli.command {
        Cmd::Install { packages } => install(&ctx, packages, cli.dry_run, false)?,
        Cmd::Update => install(&ctx, vec![], cli.dry_run, true)?,
        Cmd::Remove { packages } => {
            let _lock = ctx.writable()?;
            transaction::recover(&ctx)?;
            let db = ctx.database()?;
            solver::remove(&packages, &db)?;
            println!("Remove: {}", packages.join(", "));
            if !cli.dry_run {
                transaction::remove(&ctx, &db, &packages)?;
            }
        }
        Cmd::Recover => {
            let _lock = ctx.writable()?;
            transaction::recover(&ctx)?;
        }
        Cmd::Search { query } => {
            let (_, idx) = repo::load(&ctx)?;
            let q = query.to_lowercase();
            for p in idx.packages {
                if p.manifest.name.to_lowercase().contains(&q)
                    || p.manifest.description.to_lowercase().contains(&q)
                {
                    println!(
                        "{} {} — {}",
                        p.manifest.name, p.manifest.version, p.manifest.description
                    );
                }
            }
        }
        Cmd::Info { package } => {
            let (_, idx) = repo::load(&ctx)?;
            let p = idx
                .packages
                .iter()
                .find(|p| p.manifest.name == package)
                .ok_or_else(|| anyhow::anyhow!("package not found: {package}"))?;
            println!("{}", serde_json::to_string_pretty(p)?);
            println!(
                "Installed: {}",
                ctx.database()?
                    .packages
                    .get(&package)
                    .map(|p| p.manifest.version.as_str())
                    .unwrap_or("no")
            );
        }
        Cmd::Doctor => doctor(&ctx)?,
        Cmd::Version => system::version(&ctx)?,
        Cmd::Ip => {
            ctx.live_only()?;
            let entries = fs::read_dir("/sys/class/net")?;
            for e in entries {
                let e = e?;
                println!(
                    "{}: {}",
                    e.file_name().to_string_lossy(),
                    if e.path().join("wireless").exists() {
                        "Wi-Fi"
                    } else {
                        "other interface"
                    }
                );
            }
            system::command("ip", &["-brief", "address"])?;
        }
        Cmd::Ports => {
            ctx.live_only()?;
            println!(
                "TCP LISTEN / UDP UNCONN; local address:port, process (sudo for all processes)"
            );
            system::command("ss", &["-tulpen"])?;
        }
        Cmd::Firewall { args } => {
            ctx.live_only()?;
            ensure!(
                !cli.dry_run || args.is_empty() || args == ["status"],
                "dry-run not available for firewall changes"
            );
            system::firewall(&args)?;
        }
        Cmd::Reboot | Cmd::Shutdown => {
            ctx.live_only()?;
            system::root_required()?;
            ensure!(!cli.dry_run, "power commands do not support dry-run");
            system::command(
                "systemctl",
                &[if matches!(cli.command, Cmd::Reboot) {
                    "reboot"
                } else {
                    "poweroff"
                }],
            )?;
        }
    }
    Ok(())
}
fn install(ctx: &Context, mut names: Vec<String>, dry: bool, update: bool) -> Result<()> {
    let _lock = ctx.writable()?;
    transaction::recover(ctx)?;
    let db = ctx.database()?;
    let (base, index) = repo::load(ctx)?;
    if update {
        names = db.packages.keys().cloned().collect();
    }
    let packages = solver::plan(&names, &index, &db)?;
    for p in &packages {
        println!("{} {}", p.manifest.name, p.manifest.version);
    }
    println!("{} packages in transaction", packages.len());
    if dry {
        return Ok(());
    }
    let mut staged = vec![];
    for p in packages {
        let bytes = repo::package(&base, &p)?;
        let payload = archive::unpack(&bytes, &p.manifest)?;
        staged.push((p, payload));
    }
    transaction::install(ctx, &db, staged, index.serial)?;
    Ok(())
}
fn doctor(ctx: &Context) -> Result<()> {
    let _lock = ctx.writable()?;
    let mut errors = 0;
    if ctx.state().join("pending").exists() {
        eprintln!("ERROR pending transaction: run vos recover");
        errors += 1;
    }
    let db = ctx.database()?;
    for (name, p) in &db.packages {
        for (rel, record) in &p.files {
            let dest = ctx.safe(rel)?;
            if !transaction::actual(&dest).is_ok_and(|x| x == record.sha256) {
                println!("ERROR {name}: missing/modified {rel}");
                errors += 1;
            }
        }
    }
    let manifests = db
        .packages
        .iter()
        .map(|(n, p)| (n.clone(), p.manifest.clone()))
        .collect();
    if let Err(e) = solver::validate(&manifests) {
        println!("ERROR dependencies: {e}");
        errors += 1;
    }
    match repo::load(ctx) {
        Ok((_, i)) => println!("OK signed repository serial {}", i.serial),
        Err(e) => {
            println!("ERROR repository: {e}");
            errors += 1;
        }
    }
    if ctx.host() {
        system::command("df", &["-h", "/"])?;
        let services = std::process::Command::new("systemctl")
            .args(["--failed", "--no-legend", "--no-pager"])
            .output()?;
        if !services.status.success() || !services.stdout.is_empty() {
            println!(
                "ERROR services: {}",
                String::from_utf8_lossy(&services.stdout)
            );
            errors += 1;
        }
    }
    println!(
        "{errors} problems, {} installed packages checked",
        db.packages.len()
    );
    ensure!(errors == 0, "doctor found problems");
    Ok(())
}
