const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");
function response(chunks, length) {
  let index = 0, cancelled = false, released = false;
  const reader = {read:async()=>index < chunks.length ? {done:false,value:Uint8Array.from(chunks[index++])} : {done:true},cancel:async()=>{cancelled=true;},releaseLock(){released=true;}};
  return {headers:{get:()=>length},body:{getReader:()=>reader},cancelled:()=>cancelled,released:()=>released};
}
function context() {
  const ctx=vm.createContext({Blob,Uint8Array,console});
  vm.runInContext(source.slice(source.indexOf("async function readOwnedInbox("),source.indexOf("async function saveInboxFile(")),ctx);return ctx;
}
test("owned-file reads are exact and byte bounded with local-read progress only",async()=>{
  const ctx=context(),progress=[],input=response([[1,2],[3,4]],"4");
  const blob=await ctx.readOwnedInbox(input,4,4,{aborted:false},bytes=>progress.push(bytes));
  assert.equal(blob.size,4);assert.deepEqual([...new Uint8Array(await blob.arrayBuffer())],[1,2,3,4]);assert.deepEqual(progress,[2,4]);assert.equal(input.released(),true);
});
test("changed headers, truncated bodies and extra bytes never become a saveable copy",async()=>{
  const ctx=context();
  for(const [chunks,length,expected,pattern] of [[[[1]],"2",1,/length changed/],[[[1]],"2",2,/Incomplete/],[[[1,2,3]],"2",2,/exceeded/]]) {
    const input=response(chunks,length);await assert.rejects(ctx.readOwnedInbox(input,expected,4,{aborted:false},()=>{}),pattern);assert.equal(input.cancelled(),true);assert.equal(input.released(),true);
  }
  await assert.rejects(ctx.readOwnedInbox(response([],"0"),5,4,{aborted:false},()=>{}),/bound/);
});
test("stopping an owned-file browser read cancels its reader and releases the lock",async()=>{
  const ctx=context(),input=response([[1,2]],"2");
  await assert.rejects(ctx.readOwnedInbox(input,2,4,{aborted:true},()=>{}),/stopped/);
  assert.equal(input.cancelled(),true);assert.equal(input.released(),true);
});
test("saving retains the browser destination and does not claim remote transfer cancellation",()=>{
  assert.match(source,/save\.download = file\.id/);assert.match(source,/browser controls the save destination/);
  assert.match(source,/no peer transfer was cancelled/);assert.doesNotMatch(source.slice(source.indexOf("async function readOwnedInbox("),source.indexOf("function copyPeers(")),/response\.blob\(\)/);
});

