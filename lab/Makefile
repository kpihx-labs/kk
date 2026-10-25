SHELL := /bin/sh
.DEFAULT_GOAL := help

.PHONY: help check kk-start kk-setup kk-test kk-node-test kk-stop

help: ## Show tasks
	@printf '%s\n' 'make check        Static checks and control-adapter tests; no VM changes' 'make kk-start     Start the private network and declared guests' 'make kk-setup     Build K&K, deploy and pair fresh isolated identities' 'make kk-test      Setup, then run the full VM regression suite' 'make kk-node-test Focused three-guest persistent-node gate' 'make kk-stop      Gracefully stop guests and the private network'

check: ## No elevation and no real guest/network changes
	ruff check scripts/05-kk-prepare.py scripts/06-kk-test.py scripts/kk_guest.py scripts/kk_native.py scripts/kk_file_cases.py scripts/kk_sensitive_cases.py scripts/kk_tty_cases.py scripts/kk_pair_cases.py scripts/kk_node_cases.py tests/
	shellcheck scripts/kk-fedora-firstboot.sh scripts/kk-lab-control.sh
	uv run --no-project --directory scripts python -m unittest discover -s ../tests -p 'test_kk_*.py'

kk-start: ## Start only the dedicated lab
	uv run --no-project scripts/05-kk-prepare.py network
	uv run --no-project scripts/06-kk-test.py start

kk-setup: kk-start ## Fresh identities; no private key export
	$(MAKE) -C .. build
	uv run --no-project scripts/06-kk-test.py setup

kk-test: kk-setup ## Explicitly mutates only the test guests and their network
	uv run --no-project scripts/06-kk-test.py exercise

kk-node-test: kk-setup ## Focused three-guest gate, never replaces the full regression run
	uv run --no-project scripts/06-kk-test.py nodes

kk-stop: ## Preserve disks and results; no forced guest destruction
	uv run --no-project scripts/06-kk-test.py stop
