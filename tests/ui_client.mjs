import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(new URL("../src/ui/assets/app.js", import.meta.url), "utf8");

test("an inbox listing failure stays visible without discarding the last file view", async () => {
  const begin = source.indexOf("async function loadFiles()");
  const end = source.indexOf("\nfunction copyPeers", begin);
  assert.ok(begin >= 0 && end > begin);
  const elements = new Map();
  const node = (id) => {
    if (!elements.has(id)) elements.set(id, { hidden: false, textContent: "", count: 1, replaceChildren(...children) { this.count = children.length; } });
    return elements.get(id);
  };
  const context = vm.createContext({ $: node, selected: "team", room: () => ({name:"team"}),
    api: async () => { throw new Error("inbox view limit reached; stored files are intact"); } });
  vm.runInContext(source.slice(begin, end), context);
  await vm.runInContext("loadFiles()", context);
  assert.equal(node("files-panel").hidden, false);
  assert.equal(node("file-problem").hidden, false);
  assert.match(node("file-problem").textContent, /stored files are intact/);
  assert.equal(node("files").count, 1);
  context.api = async () => [];
  await vm.runInContext("loadFiles()", context);
  assert.equal(node("file-problem").hidden, true);
  assert.equal(node("file-problem").textContent, "");
  assert.equal(node("files-panel").hidden, true);
});

for (const status of [401, 403]) {
  test(`HTTP ${status} retires the private session and stops media before parsing a response`, async () => {
    const storage = new Map([["kk-session", "expired-test-session"], ["kk-room", "studio"]]);
    const elements = new Map();
    const node = (id) => {
      if (!elements.has(id)) elements.set(id, { textContent: "", hidden: false, open: true,
        close() { this.open = false; }, replaceChildren(...children) { this.children = children; } });
      return elements.get(id);
    };
    let mediaStops = 0, requests = 0;
    const context = vm.createContext({
      location: { hash: "", pathname: "/" }, history: { replaceState() {} },
      document: { getElementById: node, documentElement: { dataset: {} } },
      sessionStorage: { getItem: (key) => storage.get(key), setItem: (key, value) => storage.set(key, value), removeItem: (key) => storage.delete(key) },
      localStorage: { getItem() {}, setItem() {} }, matchMedia: () => ({ matches: true }),
      addEventListener() {}, recordStop: () => { mediaStops++; },
      fetch: async () => {
        requests++;
        return { status, ok: false, json() { assert.fail("Do not parse an unauthenticated response"); } };
      },
      setInterval() { assert.fail("Do not poll with a retired credential"); },
    });
    const boot = vm.runInContext(source, context);
    vm.runInContext("mediaControl = { stopAll: recordStop }; joinDrafts.set(1, {name:'private-room',bind:'127.0.0.1:1234',role:'reader'});", context);
    await boot;
    assert.equal(storage.has("kk-session"), false);
    assert.equal(storage.get("kk-room"), "studio");
    assert.equal(node("workspace").hidden, true);
    assert.equal(node("gate").hidden, false);
    assert.match(node("gate-text").textContent, /Relaunch kk ui/);
    assert.equal(mediaStops, 1);
    assert.equal(requests, 1);
    assert.equal(vm.runInContext("token", context), "");
    assert.equal(vm.runInContext("joinDrafts.size", context), 0);
    assert.equal(node("dialog").open, false);
    assert.equal(node("dialog-content").children.length, 0);
  });
}
