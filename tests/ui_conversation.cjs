const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");
const reference = () => ({room:Array(32).fill(7),member:Array(32).fill(8),epoch:3,wire_sha256:Array(32).fill(9),event:"IncomingProcessed"});
function context(extra={}) {
  const ctx=vm.createContext({conversationPending:false,current:{messages:[]},render(){},toast(){},...extra});
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

test("relation rendering rejects cross-room, future and unknown reaction metadata rather than treating it as text",()=>{
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
  const pending=ctx.conversationAction({action:"react",name:"team"});
  await assert.rejects(ctx.conversationAction({action:"reply",name:"team"}),/already pending/);
  assert.equal(calls,1);
  resolve({delivery:{ok:true,result:{all_acknowledged:false}}});await pending;
  assert.match(notices[0],/Partial delivery/);assert.equal(ctx.conversationPending,false);
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
