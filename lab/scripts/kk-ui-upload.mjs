#!/usr/bin/env node
// Deliver only an owned, loopback test invitation; never print its capability.
import { spawnSync } from "node:child_process";
import { lstat, mkdtemp, readFile, unlink, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";

try {
  const [source, fingerprint, target] = process.argv.slice(2);
  if (!source || !/^[a-f0-9]{64}$/.test(fingerprint || "") || !/^[A-F0-9]{32}$/.test(target || "")) {
    throw new Error("Expected downloaded test invitation path, expected public fingerprint and owned target ID");
  }
  const path = resolve(source), info = await lstat(path);
  if (dirname(path) !== join(homedir(), "Downloads") || !/^kk-invitation(?: \(\d+\))?\.json$/.test(basename(path)) ||
      !info.isFile() || info.uid !== process.getuid() || info.size > 4096) {
    throw new Error("Not an owned bounded test invitation download");
  }
  const bytes = await readFile(path), invitation = JSON.parse(bytes.toString("utf8"));
  if (invitation.version !== 2 || invitation.fingerprint !== fingerprint || !/^127\.0\.0\.1:\d+$/.test(invitation.address) ||
      !Array.isArray(invitation.room) || invitation.room.length !== 32) {
    throw new Error("Invitation is not for the expected loopback test identity and room");
  }
  const directory = await mkdtemp(join(tmpdir(), "kk-ui-upload-")), payload = join(directory, "payload.json");
  try {
    const expression = `(async () => {
      const input = document.querySelector('#dialog-form input[name=invitation-file]');
      if (!input || typeof input.onchange !== 'function') throw new Error('Expected owned invitation file input');
      const bytes = Uint8Array.from(atob(${JSON.stringify(bytes.toString("base64"))}), char => char.charCodeAt(0));
      const transfer = new DataTransfer();
      transfer.items.add(new File([bytes], 'kk-invitation.json', {type:'application/json'}));
      input.files = transfer.files;
      await input.onchange({target:input});
      return {attached: input.files.length === 1 && !!document.querySelector('textarea[name=invitation]').value};
    })()`;
    await writeFile(payload, JSON.stringify({ profile: "default", target_id: target, await_promise: true, expression }), { mode: 0o600 });
    const result = spawnSync("browser-proxy", ["do", "page-evaluate", payload], { encoding: "utf8", timeout: 20000 });
    if (result.status !== 0) throw new Error("Browser refused opaque test invitation delivery");
    const envelope = JSON.parse(result.stdout);
    if (envelope.meta.status !== "ok" || envelope.data.result?.attached !== true) {
      throw new Error("Browser did not attach the opaque invitation to the owned page");
    }
    console.log("Owned test invitation attached and its page handler completed; capability remained opaque.");
  } finally {
    await unlink(payload);
    bytes.fill(0);
  }
} catch (error) {
  console.error(error instanceof SyntaxError ? "Invalid test invitation or browser response" : error.message);
  process.exitCode = 1;
}
