#!/usr/bin/env -S uv run --script
"""Prepare explicitly disposable K&K domains and their isolated network."""

import argparse
import hashlib
import json
import re
import shlex
import subprocess
import tempfile
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path

from kk_native import ASSET_ROOT, environment

ROOT = Path(__file__).resolve().parents[1]
IMAGES = Path.home() / "VMs/images"
URI = "qemu:///system"
FEDORA_IMAGE = "Fedora-Cloud-Base-Generic-44-1.7.x86_64.qcow2"


def command(*args, capture=False, input=None):
    print("+", shlex.join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), input=input, text=True, check=True,
                          env=environment(ASSET_ROOT), stdout=subprocess.PIPE if capture else None).stdout


def virsh(*args, **kwargs):
    return command("virsh", "-c", URI, *args, **kwargs)


def backup(name):
    if virsh("domstate", name, capture=True).strip() != "shut off":
        raise SystemExit(f"Refusing to modify running domain {name}")
    xml = virsh("dumpxml", name, "--inactive", capture=True)
    destination = Path(tempfile.mkdtemp(prefix="kk-vm-backup-"))
    (destination / f"{name}.xml.bak").write_text(xml)
    print(f"Original domain definition preserved: {destination}", flush=True)
    return ET.fromstring(xml)


def save(name):
    xml = virsh("dumpxml", name, "--inactive", capture=True)
    (ROOT / "xml" / f"{name}.xml").write_text(xml)


def install_options(name, original, disk, memory, network_name="default"):
    return ["virt-install", "--connect", URI, "--name", name,
            "--uuid", original.findtext("uuid"), "--memory", str(memory),
            "--vcpus", "2", "--cpu", "host-passthrough",
            "--disk", f"path={disk},format=qcow2,bus=virtio,discard=unmap",
            "--network", f"network={network_name},model=virtio,mac=" + original.find("./devices/interface/mac").get("address"),
            "--channel", "unix,target.type=virtio,name=org.qemu.guest_agent.0",
            "--console", "pty,target_type=serial", "--noautoconsole"]


def control_settings():
    return ["--mkdir", "/usr/local/libexec",
            "--upload", str(ROOT / "scripts/kk-lab-control.sh") + ":/usr/local/libexec/kk-lab-control",
            "--chmod", "0755:/usr/local/libexec/kk-lab-control"]


def fedora_settings(hostname="kk-fedora"):
    return ["--hostname", hostname, *control_settings(),
            "--append-line", "/etc/selinux/targeted/contexts/files/file_contexts.local:/usr/local/libexec/kk-lab-control -- system_u:object_r:virt_qemu_ga_unconfined_exec_t:s0",
            "--firstboot", ROOT / "scripts/kk-fedora-firstboot.sh",
            "--run-command", "systemctl set-default multi-user.target && systemctl add-wants multi-user.target qemu-guest-agent.service && touch /etc/cloud/cloud-init.disabled"]


def verified_fedora():
    base = ASSET_ROOT / "isos" / FEDORA_IMAGE
    checksum_path = ASSET_ROOT / "isos/Fedora-Cloud-44-1.7-x86_64-CHECKSUM"
    keyring_dir = Path(tempfile.mkdtemp(prefix="kk-fedora-keyring-"))
    keyring = keyring_dir / "trustedkeys.gpg"
    command("gpg", "--homedir", keyring_dir, "--no-default-keyring", "--keyring", keyring,
            "--import", ASSET_ROOT / "isos/fedora.gpg")
    command("gpgv", "--homedir", keyring_dir, "--keyring", keyring, checksum_path)
    checksum = checksum_path.read_text()
    expected = re.search(r"SHA256 \(" + re.escape(FEDORA_IMAGE) + r"\) = ([a-f0-9]{64})", checksum)
    if not expected:
        raise SystemExit("No SHA-256 for the selected Fedora image")
    with base.open("rb") as image:
        actual = hashlib.file_digest(image, "sha256").hexdigest()
    if actual != expected[1]:
        raise SystemExit("Fedora image checksum mismatch")
    print(f"Fedora image SHA-256 verified: {actual}", flush=True)
    return base


def fedora(resume=False):
    base = verified_fedora()
    disk = IMAGES / "kk-fedora-44.qcow2"
    if disk.exists() and not resume:
        raise SystemExit(f"Lab disk already exists; inspect before reuse: {disk}")
    command("virt-install", "--version")
    original = backup("fedora-44")
    if disk.exists():
        info = json.loads(command("qemu-img", "info", "--output=json", disk, capture=True))
        if info.get("full-backing-filename") != str(base):
            raise SystemExit("Existing overlay does not use the verified Fedora base")
    else:
        command("qemu-img", "create", "-f", "qcow2", "-F", "qcow2", "-b", base, disk)
    command("virt-customize", "-a", disk, "--memsize", "1024", "--smp", "2",
            "--install", "qemu-guest-agent,iproute-tc,tcpdump",
            *fedora_settings())
    virsh("undefine", "fedora-44", "--keep-nvram")
    command(*install_options("fedora-44", original, disk, 1536),
            "--import", "--osinfo", "fedora40", "--graphics", "none")
    save("fedora-44")


