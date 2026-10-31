"""Temporary, MAC-scoped DHCP permission for the explicitly owned win11 lab."""
import base64
import json
import subprocess
from pathlib import Path

from kk_windows_guest import WindowsGuest

TAG = "kk-win11-l4-dhcp-20261031"
RULE = (f'insert rule inet filter input iifname "virbr0" '
        f'ether saddr 52:54:00:73:2d:9a udp sport 68 udp dport 67 '
        f'counter accept comment "{TAG}"')


def nft(*arguments):
    # This controller is launched only in the operator's armed tmux ops pane.
    result = subprocess.run(["sudo", "-n", "nft", *arguments],
                            capture_output=True, text=True, check=False)
    if result.returncode:
        raise RuntimeError("Authenticated sudo required in this pane: " + result.stderr)
    return result.stdout


def matching_rules():
    state = json.loads(nft("-j", "-a", "list", "chain", "inet", "filter", "input"))
    return [item["rule"] for item in state["nftables"]
            if item.get("rule", {}).get("comment") == TAG]


def main():
    root = Path(__file__).resolve().parents[1] / "tmp"
    matches = matching_rules()
    if not matches:
        nft(RULE)
        matches = matching_rules()
    if len(matches) != 1:
        raise RuntimeError("Temporary DHCP rule is not unique; refusing ambiguity")
    rule = matches[0]
    record = {key: rule[key] for key in ["family", "table", "chain", "handle", "comment", "expr"]}
    (root / "win11-firewall-lease.json").write_text(json.dumps(record, indent=2))
    print("EPHEMERAL RULE: " + json.dumps(record), flush=True)
    script = (root / "win11-native-renew.ps1").read_text(encoding="utf-8")
    encoded = base64.b64encode(script.encode("utf-16-le")).decode("ascii")
    guest = WindowsGuest("win11")
    result = guest.run(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                       "-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded,
                       timeout=120, check=False)
    (root / "win11-native-renew-confirmed.json").write_text(json.dumps(result, indent=2))
    print(result["out"], flush=True)
    if result["err"]:
        print(result["err"], flush=True)
    print("DHCP EXIT: " + str(result["code"]), flush=True)
    return 0 if result["code"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
