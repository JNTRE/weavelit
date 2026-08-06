NPM ?= npm
WEB_UI_DIR := web-ui

.PHONY: check check-web-ui check-rust web-ui-install web-ui-typecheck web-ui-test web-ui-build web-ui-verify

check: check-web-ui check-rust

check-web-ui: web-ui-install web-ui-typecheck web-ui-test web-ui-build web-ui-verify

web-ui-install:
	cd $(WEB_UI_DIR) && $(NPM) ci

web-ui-typecheck:
	cd $(WEB_UI_DIR) && $(NPM) run typecheck

web-ui-test:
	cd $(WEB_UI_DIR) && $(NPM) run test

# `vite build` empties the output directory, so this target is always a clean build.
web-ui-build:
	cd $(WEB_UI_DIR) && $(NPM) run build

web-ui-verify:
	cd $(WEB_UI_DIR) && $(NPM) run verify:build

check-rust:
	cargo fmt --all -- --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo test --locked --workspace --all-targets
	cargo build --locked --workspace --release