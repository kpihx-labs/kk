#!/usr/bin/env -S uv run --script
"""Actual cross-guest PUBLIC control service. Not private MLS room/data admission."""
import base64
import hashlib
import json
import subprocess
import sys
import time
import uuid
from pathlib import Path

from kk_guest import Guest, URI, declared_peers, shutdown_guests

ROOT = Path(__file__).resolve().parents[1]
HELPER = "/usr/local/bin/kk-control-fixture"
PORT = 45460


def public(guest, path):
    return base64.b64decode(guest.run("base64", "-w0", path)["out"])


def upload(guest, path, data):
    guest.run("touch", path)
    guest.run("chmod", "0600", path)
    guest.upload(path, data)


def power(name, action):
    if action == "shutdown":
        shutdown_guests([name])
        return
    peer = next(peer for peer in declared_peers() if peer["name"] == name)
    configured = False
    args = ["virsh", "-c", URI, action, name]
    subprocess.run(args, check=True)
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        state = subprocess.run(["virsh", "-c", URI, "domstate", name], check=True, text=True, stdout=subprocess.PIPE).stdout
        if "running" not in state:
            time.sleep(0.2)
            continue
        if action == "start":
            try:
                guest = Guest(name)
                guest.agent("guest-ping")
            except (subprocess.CalledProcessError, RuntimeError):
                # Guest.agent deliberately wraps virsh failures. During boot the
                # disconnected virtio agent is expected until this bounded gate.
                time.sleep(0.2)
                continue
            if not configured:
                # Setup assigns an intentionally volatile address with NM disabled.
                # A cold boot must restore it, not wait for a DHCP client we disabled.
                guest.configure_isolated_network(peer)
                configured = True
            if any(address.get("ip-address") == peer["ip"]
                   for interface in guest.interfaces()
                   for address in interface.get("ip-addresses", [])):
                return
        time.sleep(0.2)
    raise TimeoutError(name + " did not finish " + action + " with its declared network address")


def cleanup(active, stop):
    """Attempt every teardown; preserve the primary failure and all diagnostics."""
    errors = []
    for index in list(active):
        try:
            stop(index)
        except Exception as error:
            errors.append(error)
    return errors


