# Contributing

1. Build with `make build` and run `make test`.
2. Run `make lint` before opening a pull request.
3. Do not commit signing keys, `.vpk` binaries, ISO files, personal Hyprland configuration or generated `target/` directories.
4. Changes to package format, dependency solving, transaction recovery, installer partitioning or repository trust require tests and an update to `README.md`.
5. Do not add package post-install scripts until a sandbox and rollback design exists. Packages should contain files and declarative metadata only.
