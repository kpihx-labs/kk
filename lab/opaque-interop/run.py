"""Bounded independent upstream/restricted OPAQUE wire checks; no key logging."""
import json
import signal
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def check_graph(kind):
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", str(ROOT / kind / "Cargo.toml"),
        "--offline", "--locked", "--format-version", "1",
    ], timeout=30))
    for name, version in [("opaque-ke", "4.0.1"), ("voprf", "0.5.0")]:
        package = next(p for p in metadata["packages"] if p["name"] == name and p["version"] == version)
        if kind == "reference":
            if not (package["source"] or "").startswith("registry+"):
                raise RuntimeError("reference unexpectedly uses a patched SDK")
        elif package["source"] is not None or not Path(package["manifest_path"]).resolve().is_relative_to(
            ROOT.parents[1] / "third_party/overrides"
        ):
            raise RuntimeError("restricted peer does not use the product overrides")
    print(f"PASS independently resolved SDK graph: {kind}")


def frame(peer):
    header = peer.stdout.read(4)
    if len(header) != 4:
        raise EOFError("test peer refused or closed its protocol frame")
    size = struct.unpack("!I", header)[0]
    if size > 16_384:
        raise ValueError("oversized test frame")
    data = peer.stdout.read(size)
    if len(data) != size:
        raise EOFError("truncated test frame")
    return data


def transfer(source, destination):
    data = frame(source)
    destination.stdin.write(struct.pack("!I", len(data)) + data)
    destination.stdin.flush()


def run(server_kind, client_kind, negative):
    peers = []
    phase = "registration"
    signal.alarm(30)
    try:
        for kind, role in [(server_kind, "server"), (client_kind, "client")]:
            binary = ROOT / kind / "target/debug/opaque-wire-probe"
            args = [str(binary), role]
            if role == "client" and negative:
                args.append(negative)
            peers.append(subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                          stderr=subprocess.PIPE))
        server, client = peers
        transfer(client, server)
        transfer(server, client)
        transfer(client, server)
        phase = "login"
        transfer(client, server)
        transfer(server, client)
        transfer(client, server)
        client_hash, server_hash = frame(client), frame(server)
        if negative:
            raise RuntimeError("a negative exchange succeeded")
        if len(client_hash) != 32 or client_hash != server_hash:
            raise RuntimeError("session mismatch")
        for peer in peers:
            peer.stdin.close()
            if peer.wait(timeout=5) != 0:
                raise RuntimeError("successful peer failed")
    except EOFError:
        if not negative or phase != "login":
            raise
        if peers[1].wait(timeout=5) == 0:
            raise RuntimeError("negative client did not reject")
    finally:
        for peer in peers:
            if peer.poll() is None:
                peer.kill()
            peer.wait(timeout=5)
            peer.stdin.close()
            peer.stdout.close()
            peer.stderr.close()
        signal.alarm(0)
    print(f"PASS server={server_kind} client={client_kind} case={negative or 'success'}")


def expired(*_):
    raise TimeoutError("bounded interop test deadline exceeded")


signal.signal(signal.SIGALRM, expired)
for kind in ["reference", "restricted"]:
    check_graph(kind)
for server_kind, client_kind in [("reference", "restricted"), ("restricted", "reference")]:
    for negative in [None, "wrong-password", "wrong-context"]:
        run(server_kind, client_kind, negative)
