SHELL := /bin/sh
.DEFAULT_GOAL := help
GITHUB_SSH_COMMAND ?= ssh
GITLAB_SSH_COMMAND ?= ssh
PUSH_REMOTES ?= github
RUFF ?= ruff
UI_WORKFLOW_TEST := ui::tests::ui_http_pairs_independent_devices_owns_foreground_rooms_and_redacts_sensitive_events

.PHONY: help check rust-test ui-workflow-check docs-check ui-check fixture-check packaging-check python-check workflow-check sdk-check hax-check bundle verify-bundle install tag-release publish-tag build fmt run push lab-check lab-test lab-crypto-test lab-control-test lab-room-test lab-enrollment-test lab-room-cli-test lab-native-media-test lab-stop

help: ## Show available tasks
	@printf '%s\n' \
	  'make check           Format, lint, tests and documentation' \
	  'make rust-test       Concurrent Rust batch, then mandatory resource-contained UI workflow' \
	  'make ui-workflow-check Exact two-device workflow, with original deadlines and assertions' \
	  'make docs-check      Current guide links, tables, wire tokens and examples' \
	  'make fixture-check   Offline layout and opaque-input safety regressions' \
	  'make python-check    Pinned CI Python lint; may require tool cache/network' \
	  'make workflow-check  Pinned workflow syntax/lint gate (Go tool required)' \
	  'make sdk-check       Upstream expansion vectors and independent OPAQUE interoperability' \
	  'make hax-check       Explicit upstream-nightly macro check, not a formal proof' \
	  'make build           Build the release CLI' \
	  'make bundle TARGET=... Clean binary/notices/offline-source archives' \
	  'make verify-bundle ARCHIVE=... Integrity verification; attestations establish origin' \
	  'make install         Versioned user installation, no service or key creation' \
	  'make tag-release     Annotate the clean package-version commit' \
	  'make publish-tag     Push this version tag to both SSH remotes' \
	  'make fmt             Format Rust source' \
	  'make run             Show CLI help' \
	  'make lab-check       Static lab validation; no VM changes' \
	  'make lab-test        Full cross-guest regression' \
	  'make lab-crypto-test Native per-OS crypto; not a room-network gate' \
	  'make lab-control-test Actual cross-VM public control and recovery' \
	  'make lab-room-test    Private room VM gate; preapproved bootstrap' \
	  'make lab-enrollment-test Network-only enrollment into a non-voting learner' \
	  'make lab-room-cli-test Public room CLI and lifecycle across three guests' \
	  'make lab-native-media-test MEDIA_RECEIVER=... [MEDIA_XVFB=...] Native private-display contention' \
	  'make lab-stop        Stop the dedicated guests and network' \
	  'make push            Push this branch to GitHub; PUSH_REMOTES="github gitlab" syncs both'

check: ## Validate offline with local loopback network tests
	$(MAKE) python-check
	cargo fmt --package kk -- --check
	CARGO_INCREMENTAL=0 cargo clippy --offline --locked --all-targets -- -D warnings
	$(MAKE) rust-test
	cargo test --offline --locked --doc
	RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps
	$(MAKE) ui-check
	$(MAKE) fixture-check
	$(MAKE) packaging-check
	$(MAKE) docs-check

rust-test: ## Preserve concurrent coverage and run the compound workflow once outside unrelated load
	cargo test --offline --locked --all-targets -- --skip $(UI_WORKFLOW_TEST)
	$(MAKE) ui-workflow-check

ui-workflow-check: ## Mandatory integration-sized gate; no retry or deadline changes
	cargo test --offline --locked --lib $(UI_WORKFLOW_TEST) -- --exact --nocapture

docs-check: ## Pure-Python current documentation consistency checks; no network or VM mutation
	uv run --no-project python -m unittest discover -s tests -p 'test_docs.py'

packaging-check: ## Release integrity, archive safety and clean-source policy tests
	uv run --no-project python -m unittest discover -s tests -p 'test_release.py'
	uv run --no-project python -m unittest discover -s tests -p 'test_installers.py'

