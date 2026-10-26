SHELL := /bin/zsh
.DEFAULT_GOAL := help
GITHUB_SSH_COMMAND ?= ssh
GITLAB_SSH_COMMAND ?= ssh

.PHONY: help check ui-check packaging-check bundle verify-bundle install tag-release publish-tag build fmt run push lab-check lab-test lab-crypto-test lab-control-test lab-room-test lab-enrollment-test lab-room-cli-test lab-native-media-test lab-stop

help: ## Show available tasks
	@printf '%s\n' \
	  'make check           Format, lint, tests and documentation' \
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
	  'make push            Push this branch to both SSH remotes'

check: ## Validate offline with local loopback network tests
	cargo fmt --all -- --check
	cargo clippy --offline --locked --all-targets -- -D warnings
	cargo test --offline --locked --all-targets
	RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps
	$(MAKE) ui-check
	$(MAKE) packaging-check

packaging-check: ## Release integrity, archive safety and clean-source policy tests
	uv run --no-project python -m unittest discover -s tests -p 'test_release.py'

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

publish-tag: ## Publish only this version tag to both required SSH remotes
	uv run --no-project scripts/release.py push-tag

ui-check: ## Offline browser-adapter syntax and standard RTP unit tests
	node --check src/ui/assets/app.js
	node --check src/ui/assets/media.js
	node --check src/ui/assets/rtp.js
	node --check src/ui/assets/audio-worklet.js
	node --test tests/ui_rtp.mjs tests/ui_client.mjs

build: ## Build the release CLI
	cargo build --offline --locked --release

fmt: ## Format Rust source
	cargo fmt --all

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

push: ## Push the current branch to both authorized SSH remotes
	@set -e; branch="$$(git branch --show-current)"; test -n "$$branch"; \
	for remote in github gitlab; do \
		url="$$(git remote get-url "$$remote")"; \
		case "$$url" in git@*:*) ;; *) printf '%s\n' "Refusing non-SSH remote $$remote" >&2; exit 1 ;; esac; \
	done; \
	GIT_SSH_COMMAND='$(GITHUB_SSH_COMMAND)' git push github "$$branch"; \
	GIT_SSH_COMMAND='$(GITLAB_SSH_COMMAND)' git push gitlab "$$branch"
