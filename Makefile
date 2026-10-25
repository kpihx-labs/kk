SHELL := /bin/zsh
.DEFAULT_GOAL := help
GITHUB_SSH_COMMAND ?= ssh
GITLAB_SSH_COMMAND ?= ssh

.PHONY: help check build fmt run push lab-check lab-test lab-crypto-test lab-stop

help: ## Show available tasks
	@printf '%s\n' \
	  'make check           Format, lint, tests and documentation' \
	  'make build           Build the release CLI' \
	  'make fmt             Format Rust source' \
	  'make run             Show CLI help' \
	  'make lab-check       Static lab validation; no VM changes' \
	  'make lab-test        Full cross-guest regression' \
	  'make lab-crypto-test Native per-OS crypto; not a room-network gate' \
	  'make lab-stop        Stop the dedicated guests and network' \
	  'make push            Push this branch to both SSH remotes'

check: ## Validate offline with local loopback network tests
	cargo fmt --all -- --check
	cargo clippy --offline --locked --all-targets -- -D warnings
	cargo test --offline --locked --all-targets
	RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps

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
