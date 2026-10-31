const test=require("node:test");
const assert=require("node:assert/strict");
const fs=require("node:fs");
const vm=require("node:vm");
const source=fs.readFileSync(__dirname+"/../src/ui/assets/app.js","utf8");
const driver=fs.readFileSync(__dirname+"/../src/ui/driver.rs","utf8");
const receipt=()=>({accepted:true,epoch:3,replayed:true,meaning:"control-acceptance-not-invitation-resumption-or-delivery"});
function ui() {
  const calls=[],dialogs=[],nodes=new Map(),pending={id:Array(32).fill(7),id_hex:"display-only",kind:"admission",member:"c".repeat(64),epoch_before:2};
  const target={name:"team",id:"public-room",status:{active:true,foreground_instance:Array(16).fill(1)}};
  const ctx=vm.createContext({selected:"team",localAvailable:true,modalRevision:0,current:{identity:"public-device",phases:{team:{revision:"2"}}},room:()=>target,
    $:id=>{if(!nodes.has(id))nodes.set(id,{open:false});return nodes.get(id);},toast(){},memberChoice:()=>"Morgan / exact-ID",memberProvenance:()=>"Local nickname / not verified human identity",el:(tag,cls,text)=>({tag,className:cls,textContent:text,children:[],append(...children){this.children.push(...children);}}),
    field:(label,name)=>{const input={};return{label,name,input,querySelector:()=>input};},
    modal:(title,controls,label,handler)=>{ctx.modalRevision++;ctx.$("dialog").open=true;dialogs.push({title,controls,label,handler});},
    api:async(path,body)=>{assert.equal(path,"/api/action");calls.push(JSON.parse(JSON.stringify(body)));return{result:body.action==="pending-mutation"?pending:receipt()};}});
  vm.runInContext(source.slice(source.indexOf("function foregroundInstance("),source.indexOf("function fileDialog(")),ctx);
  return {ctx,target,calls,dialogs,pending,inspect:async()=>{ctx.recoveryDialog();await dialogs.at(-1).handler();},close(){ctx.modalRevision++;ctx.$("dialog").open=false;}};
}
test("recovery inspection is explicit and never joins automatic status polling",async()=>{
  const h=ui();h.ctx.recoveryDialog();assert.equal(h.calls.length,0);assert.equal(h.dialogs.length,1);
  await h.dialogs[0].handler();assert.equal(h.calls.length,1);assert.equal(h.calls[0].action,"pending-mutation");
  assert.doesNotMatch(driver.slice(driver.indexOf("pub(super) async fn snapshot(")),/PendingMutation|RetryPrepared|prepared_control/);
  const confirm=h.dialogs[1];assert.equal(confirm.controls.at(-1).input.required,true);
  assert.match(confirm.controls[1].textContent,/Morgan/);assert.equal(confirm.controls[2].children[1].textContent,h.pending.member);assert.equal(confirm.controls[3].textContent,"07".repeat(32));
  await assert.rejects(confirm.handler(new Map()),/Explicit confirmation/);assert.equal(h.calls.length,1);
  await confirm.handler(new Map([["confirm-recovery","on"]]));
  assert.deepEqual(Object.keys(h.calls[1]).sort(),["action","confirm","expected","name"]);assert.equal(h.calls[1].confirm,true);assert.deepEqual(h.calls[1].expected,Array(32).fill(7));
  assert.match(h.dialogs[2].controls[0].textContent,/Owner reports control acceptance/);assert.match(h.dialogs[2].controls[1].textContent,/not invitation resumption or recipient delivery/);
});
test("selected room, identity and foreground generations fence confirmation before any mutation",async()=>{
  for(const change of [h=>{h.ctx.selected="other";},h=>{h.ctx.current.identity="replacement";},h=>{h.target.id="other-room";},h=>{h.target.status.foreground_instance=Array(16).fill(2);},h=>{h.ctx.current.phases.team.revision="6";}]) {
    const h=ui();await h.inspect();change(h);await assert.rejects(h.dialogs[1].handler(new Map([["confirm-recovery","on"]])),/session changed/);assert.equal(h.calls.length,1);
  }
});
test("metadata IDs are frozen and failed retry outcomes stay uncertain without automatic retry",async()=>{
  const h=ui();await h.inspect();h.pending.id[0]=99;
  h.ctx.api=async(_path,body)=>{h.calls.push(JSON.parse(JSON.stringify(body)));throw Error("deadline");};
  await assert.rejects(h.dialogs[1].handler(new Map([["confirm-recovery","on"]])),/outcome uncertain/);
  assert.equal(h.calls.length,2);assert.equal(h.calls[1].expected[0],7);assert.equal(h.dialogs.length,2);
});
test("closing pending inspection or retry cannot reopen a late confirmation or acceptance view",async()=>{
  const first=ui();let inspectDone;first.ctx.api=()=>new Promise(done=>{inspectDone=done;});first.ctx.recoveryDialog();
  const inspection=first.dialogs[0].handler();first.close();inspectDone({result:first.pending});await inspection;assert.equal(first.dialogs.length,1);
  const second=ui();await second.inspect();let retryDone;second.ctx.api=()=>new Promise(done=>{retryDone=done;});
  const retry=second.dialogs[1].handler(new Map([["confirm-recovery","on"]]));second.close();retryDone({result:receipt()});await retry;assert.equal(second.dialogs.length,2);
});
test("missing or malformed public metadata never invents a proposal or issues a retry",async()=>{
  for(const pending of [null,{id:[1],kind:"admission",epoch_before:2},{id:Array(32).fill(1),kind:"unknown",epoch_before:2}]) {
    const h=ui();h.ctx.api=async()=>({result:pending});h.ctx.recoveryDialog();
    if(pending===null) {await h.dialogs[0].handler();assert.match(h.dialogs[0].controls[0].textContent,/not an invitation or recipient-delivery receipt/);}
    else await assert.rejects(h.dialogs[0].handler(),/Invalid public recovery metadata/);
    assert.equal(h.dialogs.length,1);
  }
});
