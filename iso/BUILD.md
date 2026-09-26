# ViciOS ISO build plan

The distribution must bootstrap itself. The builder may run on Fedora, Ubuntu or Arch, but the produced rootfs cannot use their package database or binaries as runtime dependencies.

1. Fetch pinned Linux and BusyBox sources and verify `iso/sources.lock.json`.
2. Build the kernel, BusyBox and the bootstrap userland in a clean sysroot.
3. Build the VOS static binary and the first signed repository packages. Every package records `depends`, `abi`, version, file hashes and whether it is essential.
4. Build systemd, udev, dbus, NetworkManager, firmware, Mesa/Wayland, a display manager, and each desktop/WM as ViciOS packages. Desktop packages must declare every runtime library and helper they need.
5. Run `tools/audit_elf.py` against the sysroot. The build stops on missing loader or `DT_NEEDED` entries.
6. Generate an initramfs and a GRUB ISO. The live environment starts the TTY installer and contains VOS, partition tools, network tools and the signed repository configuration.
7. Boot-test BIOS and UEFI with QEMU. Then install a minimal TTY profile and each graphical profile in separate disposable disks.

The build must never call host `dnf`, `pacman`, `apt` or `flatpak` to satisfy a target package. The host supplies compilers and image tools only.

The current tree deliberately stops before claiming an ISO: a binary package repository and a complete independently built rootfs are still required.
