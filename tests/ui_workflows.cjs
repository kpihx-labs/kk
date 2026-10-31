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
  let sends=0;const inputs=[],nodes=new Map(),observers=[];
  const context = {
    localAvailable:true,modalRevision:0,current:{files:{max_bytes:64},messages:[]},
    foregroundInstance:()=>Array(16).fill(1),recoverySession:()=>"foreground-a",
    roomActionAllowed:()=>true,
    room: () => ({name:"study", status:{owner:"self", members:["self", "peer"]}}),
    memberChoice: () => "Morgan", $: id => {if(!nodes.has(id))nodes.set(id,{checked:false,disabled:false,open:false,addEventListener(){},removeEventListener(){}});return nodes.get(id);}, toast() {},
    MutationObserver:class{constructor(callback){this.callback=callback;observers.push(this);}observe(){}disconnect(){}},
    el: (tag,_cls,text) => { const node={tag,textContent:text,children:[],options:[], append(...children) { this.children.push(...children);if (tag === "select") {this.options.push(...children);this.value ||= children[0]?.value;} }};if(tag==="input")inputs.push(node);return node; },
    modal: (_title, controls, _label, handler) => { context.modalRevision++;context.$("dialog").open=true;context.controls=controls;context.submit = handler; },
  };
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function fileDialog("), source.indexOf("async function loadFiles()")), context);
  context.fileDialog("Send fixture", async () => {sends++;return {ok:false, code:"DENIED", result:{error:"contact policy"}};});
  await assert.rejects(context.submit(new Map([["to", "peer"]])), /recipient to enable incoming files/);
  context.roomActionAllowed=()=>false;await assert.rejects(context.submit(new Map([["to","peer"]])),/publishing rights/);assert.equal(sends,1);
  context.roomActionAllowed=()=>true;context.fileDialog("Forward fixture",async()=>({ok:false,code:"CONFIGURATION",error:"Room inbox has not been configured"}),true);assert.equal(inputs.at(-1).checked,true,"unknown original sensitivity must not silently default forwarding to public");
  await assert.rejects(context.submit(new Map([["to","peer"],["sensitive","on"]])),/inbox has not been configured/);
  const file={name:"local-fixture",size:4},attachment={file};let admissions=0,complete,started;const handedToLocalService=new Promise(done=>{started=done;});
  context.fileDialog("Upload fixture",()=>{admissions++;started();return new Promise(done=>{complete=done;});},false,attachment);
  const picker=inputs.findLast(node=>node.type==="file"),notice=context.controls.find(node=>node.role==="status");
  const queued=context.submit(new Map([["to","peer"],["sensitive","on"]]));context.roomActionAllowed=()=>false;observers.at(-1).callback();
  assert.equal(context.$("dialog-submit").disabled,true);await assert.rejects(queued,/stopped before dispatch/);
  assert.equal(admissions,0);assert.equal(attachment.file,file);assert.deepEqual(context.current.messages,[]);assert.match(notice.textContent,/kept/);
  context.roomActionAllowed=()=>true;observers.at(-1).callback();assert.equal(admissions,0,"restored rights do not retry an admission");
  let observedJobs=0;context.transferDialog=()=>{observedJobs++;};const pending=context.submit(new Map([["to","peer"],["sensitive","on"]]));await handedToLocalService;
  context.roomActionAllowed=()=>false;observers.at(-1).callback();assert.equal(picker.disabled,true);assert.equal(context.$("dialog-submit").disabled,true);assert.equal(attachment.file,file);assert.match(notice.textContent,/may still complete/);
  complete({ok:true,result:{id:"observed-local-job",progress:{phase:"preparing"}}});await pending;assert.equal(admissions,1);assert.equal(observedJobs,1);assert.deepEqual(context.current.messages,[]);
  const draft=composerHarness();draft.context.room().status.capabilities.publish=false;draft.context.render();await draft.send();
  assert.equal(draft.nodes.text.value,"first draft");assert.equal(draft.nodes.sensitive.checked,true);assert.equal(draft.nodes.send.disabled,true);assert.equal(draft.calls.length,0);assert.match(draft.nodes["send-status"].textContent,/kept locally/);
});

