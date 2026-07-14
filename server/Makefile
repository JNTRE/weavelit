.PHONY: check

check:
	cargo fmt --all -- --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo test --locked --workspace --all-targets
	cargo build --locked --workspace --release