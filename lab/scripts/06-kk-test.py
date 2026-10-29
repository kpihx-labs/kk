#!/usr/bin/env -S uv run --script
"""Deploy and exercise K&K on declared isolated guests, controlled only over virtio."""

import argparse
import hashlib
import json
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path

from kk_file_cases import file_cases
from kk_guest import CLI, Guest, declared_peers, selected_binary, virsh, shutdown_guests
from kk_node_cases import node_cases
from kk_pair_cases import pair_cases
from kk_sensitive_cases import sensitive_cases
from kk_tty_cases import tty_cases

ROOT = Path(__file__).resolve().parents[1]
NETWORK_XML = ROOT / "xml/kk-isolated.xml"
PEERS = declared_peers()
STATE = ROOT / "tmp/kk-state.json"
PORT = 45454
PROFILES = {
    "clean": [],
    "limited": ["rate", "1mbit", "delay", "50ms", "loss", "1%"],
    "difficult": ["rate", "256kbit", "delay", "150ms", "loss", "3%"],
    "blackhole": ["loss", "100%"],
}


def isolate(peer):
    guest = Guest(peer["name"])
    tree = ET.fromstring(virsh("dumpxml", guest.name))
    interfaces = tree.findall("./devices/interface")
    if len(interfaces) != 1:
        raise RuntimeError(f"{guest.name}: unexpected additional network interfaces")
    source = interfaces[0].find("source").get("network")
    if source != "kk-isolated":
        if source != "default":
            raise RuntimeError(f"{guest.name}: refusing to replace unknown network {source}")
        virsh("detach-interface", guest.name, "network", "--mac", peer["mac"], "--live", "--config")
        virsh("attach-interface", guest.name, "network", "kk-isolated", "--model", "virtio",
              "--mac", peer["mac"], "--live", "--config")
    guest.configure_isolated_network(peer)


def setup():
    binary = selected_binary()
    data = binary.read_bytes()
    expected = hashlib.sha256(data).hexdigest()
    run_id = uuid.uuid4().hex[:12]
    work = "/var/tmp/kk-lab-" + run_id
    for peer in PEERS:
        guest = Guest(peer["name"])
        guest.agent("guest-ping")
        print(f"Preparing {guest.name}", flush=True)
        guest.upload(CLI, data)
        peer["binary_sha256"] = guest.verify_executable(expected)
        guest.run("/usr/bin/chmod", "0755", CLI)
        if guest.run("/usr/bin/test", "-x", "/usr/sbin/restorecon", check=False)["code"] == 0:
            guest.run("/usr/sbin/restorecon", CLI)
            peer["selinux"] = guest.run("getenforce")["out"].strip()
            if peer["selinux"] != "Enforcing":
                raise RuntimeError(f"{guest.name}: SELinux enforcement was unexpectedly weakened")
        print(guest.run(CLI, "--version")["out"].strip(), flush=True)
        guest.run("/usr/bin/mkdir", "-m", "0700", work)
        peer["identity"] = work + "/identity"
        peer["certificate"] = work + "/peer.der"
        peer["fingerprint"] = guest.run(CLI, "identity", "create", peer["identity"])["out"].strip()
        isolate(peer)
        (ROOT / "xml" / f"{guest.name}.xml").write_text(virsh("dumpxml", guest.name, "--inactive"))
        peer["kernel"] = guest.run("/usr/bin/uname", "-sr")["out"].strip()
        peer["libc"] = guest.run("/usr/bin/getconf", "GNU_LIBC_VERSION")["out"].strip()
        peer["os"] = guest.run("/usr/bin/cat", "/etc/os-release")["out"]
        peer["default_target"] = guest.run("/usr/bin/systemctl", "get-default")["out"].strip()
        peer["memory"] = guest.run("/usr/bin/free", "-k")["out"]
    # These legacy wire scenarios are explicitly two-party fixtures; node cases
    # use every independent guest below rather than constraining the backend.
    primary = PEERS[:2]
    for own, other in [primary, primary[::-1]]:
        Guest(own["name"]).upload(own["certificate"], Guest(other["name"]).public_certificate(other["identity"]))
    state = {"run_id": run_id, "binary_sha256": expected, "peers": PEERS}
    STATE.parent.mkdir(mode=0o700, exist_ok=True)
    (STATE.parent / f"kk-state-{run_id}.json").write_text(json.dumps(state, indent=2) + "\n")
    STATE.write_text(json.dumps(state, indent=2) + "\n")
    print(f"Isolated guest state: {STATE}", flush=True)


