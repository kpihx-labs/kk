#!/bin/sh
set -eu

# The lab controller is the trusted hypervisor. Permit its virtio management
# commands without disabling SELinux enforcement for the rest of the guest.
setsebool -P virt_qemu_ga_run_unconfined on
restorecon /usr/local/libexec/kk-lab-control
systemctl enable --now NetworkManager
touch /etc/kk-lab-ready
