const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");
const reference = () => ({room:Array(32).fill(7),member:Array(32).fill(8),epoch:3,wire_sha256:Array(32).fill(9),event:"IncomingProcessed"});
function context(extra={}) {
  const ctx=vm.createContext({conversationPending:false,current:{messages:[]},replyDrafts:new Map(),historyView:null,draftRoomKey:name=>"test-scope/"+name,foregroundInstance:target=>target?.status?Array(16).fill(1):null,recoverySession:target=>JSON.stringify([target?.name,target?.status?.foreground_instance]),render(){},toast(){},...extra});
  vm.runInContext(source.slice(source.indexOf("function roomActionAllowed("),source.indexOf("function memberName(")),ctx);
  vm.runInContext(source.slice(source.indexOf("function referenceKey("),source.indexOf("function historyEntries(")),ctx);
  return ctx;
}

test("reply targets ignore local storage direction but preserve every authenticated identity component",()=>{
  const ctx=context(),incoming=reference(),outgoing={...incoming,event:"OutgoingPrepared"};
  assert.equal(ctx.targetKey(incoming),ctx.targetKey(outgoing));
  for(const key of ["room","member","wire_sha256"]) {const changed=structuredClone(incoming);changed[key][0]^=1;assert.notEqual(ctx.targetKey(changed),ctx.targetKey(incoming));}
  assert.equal(ctx.targetKey({...incoming,epoch:Number.MAX_SAFE_INTEGER+1}),null);
  assert.equal(ctx.targetKey({...incoming,room:[1]}),null);
  assert.deepEqual(Object.keys(ctx.messageTarget(incoming)).sort(),["epoch","member","room","wire_sha256"]);
});

test("accepted preparation is not a complete delivery acknowledgement",()=>{
  const ctx=context();
  assert.equal(ctx.deliveryComplete({ok:true,result:{all_acknowledged:true}}),true);
  for(const delivery of [undefined,null,{}, {ok:false,result:{all_acknowledged:true}}, {ok:true}, {ok:true,result:{all_acknowledged:false}}, {ok:true,result:{all_acknowledged:"true"}}])
    assert.equal(ctx.deliveryComplete(delivery),false);
});

test("legacy relation reads reject cross-room, future and unknown metadata rather than treating it as text",()=>{
  const ctx=context(),ref=reference();
  const base={reference:ref,relation:{kind:"reply",target:ctx.messageTarget(ref)}};
  assert.equal(ctx.checkedRelation(base).kind,"reply");
  const foreign=structuredClone(base);foreign.relation.target.room[0]^=1;
  assert.throws(()=>ctx.checkedRelation(foreign),/Invalid authenticated/);
  const future=structuredClone(base);future.relation.target.epoch++;
  assert.throws(()=>ctx.checkedRelation(future),/Invalid authenticated/);
  for(const changes of [{kind:"unknown"},{kind:"reaction",value:"arbitrary",added:true},{kind:"reaction",value:"like",added:"yes"}])
    assert.throws(()=>ctx.checkedRelation({...base,relation:{...base.relation,...changes}}),/Invalid authenticated/);
});

test("one pending conversation action reserves before network work and preserves partial-delivery meaning",async()=>{
  let resolve,calls=0;const notices=[];
  const ctx=context({room:()=>({name:"team",status:{active:true,capabilities:{publish:true}}}),act:async()=>{calls++;return new Promise(done=>{resolve=done;});},toast:value=>notices.push(value)});
  const pending=ctx.conversationAction({action:"reply",name:"team"});
  await assert.rejects(ctx.conversationAction({action:"reply",name:"team"}),/already pending/);
  assert.equal(calls,1);
  resolve({delivery:{ok:true,result:{all_acknowledged:false}}});await pending;
  assert.match(notices[0],/Partial delivery/);assert.equal(ctx.conversationPending,false);
});

test("social actions are retired while legacy records remain readable without social controls",async()=>{
  let calls=0;const ctx=context({act:async()=>{calls++;},document:{activeElement:null},room:()=>({name:"team",status:{active:true,capabilities:{publish:true}}})});
  await assert.rejects(ctx.conversationAction({action:"react",name:"team"}),/Only authenticated replies/);assert.equal(calls,0);
  function node(tag,cls,text){return{tag,className:cls,textContent:text,children:[],dataset:{},append(...children){this.children.push(...children);},replaceChildren(...children){this.children=children;},contains(){return false;},querySelector(selector){return this.children.find(child=>selector===".message-actions"&&child.className==="message-actions");}};}
  ctx.el=node;
  const legacy={room:"team",reference:reference(),kind:"reaction",text:"",relation:{kind:"reaction",target:ctx.messageTarget(reference()),value:"thanks",added:true}};
  assert.equal(ctx.checkedRelation(legacy).kind,"reaction");assert.equal(ctx.relationLabel(legacy,{}),null);
  const retired=node("article");ctx.messageActions(retired,legacy,ctx.room());assert.equal(retired.children.length,0);
  const plain=node("article");ctx.messageActions(plain,{room:"team",reference:reference(),kind:"text"},ctx.room());
  assert.deepEqual(plain.children[0].children.map(child=>child.textContent),["Reply"]);
  assert.doesNotMatch(source,/action:\s*"react"|current\.reactions|Sensitive reaction/);
  const driver=fs.readFileSync(__dirname+"/../src/ui/driver.rs","utf8");assert.doesNotMatch(driver,/Action::React|"reactions":/);
});

