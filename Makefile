VOS=vos/target/x86_64-unknown-linux-musl/release/vos
.PHONY: build test fmt lint profiles audit clean
build:
	cargo build --release --locked --target x86_64-unknown-linux-musl --manifest-path vos/Cargo.toml
test: build
	VOS_TEST_BINARY=$(VOS) python3 tests/integration.py
fmt:
	cargo fmt --manifest-path vos/Cargo.toml
lint:
	cargo clippy --manifest-path vos/Cargo.toml --locked -- -D warnings
profiles:
	python3 installer/vicios-installer --list-profiles
audit:
	python3 tools/audit_elf.py rootfs
clean:
	cargo clean --manifest-path vos/Cargo.toml
