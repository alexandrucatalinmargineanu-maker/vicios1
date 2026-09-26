# Engineering references

The project design was checked against these public projects and documentation on 2026-09-26:

- Linux From Scratch stable systemd book: https://www.linuxfromscratch.org/lfs/view/stable-systemd/ — bootstrap order, dependency closure and the fact that LFS does not prescribe one package manager.
- Archinstall source profiles: https://github.com/archlinux/archinstall/tree/master/archinstall/default_profiles/desktops — desktop profile structure and minimal profile selection ideas.
- DriftWM upstream: https://github.com/malbiruk/driftwm — compositor requirements, session configuration and its experimental status.
- Linux kernel releases: https://www.kernel.org/ — pinned source tarball and checksum source used by `iso/sources.lock.json`.

These are design references only. ViciOS does not copy their package databases or invoke their package managers at runtime.
