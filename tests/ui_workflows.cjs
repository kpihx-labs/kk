const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");

function modalHarness() {
  const elements = Object.fromEntries(["dialog-title", "dialog-content", "dialog-error", "dialog-submit", "dialog-form"].map(id => [id, {children:[], replaceChildren(...children) { this.children = children; }, querySelectorAll() { return this.children.filter(child => typeof child === "object" && child !== null && "value" in child); }}]));
  const events = [];
  elements.dialog = {open:false, showModal() { this.open = true; }, close() { this.open = false; events.push(this.onclose); }, addEventListener(_event, handler) { this.onclose = handler; }};
  const context = vm.createContext({$: id => elements[id], modalRevision:0, historyView:{}, FormData:class {}});
  vm.runInContext(source.slice(source.indexOf("function modal("), source.indexOf("function createDialog(")), context);
  const start = source.indexOf('$("dialog").addEventListener?.("close"');
  vm.runInContext(source.slice(start, source.indexOf("\n", start)), context);
  return {context, elements, events, submit:() => elements["dialog-form"].onsubmit({preventDefault(){}, currentTarget:{}})};
}

test("queued old dialog close does not erase a newly opened form", () => {
  const {context, elements, events} = modalHarness();
  context.modal("Old", ["old private draft"], "Submit", async () => {});
  elements.dialog.close();
  const input = {value:"new private draft"};
  context.modal("New", [input], "Submit", async () => {});
  events.shift()();
  assert.equal(elements.dialog.open, true);
  assert.deepEqual(elements["dialog-content"].children, [input]);
  elements.dialog.close(); events.shift()();
  assert.equal(elements["dialog-content"].children.length, 0);
  assert.equal(input.value, "");
  assert.equal(elements["dialog-form"].onsubmit, null);
  assert.equal(context.historyView, null);
});

test("late old form success or failure cannot close, overwrite or re-enable a replacement form", async () => {
  for (const fail of [false, true]) {
    const {context, elements, events, submit} = modalHarness();
    let resolveOld, rejectOld, resolveNew;
    context.modal("Old", ["old"], "Submit", () => new Promise((resolve, reject) => { resolveOld = resolve; rejectOld = reject; }));
    const old = submit(); elements.dialog.close();
    context.modal("New", ["new"], "Submit", () => new Promise(resolve => { resolveNew = resolve; }));
    events.shift()(); const current = submit();
    if (fail) rejectOld(new Error("old failure")); else resolveOld();
    await old;
    assert.equal(elements.dialog.open, true);
    assert.deepEqual(elements["dialog-content"].children, ["new"]);
    assert.equal(elements["dialog-error"].textContent, "");
    assert.equal(elements["dialog-submit"].disabled, true);
    resolveNew(false); await current;
    assert.equal(elements["dialog-submit"].disabled, false);
  }
});

test("file refusal explains room access and live recipient consent", async () => {
  const context = {
    room: () => ({name:"study", status:{owner:"self", members:["self", "peer"]}}),
    memberChoice: () => "Morgan", $: () => ({checked:false}), toast() {},
    el: tag => ({options:[], append(...children) { if (tag === "select") this.options.push(...children); }}),
    modal: (_title, _controls, _label, handler) => { context.submit = handler; },
  };
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function fileDialog("), source.indexOf("async function loadFiles()")), context);
  context.fileDialog("Send fixture", async () => ({ok:false, code:"DENIED", result:{error:"contact policy"}}));
  await assert.rejects(context.submit(new Map([["to", "peer"]])), /recipient to enable incoming files/);
});

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

test("name sharing requires explicit checkbox consent and labels disclose provenance", async () => {
  const controls = [], calls = [], theme = {};
  const context = {current:{settings:{display_name:"Sam",share_display_name:false,appearance:{hue:null,clarity:50,saturation:20}}},
    $: () => theme,
    field: (_label, name, _type, value) => { const input = {value}; const wrapper = {name,input,querySelector:()=>input,append(){}}; controls.push(wrapper); return wrapper; },
    el: (_tag,_cls,text) => ({text, children:[], append(...nodes){this.children.push(...nodes);}}), modal: (_title,items,_submit,handler) => { context.items = items; context.submit = handler; }, act: async action => calls.push(action)};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('$("theme").onclick'), source.indexOf('$("profiles").onclick')), context);
  theme.onclick();
  assert.equal(controls.find(item => item.name === "share_display_name").input.checked, false);
  assert.ok(context.items.some(item => item.text?.includes("Previously shared names cannot be erased")));
  const values = {display_name:"Sam",hue:"",clarity:"50",saturation:"20",sidebar_mode:"auto"};
  await context.submit({get:key => values[key] ?? null}); assert.equal(Object.hasOwn(calls[0], "share_display_name"), false); assert.equal(calls[0].sidebar_mode, "auto");
  await context.submit({get:key => key === "share_display_name" ? "on" : values[key]}); assert.equal(calls[1].share_display_name, true);
  vm.runInContext(source.slice(source.indexOf("function memberProvenance("), source.indexOf("function nicknameDialog(")), context);
  assert.match(context.memberProvenance({presentation:[{id:"a",source:"self_announcement"}]}, "a"), /not a verified human identity/);
  assert.match(context.memberProvenance({presentation:[{id:"a",source:"self_profile"}]}, "a"), /visible only here; sharing off/);
});

