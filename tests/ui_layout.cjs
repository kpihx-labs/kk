const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const html = fs.readFileSync(__dirname + "/../src/ui/assets/index.html", "utf8");
const css = fs.readFileSync(__dirname + "/../src/ui/assets/style.css", "utf8");
const js = fs.readFileSync(__dirname + "/../src/ui/assets/app.js", "utf8");

test("layout markup exposes action center and sidebar controls without duplicate IDs", () => {
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(match => match[1]);
  assert.equal(new Set(ids).size, ids.length);
  assert.match(html, /aria-controls="sidebar"/);
  assert.match(html, /id="sidebar-backdrop"[^>]*aria-label=/);
  assert.match(html, /id="action-center"/);
  assert.match(html, /<details class="people-strip"/);
  assert.match(html, /<details id="media-panel"/);
  assert.match(html, /<details class="tools-menu"><summary>Room actions<\/summary>/);
});

test("mobile drawer Escape closes backdrop and returns focus", async () => {
  const classes = new Set(), listeners = {}, document = {activeElement:null}, main = {inert:false};
  const control = () => ({getClientRects:()=>[{}], focus(){document.activeElement=this;}});
  const first = control(), last = control(), elements = {
    sidebar: {classList:{add:x=>classes.add(x), remove:x=>classes.delete(x), contains:x=>classes.has(x)}, querySelector:()=>first, querySelectorAll:()=>[first,last], setAttribute(key,value){this[key]=value;}, removeAttribute(key){delete this[key];}},
    "sidebar-backdrop": {...control(),hidden:true},
    workspace: {querySelector:()=>main}, dialog:{open:false},
    menu: {setAttribute(key,value){this[key]=value;}, focus(){this.focused=true;}},
    "dialog-close": {},
  };
  const context = {document, $:id=>elements[id], matchMedia:()=>({matches:true,addEventListener:(_type,fn)=>{listeners.resize=fn;}}), addEventListener:(type,fn)=>{listeners[type]=fn;}, current:{settings:{sidebar_mode:"expanded"}}};
  vm.createContext(context);
  vm.runInContext(js.slice(js.indexOf("function closeSidebar()"), js.indexOf("function select(")) + js.slice(js.indexOf('$("menu").onclick'), js.indexOf('$("create").onclick')), context);
  await elements.menu.onclick();
  assert.equal(classes.has("open"), true); assert.equal(elements["sidebar-backdrop"].hidden, false);
  assert.equal(main.inert,true); assert.equal(elements.sidebar.role,"dialog");
  let prevented=false;last.focus();listeners.keydown({key:"Tab",preventDefault(){prevented=true;}});
  assert.equal(prevented,true);assert.equal(document.activeElement,elements["sidebar-backdrop"]);
  listeners.keydown({key:"Tab",shiftKey:true,preventDefault(){}});assert.equal(document.activeElement,last);
  elements.dialog.open=true;listeners.keydown({key:"Escape",preventDefault(){throw Error("nested dialog must own Escape");}});assert.equal(classes.has("open"),true);elements.dialog.open=false;
  listeners.keydown({key:"Escape",preventDefault(){}});
  assert.equal(classes.has("open"), false); assert.equal(elements["sidebar-backdrop"].hidden, true);
  assert.equal(elements.menu["aria-expanded"], "false"); assert.equal(elements.menu.focused, true);
  assert.equal(main.inert,false);assert.equal(elements.sidebar.role,undefined);
  await elements.menu.onclick();listeners.resize({matches:false});assert.equal(main.inert,false);assert.equal(classes.has("open"),false);
});