test("received-copy HTTP forwarding uses only the retained-source command, never a checked pathname",()=>{
  const rust=fs.readFileSync(__dirname+"/../src/ui/files.rs","utf8");
  const forward=rust.slice(rust.indexOf("pub(super) async fn forward("),rust.indexOf("pub(super) async fn list("));
  assert.match(forward,/Command::ForwardReceived\s*\{/);assert.match(forward,/id: name/);
  assert.doesNotMatch(forward,/source_file|private_directory|Command::File\s*\{|FileReception|\.join\("inbox"\)/);
  const upload=rust.slice(rust.indexOf("impl Transfer {"),rust.indexOf("impl Drop for Reservation"));
  assert.match(upload,/Command::File\s*\{/);assert.match(upload,/self\.source\.path\(\)/);
});

function fileUi(mime="image/png", previewLimit=8, content=[1,2,3,4]) {
  const nodes=new Map(),modals=[],requests=[],created=[],revoked=[],notices=[],lifecycle=new Map(),observers=[];
  const el=(tag,cls,text)=>({tag,className:cls,textContent:text,children:[],dataset:{},listeners:new Map(),append(...children){this.children.push(...children);},replaceChildren(...children){this.children=children;},contains(node){return this.children.includes(node);},setAttribute(name,value){this[name]=value;},addEventListener(name,handler){this.listeners.set(name,handler);},removeEventListener(name){this.listeners.delete(name);}});
  const input=response([content],"4");input.ok=true;input.status=200;input.headers={get:key=>key==="Content-Type"?mime:"4"};input.blob=async()=>new Blob(['{"version":2}'],{type:"application/json"});
  const ctx=vm.createContext({Blob,Uint8Array,TextDecoder,AbortController,console,token:"synthetic",selected:"team",localAvailable:true,modalRevision:0,
    current:{identity:"synthetic",operations:[{id:7,kind:"invite",room:"team",state:"waiting",invitation_ready:true}],drafts:{max_text_bytes:64},files:{max_bytes:64,preview_max_bytes:previewLimit}},el,addEventListener:(name,handler)=>lifecycle.set(name,handler),removeEventListener:name=>lifecycle.delete(name),toast:text=>notices.push(text),$:id=>{if(!nodes.has(id))nodes.set(id,el(id));return nodes.get(id);},
    MutationObserver:class{constructor(callback){this.callback=callback;this.targets=new Set();observers.push(this);}observe(target){this.targets.add(target);}disconnect(){this.disconnected=true;}},
    URL:{createObjectURL:blob=>{created.push(blob.type);return "blob:synthetic";},revokeObjectURL:url=>revoked.push(url)},
    fetch:async(path,options)=>{requests.push({path,options});return input;},refresh:async()=>{},
    modal:(title,controls,label,handler)=>{ctx.modalRevision++;ctx.$("dialog").open=true;modals.push({title,controls,label,handler});}});
  vm.runInContext(source.slice(source.indexOf("async function readOwnedInbox("),source.indexOf("async function loadFiles(")),ctx);
  return {ctx,nodes,modals,requests,created,revoked,notices,observers,hide(){lifecycle.get("pagehide")?.();},close(){ctx.modalRevision++;ctx.$("dialog").open=false;ctx.$("dialog").listeners.get("close")?.();}};
}
test("explicit raster preview uses the bounded endpoint and revokes its URL when retired",async()=>{
  const h=fileUi();assert.equal(h.requests.length,0);
  await h.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},true);
  assert.match(h.requests[0].path,/\?preview=true$/);assert.deepEqual(h.created,["image/png"]);
  const output=h.modals[0].controls[2],image=output.children.find(node=>node.tag==="img");assert.equal(image.src,"blob:synthetic");
  assert.match(h.modals[0].title,/Explicit/);assert.ok(output.children.some(node=>node.tag==="a"&&node.download==="opaque.bin"));
  h.close();assert.deepEqual(h.revoked,["blob:synthetic"]);assert.equal(h.requests[0].options.signal.aborted,true);
});
test("HTML/SVG headers never reach an image decoder and oversized preview still permits Save",async()=>{
  for(const mime of ["text/html","image/svg+xml"]) {
    const h=fileUi(mime);await h.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},true);
    assert.equal(h.created.length,0);assert.match(h.modals[0].controls[0].textContent,/not an allowed raster/);
    assert.ok(h.modals[0].controls[2].children.some(node=>node.textContent==="Save without preview"));
  }
  const h=fileUi("application/octet-stream",3);await h.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},true);assert.equal(h.requests.length,0);
  await h.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4});assert.equal(h.requests.length,1);assert.doesNotMatch(h.requests[0].path,/preview/);assert.deepEqual(h.created,["application/octet-stream"]);
  const text=fileUi("text/html",8,[60,120,62,33]);await text.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},"text");
  assert.equal(text.modals[0].controls[2].children.find(node=>node.tag==="pre").textContent,"<x>!");assert.doesNotMatch(text.requests[0].path,/preview/);assert.deepEqual(text.created,["application/octet-stream"]);
  const binary=fileUi("application/octet-stream",8,[255,40,160,161]);await binary.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},"text");assert.equal(binary.created.length,0);assert.match(binary.modals[0].controls[0].textContent,/not valid UTF-8/);
  const bounded=fileUi("text/plain");bounded.ctx.current.drafts.max_text_bytes=3;await bounded.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},"text");assert.equal(bounded.requests.length,0);
});
test("retired pending previews cannot create URLs or populate a replacement dialog",async()=>{
  const h=fileUi();let resolve;h.ctx.fetch=()=>new Promise(done=>{resolve=done;});
  const pending=h.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},true);h.close();
  const input=response([[1,2,3,4]],"4");input.ok=true;input.headers={get:key=>key==="Content-Type"?"image/png":"4"};resolve(input);await pending;
  assert.equal(h.created.length,0);assert.equal(input.cancelled(),true);
});

test("page retirement clears revealed image content and revokes the retained blob URL",async()=>{
  const h=fileUi();await h.ctx.saveInboxFile("team",{id:"opaque.bin",bytes:4},true);h.hide();
  assert.deepEqual(h.revoked,["blob:synthetic"]);assert.equal(h.modals[0].controls[2].children.length,0);assert.equal(h.requests[0].options.signal.aborted,true);
});

