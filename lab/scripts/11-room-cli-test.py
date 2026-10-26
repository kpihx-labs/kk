#!/usr/bin/env -S uv run --script
"""Exercise the public room CLI across three independent isolated guests."""
import hashlib
import json
import sys
import time
import uuid
from pathlib import Path

from kk_guest import Guest, selected_binary, shutdown_guests, virsh

ROOT = Path(__file__).resolve().parents[1]
CLI = "/usr/local/bin/kk"
PORT = 45462


def main():
    state = json.loads((ROOT / "tmp/kk-state.json").read_text())
    roster = state["peers"]
    if len(roster) != 3:
        raise RuntimeError("Room CLI gate requires exactly three independent guests")
    binary_hash = hashlib.sha256(selected_binary().read_bytes()).hexdigest()
    if binary_hash != state["binary_sha256"]:
        raise RuntimeError("Guest deployment does not match the selected release CLI")
    run = uuid.uuid4().hex[:12]
    work = "/var/tmp/kk-room-" + run
    profile = work + "/profile"
    nodes = [{**peer, "guest": Guest(peer["name"])} for peer in roster]
    active, results, received = {}, [], {}
    media_processes = {}

    def call(index, *args, check=True, data=None):
        guest = nodes[index]["guest"]
        pid = guest.start(CLI, "--profile", profile, *args, input_data=data)
        reply = guest.wait(pid, timeout=110)
        if check and reply["code"]:
            raise RuntimeError(f"{nodes[index]['name']}: CLI failed: {reply}")
        return reply

    def status(index):
        reply = call(index, "room", "status", "team", check=False)
        return json.loads(reply["out"])["result"] if reply["code"] == 0 else None

    def ready(index):
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline:
            current = status(index)
            if current and current["running"]:
                return current
            running = nodes[index]["guest"].agent("guest-exec-status", pid=active[index])
            if running.get("exited"):
                raise RuntimeError(f"{nodes[index]['name']}: room process exited before ready: {running}")
            time.sleep(0.1)
        raise TimeoutError("Public room command socket never became ready")

    def start(index, *args):
        active[index] = nodes[index]["guest"].start(CLI, "--profile", profile, *args)
        return ready(index)

    def stop(index):
        call(index, "room", "stop", "team")
        pid = active.pop(index)
        output = nodes[index]["guest"].wait(pid, timeout=25)
        if output["code"]:
            raise RuntimeError("Public room shutdown failed: " + str(output))
        received.setdefault(index, []).append(output["out"])
        if nodes[index]["guest"].run("test", "-e", profile + "/rooms/team/node.sock", check=False)["code"] == 0:
            raise RuntimeError("Stopped public room retained its command socket")
        return output

    def passed(scenario, **evidence):
        print("Room CLI: " + scenario, flush=True)
        results.append({"scenario": scenario, **evidence})

    def send(index, token, sensitive=False):
        args = ["room", "send", "team", "-" if sensitive else token]
        if sensitive:
            args.append("-s")
        reply = call(index, *args, data=token.encode() if sensitive else None)
        report = json.loads(reply["out"])
        assert report["ok"] and report["result"]["all_acknowledged"], report
        return report["result"]

    def media(source, target, kind, identities):
        # Typed RTP interoperability/transport, NOT a native capture/decoder test.
        payload_type = 96 if kind == "audio" else 97
        clock_step = 960 if kind == "audio" else 6000
        data = bytearray()
        for sequence in range(15):
            packet = bytes([0x80, payload_type]) + sequence.to_bytes(2, "big") + (sequence * clock_step).to_bytes(4, "big") + b"test" + b"x"
            data += len(packet).to_bytes(2, "big") + packet
        path = profile + "/transport-" + kind + ".rtp"
        nodes[source]["guest"].upload(path, data)
        denied = call(source, "room", "media", "team", "publish", kind, "--rtp-file", path, "--to", identities[target], "--consent", check=False)
        assert denied["code"] != 0
        before = status(target)["media"]["subscriptions"]
        pid = nodes[target]["guest"].start(CLI, "--profile", profile, "room", "media", "team", "listen", kind, "--from", identities[source], "--inspect", "--consent")
        media_processes[target] = pid
        deadline = time.monotonic() + 15
        while status(target)["media"]["subscriptions"] <= before:
            if time.monotonic() >= deadline:
                raise TimeoutError("Explicit public media subscription did not become ready")
            time.sleep(0.1)
        sent = json.loads(call(source, "room", "media", "team", "publish", kind, "--rtp-file", path, "--to", identities[target], "--consent")["out"])
        captured = nodes[target]["guest"].wait(pid, timeout=20)
        media_processes.pop(target)
        assert captured["code"] == 0, captured
        received_media = json.loads(captured["out"])
        assert sent["ok"] and sent["result"]["statistics"]["sent"] == 15, sent
        assert received_media["packets"] == 15 and received_media["bytes"] == 195, received_media
        assert not received_media["playback"] and not received_media["recorded"]
        assert status(source)["running"] and status(target)["running"]
        passed("public-cli-consented-media-" + kind + "-transport", source=source, target=target, sender=sent, receiver=received_media,
               scope="synthetic RFC4571 RTP interoperability; not physical capture, codec decoding or playback")

    try:
        identities = []
        for node in nodes:
            node["guest"].run("mkdir", "-m", "0700", work)
        for index in range(3):
            identities.append(call(index, "init")["out"].strip().removeprefix("IDENTITY sha256:"))
        assert len(set(identities)) == 3
        call(0, "room", "create", "team", "--title", "Public cross-VM CLI", "--bind", f"{nodes[0]['ip']}:{PORT}")
        start(0, "room", "serve", "team")
        passed("independent-profile-identities-and-public-room-creation", identities=identities)
        for index in [1, 2]:
            offer = "/var/tmp/kk-room-" + uuid.uuid4().hex[:12]
            guest = nodes[0]["guest"]
            guest.run("mkdir", "-m", "0700", offer)
            invitation = offer + "/scoped-invitation.json"
            invite = guest.start(CLI, "--profile", profile, "room", "invite", "team", "--bind", f"{nodes[0]['ip']}:{PORT + 1}",
                                 "--invite-out", invitation, "--role", "moderator", "--confirm")
            deadline = time.monotonic() + 15
            while guest.run("test", "-f", invitation, check=False)["code"]:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Public CLI did not publish its single-use invitation")
                time.sleep(0.1)
            target = work + "/scoped-invitation.json"
            nodes[index]["guest"].run("touch", target)
            nodes[index]["guest"].run("chmod", "0600", target)
            # Only the intended ephemeral TEST capability crosses lab control.
            # Authority/grants/packages/state use the authenticated network.
            bundle = guest.room_invitation(invitation)
            nodes[index]["guest"].upload(target, bundle)
            del bundle
            receive = ["--accept-files", "--inbox", profile + "/inbox", "--max-bytes", "524288", "--file-budget", "2097152"] if index == 2 else []
            joined = start(index, "room", "join", "team", "--invitation", target, "--bind", f"{nodes[index]['ip']}:{PORT}", "--role", "moderator", "--confirm", *receive)
            completed = guest.wait(invite, timeout=110)
            assert completed["code"] == 0 and json.loads(completed["out"])["ok"], completed
            assert joined["owner"] not in joined["voters"], joined
            passed("public-cli-network-enrollment-" + str(index), status=joined)
        for source in range(3):
            report = send(source, f"public-cli-{run}-from-{source}")
            assert len(report["acknowledged"]) == 2, report
        passed("all-three-cli-broadcasts-reach-both-independent-peers")
        report = send(0, "SENSITIVE-" + run, sensitive=True)
        passed("sensitive-stdin-through-public-cli", receipt=report)
        duplicate = call(1, "room", "serve", "team", check=False)
        assert duplicate["code"] != 0
        passed("second-foreground-owner-refused", diagnostic=duplicate["err"])
        payload = bytes(range(256)) * 512
        payload_hash = hashlib.sha256(payload).hexdigest()
        source_file = profile + "/room-file.bin"
        nodes[0]["guest"].upload(source_file, payload)
        refused = call(0, "room", "send", "team", source_file, "-f", "--to", identities[1], check=False)
        assert refused["code"] != 0
        passed("room-file-without-recipient-consent-refused", diagnostic=refused["err"])
        call(0, "room", "controllers", "team", *identities)
        before = status(1)["epoch"]
        stop(1)
        restored = start(1, "room", "serve", "team", "--accept-files", "--inbox", profile + "/inbox", "--max-bytes", "524288", "--file-budget", "2097152")
        assert restored["epoch"] == before
        send(1, "after-public-cli-cold-resume-" + run)
        passed("cold-resume-retains-identity-epoch-and-same-port", status=restored)
        prior_owner, prior_voters = restored["owner"], sorted(restored["voters"])
        stop(1)
        changed_address = f"{nodes[1]['ip']}:{PORT + 7}"
        moved = start(1, "room", "serve", "team", "--bind", changed_address, "--accept-files", "--inbox", profile + "/inbox", "--max-bytes", "524288", "--file-budget", "2097152")
        assert moved["address"] == changed_address
        assert moved["owner"] == prior_owner and sorted(moved["voters"]) == prior_voters
        assert len(send(0, "after-authenticated-peer-address-change-" + run)["acknowledged"]) == 2
        assert len(send(1, "after-authenticated-self-address-change-" + run)["acknowledged"]) == 2
        passed("authenticated-voting-member-address-change-preserves-identity-and-votes", status=moved)
        stop(1)
        recovered_address = start(1, "room", "serve", "team", "--accept-files", "--inbox", profile + "/inbox", "--max-bytes", "524288", "--file-budget", "2097152")
        assert recovered_address["address"] == changed_address
        assert recovered_address["owner"] == prior_owner and sorted(recovered_address["voters"]) == prior_voters
        assert len(send(2, "after-authenticated-address-cold-resume-" + run)["acknowledged"]) == 2
        passed("authenticated-new-address-survives-cold-resume-with-live-peers", status=recovered_address)
        transferred = json.loads(call(0, "room", "send", "team", source_file, "-f", "--to", identities[1])["out"])
        assert transferred["ok"] and transferred["result"]["sha256"] == payload_hash
        stored = profile + "/inbox/" + identities[0] + "-" + payload_hash + ".bin"
        actual = nodes[1]["guest"].run("sha256sum", stored)["out"].split()[0]
        assert actual == payload_hash
        passed("consented-room-file-verified-on-independent-recipient-disk", receipt=transferred["result"], sha256=actual)
        repeated = json.loads(call(0, "room", "send", "team", source_file, "-f", "--to", identities[1])["out"])
        assert repeated["ok"] and repeated["result"]["resumed_from"] == len(payload)
        passed("room-file-explicit-repeat-verifies-existing-prefix", receipt=repeated["result"])
        nodes[0]["guest"].upload(profile + "/oversize.bin", b"x" * 524289)
        too_large = call(0, "room", "send", "team", profile + "/oversize.bin", "-f", "--to", identities[1], check=False)
        assert too_large["code"] != 0
        assert status(1)["running"]
        passed("recipient-room-file-size-limit-refused-with-live-control", diagnostic=too_large["err"])
        call(1, "room", "files", "team", "share", stored, "--to", identities[2])
        hidden_offer = json.loads(call(0, "room", "files", "team", "locate", payload_hash, "--from", identities[1])["out"])
        public_offer = json.loads(call(2, "room", "files", "team", "locate", payload_hash, "--from", identities[1])["out"])
        assert hidden_offer["result"]["advertised"] == []
        assert public_offer["result"]["advertised"][0]["sha256"] == payload_hash
        passed("replica-advertisement-only-reaches-the-explicit-audience", allowed=public_offer["result"], excluded=hidden_offer["result"])
        call(1, "room", "files", "team", "unshare", payload_hash)
        withdrawn = json.loads(call(2, "room", "files", "team", "locate", payload_hash, "--from", identities[1])["out"])
        assert withdrawn["result"]["advertised"] == []
        passed("withdrawn-replica-offer-is-no-longer-advertised", report=withdrawn["result"])
        call(1, "room", "files", "team", "share", stored, "--to", identities[2])
        media(0, 1, "screen", identities)
        media(2, 0, "audio", identities)
        call(1, "room", "controllers", "team", *identities[1:])
        call(1, "room", "remove", "team", identities[0])
        stop(0)
        shutdown_guests([nodes[0]["name"]])
        assert virsh("domstate", nodes[0]["name"]).strip() == "shut off"
        first = send(1, "creator-vm-off-bob-" + run)
        second = send(2, "creator-vm-off-carol-" + run)
        assert len(first["acknowledged"]) == len(second["acknowledged"]) == 1
        passed("actual-creator-vm-off-surviving-public-clis-communicate", receipts=[first, second])
        old_epoch = json.loads(call(2, "room", "files", "team", "locate", payload_hash, "--from", identities[1])["out"])
        assert old_epoch["result"]["advertised"] == []
        passed("membership-epoch-change-retires-an-existing-replica-offer", report=old_epoch["result"])
        call(1, "room", "files", "team", "share", stored, "--to", identities[2])
        pulled = json.loads(call(2, "room", "files", "team", "fetch", payload_hash, "--from", identities[1])["out"])
        replica = profile + "/inbox/" + identities[1] + "-" + payload_hash + ".bin"
        assert pulled["ok"] and pulled["result"]["resumed_from"] == 0
        assert nodes[2]["guest"].run("sha256sum", replica)["out"].split()[0] == payload_hash
        passed("verified-replica-pull-after-actual-original-vm-shutdown", receipt=pulled["result"])
        pulled_again = json.loads(call(2, "room", "files", "team", "fetch", payload_hash, "--from", identities[1])["out"])
        assert pulled_again["result"]["resumed_from"] == len(payload)
        passed("replica-pull-reverifies-a-complete-existing-prefix", receipt=pulled_again["result"])
        call(1, "room", "files", "team", "unshare", payload_hash)
        refused_pull = call(2, "room", "files", "team", "fetch", payload_hash, "--from", identities[1], check=False)
        assert refused_pull["code"] != 0
        assert nodes[2]["guest"].run("sha256sum", replica)["out"].split()[0] == payload_hash
        passed("withdrawn-replica-pull-is-refused-without-replacing-the-existing-copy", diagnostic=refused_pull["err"])
        media(1, 2, "camera", identities)
        forwarded = json.loads(call(1, "room", "send", "team", stored, "-f", "--to", identities[2])["out"])
        assert forwarded["ok"] and forwarded["result"]["sha256"] == payload_hash
        replica = profile + "/inbox/" + identities[1] + "-" + payload_hash + ".bin"
        assert nodes[2]["guest"].run("sha256sum", replica)["out"].split()[0] == payload_hash
        passed("explicit-consented-copy-can-be-forwarded-after-original-vm-off", receipt=forwarded["result"], limitation="manual explicit transfer, not automatic replica discovery or an availability guarantee")
        for index in [1, 2]:
            stop(index)
        for index, chunks in received.items():
            output = "".join(chunks)
            assert "SENSITIVE-" + run not in output
            for source in range(3):
                if source != index:
                    assert f"public-cli-{run}-from-{source}" in output
            if index in [1, 2]:
                assert "[sensitive," in output
        passed("actual-event-output-and-sensitive-redaction-with-clean-socket-release")
    finally:
        primary = sys.exception()
        errors = []
        for index, pid in list(media_processes.items()):
            try:
                nodes[index]["guest"].run("kill", "-INT", str(pid), check=False)
                result = nodes[index]["guest"].wait(pid, timeout=15)
                if result["code"] not in (0, 130):
                    raise RuntimeError("Media inspector cleanup failed: " + str(result))
            except Exception as error:
                errors.append(str(error))
        for index in list(active):
            try:
                stop(index)
            except Exception as error:
                errors.append(str(error))
        report = ROOT / "tmp" / f"public-room-cli-{run}.json"
        report.write_text(json.dumps({"scope": "Actual public CLI on three independent isolated guests; not fixture commands, Byzantine consensus, native media or physical radio proof",
                                      "run_id": run, "binary_sha256": binary_hash, "roster": [{"name": node["name"], "ip": node["ip"]} for node in nodes],
                                      "results": results, "failure": str(primary) if primary else None, "cleanup_errors": errors}, indent=2) + "\n")
        print("Evidence: " + str(report), flush=True)
        if errors:
            if primary is None:
                raise RuntimeError("Room CLI cleanup failed: " + "; ".join(errors))
            for error in errors:
                primary.add_note("Cleanup: " + error)
    print(f"PASS: {len(results)} public room CLI scenarios", flush=True)


if __name__ == "__main__":
    main()
