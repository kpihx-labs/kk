"""Real PAKE/TLS/registry scenarios. Invitations are disposable synthetic fixtures."""

import base64
import json
import time
import uuid

from kk_guest import Guest


def pair_cases(peers, shape, listening, port, results):
    receiver, sender = peers
    a, b = Guest(receiver["name"]), Guest(sender["name"])
    work = "/var/tmp/kk-pair-" + uuid.uuid4().hex[:12]
    pa, pb = work + "/profile-a", work + "/profile-b"
    pair_port = port - 1
    address = f"{receiver['ip']}:{pair_port}"
    for guest in [a, b]:
        guest.run("mkdir", "-m", "0700", work)

    def invoke(guest, profile, *arguments, check=True, timeout=40):
        return guest.run("/usr/local/bin/kk", "--profile", profile, *arguments, check=check, timeout=timeout)

    def no_contact(guest, profile, alias):
        return alias not in invoke(guest, profile, "trust", "list")["out"]

    def offer(label, grants="text,files", ttl=25, alias="bob", advertise=False):
        invitation = work + "/" + label + ".json"
        qr = work + "/" + label + ".png"
        arguments = ["/usr/local/bin/kk", "--profile", pa, "pair", "--listen", address,
                     "--as", alias, "--allow", grants, "--ttl", str(ttl), "--data-port", str(port),
                     "--confirm", "--invite-out", invitation, "--qr-out", qr]
        if advertise:
            arguments.extend(["--advertise", "kk-pair-test"])
        pid = a.start(*arguments)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if a.run("test", "-s", qr, check=False)["code"] == 0:
                return pid, invitation, qr
            time.sleep(0.1)
        raise TimeoutError("private pairing invitation was not issued")

    def copy_invitation(source, destination, transform=None):
        # Never return/print the code to the model or retain it in result JSON.
        encoded = a.run("base64", "--wrap", "0", source)["out"]
        raw = base64.b64decode(encoded)
        if transform:
            value = json.loads(raw)
            transform(value)
            raw = json.dumps(value).encode()
        b.upload(destination, raw)
        b.run("chmod", "0600", destination)

    def join(invitation, profile=pb, qr=False, confirm=True):
        args = ["pair", "--as", "alice", "--allow", "text,files", "--ttl", "25",
                "--data-port", str(port), "--qr-image" if qr else "--invitation", invitation]
        if confirm:
            args.append("--confirm")
        return invoke(b, profile, *args, check=False)

    def finish(label, pid, joined, success=True):
        accepted = a.wait(pid, timeout=35)
        if success:
            assert accepted["code"] == 0 and joined["code"] == 0, (accepted, joined)
            assert "PAIRED" in accepted["out"] and "PAIRED" in joined["out"]
            a_confirm = accepted["err"].split("CONFIRM ")[1].split()[0]
            b_confirm = joined["err"].split("CONFIRM ")[1].split()[0]
            assert a_confirm == b_confirm
        else:
            assert accepted["code"] != 0 and joined["code"] != 0, (accepted, joined)
            assert "PAIRED" not in accepted["out"] and "PAIRED" not in joined["out"]
        results.append({"scenario": label, "server_result": accepted, "client_result": joined})

    print("Pair scenario private-invitation-and-discovery", flush=True)
    pid, original, _ = offer("initial", grants="text", advertise=True)
    found = b.run("/usr/local/bin/kk", "peers", "--interface", sender["ip"], "--timeout", "2")
    assert "UNVERIFIED" in found["out"] and "kk-pair-test" in found["out"] and " pair" in found["out"], found
    first = work + "/client-initial.json"
    copy_invitation(original, first)
    finish("pair-private-invitation", pid, join(first))

    print("Pair scenario aliases-and-grants", flush=True)
    receiving = a.start("/usr/local/bin/kk", "--profile", pa, "receive", "--from", "bob",
                        "--bind", f"{receiver['ip']}:{port}", "--timeout", "10")
    listening(a)
    sent = invoke(b, pb, "send", "alice", "paired alias fixture", "--timeout", "10")
    received = a.wait(receiving)
    assert received["code"] == 0 and received["out"] == "paired alias fixture\n", received
    results.append({"scenario": "pair-alias-message", "sender_result": sent, "receiver_result": received})
    source = work + "/source.bin"
    b.run("dd", "if=/dev/zero", "of=" + source, "bs=1048576", "count=4", "status=none")
    output = work + "/denied"
    outgoing = invoke(b, pb, "send", "alice", source, "-f", check=False)
    incoming = invoke(a, pa, "receive", "--from", "bob", "--bind", f"{receiver['ip']}:{port}",
                      "-f", "--output", output, "--accept", check=False)
    assert outgoing["code"] == 1 and incoming["code"] == 1
    assert "DENIED" in outgoing["err"] and "DENIED" in incoming["err"]
    assert a.run("test", "-e", output, check=False)["code"] == 1
    results.append({"scenario": "pair-permission-denial", "sender_result": outgoing, "receiver_result": incoming})

    pid, invitation, _ = offer("upgrade")
    copied = work + "/client-upgrade.json"
    copy_invitation(invitation, copied)
    finish("pair-explicit-grant-upgrade", pid, join(copied))
    output = work + "/paired-file"
    receiving = a.start("/usr/local/bin/kk", "--profile", pa, "receive", "--from", "bob",
                        "--bind", f"{receiver['ip']}:{port}", "-f", "--output", output, "--accept", "--timeout", "20")
    listening(a)
    sent = invoke(b, pb, "send", "alice", source, "-f", "--timeout", "20")
    received = a.wait(receiving)
    assert received["code"] == 0 and a.run("sha256sum", output)["out"].split()[0] == b.run("sha256sum", source)["out"].split()[0]
    results.append({"scenario": "pair-alias-file", "sender_result": sent, "receiver_result": received})

    pid, _, qr = offer("qr")
    copied_qr = work + "/client-qr.png"
    copy_invitation(qr, copied_qr)
    finish("pair-qr-image", pid, join(copied_qr, qr=True))

    for mode in ["wrong-code", "wrong-fingerprint", "replay", "no-confirm"]:
        print(f"Pair scenario {mode}", flush=True)
        pid, invitation, _ = offer(mode)
        copied = work + "/client-" + mode + ".json"
        if mode == "wrong-code":
            copy_invitation(invitation, copied, lambda value: value.update(code="wrong synthetic code"))
        elif mode == "wrong-fingerprint":
            copy_invitation(invitation, copied, lambda value: value.update(fingerprint="0" * 64))
        else:
            copy_invitation(original if mode == "replay" else invitation, copied)
        before_a = a.run("sha256sum", pa + "/contacts.json")["out"]
        before_b = b.run("sha256sum", pb + "/contacts.json")["out"]
        finish("pair-" + mode + "-refused", pid, join(copied, confirm=mode != "no-confirm"), success=False)
        assert a.run("sha256sum", pa + "/contacts.json")["out"] == before_a
        assert b.run("sha256sum", pb + "/contacts.json")["out"] == before_b

    print("Pair scenario malformed-frame-bound", flush=True)
    pid, _, _ = offer("malformed")
    before = a.run("sha256sum", pa + "/contacts.json")["out"]
    probe = b.run("bash", "-c", f"exec 3<>/dev/tcp/{receiver['ip']}/{pair_port}; dd bs=24 count=1 iflag=fullblock status=none <&3; printf '\\377\\377\\377\\377' >&3; dd bs=1 count=1 status=none <&3; exec 3>&-", check=False)
    accepted = a.wait(pid, timeout=10)
    assert accepted["code"] == 1 and "PAIRING:" in accepted["err"] and a.run("sha256sum", pa + "/contacts.json")["out"] == before
    results.append({"scenario": "pair-malformed-frame-bound", "server_result": accepted, "probe_code": probe["code"]})

    print("Pair scenario expired", flush=True)
    pid, invitation, _ = offer("expired", ttl=1)
    copied = work + "/client-expired.json"
    copy_invitation(invitation, copied)
    accepted = a.wait(pid, timeout=5)
    joined = join(copied)
    assert accepted["code"] == 1 and joined["code"] == 1
    results.append({"scenario": "pair-expired-invitation", "server_result": accepted, "client_result": joined})

    print("Pair scenario concurrent-attempt-budget", flush=True)
    pid, invitation, _ = offer("race")
    correct = work + "/race-correct.json"
    wrong = work + "/race-wrong.json"
    copy_invitation(invitation, correct)
    copy_invitation(invitation, wrong, lambda value: value.update(code="wrong synthetic code"))
    bad_pid = b.start("/usr/local/bin/kk", "--profile", pb, "pair", "--as", "alice", "--confirm", "--allow", "text,files", "--invitation", wrong)
    good_pid = b.start("/usr/local/bin/kk", "--profile", pb, "pair", "--as", "alice", "--confirm", "--allow", "text,files", "--invitation", correct)
    bad, good, accepted = b.wait(bad_pid), b.wait(good_pid), a.wait(pid)
    assert bad["code"] != 0 and sum(item["code"] == 0 for item in [bad, good]) <= 1
    assert (accepted["code"] == 0) == (good["code"] == 0)
    results.append({"scenario": "pair-single-global-attempt", "server_result": accepted, "wrong_result": bad, "correct_result": good})

    print("Pair scenario changed-key-refusal", flush=True)
    pid, invitation, _ = offer("changed-key")
    copied = work + "/client-changed.json"
    copy_invitation(invitation, copied)
    replacement = work + "/replacement-profile"
    finish("pair-changed-key-refused", pid, join(copied, profile=replacement), success=False)
    assert no_contact(b, replacement, "alice")

    print("Pair scenario active-transfer-revocation", flush=True)
    output = work + "/interrupted-revocation"
    for peer in peers:
        shape(peer, ["rate", "256kbit"])
    try:
        receiving = a.start("/usr/local/bin/kk", "--profile", pa, "receive", "--from", "bob", "--bind", f"{receiver['ip']}:{port}",
                            "-f", "--output", output, "--accept", "--timeout", "25")
        listening(a)
        sending = b.start("/usr/local/bin/kk", "--profile", pb, "send", "alice", source, "-f", "--timeout", "25")
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            progress = a.run("stat", "-c", "%s", output + ".kk-part", check=False)
            if progress["code"] == 0 and int(progress["out"]) >= 65_536:
                break
            time.sleep(0.1)
        else:
            raise TimeoutError("paired transfer made no progress before revocation")
        invoke(a, pa, "trust", "revoke", "bob")
        sent, received = b.wait(sending, timeout=35), a.wait(receiving, timeout=35)
        assert sent["code"] != 0 and received["code"] == 1 and "REVOKED" in received["err"], (sent, received)
        assert a.run("test", "-e", output, check=False)["code"] == 1
        assert a.run("test", "-e", output + ".kk-part", check=False)["code"] == 0
        results.append({"scenario": "pair-active-transfer-revocation", "sender_result": sent, "receiver_result": received})
    finally:
        for peer in peers:
            shape(peer, [])
    public = work + "/bob.der"
    a.upload(public, b.public_certificate(pb + "/identity"))
    manual = invoke(a, pa, "send", f"{sender['ip']}:{port}", "blocked", "--identity", pa + "/identity", "--peer", public, check=False)
    assert manual["code"] == 1 and "REVOKED" in manual["err"], manual
    invoke(a, pa, "trust", "restore", "bob")
    results.append({"scenario": "pair-manual-certificate-cannot-bypass-revocation", "result": manual})

    print("Pair scenario concurrent-policy-change", flush=True)
    pid, invitation, _ = offer("policy-change")
    copied = work + "/client-policy-change.json"
    copy_invitation(invitation, copied)
    invoke(a, pa, "trust", "allow", "bob", "--allow", "text")
    protected_state = a.run("sha256sum", pa + "/contacts.json")["out"]
    finish("pair-policy-change-aborts-stale-confirmation", pid, join(copied), success=False)
    assert a.run("sha256sum", pa + "/contacts.json")["out"] == protected_state
    refused = invoke(a, pa, "receive", "--from", "bob", "--bind", f"{receiver['ip']}:{port}",
                     "-f", "--output", work + "/stale-policy-file", "--accept", check=False)
    assert refused["code"] == 1 and "DENIED" in refused["err"]
