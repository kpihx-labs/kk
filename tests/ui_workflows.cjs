const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");

test("join import rejects replacement and ignores stale file reads", async () => {
  const controls = [], notices = [];
  const context = { current: {identity: "test"}, joinDraft:{name:"", bind:"", role:"participant"}, toast: text => notices.push(text),
    field: (_label, name) => { const input = {value:"", addEventListener(_event, fn) { this.input = fn; }}; const wrapper = {name, input, querySelector: () => input}; controls.push(wrapper); return wrapper; },
    roleField: () => ({}), modal: (_title, _controls, _submit, handler) => { context.submit = handler; },
    act: async () => { throw new Error("join failed"); }, select: () => { throw new Error("must not select on failure"); },
  };
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function joinDialog()"), source.indexOf("function retryJoin(")) + "\njoinDialog();", context);
  const area = controls.find(item => item.name === "invitation").input;
  const upload = controls.find(item => item.name === "invitation-file").input;
  let resolve; const delayed = new Promise(done => { resolve = done; });
  const oldRead = upload.onchange({target:{files:[{size:2, text: () => delayed}]}});
  await upload.onchange({target:{files:[{size:5000}]}});
  resolve('{"old":true}'); await oldRead;
  assert.equal(area.value, "");
  await upload.onchange({target:{files:[{size:2, text: async () => { throw new Error("read"); }}]}});
  assert.equal(area.value, ""); assert.ok(notices.length >= 2);
  await upload.onchange({target:{files:[{size:2, text: async () => "{}"}]}});
  assert.equal(area.value, "{}");
  await assert.rejects(context.submit({get: key => ({name:"room",bind:"127.0.0.1:1",role:"participant",invitation:"{}"})[key]}), /join failed/);
  assert.equal(area.value, "{}");
});

test("asynchronous join failure offers a nonsecret draft for editable retry", () => {
  const context = {joinDrafts:new Map([[42,{name:"team",bind:"127.0.0.1:45",role:"reader"}]]), joinDraft:{}, dismissedOperations:new Set(), joinDialog:()=>{ context.opened = true; }};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function retryJoin("), source.indexOf("function inviteDialog()")), context);
  context.retryJoin({id:42});
  assert.equal(context.opened, true); assert.equal(context.joinDraft.name, "team"); assert.equal(context.joinDraft.role, "reader");
  assert.equal(Object.hasOwn(context.joinDraft,"invitation"), false); assert.equal(context.joinDrafts.size, 0);
  assert.match(source, /operation\.kind === "join" && joinDrafts\.has\(operation\.id\)/);
});

test("create offers explicit device-only loopback and explains reachable interfaces", () => {
  const controls = [];
  const context = {current:{identity:"test"}, toast:()=>{}, field: (_label, name, _type, _value, note) => { const input = {}; const wrapper = {name, note, input, querySelector:()=>input}; controls.push(wrapper); return wrapper; },
    el: (_tag, _cls, text) => ({text}), modal: (_title, items) => { context.items = items; }};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function createDialog()"), source.indexOf("function joinDialog()")) + "\ncreateDialog();", context);
  const bind = controls.find(item => item.name === "bind"), preset = context.items.find(item => item.text?.includes("loopback"));
  assert.equal(bind.input.value, undefined); assert.match(bind.note, /only on this device/);
  preset.onclick(); assert.equal(bind.input.value, "127.0.0.1:45462");
});

test("room and self identity keep technical IDs secondary", () => {
  const html = fs.readFileSync(__dirname + "/../src/ui/assets/index.html", "utf8");
  assert.match(html, /<details class="technical-details"><summary>Room technical details<\/summary><code id="room-id"/);
  assert.match(source, /const name = memberName\(target, peer\)/);
  assert.match(source, /details\.append\(el\("summary", "", "Fingerprint"\), el\("code", "fingerprint", peer\)\)/);
});

test("member presentation uses exact identities and local nickname changes", async () => {
  const calls = [], controls = [];
  const context = {brief: id => id.slice(0, 2), el: (_tag, _cls, text) => ({text}),
    field: (_label, name, _type, value) => { const input = {value}; const result = {name, input, querySelector:()=>input}; controls.push(result); return result; },
    modal: (_title, _items, _button, handler) => { context.submit = handler; }, act: async action => calls.push(action)};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function memberName("), source.indexOf("function select(")), context);
  const room = {name:"team", presentation:[{id:"aa1", display_name:"Sam", source:"local_override"},{id:"bb2", display_name:"Sam", source:"local_override"}]};
  assert.equal(context.memberName(room,"aa1"), "Sam"); assert.equal(context.memberName(room,"zz"), "Unnamed member");
  assert.notEqual(context.memberChoice(room,"aa1"), context.memberChoice(room,"bb2"));
  context.nicknameDialog(room,"aa1"); assert.equal(controls[0].input.value, "Sam");
  await context.submit({get:()=>"New name"}); assert.equal(calls[0].member, "aa1"); assert.equal(calls[0].display_name, "New name");
  await context.submit({get:()=>"  "}); assert.equal(calls[1].display_name, null);
});

test("default appearance removes overrides and restores neutral CSS", () => {
  const properties = new Map([["--bg", "red"]]);
  const style = {removeProperty: key => properties.delete(key), setProperty: (key, value) => properties.set(key, value)};
  const context = {document:{documentElement:{style}}}; vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function applyAppearance("), source.indexOf('$("theme").onclick')), context);
  context.applyAppearance({hue:120, clarity:60, saturation:30}); assert.equal(properties.size, 7);
  context.applyAppearance({hue:null, clarity:50, saturation:20}); assert.equal(properties.size, 0);
  const css = fs.readFileSync(__dirname + "/../src/ui/assets/style.css", "utf8");
  assert.match(css, /:root\[data-theme=dark\]\{[^}]*--bg:#191919;--surface:#242424;--ink:#ececea;--muted:#aaa9a5/);
});

test("every quantized appearance keeps primary and secondary text readable on both surfaces", () => {
  const properties = new Map();
  const style = {removeProperty: key => properties.delete(key), setProperty: (key, value) => properties.set(key, value)};
  const context = {document:{documentElement:{style}}}; vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function applyAppearance("), source.indexOf('$("theme").onclick')), context);
  function luminance(color) {
    if (color === "#000") return 0; if (color === "#fff") return 1;
    const match = /^hsl\(([-.\d]+) ([-.\d]+)% ([-.\d]+)%\)$/.exec(color); assert.ok(match, color);
    const h = Number(match[1]) / 360, s = Number(match[2]) / 100, l = Number(match[3]) / 100;
    const channel = n => { const k = (n + h * 12) % 12, a = s * Math.min(l, 1 - l); const v = l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)); return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
    return .2126 * channel(0) + .7152 * channel(8) + .0722 * channel(4);
  }
  for (const hue of [null, ...Array.from({length:36}, (_, i) => i * 10)]) for (let clarity = 0; clarity <= 100; clarity += 10) for (let saturation = 0; saturation <= 100; saturation += 10) {
    if (hue === null && clarity === 50 && saturation === 20) continue;
    context.applyAppearance({hue, clarity, saturation});
    for (const text of ["--ink", "--muted"]) for (const background of ["--bg", "--surface"]) {
      const a = luminance(properties.get(text)), b = luminance(properties.get(background));
      const contrast = (Math.max(a,b) + .05) / (Math.min(a,b) + .05);
      assert.ok(contrast >= 4.5, `${hue}/${clarity}/${saturation}: ${text}/${background} ${contrast}`);
    }
  }
  context.applyAppearance({hue:null, clarity:100, saturation:20});
  assert.ok(luminance(properties.get("--bg")) > .9, "maximum clarity must be light");
});
