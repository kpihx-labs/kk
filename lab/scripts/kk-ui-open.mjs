#!/usr/bin/env node
// Test-only opaque launch delivery to an explicitly selected owned window. No token file
// is read, and no URL/fragment or browser-storage credential is printed.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmdirSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

function call(action, payload) {
  const directory = mkdtempSync(join(tmpdir(), "kk-ui-open-")), output = join(directory, "result.json");
  try {
    const result = spawnSync("browser-proxy", ["do", action, JSON.stringify(payload), "-o", output], { encoding: "utf8", timeout: 20000 });
    if (result.status !== 0) throw new Error("Test browser action failed: " + action);
    const envelope = JSON.parse(readFileSync(output, "utf8"));
    if (envelope.meta.status !== "ok") throw new Error("Test browser action refused: " + action);
    return envelope.data;
  } finally {
    if (existsSync(output)) unlinkSync(output);
    rmdirSync(directory);
  }
}
try {
  if (process.argv.length !== 6) throw new Error("Expected profile, owned window ID, protected launch path and public base URL");
  const profile = process.argv[2], windowId = Number(process.argv[3]);
  if (!Number.isSafeInteger(windowId) || windowId <= 0) throw new Error("Invalid owned test window ID");
  const url = pathToFileURL(process.argv[4]).href, origin = process.argv[5];
  const window = call("window-list", { profile }).windows.find((window) => window.window_id === windowId);
  if (!window) throw new Error("Owned test window no longer exists");
  const tabs = window.chrome_layout.tabs;
  const target = tabs.find((tab) => tab.url?.startsWith(origin + "/")) || tabs.find((tab) => tab.url?.startsWith("edge://newtab") || tab.url === "about:blank");
  if (target) call("tab-update", { profile, tab_id: target.chrome_tab_id, url });
  else call("tab-create", { profile, window_id: windowId, url });
  console.log("Private test workspace opened; credentials remained opaque.");
} catch (error) {
  console.error(error.message); process.exitCode = 1;
}