function transferUi() {
  const nodes=new Map(),timers=new Map(),modals=[],requests=[],notices=[];
  let nextTimer=0;
  const el=(tag,cls,text)=>({tag,className:cls,textContent:text,children:[],listeners:new Map(),append(...children){this.children.push(...children);},addEventListener(name,handler){this.listeners.set(name,handler);}});
  const target={name:"team",generation:"original"};
  const ctx=vm.createContext({console,localAvailable:true,modalRevision:0,el,room:()=>target,recoverySession:room=>room.generation,foregroundInstance:()=>"original",
    $:id=>{if(!nodes.has(id))nodes.set(id,el(id));return nodes.get(id);},toast:message=>notices.push(message),
    setTimeout:fn=>{timers.set(++nextTimer,fn);return nextTimer;},clearTimeout:id=>timers.delete(id),
    api:async(path,body)=>{requests.push({path,body});return {result:job("completed",{bytes:4})};},
    modal:(title,controls,label,handler)=>{ctx.modalRevision++;ctx.$("dialog").open=true;modals.push({title,controls,label,handler});}});
  vm.runInContext(source.slice(source.indexOf("function transferTerminal("),source.indexOf("async function readOwnedInbox(")),ctx);
  return {ctx,target,modals,requests,notices,timers,close(){ctx.$("dialog").open=false;ctx.$("dialog").listeners.get("close")?.();}};
}
function job(phase,receipt=null) { return {id:"synthetic-owner-0000000000000001",progress:{phase,source_bytes:2,resumed_from:1,total_bytes:4,handed_off:true},cancel_requested:false,receipt:receipt?{resumed_from:0,...receipt}:null}; }

test("transfer counters and awaiting-receipt never become a durable storage claim",()=>{
  const h=transferUi();
  for(const phase of ["preparing","negotiating","transferring","verifying","publishing","awaiting-receipt"]) assert.match(h.ctx.transferText(job(phase)),/Not a storage receipt/);
  assert.match(h.ctx.transferText(job("completed")),/remains unconfirmed/);
  assert.match(h.ctx.transferText(job("completed",{bytes:4})),/Verified durable recipient storage/);
  assert.match(h.ctx.transferText(job("completed",{bytes:-1})),/remains unconfirmed/);
  assert.match(h.ctx.transferText(job("uncertain")),/inspect recipient storage before retrying/);
  assert.match(h.ctx.transferText(job("cancelled")),/not undone/);
  assert.throws(()=>h.ctx.transferText(job("unknown")),/Invalid transfer status/);
});

test("denied transfers explain consent without converting uncertain publication into a clean refusal",()=>{
  const h=transferUi();
  for(const phase of ["failed","uncertain"]) { const denied=job(phase);denied.progress.error="DENIED";const text=h.ctx.transferText(denied);assert.match(text,/permissions/);assert.match(text,/incoming-file consent/);if(phase==="uncertain")assert.match(text,/Remote outcome uncertain/);assert.doesNotMatch(text,/nothing was stored|no remote effects|Verified durable/); }
});

test("only the verified receipt's bounded prefix is described as a completed resume",()=>{
  const h=transferUi();assert.match(h.ctx.transferText(job("completed",{bytes:16,resumed_from:4})),/verified resume prefix 4 bytes/);
  for(const resumed_from of [-1,17,NaN,undefined])assert.match(h.ctx.transferText(job("completed",{bytes:16,resumed_from})),/delivery remains unconfirmed/);
  assert.doesNotMatch(h.ctx.transferText(job("completed",{bytes:16,resumed_from:0})),/verified resume prefix/);
});

test("opening a transfer monitors the exact ID; closing does not cancel the job",async()=>{
  const h=transferUi();h.ctx.transferDialog("team","original",job("transferring"));
  assert.equal(h.requests.length,0);assert.equal(h.timers.size,1);
  await h.modals[0].controls[2].onclick();
  assert.match(h.requests[0].path,/\/transfers\/synthetic-owner-0000000000000001$/);
  assert.equal(h.requests[0].body,undefined);assert.match(h.modals[0].controls[0].textContent,/Verified durable recipient storage/);
  assert.equal(h.timers.size,0);h.close();assert.equal(h.requests.length,1);
});