def shape(peer, options):
    guest = Guest(peer["name"])
    device = peer["device"]
    existing = json.loads(guest.run("tc", "-j", "qdisc", "show", "dev", device)["out"])
    if options:
        guest.run("tc", "qdisc", "replace", "dev", device, "root", "netem", "limit", "1000", *options)
    elif any(item.get("kind") == "netem" for item in existing):
        guest.run("tc", "qdisc", "del", "dev", device, "root")


def listening(guest):
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if guest.run("ss", "-ulH", f"sport = :{PORT}")["out"].strip():
            return
        time.sleep(0.1)
    raise TimeoutError(f"{guest.name}: receiver did not bind UDP {PORT}")


def exchange(receiver, sender, label, expected=True, wrong_server=False, wrong_client=False, payload=None):
    a, b = Guest(receiver["name"]), Guest(sender["name"])
    text = "K&K guest exchange: " + label if payload is None else payload
    timeout = "3" if not expected else "15"
    expected_client = receiver.get("wrong_certificate") if wrong_client else receiver["certificate"]
    pid = a.start("/usr/local/bin/kk", "receive", "--bind", f"{receiver['ip']}:{PORT}",
                  "--identity", receiver["identity"], "--peer", expected_client, "--timeout", timeout)
    listening(a)
    started = time.monotonic()
    peer_certificate = sender.get("wrong_certificate") if wrong_server else sender["certificate"]
    sent = b.run("/usr/local/bin/kk", "send", f"{receiver['ip']}:{PORT}",
                 "--identity", sender["identity"], "--peer", peer_certificate,
                 text, "--timeout", timeout, check=False, timeout=25)
    received = a.wait(pid, timeout=25)
    if expected:
        assert sent["code"] == 0 and received["code"] == 0, (sent, received)
        assert sent["out"] == f"ACK {len(text.encode())} bytes\n", sent
        assert received["out"] == text + "\n", received
    else:
        assert sent["code"] == 1 and received["code"] == 1, (sent, received)
        assert not sent["out"] and not received["out"], (sent, received)
        if wrong_server:
            assert "AUTHENTICATION:" in sent["err"], sent
        elif wrong_client:
            assert "AUTHENTICATION:" in received["err"], received
        else:
            assert "TIMEOUT:" in sent["err"], sent
    return {"scenario": label, "receiver": a.name, "sender": b.name,
            "success_expected": expected, "sender_result": sent, "receiver_result": received,
            "harness_elapsed_seconds": round(time.monotonic() - started, 3)}


