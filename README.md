# ViciOS independent distribution worktree

This tree contains the VOS package manager, a signed `.vpk` repository tool, a TTY installer and the desktop profile catalog. It is the engineering foundation for a real distribution; it is not yet a releasable ISO because the repository still needs a complete ViciOS base, kernel, firmware, bootloader, libraries, desktop packages and signed artifacts.

## What VOS now does

- Uses only ViciOS `.vpk` packages. It does not invoke `dnf`, `pacman`, `apt` or another package manager.
- Reads one signed repository index over HTTPS. The index contains ABI, serial and expiry fields. A separate Ed25519 signature is required.
- Resolves versioned dependencies before downloading and stages every archive before changing the target.
- Verifies package size, SHA-256, manifest identity, ABI and archive paths.
- Rejects devices, FIFOs, hard links, unsafe paths, unmanaged file collisions, directory symlink escapes and modified `/etc` files.
- Records file ownership and hashes, uses a transaction lock, persists a recovery journal and can roll back an interrupted transaction.
- Supports install, update, remove, search, info, doctor, ip, ports, firewall, reboot, shutdown and version.

## Build VOS

```sh
cd vos
cargo build --release --locked --target x86_64-unknown-linux-musl
cargo test --locked
```

The release binary is `vos/target/x86_64-unknown-linux-musl/release/vos`. The test suite requires Python 3 and runs against a temporary signed local repository:

```sh
VOS_TEST_BINARY="$PWD/vos/target/x86_64-unknown-linux-musl/release/vos" python3 tests/integration.py
```

## Build a repository

```sh
python3 tools/vpk.py keygen private-ed25519.pem
python3 tools/vpk.py pack manifest.json staged-root packages/example-1.0.0.vpk
cp packages/example-1.0.0.vpk.json repo/packages/
python3 tools/vpk.py index repo private-ed25519.pem --serial 1
```

Installers must receive a public key in `/etc/vos/repos.json`. Never ship the private key in an ISO or in a Git repository:

```json
{"index_url":"https://vicios.example/repo/index.json","public_key":"32-byte-ed25519-public-key-as-hex"}
```

A repository is usable only after its packages are built for ABI `x86_64-vicios-gnu` and an index is signed. The signed index is the source of package dependencies; dependencies are not guessed from filenames or collected from Fedora/Arch.

## Installer

List profiles or print a non-destructive plan:

```sh
./installer/vicios-installer --list-profiles
./installer/vicios-installer --profile hyprland-vicios-custom
```

The installer supports BIOS and UEFI layouts, ext4, a dedicated `/boot`, user creation, hostname/timezone, systemd, initramfs and GRUB. `--apply` requires root from live TTY and requires typing an exact erase confirmation. It first stages and validates all requested packages and checks that the rootfs contains the kernel, systemd, shell, user tools, VOS and boot tools. It refuses to continue when those are missing.

The desktop catalog includes the common archinstall desktop profiles, DriftWM and `hyprland-vicios-custom`. Profiles currently point to package names; they become installable only after the corresponding ViciOS packages are built and published. DriftWM is treated as experimental because its upstream project is fast-moving.

## Your custom Hyprland profile

Run `tools/export-custom-config.sh ~/vicios-custom-config.tar.gz` on the current Fedora ViciOS session. Review the archive and provide it with the project. The profile requirements file records the keybinds and widgets to preserve, but monitor connector names are not hardcoded into every machine: the final profile must detect outputs and apply the vertical workspace 10 rule only when the hardware provides the matching layout.

## ISO gate

An ISO is allowed only after:

1. A reproducible ViciOS base repository is generated from source tarballs whose checksums are pinned in `iso/sources.lock.json`.
2. Every base, kernel, firmware, bootloader, systemd, networking and selected desktop package has a manifest and dependency closure.
3. The VOS public key and signed index are embedded; private signing material stays offline.
4. `tools/audit_elf.py` reports no missing interpreter or `DT_NEEDED` library in the staged rootfs.
5. The installer stages the full target and passes its rootfs checks before partitioning.
6. The ISO boots in QEMU in BIOS and UEFI modes, reaches TTY, installs a minimal profile, boots the installed system and successfully runs VOS install/update/remove/doctor.
7. At least one graphical profile is boot-tested in QEMU and on the target hardware.

The current environment has no usable `/dev/kvm` and does not have the ISO utilities installed, so it cannot honestly claim this gate is passed yet.
