const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const source = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");

function harness() {
  const calls = [], notices = [], dialogs = [];
  const target = {name:"team", status:{active:true, address:"127.0.0.1:45462", members:["self","reader"], voters:["self"], capabilities:{publish:true,invite:true,remove:true,control:true,admission_roles:["participant","reader","inviter","moderator"]}}};
  const el = (_tag, _cls, text) => ({text, children:[], append(...children){this.children.push(...children);}});
  const ctx = vm.createContext({target, URL, room:()=>ctx.target, toast:value=>notices.push(value), el, memberChoice:(_target,peer)=>peer,
    field:()=>({querySelector:()=>({})}), roleField:(_value,roles)=>({roles}),
    modal:(title,items,_label,submit)=>dialogs.push({title,items,submit}),
    act:async action=>{
      calls.push(action);
      if(action.action==="remove") return {proposal:{}};
      if(action.action==="controllers") return {log_id:{}};
      return {};
    },
  });
  vm.runInContext(source.slice(source.indexOf("function roomActionAllowed("),source.indexOf("function memberName(")),ctx);
  vm.runInContext(source.slice(source.indexOf("function inviteDialog()"),source.indexOf("function addressDialog()")),ctx);
  return {ctx,target,calls,notices,dialogs};
}

test("unknown, offline, inactive and nonboolean cached capabilities never enable actions",()=>{
  const {ctx,target}=harness();
  assert.equal(ctx.roomActionAllowed(target,"publish"),true);
  for(const status of [null,{}, {active:true}, {active:false,capabilities:{publish:true}}, {active:true,capabilities:{publish:1}}])
    assert.equal(ctx.roomActionAllowed({status},"publish"),false);
  target.status.capabilities={publish:true};
  assert.equal(ctx.roomActionAllowed(target,"remove"),false);
  assert.deepEqual(Array.from(ctx.admissionRoles(target)),[]);
});

test("invitation roles are bounded to backend choices and stale forms cannot escalate rights",async()=>{
  const {ctx,target,calls,dialogs}=harness();
  target.status.capabilities.admission_roles=["participant","reader","reader","unknown"];
  ctx.inviteDialog();
  assert.deepEqual(Array.from(dialogs[0].items[1].roles),["participant","reader"]);
  const data=new Map([["bind","127.0.0.1:45463"],["role","moderator"]]);
  await assert.rejects(dialogs[0].submit(data),/admission rights changed/);
  data.set("role","reader");await dialogs[0].submit(data);
  assert.equal(calls.length,1);assert.equal(calls[0].role,"reader");
  target.status.capabilities.invite=false;
  await assert.rejects(dialogs[0].submit(data),/admission rights changed/);
  ctx.inviteDialog();assert.equal(dialogs.length,1);assert.equal(calls.length,1);
});

test("data removal is separate from votes and fences changed member, room and rights",async()=>{
  const {ctx,target,calls,notices,dialogs}=harness();
  ctx.removeMemberDialog(target,"self");assert.equal(dialogs.length,0);assert.match(notices[0],/Retire/);
  ctx.removeMemberDialog(target,"reader");assert.equal(dialogs.length,1);
  assert.equal(dialogs[0].items[1].text,"reader");
  target.status.voters.push("reader");await assert.rejects(dialogs[0].submit(),/state changed/);
  target.status.voters.pop();target.status.capabilities.remove=false;
  await assert.rejects(dialogs[0].submit(),/state changed/);
  target.status.capabilities.remove=true;ctx.target={...target,name:"other"};
  await assert.rejects(dialogs[0].submit(),/state changed/);
  ctx.target=target;target.status.members=["self"];
  await assert.rejects(dialogs[0].submit(),/state changed/);
  target.status.members.push("reader");await dialogs[0].submit();
  assert.equal(calls.length,1);assert.equal(calls[0].peer,"reader");
  assert.equal(Object.hasOwn(calls[0],"capabilities"),false);
});

test("controller selection does not label voting as administrative data access and rechecks stale forms",async()=>{
  const {ctx,target,calls,dialogs}=harness();ctx.controllersDialog();
  assert.match(dialogs[0].items[0].text,/vote does not grant publishing or moderation/);
  assert.equal(dialogs[0].items[1].children.length,2);
  const data={getAll:()=>["self","reader"]};
  target.status.capabilities.control=false;await assert.rejects(dialogs[0].submit(data),/administrative rights changed/);
  target.status.capabilities.control=true;await dialogs[0].submit(data);
  assert.equal(calls.length,1);assert.equal(calls[0].peers[1],"reader");
});
