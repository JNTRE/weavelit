NPM ?= npm
WEB_UI_DIR := web-ui
DOCKER ?= docker
MAKEFILE_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
REPOSITORY_ROOT := $(abspath $(MAKEFILE_DIR)/..)
CONTAINER_IMAGE ?= weavelit-server-dev
CONTAINER_NAME ?= weavelit-server-local
CONTAINER_PORT ?= 8443
CONTAINERFILE := $(MAKEFILE_DIR)/containers/dev/Containerfile
CONTAINER_CARGO_REGISTRY_VOLUME := weavelit-server-cargo-registry
CONTAINER_CARGO_GIT_VOLUME := weavelit-server-cargo-git
CONTAINER_NPM_CACHE_VOLUME := weavelit-server-npm-cache
CONTAINER_TARGET_VOLUME := weavelit-server-target
CONTAINER_NODE_MODULES_VOLUME := weavelit-server-node-modules
CONTAINER_STATE_VOLUME := weavelit-server-local-state
CONTAINER_WORKSPACE_MOUNT := --mount type=bind,source="$(REPOSITORY_ROOT)",target=/workspace
CONTAINER_CACHE_MOUNTS := --mount type=volume,source=$(CONTAINER_CARGO_REGISTRY_VOLUME),target=/home/weavelit/.cargo/registry --mount type=volume,source=$(CONTAINER_CARGO_GIT_VOLUME),target=/home/weavelit/.cargo/git --mount type=volume,source=$(CONTAINER_NPM_CACHE_VOLUME),target=/home/weavelit/.npm --mount type=volume,source=$(CONTAINER_TARGET_VOLUME),target=/workspace/server/target --mount type=volume,source=$(CONTAINER_NODE_MODULES_VOLUME),target=/workspace/server/web-ui/node_modules
CONTAINER_WORKSPACE_ARGS := $(CONTAINER_WORKSPACE_MOUNT) $(CONTAINER_CACHE_MOUNTS)

.PHONY: check check-web-ui check-rust web-ui-install web-ui-format web-ui-lint web-ui-typecheck web-ui-test web-ui-script-test web-ui-build web-ui-verify web-ui-browser-install web-ui-browser-test container-build container-volumes container-launcher-test container-check container-shell container-prepare container-run container-stop container-logs

# The browser smoke test drives the release binary produced by `check-rust`, so
# the stages of this gate must not be reordered or run concurrently.
.NOTPARALLEL:

check: check-web-ui check-rust web-ui-browser-test container-launcher-test

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

container-build:
	$(DOCKER) build --tag "$(CONTAINER_IMAGE)" --file "$(CONTAINERFILE)" "$(REPOSITORY_ROOT)"

# Docker creates named volumes as root-owned, so initialize the caches and the
# persistent local Server state before the non-root development user uses them.
container-volumes: container-build
	$(DOCKER) volume create "$(CONTAINER_CARGO_REGISTRY_VOLUME)" >/dev/null
	$(DOCKER) volume create "$(CONTAINER_CARGO_GIT_VOLUME)" >/dev/null
	$(DOCKER) volume create "$(CONTAINER_NPM_CACHE_VOLUME)" >/dev/null
	$(DOCKER) volume create "$(CONTAINER_TARGET_VOLUME)" >/dev/null
	$(DOCKER) volume create "$(CONTAINER_NODE_MODULES_VOLUME)" >/dev/null
	$(DOCKER) volume create "$(CONTAINER_STATE_VOLUME)" >/dev/null
	$(DOCKER) run --rm --user root --mount type=volume,source=$(CONTAINER_CARGO_REGISTRY_VOLUME),target=/cargo-registry --mount type=volume,source=$(CONTAINER_CARGO_GIT_VOLUME),target=/cargo-git --mount type=volume,source=$(CONTAINER_NPM_CACHE_VOLUME),target=/npm-cache --mount type=volume,source=$(CONTAINER_TARGET_VOLUME),target=/target --mount type=volume,source=$(CONTAINER_NODE_MODULES_VOLUME),target=/node-modules --mount type=volume,source=$(CONTAINER_STATE_VOLUME),target=/state "$(CONTAINER_IMAGE)" sh -ec 'chown 10001:10001 /cargo-registry /cargo-git /npm-cache /target /node-modules /state; chmod 700 /cargo-registry /cargo-git /npm-cache /state; chmod 755 /target /node-modules'

container-launcher-test:
	sh containers/dev/run-local-server-lifecycle-test.sh

container-check: container-launcher-test container-volumes
	$(DOCKER) run --rm $(CONTAINER_WORKSPACE_ARGS) --workdir /workspace/server "$(CONTAINER_IMAGE)" make check

container-shell: container-volumes
	$(DOCKER) run --rm --interactive --tty $(CONTAINER_WORKSPACE_ARGS) --workdir /workspace/server "$(CONTAINER_IMAGE)" bash

# Build the embedded Web UI and release binary that the manually launched
# local Server serves. The full test suite remains container-check.
container-prepare: container-volumes
	$(DOCKER) run --rm $(CONTAINER_WORKSPACE_ARGS) --workdir /workspace/server "$(CONTAINER_IMAGE)" sh -ec 'make web-ui-install web-ui-build && cargo build --locked --workspace --release'

# The Server itself remains bound to its container loopback address. The
# launcher relays Docker's published port to that listener, so macOS can reach
# the Web UI at https://localhost:8443 without exposing it to the LAN.
container-run: container-prepare
	$(DOCKER) rm --force "$(CONTAINER_NAME)" >/dev/null 2>&1 || true
	$(DOCKER) run --detach --name "$(CONTAINER_NAME)" --publish "127.0.0.1:$(CONTAINER_PORT):8444" --env WEAVELIT_STATE_ROOT=/var/lib/weavelit/state-root $(CONTAINER_WORKSPACE_ARGS) --mount type=volume,source=$(CONTAINER_STATE_VOLUME),target=/var/lib/weavelit/state-root --workdir /workspace/server "$(CONTAINER_IMAGE)" sh containers/dev/run-local-server.sh
	@attempt=0; until $(DOCKER) exec "$(CONTAINER_NAME)" test -e /tmp/weavelit-local-server-ready; do \
		attempt=$$((attempt + 1)); \
		if [ $$attempt -eq 20 ] || ! $(DOCKER) inspect --format '{{.State.Running}}' "$(CONTAINER_NAME)" | grep -qx true; then \
			$(DOCKER) logs "$(CONTAINER_NAME)"; \
			exit 1; \
		fi; \
		sleep 1; \
	done
	@printf 'Local Server started at https://localhost:%s\n' "$(CONTAINER_PORT)"

container-stop:
	$(DOCKER) stop "$(CONTAINER_NAME)" >/dev/null 2>&1 || true

container-logs:
	$(DOCKER) logs --follow "$(CONTAINER_NAME)"