#!/usr/bin/env -S uv run --script
"""Native private-display capture, cross-guest decoding and bounded bulk contention."""
import argparse
import base64
import hashlib
import json
import os
import re
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path

from kk_guest import Guest, declared_peers, virsh

ROOT = Path(__file__).resolve().parents[1]
CLI = "/usr/local/bin/kk"


def address(ip, kind=socket.SOCK_DGRAM):
    with socket.socket(socket.AF_INET, kind) as connection:
        connection.bind((ip, 0))
        return f"{ip}:{connection.getsockname()[1]}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receiver", required=True)
    parser.add_argument("--xvfb", default="Xvfb")
    parser.add_argument("--xvfb-libraries", help="Optional private runtime libraries for this test X server only")
    parser.add_argument("--rate-kib", type=int, default=125)
    parser.add_argument("--duration", type=int, default=12)
    options = parser.parse_args()
    if not 64 <= options.rate_kib <= 4096 or not 5 <= options.duration <= 60:
        raise ValueError("Choose bounded private-test bandwidth and duration")
    roster = declared_peers()
    receiver = next(peer for peer in roster if peer["name"] == options.receiver)
    controller = next(peer for peer in roster if peer["name"] != options.receiver)
    guest = Guest(receiver["name"])
    third = Guest(controller["name"])
    definition = ET.fromstring(virsh("net-dumpxml", "kk-isolated"))
    host_ip = definition.find("./ip").get("address")
    binary = ROOT.parent / "target/release/kk"
    run = uuid.uuid4().hex[:12]
    directory = Path(tempfile.mkdtemp(prefix="native-media-", dir=ROOT / "tmp"))
    profile = directory / "host"
    remote = "/var/tmp/kk-room-native-" + run
    host_processes, guest_processes, logs = [], [], []
    members, identities, results, cleanup_errors = [], [], {}, []
    prior_qos, shaped = None, False

    def call(*args, check=True):
        reply = subprocess.run([str(binary), "--profile", str(profile), *args], capture_output=True, text=True, timeout=115, check=False)
        if check and reply.returncode:
            raise RuntimeError(f"Host CLI failed: {args[:3]}: {reply.stdout} {reply.stderr}")
        return reply

    def spawn(label, args, env=None):
        log = (directory / (label + ".log")).open("wb")
        logs.append(log)
        process = subprocess.Popen(args, stdout=log, stderr=log, env=env, start_new_session=True)
        host_processes.append(process)
        return process

    def guest_call(selected, *args, check=True):
        return selected.run(CLI, "--profile", remote + "/profile", *args, timeout=115, check=check)

    def wait_room(selected=None):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            reply = guest_call(selected, "room", "status", "team", check=False) if selected else call("room", "status", "team", check=False)
            code, output = (reply["code"], reply["out"]) if selected else (reply.returncode, reply.stdout)
            if code == 0 and json.loads(output)["result"]["running"]:
                return json.loads(output)["result"]
            time.sleep(0.1)
        raise TimeoutError("Owned native-test room did not become ready")

    def pixel():
        result = guest.run("env", "DISPLAY=" + display, "gst-launch-1.0", "-q", "ximagesrc", "display-name=" + display,
                           "use-damage=false", "num-buffers=1", "!", "videoconvert", "!", "videoscale", "!",
                           "video/x-raw,format=GRAY8,width=16,height=16", "!", "fakesink", "dump=true", timeout=15)
        rows = re.findall(r"^[0-9a-fA-F]{8} \([^)]*\): ((?:[0-9a-fA-F]{2} ?){1,16})", result["out"], re.MULTILINE)
        data = b"".join(bytes.fromhex(row) for row in rows)
        if len(data) != 256:
            raise RuntimeError("Native private-display probe produced no decoded sample: " + str(result))
        return {"mean":sum(data) / len(data), "min":min(data), "max":max(data), "sha256":hashlib.sha256(data).hexdigest()}

    try:
        for program in ["gst-launch-1.0", "Xwayland"]:
            subprocess.run([program, "--version"] if program == "gst-launch-1.0" else [program, "-version"], capture_output=True, timeout=10, check=True)
        for plugin in ["ximagesrc", "vp8enc", "rtpvp8pay", "rtpstreamdepay", "rtpvp8depay", "vp8dec", "autovideosink"]:
            guest.run("gst-inspect-1.0", plugin)
        results["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        for selected in [guest, third]:
            selected.run("mkdir", "-m", "0700", remote)
        identities.append(call("init").stdout.strip().removeprefix("IDENTITY sha256:"))
        host_address = address(host_ip)
        port = host_address.rsplit(":", 1)[1]
        for selected in [guest, third]:
            identities.append(guest_call(selected, "init")["out"].strip().removeprefix("IDENTITY sha256:"))
        guest_call(third, "room", "create", "team", "--bind", f"{controller['ip']}:{port}", "--title", "Native isolated media contention")
        pid = third.start(CLI, "--profile", remote + "/profile", "room", "serve", "team")
        guest_processes.append((third, pid))
        wait_room(third)
        members.append(third)
        for index, (selected, peer) in enumerate([(guest, receiver), (None, None)]):
            offer = "/var/tmp/kk-room-" + uuid.uuid4().hex[:12]
            third.run("mkdir", "-m", "0700", offer)
            invitation = offer + "/scoped-invitation.json"
            inviter = third.start(CLI, "--profile", remote + "/profile", "room", "invite", "team", "--bind", f"{controller['ip']}:{int(port) + 1}", "--invite-out", invitation, "--role", "moderator", "--confirm")
            guest_processes.append((third, inviter))
            deadline = time.monotonic() + 10
            while third.run("test", "-f", invitation, check=False)["code"]:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Owned native-test invitation was not published")
                time.sleep(0.05)
            bundle = third.room_invitation(invitation)
            if selected:
                destination = remote + f"/invitation-{index}.json"
                selected.run("touch", destination)
                selected.run("chmod", "0600", destination)
                selected.upload(destination, bundle)
                pid = selected.start(CLI, "--profile", remote + "/profile", "room", "join", "team", "--invitation", destination, "--bind", f"{peer['ip']}:{port}", "--role", "moderator", "--confirm", "--accept-files", "--inbox", remote + "/inbox", "--max-bytes", "2097152", "--file-budget", "4194304")
                guest_processes.append((selected, pid))
            else:
                destination = directory / "host-invitation.json"
                with os.fdopen(os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as output:
                    output.write(bundle)
                # Establish normal outbound return tuples to declared peers only.
                # This is explicit test network preparation, not a firewall change
                # or a claim of unaided inbound traversal by the product.
                with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
                    probe.bind((host_ip, int(port)))
                    for target in [receiver, controller]:
                        probe.sendto(b"kk-native-outbound-flow", (target["ip"], int(port)))
                results["host_network_preparation"] = "outbound UDP return tuples to declared peers; no firewall rules changed; not an inbound traversal proof"
                spawn("host-room", [str(binary), "--profile", str(profile), "room", "join", "team", "--invitation", str(destination), "--bind", host_address, "--role", "moderator", "--confirm"])
            del bundle
            wait_room(selected)
            members.append(selected)
            admitted = third.wait(inviter, timeout=110)
            guest_processes.remove((third, inviter))
            if admitted["code"]:
                raise RuntimeError("Owned native-test admission failed")
        call("room", "controllers", "team", *identities)

        number = 90 + int(run[:3], 16)
        display = ":" + str(number)
        if guest.run("test", "-e", "/tmp/.X11-unix/X" + str(number), check=False)["code"] == 0:
            raise RuntimeError("Private test display number is already in use")
        display_args = [options.xvfb, display, "-screen", "0", "640x360x24", "-nolisten", "tcp", "-ac", "-noreset"]
        if options.xvfb_libraries:
            display_args = ["env", "LD_LIBRARY_PATH=" + options.xvfb_libraries, *display_args]
        xserver = guest.start(*display_args)
        results["private_xvfb_libraries"] = options.xvfb_libraries
        guest_processes.append((guest, xserver))
        deadline = time.monotonic() + 10
        while guest.run("test", "-S", "/tmp/.X11-unix/X" + str(number), check=False)["code"]:
            if time.monotonic() >= deadline:
                raise TimeoutError("Private guest X server did not become ready")
            time.sleep(0.1)
        results["blank_private_display_sample"] = pixel()

        read_fd, write_fd = os.pipe()
        log = (directory / "private-xwayland.log").open("wb")
        logs.append(log)
        host_number = 100 + int(run[-3:], 16)
        if Path("/tmp/.X11-unix/X" + str(host_number)).exists():
            raise RuntimeError("Owned host display number is already in use")
        server = subprocess.Popen(["Xwayland", ":" + str(host_number), "-geometry", "640x360", "-shm", "-nokeymap", "-displayfd", str(write_fd), "-nolisten", "tcp", "-noreset", "-maxclients", "64"], pass_fds=(write_fd,), stdout=log, stderr=log, start_new_session=True)
        host_processes.append(server)
        os.close(write_fd)
        try:
            if not select.select([read_fd], [], [], 10)[0]:
                raise TimeoutError("Private host Xwayland did not publish its display")
            number = os.read(read_fd, 32).decode().strip()
            if not number.isdecimal():
                raise RuntimeError("Private Xwayland returned an invalid display")
        finally:
            os.close(read_fd)
        source_display = ":" + number
        spawn("owned-pattern", ["gst-launch-1.0", "-q", "videotestsrc", "is-live=true", "pattern=snow", "!", "video/x-raw,width=640,height=360,framerate=15/1", "!", "videoconvert", "!", "ximagesink", "display=" + source_display])
        listening = guest.start("env", "DISPLAY=" + display, CLI, "--profile", remote + "/profile", "room", "media", "team", "listen", "screen", "--from", identities[0], "--x11", "--consent")
        guest_processes.append((guest, listening))
        deadline = time.monotonic() + 15
        while wait_room(guest)["media"]["subscriptions"] < 1:
            if time.monotonic() >= deadline:
                raise TimeoutError("Native playback consent did not become ready")
            time.sleep(0.1)
        publisher = spawn("native-publish", [str(binary), "--profile", str(profile), "room", "media", "team", "publish", "screen", "--device", source_display, "--to", identities[1], "--consent"])
        deadline = time.monotonic() + 8
        while True:
            baseline = pixel()
            if baseline["mean"] > results["blank_private_display_sample"]["mean"] + 20:
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("Unshaped native playback did not render into the explicit private X11 display")
            time.sleep(0.2)
        results["unshaped_decoded_sample"] = baseline
        prior_qos = virsh("domiftune", receiver["name"], receiver["mac"], "--live")
        if any(int(value) for value in re.findall(r":\s*(\d+)", prior_qos)):
            raise RuntimeError("Refusing to replace an existing guest bandwidth policy")
        shaped = True
        virsh("domiftune", receiver["name"], receiver["mac"], "--inbound", f"{options.rate_kib},0,32", "--live")
        results["live_bandwidth_kib_per_second"] = options.rate_kib
        payload = directory / "bulk.bin"
        data = b"native-media-contention-" * 45590
        payload.write_bytes(data)
        expected = hashlib.sha256(data).hexdigest()
        started = time.monotonic()
        bulk = spawn("bulk", [str(binary), "--profile", str(profile), "room", "send", "team", str(payload), "-f", "--to", identities[1]])
        latencies, bulk_end, bulk_samples = [], None, 0
        decoded_during_bulk = False
        until = started + options.duration
        while time.monotonic() < until:
            during_bulk = bulk.poll() is None
            if not during_bulk and bulk_end is None:
                bulk_end = time.monotonic()
            before = time.monotonic()
            delivered = json.loads(call("room", "send", "team", "native-contention-" + run).stdout)
            latencies.append(time.monotonic() - before)
            bulk_samples += int(during_bulk)
            if not delivered["result"]["all_acknowledged"]:
                raise RuntimeError("Text was not acknowledged under native media/bulk contention")
            if not decoded_during_bulk and during_bulk and time.monotonic() - started >= 1:
                sample = pixel()
                decoded_during_bulk = sample["mean"] > results["blank_private_display_sample"]["mean"] + 20 and sample["sha256"] != baseline["sha256"]
                results["sample_during_bulk"] = sample
            time.sleep(0.25)
        results["decoded_private_display_sample"] = pixel()
        results["text_ack_seconds"] = latencies
        results["text_ack_max_seconds"] = max(latencies)
        results["text_samples_during_bulk"] = bulk_samples
        results["native_decoded_during_bulk"] = decoded_during_bulk
        if results["decoded_private_display_sample"]["mean"] <= results["blank_private_display_sample"]["mean"] + 20:
            raise RuntimeError("Private guest display did not show decoded native video")
        publisher.send_signal(signal.SIGINT)
        if publisher.wait(timeout=20):
            raise RuntimeError("Native publisher did not stop cleanly")
        playback = guest.wait(listening, timeout=25)
        guest_processes.remove((guest, listening))
        if playback["code"]:
            raise RuntimeError("Native receiver failed: " + str(playback))
        results["native_receiver"] = json.loads(playback["out"])
        if not results["native_receiver"]["playback"] or results["native_receiver"]["packets"] < 15:
            raise RuntimeError("Too few real native packets reached playback")
        if bulk.wait(timeout=100):
            raise RuntimeError("Bulk file failed during native playback")
        results["bulk_completion_observed_seconds"] = (bulk_end or time.monotonic()) - started
        results["text_samples_during_bulk"] = bulk_samples
        results["native_decoded_during_bulk"] = decoded_during_bulk
        if bulk_samples < 2 or not decoded_during_bulk:
            raise RuntimeError("Native decoding and foreground acknowledgements did not overlap the active bulk transfer")
        received = remote + "/inbox/" + identities[0] + "-" + expected + ".bin"
        actual = guest.run("sha256sum", received)["out"].split()[0]
        if actual != expected:
            raise RuntimeError("Bulk content mismatch under contention")
        results["bulk_sha256"] = actual
        results["text_ack_seconds"] = latencies
        results["text_ack_max_seconds"] = max(latencies)
        if max(latencies) > 5:
            raise RuntimeError("Foreground text latency exceeded the bounded contention target")
        print("PASS: actual private-display native capture, cross-guest decode and bulk/control contention", flush=True)
    finally:
        primary = sys.exception()
        if shaped:
            try:
                virsh("domiftune", receiver["name"], receiver["mac"], "--inbound", "0", "--live")
                restored = virsh("domiftune", receiver["name"], receiver["mac"], "--live")
                if restored != prior_qos:
                    raise RuntimeError("Guest bandwidth policy did not return to its exact prior state")
            except Exception as error:
                cleanup_errors.append(str(error))
        for selected in reversed(members):
            try:
                if selected:
                    guest_call(selected, "room", "stop", "team")
                else:
                    call("room", "stop", "team")
            except Exception as error:
                cleanup_errors.append(str(error))
        for selected, pid in guest_processes:
            try:
                status = selected.agent("guest-exec-status", pid=pid)
                if not status.get("exited"):
                    selected.run("kill", "-INT", str(pid), check=False)
                    final = selected.wait(pid, timeout=20)
                else:
                    final = {"code": status.get("exitcode", 128 + status.get("signal", 0)), "out":base64.b64decode(status.get("out-data", "")).decode(errors="replace"), "err":base64.b64decode(status.get("err-data", "")).decode(errors="replace")}
                results.setdefault("guest_processes", []).append({"guest":selected.name,"pid":pid,**final})
            except Exception as error:
                cleanup_errors.append(str(error))
        for process in reversed(host_processes):
            try:
                if process.poll() is None:
                    process.send_signal(signal.SIGINT)
                    process.wait(timeout=20)
            except Exception as error:
                cleanup_errors.append(str(error))
        for log in logs:
            log.close()
        report = ROOT / "tmp" / f"native-media-{run}.json"
        report.write_text(json.dumps({"scope":"Native owned private displays only; actual cross-guest decode and live libvirt inbound shaping; no host desktop or hardware capture", "run_id":run, "directory":str(directory), "receiver":receiver, "controller":controller, "results":results, "failure":str(primary) if primary else None, "cleanup_errors":cleanup_errors}, indent=2) + "\n")
        print("Evidence: " + str(report), flush=True)
        if cleanup_errors:
            if primary is None:
                raise RuntimeError("Native contention cleanup failed: " + "; ".join(cleanup_errors))
            for error in cleanup_errors:
                primary.add_note("Cleanup: " + error)


if __name__ == "__main__":
    main()
