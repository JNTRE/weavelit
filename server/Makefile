NPM ?= npm
WEB_UI_DIR := web-ui

.PHONY: check check-web-ui check-rust web-ui-install web-ui-format web-ui-lint web-ui-typecheck web-ui-test web-ui-script-test web-ui-build web-ui-verify web-ui-browser-install web-ui-browser-test

# The browser smoke test drives the release binary produced by `check-rust`, so
# the stages of this gate must not be reordered or run concurrently.
.NOTPARALLEL:

check: check-web-ui check-rust web-ui-browser-test

check-web-ui: web-ui-install web-ui-format web-ui-lint web-ui-typecheck web-ui-test web-ui-script-test web-ui-build web-ui-verify

web-ui-format:
	cd $(WEB_UI_DIR) && $(NPM) run format:check

web-ui-lint:
	cd $(WEB_UI_DIR) && $(NPM) run lint

web-ui-install:
	cd $(WEB_UI_DIR) && $(NPM) ci

web-ui-typecheck:
	cd $(WEB_UI_DIR) && $(NPM) run typecheck

web-ui-test:
	cd $(WEB_UI_DIR) && $(NPM) run test

web-ui-script-test:
	cd $(WEB_UI_DIR) && $(NPM) run test:scripts

# `vite build` empties the output directory, so this target is always a clean
# build; it then writes the build content manifest for that exact output.
web-ui-build:
	cd $(WEB_UI_DIR) && $(NPM) run build

web-ui-verify:
	cd $(WEB_UI_DIR) && $(NPM) run verify:build

# Installs only the pinned Chromium build. Its shared libraries come from the
# development container image or the CI runner, which own root-level packages.
web-ui-browser-install:
	cd $(WEB_UI_DIR) && $(NPM) exec -- playwright install chromium

# Runs after `check-rust` because the smoke test drives the release binary.
web-ui-browser-test: web-ui-browser-install
	cd $(WEB_UI_DIR) && $(NPM) run test:browser

check-rust:
	cargo fmt --all -- --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo test --locked --workspace --doc
	cargo test --locked --workspace --all-targets
	cargo build --locked --workspace --release