test("action refresh retains focus across unrelated updates and never dismisses pending SAS", () => {
  const document = {activeElement:null};
  function node(tag, cls, text) {
    return {tag, className:cls, text, dataset:{}, children:[], append(...items){for (const child of items) { child.parent=this; this.children.push(child); }},
      focus(){document.activeElement=this;}, closest(selector){let n=this; while(n) {if(selector === ".operation" && n.className === "operation") return n; n=n.parent;} return null;},
      querySelectorAll(selector){return this.children.flatMap(child=>[...(selector === "button" && child.tag === "button" ? [child] : []), ...child.querySelectorAll(selector)]);}};
  }
  const operations = node("aside", "", ""); operations.replaceChildren = function(...items){this.children=[]; this.append(...items);};
  operations.contains = function(item){while(item){if(item === this) return true; item=item.parent;} return false;};
  const summary = node("summary", "", ""), elements = {operations, "action-count":{}, "action-center":{querySelector:()=>summary}};
  const context = {document, $:id=>elements[id], el:node, current:{operations:[{id:1,room:"room",state:"confirm",proposal:{confirmation:"123",fingerprint:"abc"}}]}, joinDrafts:new Map(), dismissedOperations:new Set(), operationView:"", act:async()=>{}, toast(){}, render(){}};
  vm.createContext(context);
  vm.runInContext(js.slice(js.indexOf("function renderOperations()"), js.indexOf("async function refresh()")), context);
  context.renderOperations();
  const approve = operations.querySelectorAll("button").find(button=>button.dataset.action === "approve");
  assert.ok(approve); assert.equal(operations.querySelectorAll("button").some(button=>button.dataset.action === "dismiss"), false);
  approve.focus(); context.renderOperations(); assert.equal(document.activeElement, approve);
  context.current.operations.push({id:2,room:"other",state:"working"}); context.renderOperations();
  assert.equal(document.activeElement.dataset.action, "approve"); assert.notEqual(document.activeElement, approve);
  context.current.operations = []; context.renderOperations(); assert.equal(document.activeElement, summary);
});

test("source defines viewport containment and preserves pending operation actions", () => {
  assert.match(css, /height:100dvh/);
  assert.match(css, /#operations\{position:fixed/);
  assert.match(css, /data-sidebar=auto/);
  assert.match(js, /!operation\.proposal && !operation\.invitation_ready/);
  assert.match(js, /event\.key === "Escape"/);
  assert.match(js, /sidebar_mode:mode/);
});

test("collapsed panels conserve chat space and mobile keeps the profile name", () => {
  assert.match(css, /\.people-strip:not\(\[open\]\),\.files-panel:not\(\[open\]\)\{padding-top:4px;padding-bottom:4px\}/);
  assert.match(css, /\.room-tools\{flex-direction:row;align-items:center/);
  assert.match(css, /\.topbar #profile-name\{display:block;max-width:/);
  assert.doesNotMatch(css, /\.topbar #profile-name[^{}]*\{display:none/);
});

test("long modal contents scroll without displacing their confirmation controls", () => {
  assert.match(css, /dialog\{padding:0;max-height:calc\(100dvh - 34px\);overflow:hidden\}/);
  assert.match(css, /#dialog-form\{display:flex;flex-direction:column;max-height:/);
  assert.match(css, /#dialog-content\{min-height:0;overflow-y:auto;/);
  assert.match(css, /\.dialog-footer\{flex:none;/);
  assert.match(css, /#dialog-error:empty\{display:none\}/);
});

test("rendered-fixture regressions reserve chat space and keep notices off the composer", () => {
  // Source regressions, not a substitute for main's rendered 34-case replay.
  assert.match(css, /\.room-heading\{flex:0 1 auto;min-height:56px;overflow:auto\}/);
  assert.match(css, /\.heading-actions\{flex:0 1 170px;min-width:0;max-width:45%\}/);
  assert.match(css, /\.composer-input textarea\{min-width:0;flex:1;width:0\}/);
  assert.match(css, /\.topbar-actions\{min-width:0;flex-wrap:wrap\}/);
  assert.match(css, /\.toast\{top:8px;right:16px;bottom:auto;left:auto;transform:none;/);
  assert.match(css, /\.tools-menu:not\(\[open\]\)>\.tools-menu-panel\{display:none\}/);
  const landscape = css.slice(css.lastIndexOf("@media(max-height:500px)"));
  assert.match(landscape, /\.topbar\{height:auto;min-height:44px;/);
  assert.match(landscape, /\.room-heading\{min-height:40px\}/);
  assert.match(landscape, /\.people-strip,\.files-panel,\.media-panel\{min-height:29px;/);
  assert.match(landscape, /\.composer-input textarea\{height:36px;min-height:36px;/);
  assert.match(landscape, /\.messages\{min-height:90px;flex-basis:90px\}/);
});