test("a stale settings dialog cannot resubmit unchanged name-sharing consent", async () => {
  const saved = {display_name:"Sam",share_display_name:true,sidebar_mode:"expanded",appearance:{hue:null,clarity:50,saturation:20}};
  const calls = [], theme = {};
  const context = {current:{settings:structuredClone(saved)}, $:()=>theme,
    field:(_label,_name,_type,value)=>({querySelector:()=>({value}),append(){}}),
    el:()=>({append(){}}), modal:(_title,_items,_submit,handler)=>{context.submit = handler;},
    act:async action=>{calls.push(action);for(const key of Object.keys(action))if(key!=="action")saved[key]=action[key];context.current.settings=structuredClone(saved);},
    refresh:async()=>{context.current.settings=structuredClone(saved);}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('$("theme").onclick'), source.indexOf('$("profiles").onclick')), context);
  theme.onclick();
  saved.share_display_name=false; saved.display_name="Renamed elsewhere";
  context.current.settings=structuredClone(saved);
  const values={display_name:"Sam",share_display_name:"on",sidebar_mode:"compact",hue:"",clarity:"60",saturation:"20"};
  await context.submit({get:key=>values[key]??null});
  assert.equal(saved.share_display_name,false);
  assert.equal(saved.display_name,"Renamed elsewhere");
  assert.equal(saved.sidebar_mode,"compact"); assert.equal(saved.appearance.clarity,60);
  assert.equal(Object.hasOwn(calls[0],"share_display_name"),false);
  assert.equal(Object.hasOwn(calls[0],"display_name"),false);
  theme.onclick();
  await context.submit({get:key=>({display_name:saved.display_name,sidebar_mode:"compact",hue:"",clarity:"60",saturation:"20"})[key]??null});
  assert.equal(calls.length,2,"unchanged settings still check the expected revision");
  assert.deepEqual(Object.keys(calls[1]).sort(),["action","expected_revision"]);
});

test("settings conflicts reload consent and retain edits across opt-out, opt-in and ABA", async () => {
  for (const scenario of ["opt-out", "opt-in", "ABA"]) {
    const saved={revision:"0",display_name:"Sam",share_display_name:scenario==="opt-out",sidebar_mode:"expanded",appearance:{hue:null,clarity:50,saturation:20}};
    const controls=new Map(),theme={},calls=[]; let failRead=false;
    const context={current:{settings:structuredClone(saved)},$:()=>theme,
      field:(_label,name,_type,value)=>{const input={value};const wrapper={querySelector:()=>input,append(){}};controls.set(name,input);return wrapper;},
      el:()=>({append(){}}),modal:(_t,_i,_s,handler)=>{context.submit=handler;},
      refresh:async()=>{throw new Error("background refresh is not a fresh consent read");},
      api:async path=>{assert.equal(path,"/api/state");if(failRead)throw new Error("read unavailable");return {settings:structuredClone(saved)};},
      act:async action=>{calls.push(action);if(action.expected_revision!==saved.revision)throw Object.assign(new Error("conflict"),{code:"SETTINGS_CONFLICT"});for(const key of Object.keys(action))if(!["action","expected_revision"].includes(key))saved[key]=action[key];saved.revision=String(BigInt(saved.revision)+1n);context.current.settings=structuredClone(saved);}};
    vm.createContext(context);
    vm.runInContext(source.slice(source.indexOf('$("theme").onclick'),source.indexOf('$("profiles").onclick')),context);
    theme.onclick();
    controls.get("clarity").value="60";
    const values={display_name:"Sam",share_display_name:scenario==="opt-in"?null:"on",sidebar_mode:"expanded",hue:"",clarity:"60",saturation:"20"};
    saved.share_display_name=scenario==="opt-in"; saved.revision=scenario==="ABA"?"2":"1";
    if(scenario==="opt-in") {
      failRead=true;
      await assert.rejects(context.submit({get:key=>values[key]??null}),/current consent could not be refreshed/);
      assert.equal(saved.appearance.clarity,50);assert.equal(saved.share_display_name,true);
      failRead=false;
    }
    await assert.rejects(context.submit({get:key=>values[key]??null}),/Settings changed elsewhere/);
    assert.equal(saved.share_display_name,scenario==="opt-in");
    assert.equal(saved.appearance.clarity,50,"stale submission makes no partial change");
    assert.equal(controls.get("share_display_name").checked,saved.share_display_name);
    assert.equal(controls.get("clarity").value,"60","nonsecret draft remains editable");
    values.share_display_name=controls.get("share_display_name").checked?"on":null;
    await context.submit({get:key=>values[key]??null});
    assert.equal(saved.appearance.clarity,60);assert.equal(saved.share_display_name,scenario==="opt-in");
    assert.equal(calls.at(-1).expected_revision,scenario==="ABA"?"2":"1");
  }
});

test("duplicate and confusable self-announcements remain distinct exact-ID choices", () => {
  const context = {brief:id=>id}; vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function memberName("), source.indexOf("function memberProvenance(")), context);
  const room = {presentation:[{id:"a",display_name:"Sam"},{id:"b",display_name:"Sam"},{id:"c",display_name:"Sаm"}]};
  assert.equal(new Set(["a","b","c"].map(id=>context.memberChoice(room,id))).size, 3);
  assert.equal(context.memberName(room,"Sam"), "Unnamed member");
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
