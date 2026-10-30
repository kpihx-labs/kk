const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");

function harness(permission = "granted") {
  const calls = [], timers = new Map(), opened = [], failures = [];
  let nextTimer = 0, prompts = 0, rejectConstruction = false;
  class Notification {
    static permission = permission;
    static async requestPermission() { prompts++; Notification.permission = "granted"; return "granted"; }
    constructor(title, options) {
      if (rejectConstruction) throw new Error("unavailable");
      this.title = title; this.options = options; this.closed = false; calls.push(this);
    }
    close() { this.closed = true; }
  }
  const platform = {Notification, setTimeout: callback => { timers.set(++nextTimer, callback); return nextTimer; }, clearTimeout: id => timers.delete(id)};
  const view = {hidden:()=>true, focused:()=>false, selected:()=>"study", open:name=>opened.push(name), unavailable:()=>failures.push(true)};
  const context = vm.createContext({});
  vm.runInContext(source.slice(source.indexOf("function messageNotifications("), source.indexOf("function toast(")), context);
  const control = context.messageNotifications(platform, view);
  return {control, platform, view, calls, timers, opened, failures, prompts:()=>prompts, reject:value=>{rejectConstruction=value;}};
}
const message = (id, changes = {}) => ({id, room:"study", outgoing:false, sensitive:false, ...changes});
const snapshot = (messages, enabled = true, rooms = ["study", "other"]) => ({settings:{message_notifications:enabled}, messages, rooms:rooms.map(name=>({name}))});

test("default off, first snapshot and opt-in transitions never backfill or request permission", () => {
  const h = harness();
  h.control.observe(snapshot([message(1)], false));
  h.control.observe(snapshot([message(1), message(2)], false));
  h.control.observe(snapshot([message(1), message(2), message(3)]));
  assert.equal(h.calls.length, 0); assert.equal(h.prompts(), 0);
  const privateMessage = message(4);
  for (const property of ["text", "sender", "reference"]) Object.defineProperty(privateMessage, property, {get(){throw new Error("private field was read");}});
  h.control.observe(snapshot([privateMessage]));
  assert.equal(h.calls.length, 1);
  assert.equal(h.calls[0].title, "K&K");
  assert.deepEqual(Object.keys(h.calls[0].options).sort(), ["body", "silent", "tag"]);
  assert.equal(h.calls[0].options.body, "New message received. Open K&K to view it.");
});

test("selected visible messages, outgoing, sensitive, malformed and unknown-room events do not alert", () => {
  const h = harness(); h.view.hidden=()=>false; h.view.focused=()=>true;
  h.control.observe(snapshot([]));
  h.control.observe(snapshot([message(1), message(2, {outgoing:true}), message(3, {sensitive:true}), message(4, {room:"not joined"}), message(Number.MAX_SAFE_INTEGER + 1)]));
  assert.equal(h.calls.length, 0);
  h.control.observe(snapshot([message(5, {room:"other"})]));
  assert.equal(h.calls.length, 1);
  h.calls[0].onclick(); assert.deepEqual(h.opened, ["other"]);
});

test("polling replay is deduplicated without merging equal bodies; only one alert is retained", () => {
  const h = harness(); h.control.observe(snapshot([]));
  h.control.observe(snapshot([message(1, {text:"same"})]));
  const old = h.calls[0], oldTimer = [...h.timers.values()][0];
  h.control.observe(snapshot([message(1, {text:"same"})]));
  assert.equal(h.calls.length, 1);
  h.control.observe(snapshot([message(2, {text:"same"})]));
  assert.equal(h.calls.length, 2); assert.equal(old.closed, true); assert.equal(h.timers.size, 1);
  oldTimer(); old.onclick();
  assert.equal(h.calls[1].closed, false); assert.equal(h.opened.length, 0);
  h.calls[1].onclick(); assert.equal(h.calls[1].closed, true); assert.deepEqual(h.opened, ["study"]);
});

test("permission changes and disabled snapshots never backfill; revocation closes the active alert", async () => {
  const h = harness("default"); h.control.observe(snapshot([]));
  h.control.observe(snapshot([message(1)])); assert.equal(h.prompts(), 0);
  await h.control.allow(); assert.equal(h.prompts(), 1);
  h.control.observe(snapshot([message(2)])); assert.equal(h.calls.length, 0);
  h.control.observe(snapshot([message(3)])); assert.equal(h.calls.length, 1);
  h.platform.Notification.permission="denied";
  h.control.observe(snapshot([message(4)])); assert.equal(h.calls[0].closed, true);
  await assert.rejects(h.control.allow(), /not allowed/); assert.equal(h.prompts(), 1);
  h.control.observe(snapshot([message(5)], false)); assert.equal(h.calls.length, 1);
});

test("retirement and room removal invalidate pending click and timer callbacks", () => {
  const h = harness(); h.control.observe(snapshot([])); h.control.observe(snapshot([message(1)]));
  const first = h.calls[0]; h.control.observe(snapshot([message(1)], true, []));
  assert.equal(first.closed, true); first.onclick(); assert.equal(h.opened.length, 0);
  h.control.observe(snapshot([message(2)])); const last=h.calls[1];
  h.control.retire(); last.onclick(); h.control.observe(snapshot([message(3)]));
  assert.equal(last.closed, true); assert.equal(h.calls.length, 2); assert.equal(h.timers.size, 0); assert.equal(h.opened.length, 0);
});

test("unsupported and failed notifications remain bounded until an explicit new permission action", async () => {
  const h = harness(); h.control.observe(snapshot([])); h.reject(true);
  h.control.observe(snapshot([message(1)])); h.control.observe(snapshot([message(2)]));
  assert.equal(h.failures.length, 1); assert.equal(h.timers.size, 0);
  h.reject(false); await h.control.allow(); h.control.observe(snapshot([message(3)])); assert.equal(h.calls.length, 1);
  const absent=harness(); delete absent.platform.Notification;
  assert.equal(absent.control.permission(), "unsupported");
  await assert.rejects(absent.control.allow(), /unavailable/);
  absent.control.observe(snapshot([])); absent.control.observe(snapshot([message(1)])); assert.equal(absent.calls.length, 0);
});

test("duplicate permission requests are reserved before awaiting and retirement wins late grants", async () => {
  const h=harness("default"); let resolve;
  h.platform.Notification.requestPermission=()=>new Promise(done=>{resolve=done;});
  const pending=h.control.allow();
  await assert.rejects(h.control.allow(), /already pending/);
  h.control.retire(); resolve("granted"); await assert.rejects(pending, /ended/);
  h.control.observe(snapshot([message(1)])); assert.equal(h.calls.length, 0);
});
