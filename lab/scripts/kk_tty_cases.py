"""Drive a real guest PTY through util-linux script, using synthetic input only."""

import re
import shlex
import time
import uuid

from kk_guest import Guest


def tty_cases(peers, listening, port, results):
    receiver, sender = peers
    a, b = Guest(receiver["name"]), Guest(sender["name"])
    work = "/var/tmp/kk-tty-" + uuid.uuid4().hex[:12]
    b.run("mkdir", "-m", "0700", work)
    fixture = b"synthetic-hidden-tty-fixture"

    def start(name):
        fifo, trace = work + "/" + name + ".in", work + "/" + name + ".log"
        b.run("mkfifo", "-m", "0600", fifo)
        command = shlex.join(["/usr/local/bin/kk", "send", f"{receiver['ip']}:{port}", "-s",
                              "--identity", sender["identity"], "--peer", sender["certificate"], "--timeout", "10"])
        shell = 'printf "BEFORE "; stty -g; ' + command + '; code=$?; printf "AFTER "; stty -g; exit "$code"'
        driver = "exec 3<>" + shlex.quote(fifo) + "; exec script -q -f -e -E always -c " + shlex.quote(shell) + " " + shlex.quote(trace) + " <&3"
        pid = b.start("sh", "-c", driver)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if b.run("grep", "-q", "Sensitive input:", trace, check=False)["code"] == 0:
                return pid, fifo, trace
            time.sleep(0.1)
        raise TimeoutError("hidden PTY prompt did not appear")

    def feed(fifo, data):
        process = b.start("dd", "of=" + fifo, "status=none", input_data=data)
        assert b.wait(process)["code"] == 0

    def flags(output):
        before = re.search(r"BEFORE ([^\r\n]+)", output)
        after = re.search(r"AFTER ([^\r\n]+)", output)
        assert before and after and before[1] == after[1], "terminal attributes were not restored"

    receive = a.start("/usr/local/bin/kk", "receive", "--bind", f"{receiver['ip']}:{port}",
                      "--identity", receiver["identity"], "--peer", receiver["certificate"], "--timeout", "15")
    listening(a)
    process, fifo, _ = start("complete")
    feed(fifo, fixture + b"\n")
    sent, received = b.wait(process), a.wait(receive)
    assert sent["code"] == 0 and received["code"] == 0, (sent, received)
    assert fixture.decode() not in str(sent) and fixture.decode() not in str(received)
    flags(sent["out"])
    results.append({"scenario": "tty-hidden-input-and-restore", "sender_result": sent, "receiver_result": received})

    process, _, _ = start("cancel")
    pending, owned = [process], []
    while pending:
        parent = pending.pop()
        children = b.run("ps", "--ppid", str(parent), "-o", "pid=,comm=", check=False)["out"]
        for line in children.splitlines():
            pid, command = line.split(maxsplit=1)
            if command == "kk":
                owned.append(int(pid))
            else:
                pending.append(int(pid))
    assert len(owned) == 1, owned
    b.run("kill", "-INT", str(owned[0]))
    cancelled = b.wait(process, timeout=10)
    assert cancelled["code"] == 130, cancelled
    flags(cancelled["out"])
    results.append({"scenario": "tty-cancellation-and-restore", "result": cancelled})