def main():
    subprocess.run(["cargo", "build", "--offline", "--locked", "--release", "--jobs", "2",
                    "--example", "control-node-fixture", "--manifest-path", str(ROOT.parent / "Cargo.toml")], check=True)
    data = (ROOT.parent / "target/release/examples/control-node-fixture").read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    state = json.loads((ROOT / "tmp/kk-state.json").read_text())
    peers = state["peers"]
    if len(peers) != 3:
        raise RuntimeError("This fault scenario requires the declared three independent guests")
    run = uuid.uuid4().hex[:12]
    work = "/var/tmp/kk-control-" + run
    nodes = [{**peer, "guest": Guest(peer["name"])} for peer in peers]
    active = {}
    outcomes = []
    configs = []
    contexts = []

    def command(node, payload, check=True, background=False):
        path = work + "/command-" + uuid.uuid4().hex + ".json"
        upload(node["guest"], path, json.dumps(payload).encode())
        if background:
            return node["guest"].start(HELPER, path)
        return node["guest"].run(HELPER, path, timeout=20, check=check)

    def read_status(index):
        node = nodes[index]
        if node["guest"].run("test", "-f", configs[index]["status"], check=False)["code"]:
            return None
        status = json.loads(public(node["guest"], configs[index]["status"]))
        if status["pid"] != active.get(index):
            return None
        return status

    def wait(indices, epoch, excluded=None):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            statuses = [read_status(index) for index in indices]
            if all(status and status["epoch"] == epoch and status["leader_hint"] and status["leader_hint"] != excluded for status in statuses):
                if len({status["leader_hint"] for status in statuses}) == 1 and len({tuple(status["head"]) for status in statuses}) == 1:
                    return statuses
            time.sleep(0.1)
        raise TimeoutError(f"Public control convergence failed at epoch {epoch}: {statuses}")

    def start(index, resume=False):
        config = {**configs[index], "resume": resume, "initialize": index == 0 and not resume}
        active[index] = command(nodes[index], {"action": "serve", "config": config}, background=True)
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            if read_status(index):
                return
            status = nodes[index]["guest"].agent("guest-exec-status", pid=active[index])
            if status.get("exited"):
                detail = base64.b64decode(status.get("err-data", "")).decode(errors="replace")
                active.pop(index)
                raise RuntimeError(f"{nodes[index]['name']}: control service exited before readiness: {detail}")
            time.sleep(0.1)
        raise TimeoutError(f"{nodes[index]['name']}: control service did not publish readiness")

    def stop(index, hard=False):
        pid = active.pop(index)
        nodes[index]["guest"].run("kill", "-KILL" if hard else "-INT", str(pid), check=False)
        result = nodes[index]["guest"].wait(pid, timeout=15)
        if not hard and result["code"] != 0:
            raise RuntimeError("Foreground control shutdown failed: " + str(result))
        return result

    def submit(index, proposal, check=True):
        out = work + "/result-" + uuid.uuid4().hex + ".json"
        result = command(nodes[index], {"action": "submit", "config": configs[index], "proposal": proposal, "out": out}, check=check)
        answer = None
        if nodes[index]["guest"].run("test", "-f", out, check=False)["code"] == 0:
            answer = json.loads(public(nodes[index]["guest"], out))
        return result, answer

    def passed(name, **evidence):
        print("Cross-guest control: " + name, flush=True)
        outcomes.append({"scenario": name, **evidence})

    try:
        certificates = [node["guest"].public_certificate(node["identity"]) for node in nodes]
        for index, node in enumerate(nodes):
            guest = node["guest"]
            guest.upload(HELPER, data)
            guest.run("chmod", "0755", HELPER)
            if guest.run("test", "-x", "/usr/sbin/restorecon", check=False)["code"] == 0:
                guest.run("/usr/sbin/restorecon", HELPER)
            assert guest.run("sha256sum", HELPER)["out"].split()[0] == digest
            guest.run("mkdir", "-m", "0700", work)
            for cert_index, certificate in enumerate(certificates):
                upload(guest, work + f"/cert-{cert_index}.der", certificate)
            contexts.append({"identity": node["identity"], "genesis": work + "/genesis.json", "creator_certificate": work + "/cert-0.der"})
            configs.append({"context": contexts[-1], "peers": [{"certificate": work + f"/cert-{i}.der", "endpoint": f"{other['ip']}:{PORT}"} for i, other in enumerate(nodes)],
                            "listen": f"{node['ip']}:{PORT}", "state": work + "/control-db", "status": work + "/status.json", "initialize": False, "resume": False})
        command(nodes[0], {"action": "genesis", "identity": nodes[0]["identity"], "out": work + "/genesis.json"})
        genesis = public(nodes[0]["guest"], work + "/genesis.json")
        for node in nodes[1:]:
            upload(node["guest"], work + "/genesis.json", genesis)
        upload(nodes[0]["guest"], work + "/empty.json", b"[]")
        command(nodes[0], {"action": "grant", "context": contexts[0], "subject_certificate": work + "/cert-1.der", "parents": work + "/empty.json", "role": "moderator", "out": work + "/bob-chain.json"})
        upload(nodes[1]["guest"], work + "/bob-chain.json", public(nodes[0]["guest"], work + "/bob-chain.json"))
        command(nodes[1], {"action": "package", "context": contexts[1], "authority": work + "/bob-chain.json", "state": work + "/candidate", "out": work + "/bob-package.bin"})
        upload(nodes[0]["guest"], work + "/bob-package.bin", public(nodes[1]["guest"], work + "/bob-package.bin"))
        start(1)
        start(2)
        start(0)
        initial = wait([0, 1, 2], 0)
        passed("three-independent-processes-elect", status=initial)
        add = work + "/add.bin"
        command(nodes[0], {"action": "add-proposal", "context": contexts[0], "status": configs[0]["status"], "package": work + "/bob-package.bin", "out": add})
        _, answer = submit(0, add)
        assert answer["Applied"]["accepted"] and not answer["Applied"]["replayed"]
        wait([0, 1, 2], 1)
        _, replay = submit(0, add)
        assert replay["Applied"]["accepted"] and replay["Applied"]["replayed"]
        passed("quorum-and-exact-retry", original=answer, replay=replay)
        creator_id = initial[0]["id"]
        stop(0, hard=True)
        power(nodes[0]["name"], "shutdown")
        survivors = wait([1, 2], 1, creator_id)
        passed("creator-vm-off-majority-survives", status=survivors)
        removal = work + "/remove-creator.bin"
        command(nodes[1], {"action": "remove-proposal", "context": contexts[1], "status": configs[1]["status"], "subject_certificate": work + "/cert-0.der", "out": removal})
        _, removed = submit(1, removal)
        assert removed["Applied"]["accepted"]
        wait([1, 2], 2, creator_id)
        passed("delegated-public-control-without-creator", receipt=removed)
        command(nodes[1], {"action": "grant", "context": contexts[1], "subject_certificate": work + "/cert-2.der", "parents": work + "/bob-chain.json", "role": "participant", "out": work + "/charlie-chain.json"})
        upload(nodes[2]["guest"], work + "/charlie-chain.json", public(nodes[1]["guest"], work + "/charlie-chain.json"))
        command(nodes[2], {"action": "package", "context": contexts[2], "authority": work + "/charlie-chain.json", "state": work + "/candidate", "out": work + "/charlie-package.bin"})
        upload(nodes[1]["guest"], work + "/charlie-package.bin", public(nodes[2]["guest"], work + "/charlie-package.bin"))
        add_charlie = work + "/add-charlie.bin"
        command(nodes[1], {"action": "add-proposal", "context": contexts[1], "status": configs[1]["status"], "package": work + "/charlie-package.bin", "out": add_charlie})
        stop(2, hard=True)
        power(nodes[2]["name"], "shutdown")
        failed, no_ack = submit(1, add_charlie, check=False)
        assert not (no_ack and no_ack.get("Applied", {}).get("accepted")), (failed, no_ack)
        assert read_status(1)["epoch"] == 2
        passed("one-of-three-never-acks", process=failed, response=no_ack)
        power(nodes[2]["name"], "start")
        start(2, resume=True)
        # The timed-out exact proposal may already commit on recovery. Reuse its
        # exact ID, never conclude rejection from timeout or create a new nonce.
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            status = read_status(1)
            if status and status["leader_hint"] and status["leader_hint"] != creator_id:
                break
            time.sleep(0.1)
        _, recovered = submit(1, add_charlie)
        assert recovered["Applied"]["accepted"]
        wait([1, 2], 3, creator_id)
        passed("restored-quorum-resolves-exact-uncertain-operation", receipt=recovered)
        power(nodes[0]["name"], "start")
        start(0, resume=True)
        final = wait([0, 1, 2], 3)
        passed("creator-cold-control-recovery-catches-up", status=final)
    finally:
        primary = sys.exception()
        errors = cleanup(active, stop)
        report = ROOT / "tmp" / f"cross-guest-control-{run}.json"
        report.write_text(json.dumps({"scope": "Actual cross-VM PUBLIC control; fixed explicit control voter trust; NOT private MLS admission/data", "helper_sha256": digest,
                                      "run_id": run, "roster": [{"name": node["name"], "ip": node["ip"]} for node in nodes], "results": outcomes,
                                      "failure": str(primary) if primary else None, "cleanup_errors": [str(error) for error in errors]}, indent=2) + "\n")
        print(f"Evidence: {report}", flush=True)
        if errors:
            if primary is None:
                raise ExceptionGroup("Control service teardown failures", errors)
            for error in errors:
                primary.add_note("Teardown: " + str(error))
    print(f"PASS: {len(outcomes)} actual cross-guest public-control scenarios", flush=True)


if __name__ == "__main__":
    main()