test("join import rejects replacement and ignores stale file reads", async () => {
  const controls = [], notices = [];
  const context = { current: {identity: "test"}, modalRevision:0, el:()=>({}), joinDraft:{name:"", bind:"", role:"participant"}, toast: text => notices.push(text),
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
  const context = {notificationControl:{permission:()=>"unsupported"}, current:{settings:{display_name:"Sam",share_display_name:false,appearance:{hue:null,clarity:50,saturation:20}}},
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
  const context = {notificationControl:{permission:()=>"unsupported"}, current:{settings:structuredClone(saved)}, $:()=>theme,
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

test("notification opt-in requires browser permission and stale dialogs cannot undo opt-out", async () => {
  let permission="default", prompts=0;
  const saved={revision:"0",display_name:"Sam",share_display_name:false,message_notifications:false,sidebar_mode:"expanded",appearance:{hue:null,clarity:50,saturation:20}};
  const controls=new Map(),theme={},dialog={open:false},calls=[];
  const context={current:{settings:structuredClone(saved)},modalRevision:0,$:id=>id==="dialog"?dialog:theme,
    notificationControl:{permission:()=>permission,allow:async()=>{prompts++;permission="granted";}},
    field:(_label,name,_type,value)=>{const input={value};const wrapper={querySelector:()=>input,append(){}};controls.set(name,input);return wrapper;},
    el:(_tag,_class,text)=>({text,append(){}}),modal:(_t,items,_s,handler)=>{context.items=items;context.submit=handler;dialog.open=true;},
    api:async()=>({settings:structuredClone(saved)}),
    act:async action=>{calls.push(action);if(action.expected_revision!==saved.revision)throw Object.assign(new Error("conflict"),{code:"SETTINGS_CONFLICT"});for(const key of Object.keys(action))if(!["action","expected_revision"].includes(key))saved[key]=action[key];saved.revision=String(BigInt(saved.revision)+1n);context.current.settings=structuredClone(saved);}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('$("theme").onclick'),source.indexOf('$("profiles").onclick')),context);
  const values={display_name:"Sam",share_display_name:null,message_notifications:"on",sidebar_mode:"expanded",hue:"",clarity:"50",saturation:"20"};
  theme.onclick();
  await assert.rejects(context.submit({get:key=>values[key]??null}),/Allow notifications in this browser first/);
  assert.equal(calls.length,0);assert.equal(prompts,0);
  await context.items.find(item=>item.text==="Allow notifications in this browser").onclick();
  assert.equal(prompts,1);assert.equal(controls.get("message_notifications").checked,true);
  await context.submit({get:key=>values[key]??null});assert.equal(saved.message_notifications,true);
  theme.onclick();saved.message_notifications=false;saved.revision="2";
  controls.get("display_name").value="Edited";values.display_name="Edited";
  await assert.rejects(context.submit({get:key=>values[key]??null}),/Settings changed elsewhere/);
  assert.equal(saved.message_notifications,false);assert.equal(saved.display_name,"Sam");
  assert.equal(controls.get("message_notifications").checked,false);
  assert.equal(controls.get("display_name").value,"Edited");
  values.message_notifications=null;
  await context.submit({get:key=>values[key]??null});
  assert.equal(saved.message_notifications,false);assert.equal(saved.display_name,"Edited");
  assert.equal(Object.hasOwn(calls.at(-1),"message_notifications"),false);
});

test("settings conflicts reload consent and retain edits across opt-out, opt-in and ABA", async () => {
  for (const scenario of ["opt-out", "opt-in", "ABA"]) {
    const saved={revision:"0",display_name:"Sam",share_display_name:scenario==="opt-out",sidebar_mode:"expanded",appearance:{hue:null,clarity:50,saturation:20}};
    const controls=new Map(),theme={},calls=[]; let failRead=false;
    const context={notificationControl:{permission:()=>"unsupported"},current:{settings:structuredClone(saved)},$:()=>theme,
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

test("pending modal submit reserves before awaiting and ignores keyboard resubmission", async () => {
  const {context, submit, elements} = modalHarness(); let calls = 0, resolve;
  context.modal("Save", [], "Save", () => { calls++; return new Promise(done => { resolve = done; }); });
  const first = submit(); await submit(); assert.equal(calls, 1); assert.equal(elements["dialog-submit"].textContent,"Working...");assert.equal(elements["dialog-submit"].ariaBusy,"true");resolve(false); await first;
  assert.equal(elements["dialog-submit"].textContent,"Save");assert.equal(elements["dialog-submit"].ariaBusy,"false");
});

function testClock() {
  let now=0,next=0;const timers=new Map();
  return {Date:{now:()=>now},timers,setTimeout:(callback,delay)=>{const id=++next;timers.set(id,{callback,at:now+delay});return id;},clearTimeout:id=>timers.delete(id),advance(milliseconds,run=true){now+=milliseconds;if(run)for(const [id,timer] of [...timers])if(timer.at<=now){timers.delete(id);timer.callback();}}};
}
function composerHarness() {
  const nodes = {}, calls = [], requests = [], storage = new Map(); let stops = 0;
  const clock=testClock();
  const context = {selected:"first", composerDrafts:new Map(), pendingSends:new Map(), replyDrafts:new Map(), composerScope:null,draftIdentity:null,draftGeneration:0,draftError:false,localAvailable:true,historyView:null,TextEncoder,AbortController,Date:clock.Date,setTimeout:clock.setTimeout,clearTimeout:clock.clearTimeout,
    current:{identity:"a".repeat(64),client:{request_timeout_ms:50},drafts:{max_rooms:64,max_text_bytes:65536},phases:{first:{revision:"2"},second:{revision:"2"}},rooms:[{name:"first",id:"1".repeat(64),status:{active:true,capabilities:{publish:true},foreground_instance:Array(16).fill(1)}},{name:"second",id:"2".repeat(64),status:{active:true,capabilities:{publish:true},foreground_instance:Array(16).fill(2)}}]}, token:"synthetic",
    $:id=>nodes[id] ||= {value:"",checked:false,textContent:"",hidden:false,close(){this.open=false;},replaceChildren(){},classList:{contains:()=>false}},room:()=>context.current.rooms.find(entry=>entry.name===context.selected),
    render(){context.reconcileDrafts();if(context.room())context.renderComposer(context.room());},refresh:async()=>{},toast(){},closeSidebar(){},sessionStorage:{setItem:(key,value)=>storage.set(key,value)},mediaControl:{stopAll(){stops++;}},
    deliveryComplete:reply=>reply?.ok === true && reply.result?.all_acknowledged === true,
    act:(action,options)=>{calls.push(action);return new Promise((resolve,reject)=>requests.push({resolve,reject,signal:options.signal}));}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function clientRequestWaitMs("),source.indexOf("async function api("))+source.slice(source.indexOf("function draftPolicy("),source.indexOf("function field("))+source.slice(source.indexOf("function foregroundInstance("),source.indexOf("function recoveryDialog("))+source.slice(source.indexOf("function roomActionAllowed("),source.indexOf("function admissionRoles("))+source.slice(source.indexOf("function renderComposer("),source.indexOf("function reconcileMediaSession("))+source.slice(source.indexOf("async function sendComposer("),source.indexOf('$("composer").onsubmit = sendComposer;')),context);
  context.render();nodes.text.value="first draft";nodes.sensitive.checked=true;
  return {context,nodes,calls,requests,storage,clock,stops:()=>stops,key:name=>context.draftRoomKey(name),switch:name=>context.select(name),send:()=>context.sendComposer({preventDefault(){}}),finish:(value,index=requests.length-1)=>requests[index].resolve(value),fail:(index=requests.length-1)=>requests[index].reject(new Error("connection lost"))};
}

test("pending send survives refreshes without duplicate requests and cannot erase a newer edit", async () => {
  const h = composerHarness(), pending = h.send(); await h.send(); assert.equal(h.calls.length,1);
  h.nodes.text.value = "newer edit"; h.finish({delivery:{ok:true,result:{all_acknowledged:true}}}); await pending;
  assert.equal(h.nodes.text.value,"newer edit"); assert.equal(h.context.pendingSends.size,0);
  assert.match(h.nodes["send-status"].textContent,/not human reading/);
});

test("late send completion never clears another room's draft or loses an edited originating draft", async () => {
  for (const edited of [false,true]) {
    const h = composerHarness(), pending = h.send();
    if (edited) h.nodes.text.value="first edited";
    h.switch("second");h.nodes.text.value="second private draft";
    h.finish({delivery:{ok:true,result:{all_acknowledged:false}}});await pending;
    assert.equal(h.nodes.text.value,"second private draft");
    assert.equal(h.context.composerDrafts.get(h.key("first")).text,edited?"first edited":"");
    assert.match(h.context.composerDrafts.get(h.key("first")).status,/duplicates are possible/);
  }
});

test("refused and uncertain sends retain the original draft without a successful delivery claim", async () => {
  for (const network of [false,true]) {
    const h = composerHarness(), pending=h.send();
    if(network) h.fail();else h.finish({delivery:{ok:false,code:"DENIED"}});
    await pending;assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.equal(h.context.pendingSends.size,0);
    assert.match(h.nodes["send-status"].textContent,network?/outcome uncertain/:/unconfirmed delivery/);
  }
});

test("stale local state blocks mutations before network work and cached publish permissions", async () => {
  let requests=0;const context={current:null,localAvailable:false,token:"synthetic",fetch:async()=>{requests++;return {ok:true,status:200,json:async()=>({ok:true})};}};
  vm.createContext(context);vm.runInContext(source.slice(source.indexOf("function clientRequestWaitMs("),source.indexOf("async function act(")) + source.slice(source.indexOf("function roomActionAllowed("),source.indexOf("function admissionRoles(")),context);
  assert.equal(context.roomActionAllowed({status:{active:true,capabilities:{publish:true}}},"publish"),false);
  await assert.rejects(context.api("/api/action",{action:"send"}),/previous outcome may be uncertain/);assert.equal(requests,0);
  await context.api("/api/state");assert.equal(requests,1);
});

test("media retires on same-name lifecycle replacement even when intermediate stop snapshots are missed", () => {
  let stops=0;const target={name:"first",id:"public-room-id",status:{active:true}};
  const context={current:{identity:"public-device-id",phases:{first:{state:"online",revision:"2"}}},selected:"first",localAvailable:true,mediaSessionFence:null,
    room:()=>target,mediaControl:{stopAll(){stops++;}}};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf("function foregroundInstance("),source.indexOf("function recoveryDialog("))+source.slice(source.indexOf("function reconcileMediaSession()"),source.indexOf("async function refresh()")),context);
  context.reconcileMediaSession();context.reconcileMediaSession();assert.equal(stops,0);
  context.current.phases.first.revision="6";context.reconcileMediaSession();assert.equal(stops,1);
  context.reconcileMediaSession();assert.equal(stops,1);
  context.localAvailable=false;context.reconcileMediaSession();assert.equal(stops,2);
  context.localAvailable=true;context.reconcileMediaSession();assert.equal(stops,3);
  target.status.active=false;context.reconcileMediaSession();assert.equal(stops,4);
  target.status.active=true;target.id="replacement-room-id";context.reconcileMediaSession();assert.equal(stops,5);
  context.current.identity="replacement-device";context.reconcileMediaSession();assert.equal(stops,6);
  context.selected="second";context.reconcileMediaSession();assert.equal(stops,7);
});

test("a real external foreground incarnation change retires media without a UI phase transition",()=>{
  let stops=0;const target={name:"same",id:"public-room",status:{active:true,foreground_instance:Array(16).fill(1)}};
  const ctx=vm.createContext({current:{identity:"profile",phases:{same:{revision:"2"}}},selected:"same",localAvailable:true,mediaSessionFence:null,room:()=>target,mediaControl:{stopAll(){stops++;}}});
  vm.runInContext(source.slice(source.indexOf("function foregroundInstance("),source.indexOf("function recoveryDialog("))+source.slice(source.indexOf("function reconcileMediaSession()"),source.indexOf("async function refresh()")),ctx);
  ctx.reconcileMediaSession();target.status.foreground_instance=Array(16).fill(2);ctx.reconcileMediaSession();assert.equal(stops,1);ctx.reconcileMediaSession();assert.equal(stops,1);
});

test("selected-room stop, rejoin and restart retire pending capture before awaiting network", async () => {
  let stops=0,requests=0;const context={selected:"same",mediaControl:{stopAll(){stops++;}},api:async()=>{requests++;throw new Error("pending or refused");},refresh:async()=>{}};
  vm.createContext(context);vm.runInContext(source.slice(source.indexOf("async function act("),source.indexOf("function room()")),context);
  for(const action of ["stop","serve","join"]) { const pending=context.act({action,name:"same"});assert.equal(stops,requests);await assert.rejects(pending,/pending or refused/); }
  await assert.rejects(context.act({action:"stop",name:"other"}),/pending or refused/);assert.equal(stops,3);
});

test("offline, inactive and read-only rooms keep local drafts editable without enabling publication", () => {
  const {context,nodes}=composerHarness();
  for(const status of [null,{active:false,capabilities:{publish:true}},{active:true,capabilities:{publish:false}}]) {
    context.renderComposer({name:"first",status});assert.equal(nodes.text.disabled,false);assert.equal(nodes.text.value,"first draft");assert.equal(nodes.send.disabled,true);assert.equal(nodes.attach.disabled,true);
  }
  context.localAvailable=false;context.renderComposer(context.room());assert.equal(nodes.text.disabled,false);assert.equal(nodes.send.disabled,true);
  assert.match(nodes.text.placeholder,/Reconnect/);
});

test("text and sensitivity switch together in volatile profile/room-bound drafts, including offline",()=>{
  const h=composerHarness();h.switch("second");assert.equal(h.nodes.text.value,"");assert.equal(h.nodes.sensitive.checked,false);
  h.nodes.text.value="second draft";h.context.localAvailable=false;h.context.render();h.switch("first");
  assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.equal(h.nodes.text.disabled,false);assert.equal(h.nodes.send.disabled,true);
  h.switch("second");assert.equal(h.nodes.text.value,"second draft");assert.equal(h.nodes.sensitive.checked,false);
  assert.deepEqual([...h.storage.keys()],["kk-room"]);assert.doesNotMatch(JSON.stringify([...h.storage]),/draft|sensitive/);
});

test("alias recreation and removal retire old drafts, flags, reply contexts and room views",()=>{
  const h=composerHarness();h.context.saveComposerDraft();const old=h.key("first");h.context.replyDrafts.set("old",{scope:old,text:"private reply"});h.context.$("dialog").open=true;
  h.context.room().id="3".repeat(64);h.context.render();assert.equal(h.nodes.text.value,"");assert.equal(h.nodes.sensitive.checked,false);assert.equal(h.context.composerDrafts.has(old),false);assert.equal(h.context.replyDrafts.size,0);assert.equal(h.context.$("dialog").open,false);assert.ok(h.stops()>0);
  h.nodes.text.value="new room draft";h.nodes.sensitive.checked=true;h.context.saveComposerDraft();h.context.current.rooms=h.context.current.rooms.filter(entry=>entry.name!=="first");h.context.render();
  assert.equal(h.nodes.text.value,"");assert.equal(h.nodes.sensitive.checked,false);assert.equal(h.context.composerDrafts.size,0);
});

test("late successful sends cannot clear a new same-alias room or a replacement profile draft",async()=>{
  for(const replacement of ["room","profile","private-session"]) {
    const h=composerHarness(),pending=h.send();
    if(replacement==="room")h.context.room().id="3".repeat(64);
    else if(replacement==="profile")h.context.current.identity="b".repeat(64);
    else h.context.retireDrafts();
    h.context.render();assert.equal(h.nodes.text.value,"");assert.equal(h.nodes.sensitive.checked,false);
    h.nodes.text.value="new context draft";h.finish({delivery:{ok:true,result:{all_acknowledged:true}}});await pending;
    assert.equal(h.nodes.text.value,"new context draft");assert.equal(h.nodes.sensitive.checked,false);assert.doesNotMatch(h.nodes["send-status"].textContent,/Acknowledged/);
  }
});

test("draft count and UTF-8 byte bounds block unsafe switching/sending without truncating text",async()=>{
  const h=composerHarness();h.context.current.drafts.max_text_bytes=8;h.nodes.text.value="雪雪雪";
  h.switch("second");assert.equal(h.context.selected,"first");assert.equal(h.nodes.text.value,"雪雪雪");assert.equal(h.nodes.sensitive.checked,true);await h.send();assert.equal(h.calls.length,0);
  h.nodes.text.value="雪雪";h.context.current.drafts.max_rooms=1;h.switch("second");assert.equal(h.nodes.text.value,"");assert.equal(h.nodes.sensitive.checked,false);assert.equal(h.nodes.text.disabled,true);
  h.nodes.text.value="second";h.switch("first");assert.equal(h.context.selected,"second");assert.equal(h.nodes.text.value,"second");assert.equal(h.context.composerDrafts.size,1);
  h.nodes.text.value="";h.switch("first");assert.equal(h.nodes.text.value,"雪雪");assert.equal(h.nodes.sensitive.checked,true);
});

test("an offline failed send keeps its text and sensitivity scoped across subsequent navigation",async()=>{
  const h=composerHarness(),pending=h.send();h.context.localAvailable=false;h.context.render();h.fail();await pending;
  assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.equal(h.nodes.text.disabled,false);
  h.switch("second");assert.equal(h.nodes.text.value,"");assert.equal(h.nodes.sensitive.checked,false);h.switch("first");
  assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.match(h.nodes["send-status"].textContent,/uncertain/);
});

test("an old same-alias completion cannot erase the new room's pending-send reservation",async()=>{
  const h=composerHarness(),old=h.send();h.context.room().id="3".repeat(64);h.context.room().status.foreground_instance=Array(16).fill(3);h.context.render();
  h.nodes.text.value="new room draft";const next=h.send(),key=h.key("first");assert.equal(h.calls.length,2);
  h.finish({delivery:{ok:true,result:{all_acknowledged:true}}},0);await old;assert.equal(h.context.pendingSends.has(key),true);assert.equal(h.nodes.text.value,"new room draft");assert.equal(h.nodes.sensitive.checked,false);
  h.finish({delivery:{ok:true,result:{all_acknowledged:true}}},1);await next;assert.equal(h.nodes.text.value,"");assert.equal(h.context.pendingSends.size,0);
});

test("a stuck client send expires once, keeps text/sensitivity and exposes duplicate risk without auto-retry",async()=>{
  const h=composerHarness(),pending=h.send();assert.equal(h.calls.length,1);h.clock.advance(50);await pending;
  assert.equal(h.requests[0].signal.aborted,true);assert.equal(h.context.pendingSends.size,0);assert.equal(h.clock.timers.size,0);
  assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.equal(h.nodes.send.disabled,false);
  assert.match(h.nodes["send-status"].textContent,/outcome uncertain/);assert.match(h.nodes["send-status"].textContent,/duplicate/);assert.equal(h.calls.length,1);
  const retry=h.send(),reservation=h.context.pendingSends.get(h.key("first"));assert.equal(h.calls.length,2);
  h.finish({delivery:{ok:true,result:{all_acknowledged:true}}},0);await Promise.resolve();
  assert.equal(h.nodes.text.value,"first draft");assert.equal(h.context.pendingSends.get(h.key("first")),reservation);
  h.finish({delivery:{ok:true,result:{all_acknowledged:true}}},1);await retry;assert.equal(h.context.pendingSends.size,0);
});

test("returning to a throttled page expires overdue waits without relying on the timer callback",async()=>{
  const h=composerHarness(),pending=h.send();h.clock.advance(51,false);h.context.expirePendingSends();await pending;
  assert.equal(h.context.pendingSends.size,0);assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.equal(h.calls.length,1);
});

test("a known SDK reply releases the composer without waiting for a stuck follow-up view refresh",async()=>{
  const h=composerHarness();let refreshes=0;
  h.context.api=async()=>({result:{delivery:{ok:true,result:{all_acknowledged:true}}}});
  h.context.refresh=()=>{refreshes++;return new Promise(()=>{});};
  vm.runInContext(source.slice(source.indexOf("async function act("),source.indexOf("function room()")),h.context);
  await h.send();assert.equal(h.context.pendingSends.size,0);assert.equal(h.nodes.text.value,"");assert.equal(refreshes,1);assert.equal(h.clock.timers.size,0);
  assert.match(h.nodes["send-status"].textContent,/not human reading/);
});

test("a local composer rendering failure cannot strand its pending reservation before SDK dispatch",async()=>{
  const h=composerHarness(),render=h.context.renderComposer;let first=true;
  h.context.renderComposer=target=>{if(first){first=false;throw Error("synthetic renderer failure");}render(target);};
  await h.send();assert.equal(h.calls.length,0);assert.equal(h.context.pendingSends.size,0);assert.equal(h.nodes.text.value,"first draft");assert.equal(h.clock.timers.size,0);
});

test("missing client wait policy refuses a send before effects rather than selecting an unbounded fallback",async()=>{
  const h=composerHarness();delete h.context.current.client;await h.send();
  assert.equal(h.calls.length,0);assert.equal(h.context.pendingSends.size,0);assert.equal(h.nodes.text.value,"first draft");assert.equal(h.nodes.sensitive.checked,true);assert.match(h.nodes["send-status"].textContent,/nothing was sent/);
});

test("client HTTP bounds include response bodies and suppress late auth retirement after timeout",async()=>{
  for(const stage of ["headers","body"]) {
    const clock=testClock();let complete,signal,retired=0,requests=0,started;const ready=new Promise(done=>{started=done;});
    const ctx=vm.createContext({current:{client:{request_timeout_ms:50}},token:"synthetic",localAvailable:true,AbortController,Date:clock.Date,setTimeout:clock.setTimeout,clearTimeout:clock.clearTimeout,
      retireDrafts(){retired++;},fetch:(_path,options)=>{requests++;signal=options.signal;return stage==="headers"?new Promise(done=>{complete=done;started();}):Promise.resolve({status:200,ok:true,json:()=>new Promise(done=>{complete=done;started();})});}});
    vm.runInContext(source.slice(source.indexOf("function clientRequestWaitMs("),source.indexOf("async function act(")),ctx);
    const pending=ctx.api("/api/action",{action:"send"}),rejection=assert.rejects(pending,error=>error.code==="CLIENT_WAIT_ENDED");
    await ready;clock.advance(50);await rejection;assert.equal(signal.aborted,true);assert.equal(requests,1);
    complete(stage==="headers"?{status:403,ok:false,json:async()=>({ok:false})}:{ok:true});await Promise.resolve();await Promise.resolve();
    assert.equal(ctx.token,"synthetic");assert.equal(retired,0);assert.equal(clock.timers.size,0);
  }
});