test("reply form copies no original body, defaults sensitive safely and refuses a changed room before network",async()=>{
  const controls=new Map(),calls=[];
  const ctx=context({selected:"study",room:()=>({name:"study",status:{active:true,capabilities:{publish:true}}}),memberName:()=>"Friend",
    field:(_label,name)=>{const input={};controls.set(name,input);return{querySelector:()=>input};},
    el:()=>({}),modal:(_title,_items,_submit,handler)=>{ctx.submit=handler;},
    act:async value=>{calls.push(value);return{delivery:{ok:true,result:{all_acknowledged:true}}};}});
  const message={room:"study",sender:"peer",reference:reference(),sensitive:true};
  Object.defineProperty(message,"text",{get(){throw Error("Original body must not be copied");}});
  ctx.replyDialog(message,{name:"study",title:"Study"});
  assert.equal(controls.get("reply-sensitive").checked,true);
  ctx.selected="other";
  await assert.rejects(ctx.submit(new Map([["reply-text","new body"],["reply-sensitive","on"]])),/Room changed/);
  assert.equal(calls.length,0);
  ctx.selected="study";await ctx.submit(new Map([["reply-text","new body"],["reply-sensitive","on"]]));
  assert.equal(calls[0].name,"study");assert.equal(calls[0].text,"new body");assert.equal(calls[0].sensitive,true);
  assert.equal(Object.hasOwn(calls[0].target,"event"),false);assert.equal(Object.hasOwn(calls[0],"author"),false);
});

test("editable reply drafts survive close, failed delivery and another target without persistent storage",async()=>{
  const controls=new Map();const ctx=context({selected:"team",room:()=>({name:"team",status:{active:true,capabilities:{publish:true}}}),memberName:()=>"Peer",
    field:(_label,name)=>{const input={};controls.set(name,input);return{querySelector:()=>input};},el:()=>({}),modal:(_title,_items,_submit,handler)=>{ctx.submit=handler;},act:async()=>{throw Error("offline");}});
  const message={room:"team",sender:"peer",reference:reference(),sensitive:false};
  ctx.replyDialog(message,{name:"team"});controls.get("reply-text").value="editable draft";controls.get("reply-text").oninput();
  controls.get("reply-sensitive").checked=true;controls.get("reply-sensitive").onchange();
  const other=structuredClone(message);other.reference.wire_sha256[0]^=1;ctx.replyDialog(other,{name:"team"});assert.equal(controls.get("reply-text").value,"");
  ctx.replyDialog(message,{name:"team"});assert.equal(controls.get("reply-text").value,"editable draft");assert.equal(controls.get("reply-sensitive").checked,true);
  await assert.rejects(ctx.submit(new Map([["reply-text","retry body"],["reply-sensitive","on"]])),/offline/);
  ctx.replyDialog(message,{name:"team"});assert.equal(controls.get("reply-text").value,"retry body");
});

test("copy is explicit, excludes hidden text and stale rooms; reply navigation selects only a loaded reference",async()=>{
  const copied=[],notices=[];const original={dataset:{reference:null},scrollIntoView(){this.scrolled=true;},focus(){this.focused=true;}};
  const ctx=context({selected:"team",navigator:{clipboard:{writeText:async text=>copied.push(text)}},toast:text=>notices.push(text),$:()=>({querySelectorAll:()=>[original]})});
  await ctx.copyMessageText({text:"literal 雪",sensitive:false},"team");assert.deepEqual(copied,["literal 雪"]);
  await assert.rejects(ctx.copyMessageText({text:"hidden",sensitive:true},"team"),/nonsensitive/);
  await assert.rejects(ctx.copyMessageText({text:"other"},"other"),/this room/);assert.equal(copied.length,1);
  original.dataset.reference=ctx.targetKey(reference());assert.equal(ctx.focusMessageReference(reference()),true);assert.ok(original.focused&&original.scrolled);
  const missing=reference();missing.wire_sha256[0]^=1;assert.equal(ctx.focusMessageReference(missing),false);assert.match(notices.at(-1),/not loaded/);
});

test("unavailable publishing still allows editable reply drafts but Keep draft never sends",async()=>{
  let calls=0;const ctx=context({selected:"team",localAvailable:false,room:()=>({name:"team",status:{active:true,capabilities:{publish:true}}}),memberName:()=>"Peer",
    field:()=>{const input={};return{querySelector:()=>input};},el:()=>({}),modal:(_title,_items,label,handler)=>{ctx.label=label;ctx.submit=handler;},act:async()=>{calls++;}});
  ctx.replyDialog({room:"team",sender:"peer",reference:reference()},{name:"team"});assert.equal(ctx.label,"Keep draft");
  ctx.localAvailable=true;await ctx.submit(new Map([["reply-text","local draft"]]));assert.equal(calls,0);
  ctx.replyDialog({room:"team",sender:"peer",reference:reference()},{name:"team"});assert.equal(ctx.label,"Send reply");
  assert.equal([...ctx.replyDrafts.values()][0].text,"local draft");
});