python-check: ## Python tool/fixture lint; CI provides the pinned checker through RUFF
	$(RUFF) check --target-version py311 scripts tests/*.py

fixture-check: ## Layout and opaque-input safety guards; no browser execution
	uv run --no-project python -m unittest discover -s tests -p 'test_*fixture.py'

workflow-check: ## Pinned actionlint validation; separate from the offline product check
	go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12 .github/workflows/*.yml

sdk-check: ## Historical reference packages may need network/cache; not part of offline product checks
	cargo test --locked --manifest-path third_party/overrides/elliptic-curve-xmd/Cargo.toml
	cargo fetch --locked --manifest-path lab/opaque-interop/reference/Cargo.toml
	cargo fetch --locked --manifest-path lab/opaque-interop/restricted/Cargo.toml
	cargo build --locked --manifest-path lab/opaque-interop/reference/Cargo.toml
	cargo build --locked --manifest-path lab/opaque-interop/restricted/Cargo.toml
	uv run --no-project lab/opaque-interop/run.py

HAX_TOOLCHAIN ?= nightly-2025-11-08
hax-check: ## Compile the actual macro implementation with its upstream nightly
	env RUSTFLAGS="--cfg hax" cargo +$(HAX_TOOLCHAIN) check --locked --manifest-path third_party/overrides/hax-lib-macros-0.3.7/Cargo.toml --lib

TARGET ?= x86_64-unknown-linux-gnu

bundle: ## Clean committed binary, notices and offline vendored source archives
	cargo build --offline --locked --release --target "$(TARGET)"
	uv run --no-project scripts/release.py bundle --target "$(TARGET)"

verify-bundle: ## Verify archive integrity; authenticate its CI attestation separately
	@test -n "$(ARCHIVE)" || { printf '%s\n' 'Set ARCHIVE to the downloaded binary archive.'; exit 1; }
	uv run --no-project scripts/release.py verify "$(ARCHIVE)"

PREFIX ?= $(HOME)/.local
install: build ## Immutable per-version user binary; refuse unrelated kk executables
	uv run --no-project scripts/release.py install --prefix "$(PREFIX)"

tag-release: ## Annotate the package version at clean HEAD; existing tags are never moved
	uv run --no-project scripts/release.py tag

publish-tag: ## Publish only this version tag to the selected SSH remotes
	uv run --no-project scripts/release.py push-tag --remotes $(PUSH_REMOTES)

ui-check: ## Offline browser-adapter syntax and standard RTP unit tests
	node --check src/ui/assets/app.js
	node --check src/ui/assets/media.js
	node --check src/ui/assets/rtp.js
	node --check src/ui/assets/audio-worklet.js
	node --test tests/ui_rtp.mjs tests/ui_client.mjs tests/ui_workflows.cjs tests/ui_files.cjs tests/ui_recovery.cjs tests/ui_media.mjs tests/ui_generated_audio.mjs tests/ui_layout.cjs tests/ui_history.mjs tests/ui_notifications.cjs tests/ui_conversation.cjs tests/ui_permissions.cjs

build: ## Build the release CLI
	cargo build --offline --locked --release

fmt: ## Format Rust source
	cargo fmt --package kk

run: ## Show CLI help
	cargo run --offline --locked -- --help

lab-check: ## Static lab validation; no VM changes
	$(MAKE) -C lab check

lab-test: ## Explicitly boot and exercise the dedicated disposable guests
	$(MAKE) -C lab kk-test

lab-crypto-test: ## Execute native room crypto tests on each declared OS
	$(MAKE) -C lab crypto-test

lab-control-test: ## Actual cross-guest public control quorum and recovery
	$(MAKE) -C lab control-test

lab-room-test: ## Actual private room data, delegated joins and recovery across guests
	$(MAKE) -C lab room-test

lab-enrollment-test: ## Scope proof then authority/grants/package/learner over authenticated network
	$(MAKE) -C lab enrollment-test

lab-room-cli-test: ## User-facing commands across real guests, not fixture adapters
	$(MAKE) -C lab room-cli-test

lab-native-media-test: ## Actual native capture/playback across a private shaped guest link
	$(MAKE) -C lab native-media-test

lab-stop: ## Gracefully stop only the dedicated lab
	$(MAKE) -C lab kk-stop

push: ## Push via SSH; GitHub now, PUSH_REMOTES="github gitlab" for later synchronization
	@set -e; branch="$$(git branch --show-current)"; test -n "$$branch"; \
	test -n '$(strip $(PUSH_REMOTES))'; \
	for remote in $(PUSH_REMOTES); do \
		case "$$remote" in github|gitlab) ;; *) printf '%s\n' "Refusing unauthorized remote $$remote" >&2; exit 1 ;; esac; \
		url="$$(git remote get-url "$$remote")"; \
		case "$$url" in git@*:*) ;; *) printf '%s\n' "Refusing non-SSH remote $$remote" >&2; exit 1 ;; esac; \
	done; \
	for remote in $(PUSH_REMOTES); do \
		case "$$remote" in github) transport='$(GITHUB_SSH_COMMAND)' ;; gitlab) transport='$(GITLAB_SSH_COMMAND)' ;; esac; \
		GIT_SSH_COMMAND="$$transport" git push "$$remote" "$$branch"; \
	done