def mint(resume=False):
    disk = IMAGES / "kk-mint-22.3.qcow2"
    if disk.exists() and not resume:
        raise SystemExit(f"Lab disk already exists; inspect before reuse: {disk}")
    command("virt-install", "--version")
    original = backup("mint-22.3")
    if disk.exists():
        info = json.loads(command("qemu-img", "info", "--output=json", disk, capture=True))
        if info.get("actual-size", 0) > 1024 * 1024 or info.get("backing-filename"):
            raise SystemExit("Refusing to rerun an installer over a populated disk")
    else:
        command("qemu-img", "create", "-f", "qcow2", disk, "24G")
    virsh("undefine", "mint-22.3", "--keep-nvram")
    iso = ASSET_ROOT / "isos/linuxmint-22.3-cinnamon-64bit.iso"
    command(*install_options("mint-22.3", original, disk, 3072),
            "--osinfo", "ubuntu24.04", "--location",
            f"{iso},kernel=casper/vmlinuz,initrd=casper/initrd.lz",
            "--disk", f"path={iso},device=cdrom,bus=sata",
            "--initrd-inject", ROOT / "preseed/kk/preseed.cfg",
            "--extra-args", "boot=casper automatic-ubiquity auto=true priority=critical noprompt console=ttyS0,115200",
            "--graphics", "spice,listen=127.0.0.1", "--video", "virtio",
            "--noreboot", "--wait", "-1")
    # The installer is stopped; only the installed guest gets the lower budget.
    virsh("setvcpus", "mint-22.3", "2", "--config")
    virsh("setmem", "mint-22.3", "1536M", "--config")
    virsh("setmaxmem", "mint-22.3", "1536M", "--config")
    save("mint-22.3")
    finish("mint-22.3")


def network():
    # This is a dedicated bridge: default NAT and physical interfaces are unchanged.
    definition = ROOT / "xml/kk-isolated.xml"
    name = ET.parse(definition).findtext("name")
    virsh("net-define", definition)
    if name not in virsh("net-list", "--name", capture=True).splitlines():
        virsh("net-start", name)


def third():
    name = "kk-peer-3"
    if name in virsh("list", "--all", "--name", capture=True).splitlines():
        raise SystemExit("Third domain already exists; refusing to recreate its identity or data")
    base = verified_fedora()
    disk = IMAGES / f"{name}.qcow2"
    if disk.exists():
        raise SystemExit(f"Refusing to overwrite existing third-guest disk: {disk}")
    host = next(item for item in ET.parse(ROOT / "xml/kk-isolated.xml").findall("./ip/dhcp/host") if item.get("name") == name)
    command("qemu-img", "create", "-f", "qcow2", "-F", "qcow2", "-b", base, disk)
    command("virt-customize", "-a", disk, "--memsize", "1024", "--smp", "2",
            "--install", "qemu-guest-agent,iproute-tc,tcpdump", *fedora_settings(name))
    network()
    original = ET.fromstring(f'<domain><uuid>{uuid.uuid4()}</uuid><devices><interface><mac address="{host.get("mac")}"/></interface></devices></domain>')
    command(*install_options(name, original, disk, 1536, "kk-isolated"), "--import", "--osinfo", "fedora40", "--graphics", "none")
    save(name)


def finish(name):
    original = backup(name)
    disk = IMAGES / f"kk-{name}.qcow2"
    options = fedora_settings() if name == "fedora-44" else [
        "--install", "qemu-guest-agent,tcpdump,iproute2",
        *control_settings(),
        "--run-command", "systemctl set-default multi-user.target && systemctl add-wants multi-user.target qemu-guest-agent.service && touch /etc/kk-lab-ready",
    ]
    command("virt-customize", "-a", disk, "--memsize", "1024", "--smp", "2", *options)
    active = virsh("net-list", "--name", capture=True).splitlines()
    for source in original.findall("./devices/interface/source"):
        network_name = source.get("network")
        if network_name and network_name not in active:
            virsh("net-start", network_name)
    virsh("start", name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["fedora", "mint", "third", "network", "finish-fedora", "finish-mint"])
    parser.add_argument("--resume", action="store_true", help="Reuse a verified overlay or still-empty installer disk")
    parser.add_argument("--assets", type=Path, default=ASSET_ROOT, help="Existing verified ISO/cloud image and host-kernel asset directory")
    args = parser.parse_args()
    ASSET_ROOT = args.assets.resolve()
    if args.action == "network":
        network()
    elif args.action == "third":
        third()
    elif args.action.startswith("finish-"):
        finish("fedora-44" if args.action == "finish-fedora" else "mint-22.3")
    else:
        {"fedora": fedora, "mint": mint}[args.action](args.resume)
