"""File scenarios: only K&K carries fixture bytes between the guests."""

import re
import time
import uuid

from kk_guest import Guest


def file_cases(peers, shape, listening, port, results):
    receiver, sender = peers
    a, b = Guest(receiver["name"]), Guest(sender["name"])
    work = "/var/tmp/kk-files-" + uuid.uuid4().hex[:12]
    for guest in [a, b]:
        guest.run("mkdir", "-m", "0700", work)
    source = work + "/source.bin"
    b.run("dd", "if=/dev/zero", "of=" + source, "bs=1048576", "count=4", "status=none")
    empty = work + "/empty.bin"
    b.run("truncate", "-s", "0", empty)
    small = work + "/small.bin"
    b.run("dd", "if=/dev/zero", "of=" + small, "bs=65536", "count=1", "status=none")

    def exists(path):
        return a.run("test", "-e", path, check=False)["code"] == 0

    def receive(path, limit=64 * 1024 * 1024, peer=None, advertise=False, timeout=30):
        arguments = ["/usr/local/bin/kk", "receive", "-f", "--bind", f"{receiver['ip']}:{port}",
                     "--identity", receiver["identity"], "--peer", peer or receiver["certificate"],
                     "--output", path, "--accept", "--max-bytes", str(limit), "--timeout", str(timeout)]
        if advertise:
            arguments.extend(["--advertise", "kk-files-test"])
        pid = a.start(*arguments)
        listening(a)
        return pid

    def send(path=source, timeout=30):
        return b.run("/usr/local/bin/kk", "send", f"{receiver['ip']}:{port}", path, "-f",
                     "--identity", sender["identity"], "--peer", sender["certificate"],
                     "--timeout", str(timeout), check=False, timeout=timeout + 10)

    def verify(path, original):
        expected = b.run("sha256sum", original)["out"].split()[0]
        actual = a.run("sha256sum", path)["out"].split()[0]
        assert actual == expected, (actual, expected)
        assert not exists(path + ".kk-part") and not exists(path + ".kk-meta")
        return expected

    def transfer(label, output, original=source, resumed=None, advertise=False):
        print(f"File scenario {label}", flush=True)
        pid = receive(output, advertise=advertise)
        if advertise:
            found = b.run("/usr/local/bin/kk", "peers", "--interface", sender["ip"], "--timeout", "2")
            assert "UNVERIFIED" in found["out"] and "kk-files-test" in found["out"] and " file" in found["out"], found
        sent, received = send(original), a.wait(pid, timeout=40)
        assert sent["code"] == 0 and received["code"] == 0, (sent, received)
        offset = int(re.search(r"resumed=(\d+)", sent["out"])[1])
        if resumed is not None:
            assert offset == resumed, (offset, resumed)
        digest = verify(output, original)
        result = {"scenario": label, "source": original, "destination": output,
                  "sender_result": sent, "receiver_result": received, "sha256": digest}
        results.append(result)
        return result

    def reject(label, output, limit=64 * 1024 * 1024, peer=None, code=None):
        print(f"File scenario {label}", flush=True)
        pid = receive(output, limit=limit, peer=peer)
        sent, received = send(), a.wait(pid, timeout=40)
        assert sent["code"] == 1 and received["code"] == 1, (sent, received)
        assert not sent["out"] and not received["out"], (sent, received)
        if code:
            assert code + ":" in received["err"], received
        results.append({"scenario": label, "destination": output, "sender_result": sent, "receiver_result": received})

    def interrupt(label, output, victim):
        print(f"File scenario {label}", flush=True)
        for peer in peers:
            shape(peer, ["rate", "256kbit"])
        pid = receive(output)
        sending = b.start("/usr/local/bin/kk", "send", f"{receiver['ip']}:{port}", source, "-f",
                          "--identity", sender["identity"], "--peer", sender["certificate"], "--timeout", "30")
        deadline = time.monotonic() + 12
        progress = 0
        while time.monotonic() < deadline:
            state = a.run("stat", "-c", "%s", output + ".kk-part", check=False)
            if state["code"] == 0:
                progress = int(state["out"])
                if progress >= 65_536:
                    break
            time.sleep(0.1)
        assert 0 < progress < 4 * 1024 * 1024, progress
        killed, killed_pid = (b, sending) if victim == "sender" else (a, pid)
        killed.run("kill", "-INT", str(killed_pid))
        sent, received = b.wait(sending, timeout=40), a.wait(pid, timeout=40)
        assert sent["code"] != 0 and received["code"] != 0, (sent, received)
        assert (sent if victim == "sender" else received)["code"] == 130
        assert not exists(output)
        progress = int(a.run("stat", "-c", "%s", output + ".kk-part")["out"])
        for peer in peers:
            shape(peer, [])
        return progress, {"scenario": label, "destination": output, "sender_result": sent,
                          "receiver_result": received, "retained_bytes": progress}

    try:
        complete = work + "/complete"
        transfer("file-binary-and-discovery", complete, resumed=0, advertise=True)
        print("File scenario file-reverse", flush=True)
        reverse = work + "/reverse"
        pid = b.start("/usr/local/bin/kk", "receive", "-f", "--bind", f"{sender['ip']}:{port}",
                      "--identity", sender["identity"], "--peer", sender["certificate"],
                      "--output", reverse, "--accept", "--timeout", "30")
        listening(b)
        sent = a.run("/usr/local/bin/kk", "send", f"{sender['ip']}:{port}", complete, "-f",
                     "--identity", receiver["identity"], "--peer", receiver["certificate"], "--timeout", "30")
        received = b.wait(pid, timeout=40)
        assert received["code"] == 0, received
        assert b.run("sha256sum", reverse)["out"].split()[0] == a.run("sha256sum", complete)["out"].split()[0]
        results.append({"scenario": "file-reverse", "sender_result": sent, "receiver_result": received,
                        "source": complete, "destination": reverse})
        transfer("file-empty", work + "/empty", original=empty, resumed=0)
        before = a.run("stat", "-c", "%y:%s:%i", complete)["out"]
        transfer("file-idempotent-receipt", complete, resumed=4 * 1024 * 1024)
        assert a.run("stat", "-c", "%y:%s:%i", complete)["out"] == before

        output = work + "/too-large"
        reject("file-size-refusal", output, limit=1, code="FILE_TOO_LARGE")
        assert not exists(output) and not exists(output + ".kk-part")
        missing_consent = a.run("/usr/local/bin/kk", "receive", "-f", "--bind", f"{receiver['ip']}:{port}",
                                "--identity", receiver["identity"], "--peer", receiver["certificate"],
                                "--output", work + "/no-consent", check=False)
        assert missing_consent["code"] == 2 and not exists(work + "/no-consent")
        results.append({"scenario": "file-consent-required", "result": missing_consent})

        victim = work + "/victim"
        a.upload(victim, b"preserve this fixture")
        a.run("chmod", "0600", victim)
        original_hash = a.run("sha256sum", victim)["out"]
        reject("file-existing-refusal", victim, code="DESTINATION")
        link = work + "/link"
        a.run("ln", "-s", victim, link)
        reject("file-symlink-refusal", link, code="DESTINATION")
        assert a.run("sha256sum", victim)["out"] == original_hash

        rogue = work + "/rogue"
        b.run("/usr/local/bin/kk", "identity", "create", rogue)
        rogue_certificate = work + "/rogue.der"
        a.upload(rogue_certificate, b.public_certificate(rogue))
        output = work + "/unauthorized"
        reject("file-wrong-client", output, peer=rogue_certificate, code="AUTHENTICATION")
        assert not exists(output + ".kk-part")

        for peer in peers:
            shape(peer, ["rate", "1mbit", "delay", "50ms", "loss", "1%"])
        transfer("file-constrained-network", work + "/constrained", original=small)
        for peer in peers:
            shape(peer, [])

        for target in ["sender", "receiver"]:
            output = work + "/resume-" + target
            progress, cut = interrupt("file-interrupt-" + target, output, target)
            result = transfer("file-resume-" + target, output, resumed=progress)
            result["interruption"] = cut

        output = work + "/corrupt"
        _, cut = interrupt("file-corrupt-prefix", output, "sender")
        write = a.start("dd", "of=" + output + ".kk-part", "bs=1", "count=1", "conv=notrunc", "status=none", input_data=b"X")
        assert a.wait(write)["code"] == 0
        pid = receive(output)
        sent, received = send(), a.wait(pid, timeout=40)
        assert sent["code"] == 1 and "INTEGRITY:" in sent["err"] and received["code"] == 1, (sent, received)
        assert not exists(output) and exists(output + ".kk-part")
        results.append({"scenario": "file-corrupt-prefix-refused", "sender_result": sent, "receiver_result": received, "interruption": cut})

        for peer in peers:
            shape(peer, ["loss", "100%"])
        output = work + "/blackhole"
        pid = receive(output, timeout=3)
        sent, received = send(timeout=3), a.wait(pid, timeout=10)
        assert sent["code"] == 1 and received["code"] == 1 and "TIMEOUT:" in sent["err"], (sent, received)
        assert not exists(output) and not exists(output + ".kk-part")
        results.append({"scenario": "file-blackhole", "sender_result": sent, "receiver_result": received})
    finally:
        for peer in peers:
            shape(peer, [])
