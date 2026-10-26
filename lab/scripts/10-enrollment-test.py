#!/usr/bin/env -S uv run --script
"""Actual peer enrollment: no host-copied root/grant/package/control bootstrap."""
import base64
import hashlib
import importlib.util
import json
import subprocess
import sys
import time
import uuid
from pathlib import Path

from kk_guest import Guest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("kk_private_room_lab", ROOT / "scripts/09-room-test.py")
PRIVATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PRIVATE)
CONTROL = PRIVATE.CONTROL
HELPER = "/usr/local/bin/kk-room-fixture"
PORT = 45461


def main():
    subprocess.run(["cargo", "build", "--offline", "--locked", "--release", "--jobs", "2", "--example", "control-node-fixture",
                    "--manifest-path", str(ROOT.parent / "Cargo.toml")], check=True)
    binary = (ROOT.parent / "target/release/examples/control-node-fixture").read_bytes()
    digest = hashlib.sha256(binary).hexdigest()
    roster = json.loads((ROOT / "tmp/kk-state.json").read_text())["peers"]
    if len(roster) != 3:
        raise RuntimeError("Forwarded enrollment gate requires three independent guests")
    nodes = [{**peer, "guest": Guest(peer["name"])} for peer in roster]
    run = uuid.uuid4().hex[:12]
    work = "/var/tmp/kk-room-" + run
    active, outcomes, configs = {}, [], [None, None, None]
    consent_pins = {}
    candidates = {}

    def bootstrap(index, command, background=False):
        path = work + "/bootstrap-" + uuid.uuid4().hex + ".json"
        CONTROL.upload(nodes[index]["guest"], path, json.dumps(command).encode())
        return nodes[index]["guest"].start(HELPER, path) if background else nodes[index]["guest"].run(HELPER, path, timeout=30)

    def read_json(index, path):
        if nodes[index]["guest"].run("test", "-f", path, check=False)["code"]:
            return None
        return json.loads(CONTROL.public(nodes[index]["guest"], path))

    def status(index):
        data = read_json(index, configs[index]["status"])
        return data if data and data["pid"] == active.get(index) else None

    def ready(index):
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if status(index):
                return
            result = nodes[index]["guest"].agent("guest-exec-status", pid=active[index])
            if result.get("exited"):
                active.pop(index)
                error = base64.b64decode(result.get("err-data", "")).decode(errors="replace")
                raise RuntimeError(f"{nodes[index]['name']}: enrollment foreground exited: {error}")
            time.sleep(0.1)
        raise TimeoutError("Enrollment foreground readiness exceeded its bound")

    def start(index, enrollment=None):
        active[index] = bootstrap(index, {"action": "serve-room", "config": configs[index], "private_state": work + "/private", "profile": work + "/profile",
                                         "commands": work + "/requests", "member": True, "enrollment": enrollment}, True)
        ready(index)

    def act(index, command, check=True):
        current = status(index)
        if not current:
            raise RuntimeError("Enrollment actor is not ready")
        response = PRIVATE.enqueue(nodes[index]["guest"], work + "/requests", current["instance"], command)
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            result = read_json(index, response)
            if result is not None:
                if not result["ok"] and check:
                    raise RuntimeError("Enrollment action failed: " + str(result))
                return result["value"] if result["ok"] else result
            time.sleep(0.1)
        raise TimeoutError("Enrollment action delivery uncertain")

    def stop(index):
        act(index, {"action": "stop"})
        result = nodes[index]["guest"].wait(active.pop(index), timeout=25)
        if result["code"]:
            raise RuntimeError("Enrollment stop failed: " + str(result))

    def stop_candidate(index):
        pid = candidates.pop(index)
        nodes[index]["guest"].run("kill", "-INT", str(pid), check=False)
        result = nodes[index]["guest"].wait(pid, timeout=15)
        if result["code"] not in (0, 130):
            raise RuntimeError("Enrollment candidate cleanup failed: " + str(result))

    def passed(scenario, **evidence):
        print("Cross-guest NETWORK ENROLLMENT: " + scenario, flush=True)
        outcomes.append({"scenario": scenario, **evidence})

    def wait_voters(indices, expected):
        deadline = time.monotonic() + 30
        seen = None
        while time.monotonic() < deadline:
            seen = [status(index) for index in indices]
            if all(node and node["joint"] == 1 and set(node["voters"]) == set(expected) for node in seen):
                return seen
            time.sleep(0.1)
        raise TimeoutError("Uniform controller configuration did not converge: " + str(seen))

    def enroll(source_index, target_index, role):
        offer = "/var/tmp/kk-room-" + uuid.uuid4().hex[:12]
        guest = nodes[source_index]["guest"]
        guest.run("mkdir", "-m", "0700", offer)
        consent_pins[source_index, target_index] = offer + "/recipient.der"
        CONTROL.upload(guest, offer + "/recipient.der", public[target_index])
        if source_index in active:
            stop(source_index)
            configs[source_index]["initialize"] = False
            configs[source_index]["resume"] = True
        start(source_index, {"listen": f"{nodes[source_index]['ip']}:{PORT + 1}", "expected_certificate": offer + "/recipient.der",
                             "invite": offer + "/scoped-invitation.json", "status": offer + "/invite-status.json", "result": offer + "/admission.json", "role": role})
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            available = read_json(source_index, offer + "/invite-status.json")
            if available and available["pid"] == active[source_index]:
                break
            time.sleep(0.1)
        else:
            raise TimeoutError("Enrollment invitation did not become ready")
        invitation = guest.room_invitation(offer + "/scoped-invitation.json")
        CONTROL.upload(nodes[target_index]["guest"], work + "/scoped-invitation.json", invitation)
        del invitation
        candidates[target_index] = bootstrap(target_index, {"action": "enroll-candidate", "config": {
            "identity": nodes[target_index]["identity"], "invite": work + "/scoped-invitation.json", "role": role, "private_state": work + "/private",
            "control_state": work + "/control-db", "profile": work + "/profile", "listen": f"{nodes[target_index]['ip']}:{PORT}", "status": work + "/status.json",
            "config_out": work + "/node.json", "result": work + "/joined.json"}}, True)
        result = nodes[target_index]["guest"].wait(candidates[target_index], timeout=110)
        candidates.pop(target_index)
        if result["code"]:
            raise RuntimeError("Network enrollment failed: " + str(result))
        receipt = read_json(target_index, work + "/joined.json")
        deadline = time.monotonic() + 20
        source = None
        while time.monotonic() < deadline:
            source = read_json(source_index, offer + "/admission.json")
            if source is not None:
                break
            time.sleep(0.1)
        if source is None:
            raise TimeoutError("Source admission result did not become observable")
        assert receipt["ok"] and source["ok"]
        assert receipt["room"] == source["admission"]["room"] and receipt["admission"] == source["admission"]["admission"]
        assert receipt["voters"] == [status(0)["id"]] and receipt["member"] not in receipt["voters"]
        configs[target_index] = read_json(target_index, work + "/node.json")
        start(target_index)
        return {"recipient": receipt, "source": source}

    def exchange(source_index, target_index, fixture, size=128):
        before = status(target_index)["received"]
        path = consent_pins.get((source_index, target_index))
        if path is None:
            path = next(peer["certificate"] for peer in configs[source_index]["peers"] if peer["endpoint"] == f"{nodes[target_index]['ip']}:{PORT}")
        answer = act(source_index, {"action": "send", "target_certificate": path, "fixture": fixture, "bytes": size, "sensitive": True})
        prefix = f"kk-room-fixture-{fixture}:"
        expected = hashlib.sha256((prefix + "x" * max(0, size - len(prefix)))[:size].encode()).hexdigest()
        assert answer["sha256"] == expected
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            received = status(target_index)
            if received["received"] > before:
                assert received["received"] == before + 1
                assert received["latest"]["sha256"] == expected and received["latest"]["sensitive"] is True
                return {"sent": answer, "received": received["latest"]}
            time.sleep(0.1)
        raise TimeoutError("Enrolled private event did not reach its sink")

    try:
        public = [node["guest"].public_certificate(node["identity"]) for node in nodes]
        for index, node in enumerate(nodes):
            guest = node["guest"]
            guest.upload(HELPER, binary)
            guest.run("chmod", "0755", HELPER)
            if guest.run("test", "-x", "/usr/sbin/restorecon", check=False)["code"] == 0:
                guest.run("/usr/sbin/restorecon", HELPER)
            assert guest.run("sha256sum", HELPER)["out"].split()[0] == digest
            guest.run("mkdir", "-m", "0700", work)
        # Source consent pins the recipient certificate, NOT a vote/controller.
        CONTROL.upload(nodes[0]["guest"], work + "/own.der", public[0])
        bootstrap(0, {"action": "genesis-room", "identity": nodes[0]["identity"], "state": work + "/private", "out": work + "/genesis.json"})
        configs[0] = {"context": {"identity": nodes[0]["identity"], "genesis": work + "/genesis.json", "creator_certificate": work + "/own.der"},
                      "peers": [{"certificate": work + "/own.der", "endpoint": f"{nodes[0]['ip']}:{PORT}"}], "listen": f"{nodes[0]['ip']}:{PORT}",
                      "state": work + "/control-db", "status": work + "/status.json", "initialize": True, "resume": False}
        passed("authority-grants-package-and-replication-over-proved-network-session", **enroll(0, 1, "moderator"))
        assert all(status(index)["profile_contacts"] == 0 for index in [0, 1])
        passed("learner-reopens-private-state-without-implicit-vote-or-global-contacts", status=status(1))
        # Locate only already-approved public certificates by full fingerprint.
        # Public pins originate from enrollment, not host-provided authority.
        for source_index, target_index, fixture, size in [(0, 1, 1, 128), (1, 0, 2, 65536), (0, 1, 3, 0)]:
            passed("enrolled-private-data-" + str(fixture), **exchange(source_index, target_index, fixture, size))
        assert status(1)["leader_hint"] == status(0)["id"] and status(1)["id"] != status(0)["id"]
        passed("nonleader-moderator-forwards-proved-enrollment", **enroll(1, 2, "participant"))
        assert all(status(index)["profile_contacts"] == 0 for index in [0, 1, 2])
        passed("new-peer-reaches-nonleader-inviter", **exchange(2, 1, 4))
        passed("nonleader-inviter-reaches-new-peer", **exchange(1, 2, 5))
        stop(1)
        start(1)  # Same originally received a/b config, not a host-provided C pin.
        passed("cold-scoped-controller-certificates-recover-without-host-pins", **exchange(2, 1, 6))
        all_ids = [status(index)["id"] for index in [0, 1, 2]]
        denied = act(2, {"action": "controllers", "voters": all_ids}, False)
        assert not denied["ok"]
        approved = act(1, {"action": "controllers", "voters": all_ids})
        passed("explicit-signed-controller-approval-from-nonleader", approval=approved, configuration=wait_voters([0, 1, 2], all_ids), participant_refusal=denied)
        denied = act(2, {"action": "controllers", "voters": all_ids}, False)
        assert not denied["ok"]  # A vote does not create moderator application rights.
        stop(0)
        CONTROL.power(nodes[0]["name"], "shutdown")
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if all(status(index)["leader_hint"] and status(index)["leader_hint"] != all_ids[0] for index in [1, 2]):
                break
            time.sleep(0.1)
        else:
            raise TimeoutError("Surviving elected controller did not replace the creator")
        passed("dynamic-voter-majority-continues-with-creator-vm-off", **exchange(1, 2, 7))
        changed = act(1, {"action": "controllers", "voters": all_ids[1:]})
        passed("joint-to-uniform-controller-retirement-without-creator", approval=changed, configuration=wait_voters([1, 2], all_ids[1:]))
        act(1, {"action": "remove", "subject_certificate": next(peer["certificate"] for peer in configs[1]["peers"] if peer["endpoint"] == f"{nodes[0]['ip']}:{PORT}")})
        passed("delegated-data-removal-after-controller-retirement", **exchange(2, 1, 8))
        CONTROL.power(nodes[0]["name"], "start")
        configs[0]["initialize"] = False
        configs[0]["resume"] = True
        start(0)
        refused = act(0, {"action": "sync"}, False)
        assert not refused["ok"] and status(0)["private"]["active"] is False
        passed("retired-creator-cold-learner-cannot-reactivate-data", status=status(0), refusal=refused, configuration=wait_voters([0, 1, 2], all_ids[1:]))
    finally:
        primary = sys.exception()
        errors = CONTROL.cleanup(active, stop)
        errors.extend(CONTROL.cleanup(candidates, stop_candidate))
        report = ROOT / "tmp" / f"network-enrollment-{run}.json"
        report.write_text(json.dumps({"scope": "Three independent guests; network-only enrollment; no implicit votes; explicit signed controller selection, actual creator-VM-off majority, joint transition and cold retired creator; NOT Byzantine-consensus or physical-radio proof",
                                      "run_id": run, "helper_sha256": digest, "roster": [{"name": node["name"], "ip": node["ip"]} for node in nodes],
                                      "results": outcomes, "failure": str(primary) if primary else None, "cleanup_errors": [str(error) for error in errors]}, indent=2) + "\n")
        print("Evidence: " + str(report), flush=True)
        if errors:
            if primary is None:
                raise ExceptionGroup("Enrollment teardown failed", errors)
            for error in errors:
                primary.add_note("Teardown: " + str(error))
    print(f"PASS: {len(outcomes)} network enrollment scenarios", flush=True)


if __name__ == "__main__":
    main()
