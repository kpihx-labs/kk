#!/usr/bin/env -S uv run --script
"""Real private room VM gate; explicit preapproved bootstrap, NOT scoped PAKE."""
import importlib.util
import base64
import hashlib
import json
import subprocess
import sys
import time
import uuid
from pathlib import Path

from kk_guest import Guest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("kk_public_control_lab", ROOT / "scripts/08-control-test.py")
CONTROL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTROL)
HELPER = "/usr/local/bin/kk-room-fixture"
PORT = 45461


def enqueue(guest, directory, instance, command):
    nonce = uuid.uuid4().hex
    request = directory + f"/request-{nonce}.json"
    staging = directory + f"/pending-{nonce}.json"
    response = directory + f"/result-{nonce}.json"
    # Existence means complete input, never touch/chmod/partial-upload progress.
    CONTROL.upload(guest, staging, json.dumps({"instance": instance, "command": command}).encode())
    guest.run("mv", "-n", staging, request)
    return response


def main():
    subprocess.run(["cargo", "build", "--offline", "--locked", "--release", "--jobs", "2", "--example", "control-node-fixture",
                    "--manifest-path", str(ROOT.parent / "Cargo.toml")], check=True)
    binary = (ROOT.parent / "target/release/examples/control-node-fixture").read_bytes()
    digest = hashlib.sha256(binary).hexdigest()
    roster = json.loads((ROOT / "tmp/kk-state.json").read_text())["peers"]
    if len(roster) != 3:
        raise RuntimeError("Private room fault gate requires exactly three declared guests")
    nodes = [{**peer, "guest": Guest(peer["name"])} for peer in roster]
    run = uuid.uuid4().hex[:12]
    work = "/var/tmp/kk-room-" + run
    configs, contexts = [], []
    active, outcomes = {}, []
    ceremonies = {}
    joined = [True, False, False]

    def bootstrap(index, payload, background=False):
        path = work + "/bootstrap-" + uuid.uuid4().hex + ".json"
        CONTROL.upload(nodes[index]["guest"], path, json.dumps(payload).encode())
        if background:
            return nodes[index]["guest"].start(HELPER, path)
        return nodes[index]["guest"].run(HELPER, path, timeout=25)

    def read_json(index, path):
        guest = nodes[index]["guest"]
        if guest.run("test", "-f", path, check=False)["code"]:
            return None
        return json.loads(CONTROL.public(guest, path))

    def status(index):
        result = read_json(index, configs[index]["status"])
        return result if result and result["pid"] == active.get(index) else None

    def wait_ready(index, path, owned):
        pid = owned[index]
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            current = read_json(index, path)
            if current and current["pid"] == pid:
                return current
            execution = nodes[index]["guest"].agent("guest-exec-status", pid=pid)
            if execution.get("exited"):
                owned.pop(index)
                result = {"code": execution.get("exitcode", 1),
                          "out": base64.b64decode(execution.get("out-data", "")).decode(errors="replace"),
                          "err": base64.b64decode(execution.get("err-data", "")).decode(errors="replace")}
                raise RuntimeError(f"Private fixture exited before readiness: {result}")
            time.sleep(0.1)
        raise TimeoutError(f"{nodes[index]['name']}: private fixture did not publish readiness")

    def start(index, resume=False):
        config = {**configs[index], "resume": resume, "initialize": index == 0 and not resume}
        active[index] = bootstrap(index, {"action": "serve-room", "config": config, "private_state": work + "/private",
                                         "profile": work + "/profile", "commands": work + "/requests", "member": joined[index]}, True)
        wait_ready(index, configs[index]["status"], active)

    def stop_ceremony(index):
        pid = ceremonies.pop(index)
        nodes[index]["guest"].run("kill", "-INT", str(pid), check=False)
        result = nodes[index]["guest"].wait(pid, timeout=15)
        if result["code"] not in (0, 130):
            raise RuntimeError("Scoped ceremony teardown failed: " + str(result))

    def scope_pair():
        invite_path = work + "/scoped-invitation.json"
        ceremonies[0] = bootstrap(0, {"action": "scope-listen", "config": {
            "context": contexts[0], "listen": f"{nodes[0]['ip']}:{PORT + 1}", "expected_certificate": work + "/cert-1.der",
            "invite": invite_path, "approval": work + "/scope-approved.json", "result": work + "/scope-complete.json",
            "status": work + "/scope-status.json", "data_port": PORT}}, True)
        wait_ready(0, work + "/scope-status.json", ceremonies)
        # Intended out-of-band delivery of an EPHEMERAL invitation, not a device
        # or MLS key export. Keep the opaque bundle out of logs/report fields.
        invitation = nodes[0]["guest"].room_invitation(invite_path)
        CONTROL.upload(nodes[1]["guest"], invite_path, invitation)
        del invitation
        ceremonies[1] = bootstrap(1, {"action": "scope-join", "config": {
            "context": contexts[1], "expected_certificate": work + "/cert-0.der", "invite": invite_path,
            "approval": work + "/scope-approved.json", "result": work + "/scope-complete.json", "data_port": PORT}}, True)
        for index in [1, 0]:
            result = nodes[index]["guest"].wait(ceremonies[index], timeout=45)
            ceremonies.pop(index)
            if result["code"]:
                raise RuntimeError("Scoped ceremony failed: " + str(result))
        results = [read_json(index, work + "/scope-complete.json") for index in [0, 1]]
        assert all(result["kind"] == "scoped-bootstrap-only" for result in results)
        assert results[0]["room"] == results[1]["room"]
        return results

    def act(index, command, check=True):
        current = status(index)
        if not current:
            raise RuntimeError("Private fixture process is not ready")
        response = enqueue(nodes[index]["guest"], work + "/requests", current["instance"], command)
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            result = read_json(index, response)
            if result is not None:
                if check and not result["ok"]:
                    raise RuntimeError(f"{nodes[index]['name']}: {command['action']}: {result}")
                return result
            time.sleep(0.1)
        raise TimeoutError(f"{nodes[index]['name']}: {command['action']} result uncertain")

    def stop(index, hard=False):
        if hard:
            pid = active.pop(index)
            nodes[index]["guest"].run("kill", "-KILL", str(pid))
            return nodes[index]["guest"].wait(pid, timeout=25)
        act(index, {"action": "stop"})
        result = nodes[index]["guest"].wait(active.pop(index), timeout=25)
        if result["code"]:
            raise RuntimeError("Private foreground cleanup failed: " + str(result))
        return result

    def passed(scenario, **evidence):
        print("Cross-guest PRIVATE room: " + scenario, flush=True)
        outcomes.append({"scenario": scenario, **evidence})

    def wait_public(indices, epoch):
        deadline = time.monotonic() + 30
        seen = []
        while time.monotonic() < deadline:
            seen = [status(index) for index in indices]
            if all(item and item["epoch"] == epoch and item["leader_hint"] for item in seen):
                if len({tuple(item["head"]) for item in seen}) == 1:
                    return seen
            time.sleep(0.1)
        raise TimeoutError(f"Private fixture public convergence epoch {epoch}: {seen}")

    def send(sender, receiver, fixture, sensitive=False, size=128, capture=False):
        before = status(receiver)["received"]
        answer = act(sender, {"action": "send", "target_certificate": work + f"/cert-{receiver}.der",
                              "fixture": fixture, "bytes": size, "sensitive": sensitive, "capture": capture})["value"]
        prefix = f"kk-room-fixture-{fixture}:"
        expected = (prefix + "x" * max(0, size - len(prefix)))[:size].encode()
        expected_hash = hashlib.sha256(expected).hexdigest()
        assert answer["sha256"] == expected_hash and answer["bytes"] == size
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            received = status(receiver)
            if received["received"] > before:
                event = received["latest"]
                assert received["received"] == before + 1
                assert event["sha256"] == expected_hash and event["bytes"] == size and event["sensitive"] == sensitive
                return {"sender": nodes[sender]["name"], "receiver": nodes[receiver]["name"], "sent": answer, "received": event}
            time.sleep(0.1)
        raise TimeoutError("Ciphertext ACK did not reach the observable private event sink")

    try:
        certificates = [node["guest"].public_certificate(node["identity"]) for node in nodes]
        for index, node in enumerate(nodes):
            guest = node["guest"]
            guest.upload(HELPER, binary)
            guest.run("chmod", "0755", HELPER)
            if guest.run("test", "-x", "/usr/sbin/restorecon", check=False)["code"] == 0:
                guest.run("/usr/sbin/restorecon", HELPER)
            assert guest.run("sha256sum", HELPER)["out"].split()[0] == digest
            guest.run("mkdir", "-m", "0700", work)
            for number, certificate in enumerate(certificates):
                CONTROL.upload(guest, work + f"/cert-{number}.der", certificate)
            contexts.append({"identity": node["identity"], "genesis": work + "/genesis.json", "creator_certificate": work + "/cert-0.der"})
            configs.append({"context": contexts[-1], "peers": [{"certificate": work + f"/cert-{number}.der", "endpoint": f"{peer['ip']}:{PORT}"}
                                                               for number, peer in enumerate(nodes)], "listen": f"{node['ip']}:{PORT}",
                            "state": work + "/control-db", "status": work + "/status.json", "initialize": False, "resume": False})
        bootstrap(0, {"action": "genesis-room", "identity": nodes[0]["identity"], "state": work + "/private", "out": work + "/genesis.json"})
        genesis = CONTROL.public(nodes[0]["guest"], work + "/genesis.json")
        for index in [1, 2]:
            CONTROL.upload(nodes[index]["guest"], work + "/genesis.json", genesis)
        scoped = scope_pair()
        CONTROL.upload(nodes[0]["guest"], work + "/empty.json", b"[]")
        bootstrap(0, {"action": "grant", "context": contexts[0], "subject_certificate": work + "/cert-1.der", "parents": work + "/empty.json",
                      "role": "moderator", "out": work + "/bob-chain.json"})
        CONTROL.upload(nodes[1]["guest"], work + "/bob-chain.json", CONTROL.public(nodes[0]["guest"], work + "/bob-chain.json"))
        bootstrap(1, {"action": "package", "context": contexts[1], "authority": work + "/bob-chain.json", "state": work + "/private", "out": work + "/bob-package.bin"})
        CONTROL.upload(nodes[0]["guest"], work + "/bob-package.bin", CONTROL.public(nodes[1]["guest"], work + "/bob-package.bin"))
        for index in [1, 2, 0]:
            start(index)
        wait_public([0, 1, 2], 0)
        assert all(status(index)["profile_contacts"] == 0 for index in [0, 1, 2])
        passed("room-scoped-pake-and-key-proof-without-global-contacts", approvals=scoped)
        admitted = act(0, {"action": "admit", "package": work + "/bob-package.bin"})
        activated = act(1, {"action": "join", "inviter_certificate": work + "/cert-0.der"})
        joined[1] = True
        passed("real-private-join-from-committed-replica", admission=admitted, activation=activated)
        passed("first-private-quic-edge", exchange=send(0, 1, 1, True))
        stop(0, hard=True)
        CONTROL.power(nodes[0]["name"], "shutdown")
        bootstrap(1, {"action": "grant", "context": contexts[1], "subject_certificate": work + "/cert-2.der", "parents": work + "/bob-chain.json",
                      "role": "participant", "out": work + "/charlie-chain.json"})
        CONTROL.upload(nodes[2]["guest"], work + "/charlie-chain.json", CONTROL.public(nodes[1]["guest"], work + "/charlie-chain.json"))
        bootstrap(2, {"action": "package", "context": contexts[2], "authority": work + "/charlie-chain.json", "state": work + "/private", "out": work + "/charlie-package.bin"})
        CONTROL.upload(nodes[1]["guest"], work + "/charlie-package.bin", CONTROL.public(nodes[2]["guest"], work + "/charlie-package.bin"))
        admitted = act(1, {"action": "admit", "package": work + "/charlie-package.bin"})
        activated = act(2, {"action": "join", "inviter_certificate": work + "/cert-1.der"})
        joined[2] = True
        passed("creator-vm-off-delegate-admits-with-own-keys", admission=admitted, activation=activated)
        passed("survivor-private-quic-both-directions", exchanges=[send(1, 2, 2), send(2, 1, 3, True, 65536), send(1, 2, 4, size=0)])
        act(1, {"action": "revoke", "subject_certificate": work + "/cert-0.der"})
        refused = act(1, {"action": "send", "target_certificate": work + "/cert-2.der", "fixture": 5, "bytes": 128, "sensitive": True}, False)
        assert not refused["ok"]
        act(1, {"action": "remove", "subject_certificate": work + "/cert-0.der"})
        passed("profile-veto-until-removed-new-epoch", refusal=refused,
               exchanges=[send(1, 2, 6, True), send(2, 1, 10, True, capture=True)])
        stop(1, hard=True)
        CONTROL.power(nodes[1]["name"], "shutdown")
        before = status(2)
        refused = act(2, {"action": "send", "target_certificate": work + "/cert-1.der", "fixture": 7, "bytes": 128, "sensitive": True}, False)
        assert not refused["ok"]
        passed("one-of-three-never-releases-new-data", refusal=refused, durable_private_before=before["private"])
        CONTROL.power(nodes[1]["name"], "start")
        start(1, True)
        passed("private-restart-preserves-epoch-and-survivor-links", exchange=send(2, 1, 8, True))
        passed("sender-generation-not-reused-after-private-restart", exchange=send(1, 2, 9, True))
        before_replay = status(1)["received"]
        replay = act(2, {"action": "replay-captured", "target_certificate": work + "/cert-1.der"}, False)
        assert not replay["ok"] and "deadline" not in replay["error"]
        assert status(1)["received"] == before_replay
        # A succeeding fresh frame also distinguishes replay refusal from a
        # dead link/profile refusal, and exposes any delayed duplicate event.
        passed("pre-crash-ciphertext-replay-refused-after-private-restart", refusal=replay, fresh=send(2, 1, 11, True))
        CONTROL.power(nodes[0]["name"], "start")
        start(0, True)
        refusal = act(0, {"action": "sync"}, False)
        assert not refusal["ok"]
        recovered = status(0)
        assert recovered["private"]["active"] is False and recovered["private"]["epoch"] == 3
        passed("removed-creator-cold-recovery-cannot-activate", status=recovered, refusal=refusal)
    finally:
        primary = sys.exception()
        errors = CONTROL.cleanup(active, stop)
        errors.extend(CONTROL.cleanup(ceremonies, stop_ceremony))
        report = ROOT / "tmp" / f"cross-guest-private-room-{run}.json"
        report.write_text(json.dumps({"scope": "Actual cross-VM private MLS/QUIC plus room-scoped PAKE/key possession; public authority/bootstrap and fixed control voters preapproved; NOT full scoped enrollment or dynamic voter proof",
                                      "run_id": run, "helper_sha256": digest, "roster": [{"name": node["name"], "ip": node["ip"]} for node in nodes],
                                      "results": outcomes, "failure": str(primary) if primary else None, "cleanup_errors": [str(error) for error in errors]}, indent=2) + "\n")
        print("Evidence: " + str(report), flush=True)
        if errors:
            if primary is None:
                raise ExceptionGroup("Private room teardown errors", errors)
            for error in errors:
                primary.add_note("Teardown: " + str(error))
    print(f"PASS: {len(outcomes)} actual cross-guest private room scenarios", flush=True)


if __name__ == "__main__":
    main()