def discovery_cases(receiver, sender, run_id):
    a, b = Guest(receiver["name"]), Guest(sender["name"])
    label = "kk-" + run_id

    def advertise():
        pid = a.start("/usr/local/bin/kk", "receive", "--bind", f"{receiver['ip']}:{PORT}",
                      "--identity", receiver["identity"], "--peer", receiver["certificate"],
                      "--advertise", label, "--timeout", "15")
        listening(a)
        return pid

    def deliver(pid, name):
        text = "Discovery still requires the pinned certificate"
        sent = b.run("/usr/local/bin/kk", "send", f"{receiver['ip']}:{PORT}",
                     "--identity", sender["identity"], "--peer", sender["certificate"],
                     text, "--timeout", "10")
        received = a.wait(pid)
        assert received["code"] == 0 and received["out"] == text + "\n", received
        return {"scenario": name, "sender_result": sent, "receiver_result": received}

    pid = advertise()
    candidates = b.run("/usr/local/bin/kk", "peers", "--interface", sender["ip"], "--timeout", "3")
    assert "UNVERIFIED" in candidates["out"] and label in candidates["out"], candidates
    assert f"{receiver['ip']}:{PORT}" in candidates["out"] and receiver["fingerprint"] in candidates["out"], candidates
    result = deliver(pid, "discovery-unverified")
    result["candidates"] = candidates
    gone = b.run("/usr/local/bin/kk", "peers", "--interface", sender["ip"], "--timeout", "2")
    assert label not in gone["out"], gone
    results = [result, {"scenario": "discovery-withdrawal", "browse": gone}]

    installed = []
    try:
        for peer in [receiver, sender]:
            guest = Guest(peer["name"])
            guest.run("tc", "qdisc", "add", "dev", peer["device"], "clsact")
            installed.append(peer)
            guest.run("tc", "filter", "add", "dev", peer["device"], "ingress", "protocol", "ip",
                      "flower", "ip_proto", "udp", "dst_port", "5353", "action", "drop")
        pid = advertise()
        blocked = b.run("/usr/local/bin/kk", "peers", "--interface", sender["ip"], "--timeout", "2")
        assert label not in blocked["out"], blocked
        result = deliver(pid, "discovery-blocked-explicit-fallback")
        result["browse"] = blocked
        results.append(result)
    finally:
        for peer in installed:
            Guest(peer["name"]).run("tc", "qdisc", "del", "dev", peer["device"], "clsact")
    return results


def verified_state():
    state = json.loads(STATE.read_text())
    for peer in state["peers"]:
        installed_hash = Guest(peer["name"]).run("sha256sum", "/usr/local/bin/kk")["out"].split()[0]
        if installed_hash != state["binary_sha256"]:
            raise RuntimeError(f"{peer['name']}: deployed K&K binary hash changed")
    return state


def report(state, results, **evidence):
    output = ROOT / "tmp" / f"kk-results-{state['run_id']}-{time.time_ns()}.json"
    output.write_text(json.dumps({**state, "profiles": PROFILES, "results": results, **evidence}, indent=2) + "\n")
    print(f"Results: {output}", flush=True)


def nodes_only():
    state = verified_state()
    results = []
    try:
        node_cases(state["peers"], shape, PORT, results)
    finally:
        report(state, results, scope="persistent-nodes-only")
    print(f"PASS: {len(results)} persistent-node scenarios", flush=True)


