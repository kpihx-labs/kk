#!/bin/sh
# Trusted hypervisor-only entrypoint, labelled for Fedora's guest-agent domain transition.
set -eu
exec "$@"
