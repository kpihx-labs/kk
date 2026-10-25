"""Synthetic fixtures only: never collect an operator token or private key."""

import hashlib
import uuid

from kk_guest import Guest


def sensitive_cases(peers, listening, port, results):
    receiver, sender = peers
    a, b = Guest(receiver["name"]), Guest(sender["name"])
    fixture = b"synthetic-sensitive-fixture-not-an-operator-token"
    work = "/var/tmp/kk-sensitive-" + uuid.uuid4().hex[:12]
    for guest in [a, b]:
        guest.run("mkdir", "-m", "0700", work)

    def receive(extra=None):
        arguments = ["/usr/local/bin/kk", "receive", "--bind", f"{receiver['ip']}:{port}",
                     "--identity", receiver["identity"], "--peer", receiver["certificate"], "--timeout", "20"]
        pid = a.start(*arguments, *(extra or []))
        listening(a)
        return pid

    def send(extra=None):
        pid = b.start("/usr/local/bin/kk", "send", f"{receiver['ip']}:{port}", "-", "-s",
                      "--identity", sender["identity"], "--peer", sender["certificate"],
                      "--timeout", "20", *(extra or []), input_data=fixture)
        return b.wait(pid, timeout=30)

    def redacted(label, sent, received):
        assert sent["code"] == 0 and received["code"] == 0, (sent, received)
        assert fixture.decode() not in str(sent) and fixture.decode() not in str(received)
        results.append({"scenario": label, "sender_result": sent, "receiver_result": received})

    print("Sensitive scenario stdin-redacted", flush=True)
    pid = receive()
    redacted("sensitive-stdin-redacted", send(), a.wait(pid))

    print("Sensitive scenario explicit-private-output", flush=True)
    private_output = work + "/received"
    pid = receive(["--output", private_output])
    redacted("sensitive-private-output", send(), a.wait(pid))
    assert a.run("sha256sum", private_output)["out"].split()[0] == hashlib.sha256(fixture).hexdigest()
    assert a.run("stat", "-c", "%a", private_output)["out"].strip() == "600"

    print("Sensitive scenario argv-refusal", flush=True)
    refused = b.run("/usr/local/bin/kk", "send", f"{receiver['ip']}:{port}", fixture.decode(), "-s",
                    "--identity", sender["identity"], "--peer", sender["certificate"], check=False)
    assert refused["code"] == 1 and fixture.decode() not in str(refused)
    results.append({"scenario": "sensitive-argv-refused", "result": refused})

    print("Sensitive scenario sealed-artifact", flush=True)
    private_key = work + "/recipient.key"
    public_key = a.run("/usr/local/bin/kk", "keygen", private_key)["out"].strip()
    assert public_key.startswith("age1") and len(public_key) < 128
    encrypted = work + "/received.age"
    artifact = work + "/sender.age"
    pid = receive(["-f", "--output", encrypted, "--accept"])
    sent = send(["--seal-to", public_key, "--envelope", artifact])
    received = a.wait(pid)
    redacted("sealed-send-and-receive", sent, received)
    assert "sensitive" in received["out"] and "sha256:" not in received["out"]
    assert a.run("sha256sum", encrypted)["out"].split()[0] == b.run("sha256sum", artifact)["out"].split()[0]
    plain = work + "/decrypted"
    opened = a.run("/usr/local/bin/kk", "unseal", encrypted, "--key-file", private_key, "--output", plain)
    assert a.run("sha256sum", plain)["out"].split()[0] == hashlib.sha256(fixture).hexdigest()
    assert fixture.decode() not in str(opened)
    results.append({"scenario": "sealed-explicit-decryption", "result": opened})

    print("Sensitive scenario wrong-key-and-modification", flush=True)
    wrong = work + "/wrong.key"
    a.run("/usr/local/bin/kk", "keygen", wrong)
    wrong_output = work + "/wrong-output"
    refused = a.run("/usr/local/bin/kk", "unseal", encrypted, "--key-file", wrong, "--output", wrong_output, check=False)
    assert refused["code"] == 1 and a.run("test", "-e", wrong_output, check=False)["code"] == 1
    results.append({"scenario": "sealed-wrong-key-refused", "result": refused})
    size = int(a.run("stat", "-c", "%s", encrypted)["out"])
    last = int(a.run("od", "-An", "-tu1", "-j", str(size - 1), "-N", "1", encrypted)["out"])
    changed = a.start("dd", "of=" + encrypted, "bs=1", "count=1", "seek=" + str(size - 1), "conv=notrunc",
                      "status=none", input_data=bytes([last ^ 1]))
    assert a.wait(changed)["code"] == 0
    tampered = work + "/tampered-output"
    refused = a.run("/usr/local/bin/kk", "unseal", encrypted, "--key-file", private_key, "--output", tampered, check=False)
    assert refused["code"] == 1 and a.run("test", "-e", tampered, check=False)["code"] == 1
    results.append({"scenario": "sealed-modification-refused", "result": refused})
