SHELL := /bin/sh
.DEFAULT_GOAL := help

.PHONY: help check kk-start kk-setup kk-test kk-node-test crypto-test control-test room-test enrollment-test room-cli-test native-media-test kk-stop

MEDIA_RECEIVER ?=
MEDIA_XVFB ?= Xvfb
MEDIA_XVFB_LIBS ?=

help: ## Show tasks
	@printf '%s\n' 'make check        Static checks and control-adapter tests; no VM changes' 'make kk-start     Start the private network and declared guests' 'make kk-setup     Build K&K, deploy and pair fresh isolated identities' 'make kk-test      Setup, then run the full VM regression suite' 'make kk-node-test Focused three-guest persistent-node gate' 'make kk-stop      Gracefully stop guests and the private network'

check: ## No elevation and no real guest/network changes
	ruff check scripts/ tests/
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

crypto-test: kk-start ## Real native crypto on each OS; not a room-network gate
	uv run --no-project scripts/07-crypto-test.py

control-test: kk-setup ## Actual cross-VM public control; not private MLS room data
	uv run --no-project scripts/08-control-test.py

room-test: kk-setup ## Actual private MLS/QUIC across guests; preapproved bootstrap
	uv run --no-project scripts/09-room-test.py

enrollment-test: kk-setup ## Authority/grants/package over authenticated enrollment, no implicit vote
	uv run --no-project scripts/10-enrollment-test.py

room-cli-test: kk-setup ## Public room CLI, network enrollment, cold resume and creator VM off
	uv run --no-project scripts/11-room-cli-test.py

native-media-test: ## Explicit private-display/native playback guest and live bandwidth contention
	@test -n "$(MEDIA_RECEIVER)" || { printf '%s\n' 'Set MEDIA_RECEIVER to a declared native-media guest.'; exit 1; }
	$(MAKE) kk-setup
	uv run --no-project scripts/12-native-media-test.py --receiver "$(MEDIA_RECEIVER)" --xvfb "$(MEDIA_XVFB)" $(if $(MEDIA_XVFB_LIBS),--xvfb-libraries "$(MEDIA_XVFB_LIBS)")

kk-stop: ## Preserve disks and results; no forced guest destruction
	uv run --no-project scripts/06-kk-test.py stop