test("cancellation reserves before awaiting and distinguishes request from acknowledgement",async()=>{
  const h=transferUi();let resolve;
  h.ctx.api=(path,body)=>{h.requests.push({path,body});return new Promise(done=>{resolve=done;});};
  h.ctx.transferDialog("team","original",job("transferring"));
  const cancel=h.modals[0].controls[1],first=cancel.onclick();await cancel.onclick();
  assert.equal(h.requests.length,1);assert.match(h.requests[0].path,/\/cancel$/);assert.ok(h.requests[0].body);assert.equal(cancel.disabled,true);
  const pending=job("awaiting-receipt");pending.cancel_requested=true;
  resolve({result:{requested:true,acknowledged:false,job:pending}});await first;
  assert.match(h.modals[0].controls[0].textContent,/not yet acknowledged/);assert.equal(cancel.disabled,true);
  h.close();assert.equal(h.timers.size,0);
});

test("old foreground IDs and retired responses cannot control a new room lifetime",async()=>{
  const h=transferUi();let resolve;
  h.ctx.api=()=>new Promise(done=>{resolve=done;});h.ctx.transferDialog("team","original",job("transferring"));
  const view=h.modals[0],pending=view.controls[2].onclick(),before=view.controls[0].textContent;
  h.target.generation="replacement";resolve({result:job("completed",{bytes:4})});await pending;
  assert.equal(view.controls[0].textContent,before);assert.equal(h.timers.size,0);
  await view.controls[1].onclick();assert.equal(h.requests.length,0);
  h.ctx.transferDialog("team","original",job("transferring"));assert.equal(h.modals.length,1);assert.equal(h.notices.length,1);
});

test("unknown transfer status stops polling without resending or inventing completion",async()=>{
  const h=transferUi();h.ctx.api=async()=>{throw new Error("owner unavailable");};h.ctx.transferDialog("team","original",job("transferring"));
  await h.modals[0].controls[2].onclick();assert.equal(h.timers.size,0);
  assert.match(h.modals[0].controls[0].textContent,/may still complete/);assert.match(h.modals[0].controls[0].textContent,/not a duplicate send/);
});

test("invitation clipboard is explicit, stays out of notices and creates no downloads",async()=>{
  const writes=[],notices=[],requests=[];
  const ctx=vm.createContext({navigator:{clipboard:{writeText:async text=>writes.push(text)}},current:{identity:"synthetic-device",operations:[{id:7,kind:"invite",invitation_ready:true,state:"waiting"}]},token:"synthetic-session",localAvailable:true,
    fetch:async path=>{requests.push(path);return {ok:true,text:async()=>"synthetic invitation fixture"};},toast:text=>notices.push(text)});
  vm.runInContext(source.slice(source.indexOf("function invitationAvailable("),source.indexOf("async function saveInvitation("))+source.slice(source.indexOf("async function copyInvitation("),source.indexOf("function renderOperations(")),ctx);
  assert.equal(writes.length,0);await ctx.copyInvitation(7);
  assert.equal(writes.length,1);assert.equal(requests.length,1);assert.match(notices[0],/clear your clipboard/);
  assert.ok(notices.every(text=>!text.includes("synthetic invitation fixture")));
  ctx.current.operations[0].state="failed";await assert.rejects(ctx.copyInvitation(7),/no longer available/);assert.equal(requests.length,1);
});

test("a stale invitation read cannot copy into a replacement profile or after expiry",async()=>{
  for(const replace of [ctx=>ctx.current.identity="replacement",...["failed","declined","complete","stopped","joined","working"].map(state=>ctx=>{ctx.current.operations[0].state=state;}),ctx=>ctx.token=""]) {
    let resolve;const writes=[];
    const ctx=vm.createContext({navigator:{clipboard:{writeText:async text=>writes.push(text)}},current:{identity:"synthetic-device",operations:[{id:7,kind:"invite",invitation_ready:true,state:"waiting"}]},token:"synthetic-session",localAvailable:true,
      fetch:()=>new Promise(done=>{resolve=done;}),toast:()=>{}});
    vm.runInContext(source.slice(source.indexOf("function invitationAvailable("),source.indexOf("async function saveInvitation("))+source.slice(source.indexOf("async function copyInvitation("),source.indexOf("function renderOperations(")),ctx);
    const pending=ctx.copyInvitation(7);replace(ctx);resolve({ok:true,text:async()=>"synthetic invitation fixture"});
    await assert.rejects(pending,/Nothing was copied/);assert.equal(writes.length,0);
  }
});

