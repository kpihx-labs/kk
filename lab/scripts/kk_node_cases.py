"""Three independent guests, real pairing, persistent links and explicit failures.

This tests a pre-paired topology, NOT dynamic room admission or replicated state.
Only synthetic invitations and payloads are used. Private keys never leave guests.
"""

import base64
import hashlib
import json
import time
import uuid

from kk_guest import Guest


def node_cases(peers, shape, port, results):
    if len(peers) < 3:
        raise RuntimeError("Persistent-node acceptance requires three independent guests")
    attempt = uuid.uuid4().hex[:8]
    nodes = [{**peer, "guest": Guest(peer["name"]), "profile": peer["identity"] + "-node-" + attempt} for peer in peers]
    processes = {}

    def call(node, *args, check=True, timeout=45):
        return node["guest"].run("/usr/local/bin/kk", "--profile", node["profile"],
                                 *args, check=check, timeout=timeout)

    def status(node):
        return json.loads(call(node, "status")["out"])

    def ready(node):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            reply = call(node, "status", check=False)
            if reply["code"] == 0:
                return json.loads(reply["out"])
            time.sleep(0.1)
        raise TimeoutError(f"{node['name']}: private node control never became ready")

    def start(node):
        processes[node["name"]] = node["guest"].start(
            "/usr/local/bin/kk", "--profile", node["profile"], "serve", "--bind", f"{node['ip']}:{port}",
            "--accept-files", "--inbox", node["profile"] + "/inbox", "--max-bytes", "1048576",
            "--file-budget", "2097152")
        ready(node)

    def record(label, **evidence):
        print("Node scenario " + label, flush=True)
        results.append({"scenario": label, **evidence})

    def send(source, target, text, check=True):
        return call(source, "send", target["name"], text, "--timeout", "5", check=check)

    try:
        identities = [call(node, "init")["out"].strip() for node in nodes]
        assert len(set(identities)) == len(nodes), "Guests must have independent identities"
        for i, own in enumerate(nodes):
            for j in range(i + 1, len(nodes)):
                other = nodes[j]
                invite = own["profile"] + f"/invitation-{j}.json"
                pid = own["guest"].start("/usr/local/bin/kk", "--profile", own["profile"], "pair",
                                         "--listen", f"{own['ip']}:45453", "--as", other["name"],
                                         "--allow", "text,files", "--confirm", "--invite-out", invite,
                                         "--data-port", str(port))
                deadline = time.monotonic() + 10
                while own["guest"].run("test", "-f", invite, check=False)["code"]:
                    if time.monotonic() >= deadline:
                        raise TimeoutError("Synthetic invitation was not created")
                    time.sleep(0.1)
                # Explicit synthetic capability transfer over lab control, never a key.
                bundle = base64.b64decode(own["guest"].run("base64", "-w0", invite)["out"])
                imported = other["profile"] + f"/invitation-from-{i}.json"
                other["guest"].run("touch", imported)
                other["guest"].run("chmod", "0600", imported)
                other["guest"].upload(imported, bundle)
                call(other, "pair", "--invitation", imported, "--as", own["name"],
                     "--allow", "text,files", "--confirm")
                assert own["guest"].wait(pid)["code"] == 0
        record("node-independent-identities-and-pairing", identities=identities)
        for node in nodes:
            start(node)
        for source in nodes:
            for target in nodes:
                if source is not target:
                    send(source, target, f"synthetic {source['name']} to {target['name']}")
        snapshots = [status(node) for node in nodes]
        assert all(item["received_operations"] == len(nodes) - 1 for item in snapshots), snapshots
        record("node-all-directed-text-links", status=snapshots)
        a, b, c = nodes[:3]
        before = status(b)["connections_accepted"]
        for text in ["reuse-one", "reuse-two", "reuse-three"]:
            send(a, b, text)
        assert status(b)["connections_accepted"] == before
        record("node-authenticated-link-reuse", connections=before)
        for i, source in enumerate(nodes):
            target = nodes[(i + 1) % len(nodes)]
            data = (f"synthetic node file {i}\n".encode() * 8192)[:131072]
            source_path = source["profile"] + "/source.bin"
            source["guest"].upload(source_path, data)
            call(source, "send", target["name"], source_path, "-f", "--timeout", "15")
            digest = hashlib.sha256(data).hexdigest()
            inbox = target["profile"] + "/inbox"
            files = target["guest"].run("ls", "-1", inbox)["out"].splitlines()
            destination = [name for name in files if name.endswith("-" + digest + ".bin")]
            assert len(destination) == 1
            assert target["guest"].run("sha256sum", inbox + "/" + destination[0])["out"].split()[0] == digest
        record("node-file-ring-integrity", status=[status(node) for node in nodes])
        count = status(b)["received_operations"]
        for size, expected in [(65536, 0), (65537, 1)]:
            pid = a["guest"].start("/usr/local/bin/kk", "--profile", a["profile"], "send",
                                     b["name"], "-", "-s", "--timeout", "5", input_data=b"\0" * size)
            reply = a["guest"].wait(pid)
            assert reply["code"] == expected, reply
        assert status(b)["received_operations"] == count + 1
        record("node-sensitive-maximum-input-and-oversize-refusal")
        bulk_path = a["profile"] + "/bulk.bin"
        bulk_data = bytes(range(256)) * 4096
        bulk_hash = hashlib.sha256(bulk_data).hexdigest()
        a["guest"].upload(bulk_path, bulk_data)
        shape(a, ["rate", "4mbit", "delay", "50ms"])
        uploads = []
        try:
            for target in [b, c]:
                uploads.append(a["guest"].start("/usr/local/bin/kk", "--profile", a["profile"], "send",
                                               target["name"], bulk_path, "-f", "--timeout", "30"))
            deadline = time.monotonic() + 5
            while status(a)["local_file_operations"] != 2:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Two concurrent bulk commands never became active")
                time.sleep(0.05)
            refused = call(a, "send", b["name"], bulk_path, "-f", "--timeout", "5", check=False)
            assert refused["code"] != 0, refused
            started = time.monotonic()
            snapshot = status(a)
            latency = time.monotonic() - started
            assert latency < 3, (latency, snapshot)
            send(a, b, "text-during-two-bulk-transfers")
            for pid in uploads[:]:
                reply = a["guest"].wait(pid, timeout=40)
                uploads.remove(pid)
                assert reply["code"] == 0, reply
            for target in [b, c]:
                inbox = target["profile"] + "/inbox"
                found = [name for name in target["guest"].run("ls", "-1", inbox)["out"].splitlines()
                         if name.endswith("-" + bulk_hash + ".bin")]
                assert len(found) == 1
                assert target["guest"].run("sha256sum", inbox + "/" + found[0])["out"].split()[0] == bulk_hash
            record("node-bounded-bulk-lane-with-responsive-text-and-control",
                   status_during_bulk=snapshot, status_latency_seconds=latency, excess_refusal=refused)
        finally:
            shape(a, [])
            for pid in uploads:
                a["guest"].run("kill", "-INT", str(pid), check=False)
                a["guest"].wait(pid, timeout=10)
        denied = a["guest"].run("runuser", "--user", "nobody", "--", "/usr/local/bin/kk",
                                "--profile", a["profile"], "status", check=False)
        assert denied["code"] != 0
        duplicate = call(a, "serve", "--bind", a["ip"] + ":45455", check=False)
        assert duplicate["code"] != 0
        status(a)
        record("node-private-ipc-and-single-owner", denied=denied, duplicate=duplicate)
        live_path = b["profile"] + "/live-revoke.bin"
        live_data = b"synthetic-live-revocation\n" * 16384
        live_hash = hashlib.sha256(live_data).hexdigest()
        destination = a["profile"] + "/inbox/" + identities[1].rsplit(":", 1)[1] + "-" + live_hash + ".bin"
        b["guest"].upload(live_path, live_data)
        shape(b, ["rate", "512kbit", "delay", "50ms"])
        pid = b["guest"].start("/usr/local/bin/kk", "--profile", b["profile"], "send",
                                 a["name"], live_path, "-f", "--timeout", "20")
        try:
            deadline = time.monotonic() + 10
            while True:
                partial = a["guest"].run("stat", "-c", "%s", destination + ".kk-part", check=False)
                if partial["code"] == 0 and int(partial["out"]) > 0:
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError("Live file never retained a nonempty partial")
                time.sleep(0.05)
            call(a, "trust", "revoke", b["name"])
            refused = b["guest"].wait(pid, timeout=10)
            assert refused["code"] != 0, refused
            a["guest"].run("test", "!", "-e", destination)
        finally:
            shape(b, [])
        call(a, "trust", "restore", b["name"])
        time.sleep(1.2)
        resumed = call(b, "send", a["name"], live_path, "-f", "--timeout", "15")
        assert a["guest"].run("sha256sum", destination)["out"].split()[0] == live_hash
        record("node-active-file-revocation-preserves-partial-and-explicit-resume",
               partial_bytes_before_revocation=int(partial["out"]), refusal=refused, resumed=resumed)
        prior = status(b)["received_operations"]
        call(b, "trust", "revoke", c["name"])
        rejected = send(c, b, "must-not-be-delivered", check=False)
        assert rejected["code"] != 0 and status(b)["received_operations"] == prior
        call(b, "trust", "restore", c["name"])
        # Allow the documented bounded trust-root refresh, not an operation retry.
        time.sleep(1.2)
        send(c, b, "restored-rights")
        record("node-revocation-and-restoration-on-live-links", refusal=rejected)
        before = status(b)["sent_operations"]
        shape(b, ["loss", "100%"])
        try:
            failed = send(b, c, "partition-fixture", check=False)
            assert failed["code"] != 0
            assert status(b)["sent_operations"] == before
        finally:
            shape(b, [])
        send(b, c, "explicit-after-heal")
        record("node-partition-fails-without-false-ack-and-explicit-heal", failure=failed)
        a["guest"].run("kill", "-KILL", str(processes.pop(a["name"])))
        send(b, c, "after-first-node-crash")
        send(c, b, "survivor-reply")
        record("node-first-node-crash-survivors-continue", status=[status(b), status(c)])
        start(a)
        first = send(b, a, "after-explicit-restart", check=False)
        if first["code"]:
            # A separate explicit request after uncertainty; product never retries data.
            send(b, a, "after-explicit-restart")
        assert status(a)["received_operations"] == 1
        record("node-restart-with-same-identity-and-stale-socket", first_attempt=first, status=status(a))
    finally:
        for node in nodes:
            shape(node, [])
            pid = processes.get(node["name"])
            if pid is not None:
                node["guest"].run("kill", "-INT", str(pid), check=False)
                stopped = node["guest"].wait(pid, timeout=10)
                assert stopped["code"] == 130, stopped
                node["guest"].run("test", "!", "-e", node["profile"] + "/node.sock")
                assert not node["guest"].run("ss", "-ulH", f"sport = :{port}")["out"].strip()
        record("node-graceful-stop-and-socket-release")