def exercise():
    state = verified_state()
    peers = state["peers"]
    primary = peers[:2]
    results = []
    capture_guest = Guest(peers[0]["name"])
    trace = peers[0]["identity"] + ".pcap"
    capture_pid = capture_guest.start("tcpdump", "-U", "-i", peers[0]["device"], "-n", "-w", trace,
                                      "udp", "port", str(PORT))
    capture = {}
    try:
        for name, options in PROFILES.items():
            for peer in peers:
                shape(peer, options)
            print(f"Scenario {name}: {options}", flush=True)
            results.append(exchange(*primary, name, expected=name != "blackhole"))
        for peer in peers:
            shape(peer, [])
        results.append(exchange(*primary[::-1], "reverse"))
        results.append(exchange(*primary, "empty", payload=""))
        results.append(exchange(*primary, "unicode", payload="Bonjour, \u00e9change s\u00e9curis\u00e9\nEntre deux syst\u00e8mes"))
        results.append(exchange(*primary, "maximum-size", payload="x" * 65_536))
        receiver, sender = primary
        rogue = sender["identity"] + "-untrusted-" + uuid.uuid4().hex[:6]
        Guest(sender["name"]).run("/usr/local/bin/kk", "identity", "create", rogue)
        sender["wrong_certificate"] = sender["certificate"] + ".wrong"
        Guest(sender["name"]).upload(sender["wrong_certificate"], Guest(sender["name"]).public_certificate(rogue))
        results.append(exchange(receiver, sender, "wrong-server", expected=False, wrong_server=True))
        receiver["wrong_certificate"] = receiver["certificate"] + ".wrong"
        Guest(receiver["name"]).upload(receiver["wrong_certificate"], Guest(sender["name"]).public_certificate(rogue))
        results.append(exchange(receiver, sender, "wrong-client", expected=False, wrong_client=True))
        results.extend(discovery_cases(receiver, sender, state["run_id"]))
        file_cases(primary, shape, listening, PORT, results)
        sensitive_cases(primary, listening, PORT, results)
        tty_cases(primary, listening, PORT, results)
        pair_cases(primary, shape, listening, PORT, results)
        node_cases(peers, shape, PORT, results)
    finally:
        for peer in peers:
            shape(peer, [])
        capture_guest.run("kill", "-INT", str(capture_pid))
        capture["process"] = capture_guest.wait(capture_pid)
        capture["packets"] = capture_guest.run("tcpdump", "-nn", "-r", trace)["out"]
        sources = " or ".join("src host " + peer["ip"] for peer in peers)
        destinations = " or ".join("dst host " + peer["ip"] for peer in peers)
        capture["outside_declared_guests"] = capture_guest.run(
            "tcpdump", "-nn", "-r", trace,
            f"not (({sources}) and ({destinations}))")["out"]
        report(state, results, capture=capture)
    assert capture["packets"] and not capture["outside_declared_guests"], capture
    print(f"PASS: {len(results)} multi-guest scenarios", flush=True)


def start():
    for peer in PEERS:
        state = virsh("domstate", peer["name"]).strip()
        if state == "shut off":
            print(virsh("start", peer["name"]), flush=True)
        elif state != "running":
            raise RuntimeError(f"{peer['name']}: unexpected power state {state}")
    deadline = time.monotonic() + 90
    pending = {peer["name"] for peer in PEERS}
    while pending and time.monotonic() < deadline:
        for name in list(pending):
            try:
                guest = Guest(name)
                guest.agent("guest-ping")
                if guest.run("/usr/bin/systemctl", "is-active", "NetworkManager", check=False)["code"] == 0:
                    pending.remove(name)
            except RuntimeError:
                # Boot readiness only: no operation has been submitted yet.
                pass
        if pending:
            time.sleep(1)
    if pending:
        raise TimeoutError(f"Guest agents unavailable after boot: {sorted(pending)}")


def stop():
    shutdown_guests([peer["name"] for peer in PEERS])
    saved = Path(tempfile.mkdtemp(prefix="kk-headless-backup-"))
    for peer in PEERS:
        original = virsh("dumpxml", peer["name"], "--inactive")
        (saved / f"{peer['name']}.xml.bak").write_text(original)
        tree = ET.fromstring(original)
        devices = tree.find("devices")
        for device in list(devices):
            if device.tag in {"graphics", "video", "sound", "audio", "redirdev", "input"} or (
                device.tag == "disk" and device.get("device") == "cdrom"
            ) or (
                device.tag == "channel" and device.get("type") in {"spicevmc", "spiceport"}
            ):
                devices.remove(device)
        definition = ET.tostring(tree, encoding="unicode")
        virsh("define", "/dev/stdin", input=definition)
        (ROOT / "xml" / f"{peer['name']}.xml").write_text(virsh("dumpxml", peer["name"], "--inactive"))
    if "kk-isolated" in virsh("net-list", "--name").splitlines():
        virsh("net-destroy", "kk-isolated")
    print(f"Guests stopped; headless definitions saved; previous definitions: {saved}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["start", "setup", "exercise", "nodes", "stop"])
    action = parser.parse_args().action
    if action == "start":
        start()
    elif action == "setup":
        setup()
    elif action == "exercise":
        exercise()
    elif action == "nodes":
        nodes_only()
    else:
        stop()
