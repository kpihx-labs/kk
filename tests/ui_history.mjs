import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(new URL("../src/ui/assets/app.js", import.meta.url), "utf8");
const functions = source.slice(source.indexOf("function referenceKey("), source.indexOf("\nfunction render()"));
const ref = (digest = 1, event = "IncomingProcessed") => ({room:Array(32).fill(2), member:Array(32).fill(3), epoch:4, event, wire_sha256:Array(32).fill(digest)});
const row = (digest = 1, body = "same 雪 %_") => ({reference:ref(digest), record:{id:digest, event:"IncomingProcessed", epoch:4, wire_sha256:Array(32).fill(digest), body}});
function context() { const ctx = vm.createContext({}); vm.runInContext(functions, ctx); return ctx; }

test("history merges only the complete authenticated tuple, never equal text or transient IDs", () => {
  const ctx = context(); ctx.rows = [row(1), row(2)];
  ctx.live = [{id:777, reference:ref(1), text:"same 雪 %_"}, {id:778, reference:null, text:"same 雪 %_"}, {id:779, reference:null, text:"same 雪 %_"}];
  assert.equal(vm.runInContext("historyEntries(rows, live).length", ctx), 4);
  assert.equal(vm.runInContext("historyEntries(rows, live).filter(entry => entry.stored).length", ctx), 2);
  ctx.live[0].reference = ref(1, "OutgoingPrepared");
  assert.equal(vm.runInContext("historyEntries(rows, live).length", ctx), 5);
  ctx.rows[0].record.epoch++;
  assert.throws(() => vm.runInContext("historyEntries(rows, live)", ctx), /Invalid authenticated history reference/);
});

test("unknown, malformed and locked references/status never become empty-history or sent/read claims", () => {
  const ctx = context();
  for (const value of [null, {}, {...ref(), room:[1]}, {...ref(), epoch:1.5}, {...ref(), event:"Read"}, {...ref(), wire_sha256:Array(32).fill(-1)}]) {
    ctx.reference = value; assert.equal(vm.runInContext("referenceKey(reference)", ctx), null);
  }
  ctx.summary = {Enrolled:{enabled:true, unlocked:false}};
  assert.match(vm.runInContext("historyStateText(summary)", ctx), /LOCKED.*not empty history/);
  assert.match(vm.runInContext("historyStateText('Unknown')", ctx), /UNKNOWN/);
  assert.match(vm.runInContext("historyStateText('Off')", ctx), /OFF.*not enrolled/);
});

class Node {
  constructor(tag, cls, text = "") { this.tag = tag; this.className = cls; this.textContent = text; this.children = []; this.disabled = false; this.isConnected = true; this.value = ""; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  querySelector() { return this.input; }
}
function ui() {
  const calls = [], elements = new Map(), dialogs = [], target = {name:"team", title:"Team", status:{}};
  const ctx = vm.createContext({
    token:"synthetic session", selected:"team", historyView:null, current:{messages:[]}, room:() => target,
    memberName:() => "Member", toast() {}, el:(...args) => new Node(...args),
    field:(label, name) => { const node = new Node("label", "", label); node.input = new Node("input"); node.input.name = name; return node; },
    $: id => { if (!elements.has(id)) elements.set(id, new Node(id)); return elements.get(id); },
    modal:(title, controls, submit, handler) => { dialogs.push({title, controls, submit, handler}); },
    api:async (path, body) => { assert.equal(path, "/api/action"); calls.push(body); return {result:body.command.operation === "search" ? {rows:[row(calls.length)], next_after:calls.length, has_more:true, withheld:2} : "Off"}; },
  });
  vm.runInContext(functions, ctx);
  return {ctx, calls, elements, dialogs};
}

test("opening the panel requests cached status only; Unicode literal paging uses the frozen submitted query and actual cursor", async () => {
  const {ctx, calls, elements, dialogs} = ui(); vm.runInContext("historyDialog()", ctx);
  await Promise.resolve();
  assert.deepEqual(calls.map(call => call.command.operation), ["status"]);
  const controls = dialogs[0].controls;
  const input = controls.find(node => node.input)?.input; input.value = "雪 %_";
  await elements.get("dialog-form").onsubmit({preventDefault(){}});
  assert.equal(calls[1].command.text, "雪 %_"); assert.equal(calls[1].command.after_id, 0);
  input.value = "changed but not submitted";
  const next = controls.find(node => node.textContent === "Next stored page");
  await next.onclick();
  assert.equal(calls[2].command.text, "雪 %_"); assert.equal(calls[2].command.after_id, 2);
  assert.match(controls.find(node => node.textContent.includes("withheld"))?.textContent, /2 withheld.*cursor 3/);
});

test("enable, clear and orphan recovery require their own explicit confirmation and only accept a public recipient", async () => {
  for (const [label, operation] of [["Enable future capture", "enable"], ["Clear stored records", "clear"], ["Recover orphan key", "recover"]]) {
    const {ctx, calls, dialogs} = ui(); vm.runInContext("historyDialog()", ctx); await Promise.resolve();
    const buttons = dialogs[0].controls.find(node => node.className === "history-controls").children;
    await buttons.find(button => button.textContent === label).onclick();
    assert.deepEqual(calls.map(call => call.command.operation), ["status"]);
    const confirmation = dialogs.at(-1); assert.match(confirmation.title, operation === "recover" ? /Confirm orphan/ : operation === "clear" ? /Confirm clear/ : /Enable local/);
    const recipient = "age1public-confirmation-fixture";
    await confirmation.handler({get:() => recipient});
    assert.equal(calls[1].command.operation, operation); assert.equal(calls[1].command.confirm, true);
    if (operation === "recover") { assert.equal(calls[1].command.expected_recipient, recipient); assert.match(confirmation.controls.at(-1).textContent, /PUBLIC/); }
    assert.equal(calls.some(call => "path" in call.command || "key" in call.command), false);
  }
});

test("a failed page is unavailable, not a successful empty page, and a retired panel ignores late responses", async () => {
  const {ctx, calls, elements, dialogs} = ui(); vm.runInContext("historyDialog()", ctx); await Promise.resolve();
  ctx.api = async () => { throw new Error("key unavailable"); };
  await elements.get("dialog-form").onsubmit({preventDefault(){}});
  assert.match(dialogs[0].controls[1].textContent, /Unavailable.*No empty-history claim.*key unavailable/);
  assert.equal(dialogs[0].controls.find(node => node.textContent === "Next stored page").disabled, true);
  let resolve;
  ctx.api = () => new Promise(done => { resolve = done; });
  const pending = elements.get("dialog-form").onsubmit({preventDefault(){}});
  ctx.historyView = null; resolve({result:{rows:[row()], next_after:1, has_more:false, withheld:0}}); await pending;
  assert.equal(calls.length, 1);
  assert.equal(dialogs[0].controls.at(-1).children.length, 0);
});

test("closing a pending confirmed mutation does not reopen the history panel after commit", async () => {
  const {ctx, dialogs} = ui(); vm.runInContext("historyDialog()", ctx); await Promise.resolve();
  const controls = dialogs[0].controls.find(node => node.className === "history-controls");
  await controls.children.find(button => button.textContent === "Clear stored records").onclick();
  let resolve;
  ctx.api = () => new Promise(done => { resolve = done; });
  const pending = dialogs.at(-1).handler({get(){}});
  ctx.historyView = null;
  resolve({result:"Off"}); await pending;
  assert.equal(dialogs.length, 2);
});