function joinClipboardUi(readText) {
  const controls=[],notices=[],dialog={open:true};let reads=0;
  const ctx=vm.createContext({Blob,modalRevision:0,current:{identity:"synthetic"},joinDraft:{name:"",bind:"",role:"participant"},
    navigator:{clipboard:{readText:()=>{reads++;return readText();}}},el:(_tag,_cls,text)=>({textContent:text}),toast:text=>notices.push(text),
    field:(_label,name)=>{const input={value:"",addEventListener(_name,handler){this.input=handler;}};const wrapper={name,input,querySelector:()=>input};controls.push(wrapper);return wrapper;},
    roleField:()=>({}),$:()=>dialog,modal:(_title,items)=>{ctx.modalRevision++;ctx.items=items;dialog.open=true;}});
  vm.runInContext(source.slice(source.indexOf("function joinDialog()"),source.indexOf("function retryJoin(")),ctx);ctx.joinDialog();
  return {ctx,dialog,notices,area:controls.find(control=>control.name==="invitation").input,paste:ctx.items.at(-1),reads:()=>reads};
}
test("Join reads the clipboard only on explicit paste and never submits or overwrites edits",async()=>{
  const h=joinClipboardUi(async()=>'{"version":2}');assert.equal(h.reads(),0);
  await h.paste.onclick();assert.equal(h.reads(),1);assert.equal(h.area.value,'{"version":2}');
  await h.paste.onclick();assert.equal(h.reads(),1);assert.match(h.notices[0],/Clear the invitation field intentionally/);
});
test("late clipboard permission/read cannot populate a retired dialog, profile or newer edit",async()=>{
  for(const retire of [h=>h.ctx.modalRevision++,h=>h.dialog.open=false,h=>h.ctx.current.identity="replacement",h=>{h.area.value="new edit";h.area.input();}]) {
    let resolve;const h=joinClipboardUi(()=>new Promise(done=>{resolve=done;})),pending=h.paste.onclick();
    await h.paste.onclick();assert.equal(h.reads(),1);retire(h);const before=h.area.value;resolve('{"version":2}');await pending;assert.equal(h.area.value,before);
  }
});
test("clipboard denial, malformed input and oversized invitation stay local and editable",async()=>{
  for(const read of [async()=>{throw new Error("permission denied");},async()=>"not JSON",async()=>JSON.stringify("x".repeat(4096))]) {
    const h=joinClipboardUi(read);await h.paste.onclick();assert.equal(h.area.value,"");assert.equal(h.paste.disabled,false);assert.match(h.notices[0],/no joining request was sent/);
  }
});

function invitationSaveUi() {
  const h=fileUi();Object.assign(h.ctx,{joinDrafts:new Map(),dismissedOperations:new Set(),operationView:"",document:{activeElement:null},navigator:{clipboard:{writeText:async()=>{throw Error("No automatic clipboard write allowed");}}},retryJoin(){},render(){h.ctx.renderOperations();}});
  vm.runInContext(source.slice(source.indexOf("function invitationAvailable("),source.indexOf("function renderComposer(")),h.ctx);return h;
}
test("invitation download is explicit and retains its URL across the browser Save/Open choice",async()=>{
  const h=invitationSaveUi();await h.ctx.saveInvitation(7);
  assert.equal(h.requests.length,1);assert.deepEqual(h.created,["application/json"]);assert.equal(h.revoked.length,0);
  const save=h.modals[0].controls[1].children[0];assert.equal(save.download,"kk-invitation.json");assert.equal(save.textContent,"Save invitation");
  save.onclick({preventDefault(){throw new Error("live save must remain available");}});
  assert.match(h.modals[0].controls[0].textContent,/Complete the browser's Save\/Open choice/);assert.equal(h.revoked.length,0);
  h.close();assert.deepEqual(h.revoked,["blob:synthetic"]);assert.equal(h.modals[0].controls[1].children.length,0);assert.equal(h.observers[0].disconnected,true);
});
test("retiring or replacing an invitation view aborts reads and revokes only its owned URL",async()=>{
  for(const retire of [h=>{h.ctx.modalRevision++;h.observers[0].callback();},h=>h.hide()]) {
    const h=invitationSaveUi();await h.ctx.saveInvitation(7);retire(h);assert.deepEqual(h.revoked,["blob:synthetic"]);assert.equal(h.requests[0].options.signal.aborted,true);
  }
  const h=invitationSaveUi();let resolve;h.ctx.fetch=()=>new Promise(done=>{resolve=done;});const pending=h.ctx.saveInvitation(7);h.close();
  resolve({ok:true,blob:async()=>new Blob(['{"version":2}'])});await pending;assert.equal(h.created.length,0);
});
test("an expired invitation cannot be saved through a retained link",async()=>{
  const h=invitationSaveUi();await h.ctx.saveInvitation(7);const save=h.modals[0].controls[1].children[0];h.ctx.current.operations[0].state="failed";let prevented=false;
  save.onclick({preventDefault(){prevented=true;}});assert.equal(prevented,true);assert.deepEqual(h.revoked,["blob:synthetic"]);
});

function operationButtons(node) { return [...(node.tag==="button"?[node]:[]),...node.children.flatMap(operationButtons)]; }
test("stale terminal invitation/proposal flags cannot retain controls or prevent dismissal",async()=>{
  for(const state of ["failed","declined","complete","stopped","joined"]) {
    const h=invitationSaveUi();Object.assign(h.ctx.current.operations[0],{state,invitation_ready:true,proposal:{confirmation:"synthetic",fingerprint:"public"}});
    h.ctx.renderOperations();const buttons=operationButtons(h.nodes.get("operations")),retry=buttons.find(node=>node.dataset.action==="retry-invitation");
    assert.deepEqual(buttons.map(node=>node.dataset.action),[...(["failed","declined","stopped"].includes(state)?["retry-invitation"]:[]),"dismiss"]);
    assert.equal(h.requests.length,0,"render must not fetch or copy invitation data");
    if(retry) {
      const opened=[];h.ctx.current.rooms=[{name:"team"}];h.ctx.room=()=>h.ctx.current.rooms[0];h.ctx.inviteDialog=(...args)=>opened.push(args);
      await retry.onclick();assert.deepEqual(opened,[[]],"retry opens a normal fresh form without passing old JSON, proposal or confirmation");assert.equal(h.requests.length,0);
      h.ctx.current.operations[0].state="waiting";await retry.onclick();assert.equal(opened.length,1,"a retired retry callback cannot act on a changed operation");h.ctx.current.operations[0].state=state;
    }
    await buttons.find(node=>node.dataset.action==="dismiss").onclick();assert.equal(h.ctx.dismissedOperations.has(7),true);
  }
});
test("terminal transition retires an open Save URL and retained invitation callbacks without clipboard writes",async()=>{
  for(const state of ["failed","declined","complete","stopped","joined"]) {
    const h=invitationSaveUi();h.ctx.renderOperations();const controls=operationButtons(h.nodes.get("operations"));assert.deepEqual(controls.map(node=>node.dataset.action),["copy-invitation","download"]);assert.equal(h.requests.length,0);
    await h.ctx.saveInvitation(7);const save=h.modals[0].controls[1].children[0];assert.equal(h.observers[0].targets.has(h.nodes.get("operations")),true);
    h.ctx.current.operations[0].state=state;h.ctx.renderOperations();h.observers[0].callback();
    assert.deepEqual(h.revoked,["blob:synthetic"]);assert.equal(h.modals[0].controls[1].children.length,0);assert.equal(h.requests[0].options.signal.aborted,true);
    let prevented=false;save.onclick({preventDefault(){prevented=true;}});assert.equal(prevented,true);
    await controls[0].onclick();await controls[1].onclick();assert.equal(h.requests.length,1,"retired controls cannot initiate another request");
  }
});

test("retained approval callbacks cannot approve a replaced confirmation or a terminal stale proposal",async()=>{
  const h=invitationSaveUi(),commands=[];h.ctx.act=async action=>commands.push(action);
  h.ctx.current.operations[0].proposal={confirmation:"synthetic-original",fingerprint:"public-original"};h.ctx.renderOperations();
  const approve=operationButtons(h.nodes.get("operations")).find(node=>node.dataset.action==="approve");
  h.ctx.current.operations[0].proposal.confirmation="synthetic-replaced";await approve.onclick();assert.equal(commands.length,0);assert.match(h.notices.at(-1),/Confirmation changed or retired/);
  h.ctx.current.operations[0].state="joined";await approve.onclick();assert.equal(commands.length,0);
});

test("a terminal stale-ready flag during invitation body loading cannot create a late URL",async()=>{
  const h=invitationSaveUi();let complete;const ready=new Promise(done=>{h.ctx.fetch=async()=>({ok:true,blob:()=>new Promise(resolve=>{complete=resolve;done();})});});
  const pending=h.ctx.saveInvitation(7);await ready;h.ctx.current.operations[0].state="joined";h.ctx.renderOperations();h.observers[0].callback();
  complete(new Blob(['{"version":2}']));await pending;assert.equal(h.created.length,0);assert.equal(h.modals[0].controls[1].children.length,0);
});
