"use strict";
const launchCode = location.hash.slice(1); history.replaceState(null, "", location.pathname);
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => { const node = document.createElement(tag); if (cls) node.className = cls; if (text !== undefined) node.textContent = text; return node; };
let token = sessionStorage.getItem("kk-session") || "";
let current = null, selected = sessionStorage.getItem("kk-room") || "", refreshing = false, mediaControl = null, notificationControl = null;
let shownRoom = "", rendered = new Map(), revealed = new Map(), toastTimer;
let fileConsentPending = false;
let conversationPending = false;
let operationView = "";
let historyView = null;
let modalRevision = 0;
let joinDraft = {name:"", bind:"", role:"participant"};
const joinDrafts = new Map();
const dismissedOperations = new Set();
const brief = (fp) => fp ? fp.slice(0, 8) + ":" + fp.slice(-8) : "";
function messageNotifications(platform, view) {
  let cursor = null, enabled = false, lastPermission = "default", active = null, activeRoom = null, timer = null, retired = false, failed = false, permissionPending = false, generation = 0;
  const supported = () => typeof platform.Notification === "function";
  const permission = () => supported() ? platform.Notification.permission : "unsupported";
  const close = () => { generation++; if (timer !== null) platform.clearTimeout(timer); timer = null; try { active?.close(); } catch {} active = null; activeRoom = null; };
  return {
    permission,
    async allow() {
      if (retired || !supported()) throw new Error("System notifications are unavailable in this session/browser.");
      if (permissionPending) throw new Error("A notification permission request is already pending.");
      permissionPending = true;
      let result;
      try { result = permission() === "default" ? await platform.Notification.requestPermission() : permission(); }
      finally { permissionPending = false; }
      if (retired) throw new Error("This notification session has ended.");
      if (result !== "granted") throw new Error("Notifications were not allowed. Change this site's browser permission or keep alerts off.");
      failed = false;
      return result;
    },
    observe(state) {
      if (retired) return;
      const allowed = state?.settings?.message_notifications === true, access = permission();
      const ready = cursor !== null && enabled && allowed && access === "granted" && lastPermission === "granted" && !failed;
      let newest = cursor ?? 0, candidate = null;
      for (const message of state?.messages || []) {
        if (!Number.isSafeInteger(message.id) || message.id <= 0) continue;
        newest = Math.max(newest, message.id);
        if (ready && message.id > cursor && message.outgoing === false && message.sensitive === false && message.kind !== "reaction"
            && typeof message.room === "string" && (state.rooms || []).some(room => room.name === message.room)
            && (view.hidden() || !view.focused() || message.room !== view.selected())) candidate = message.room;
      }
      cursor = newest; enabled = allowed; lastPermission = access;
      if (!allowed || access !== "granted" || (activeRoom !== null && !(state.rooms || []).some(room => room.name === activeRoom))) close();
      if (candidate === null) return;
      close();
      try {
        // No room/member names, bodies, previews or references cross into the OS.
        active = new platform.Notification("K&K", {body:"New message received. Open K&K to view it.", tag:"kk-message-activity", silent:true});
        activeRoom = candidate;
        const ticket = generation;
        active.onclick = () => { if (!retired && ticket === generation && enabled && permission() === "granted") { close(); view.open(candidate); } };
        timer = platform.setTimeout(() => { if (ticket === generation) close(); }, 10000);
      } catch { failed = true; close(); view.unavailable(); }
    },
    close,
    retire() { retired = true; close(); },
  };
}
function toast(text) { $("toast").textContent = text; $("toast").hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => { $("toast").hidden = true; }, 5500); }
async function api(path, body) {
  const headers = { Authorization: "Bearer " + token };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, { method: body === undefined ? "GET" : "POST", headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store" });
  if (response.status === 401 || response.status === 403) {
    token = ""; sessionStorage.removeItem("kk-session"); mediaControl?.stopAll(); notificationControl?.retire(); historyView = null; joinDrafts.clear(); joinDraft = {name:"", bind:"", role:"participant"}; $("dialog").close(); $("dialog-content").replaceChildren();
    const message = "This private session expired or the local service restarted. Relaunch kk ui and use its new private browser launch.";
    $("gate-text").textContent = message; $("gate").hidden = false; $("workspace").hidden = true;
    throw new Error(message);
  }
  let value; try { value = await response.json(); } catch { throw new Error("The local service returned an invalid response."); }
  if (!response.ok || value.ok === false) { const error = new Error(value.error || value.code || "Operation refused."); error.code = typeof value.code === "string" ? value.code : undefined; throw error; }
  return value;
}
async function act(action) { const value = await api("/api/action", action); await refresh(); return value.result; }
function room() { return current?.rooms.find((entry) => entry.name === selected); }
function roomActionAllowed(target, action) { return target?.status?.active === true && target.status.capabilities?.[action] === true; }
function admissionRoles(target) { const roles = target?.status?.capabilities?.admission_roles; return roomActionAllowed(target, "invite") && Array.isArray(roles) ? [...new Set(roles)].filter(role => ["participant", "reader", "inviter", "moderator"].includes(role)) : []; }
function memberName(target, id) { return target?.presentation?.find(member => member.id === id)?.display_name || "Unnamed member"; }
function memberChoice(target, id) { return memberName(target, id) + " / " + brief(id); }
function memberProvenance(target, id) {
  const source = target?.presentation?.find(member => member.id === id)?.source;
  return source === "self_announcement" ? "Self-announced name / not a verified human identity" : source === "local_override" ? "Local nickname / visible only here" : source === "self_profile" ? (current.settings.share_display_name ? "Your profile / sharing enabled" : "Your profile / visible only here; sharing off") : "Name not shared";
}
function nicknameDialog(target, peer) {
  const entry = target.presentation?.find(member => member.id === peer);
  const name = field("Local nickname (leave blank to clear)", "display_name", "text", entry?.source === "local_override" ? entry.display_name : "");
  name.querySelector("input").maxLength = 64;
  modal("Local member nickname", [el("p", "dialog-note", "This nickname is LOCAL ONLY. It is not a verified remote profile name, is not announced to the member, and does not change trust or permissions."), el("code", "fingerprint", peer), name], "Save nickname", async data => { const value = data.get("display_name").trim(); await act({action:"member-label", name:target.name, member:peer, display_name:value || null}); });
}
function closeSidebar() { $("sidebar").classList.remove("open"); $("sidebar").removeAttribute("role"); $("sidebar").removeAttribute("aria-modal"); $("sidebar-backdrop").hidden = true; $("menu").setAttribute("aria-expanded", "false"); const main = $("workspace")?.querySelector("main"); if (main) main.inert = false; }
function select(name) { if (selected !== name) { mediaControl?.stopAll(); historyView = null; $("dialog").close(); $("dialog-content").replaceChildren(); $("copy-panel").hidden = true; $("copies").replaceChildren(); } selected = name; sessionStorage.setItem("kk-room", name); const wasOpen = $("sidebar").classList.contains("open"); closeSidebar(); if (wasOpen) $("menu").focus(); render(); }
function field(label, name, type = "text", value = "", note) {
  const wrapper = el("label", "field", label); const input = el(type === "textarea" ? "textarea" : "input"); input.name = name;
  if (type !== "textarea") input.type = type; if (value) input.value = value;
  wrapper.append(input); if (note) wrapper.append(el("small", "muted", note)); return wrapper;
}
function roleField(value = "participant", roles = ["participant", "reader", "inviter", "moderator"]) { const wrapper = el("label", "field", "Accepted data role"); const input = el("select"); input.name = "role"; for (const role of roles) { const option = el("option", "", role); option.value = role; input.append(option); } input.value = roles.includes(value) ? value : roles[0] || ""; wrapper.append(input); return wrapper; }
function modal(title, controls, submit, handler) {
  const revision = ++modalRevision, active = () => revision === modalRevision && $("dialog").open;
  $("dialog-title").textContent = title; $("dialog-content").replaceChildren(...controls); $("dialog-error").textContent = ""; $("dialog-submit").textContent = submit;
  $("dialog-submit").disabled = false;
  $("dialog-form").onsubmit = async (event) => { event.preventDefault(); const button = $("dialog-submit"); button.disabled = true;
    try { const result = await handler(new FormData(event.currentTarget)); if (active() && result !== false) $("dialog").close(); } catch (error) { if (active()) $("dialog-error").textContent = error.message; } finally { if (revision === modalRevision) button.disabled = false; }
  }; $("dialog").showModal();
}
function createDialog() {
  if (!current?.identity) { toast("Initialize this device first."); return; }
  const name = field("Local room name", "name", "text", "", "1-40 lowercase letters, digits or hyphens."); name.querySelector("input").required = true;
  const title = field("Room title", "title"); title.querySelector("input").required = true; title.querySelector("input").maxLength = 80;
  const bind = field("Local listen IP:port", "bind", "text", "", "For another device, enter this device's specific reachable LAN or VPN interface address and an available port. Loopback works only on this device; never use 0.0.0.0 as the advertised address."); bind.querySelector("input").placeholder = "192.168.1.10:45462"; bind.querySelector("input").required = true;
  const loopback = el("button", "outline compact", "Use loopback for this device only"); loopback.type = "button"; loopback.onclick = () => { bind.querySelector("input").value = "127.0.0.1:45462"; };
  modal("Create a room", [name, title, bind, loopback], "Create and start", async (data) => { const name = data.get("name").trim(); await act({ action: "create", name, title: data.get("title"), bind: data.get("bind").trim() }); select(name); await act({ action: "serve", name, accept_files: false }); toast("Room created. Once online, use Invite a member to create a private invitation."); });
}
function joinDialog() {
  if (!current?.identity) { toast("Initialize this device first."); return; }
  const name = field("Local room name", "name", "text", joinDraft.name); name.querySelector("input").required = true;
  const bind = field("This device's reachable IP:port", "bind", "text", joinDraft.bind); bind.querySelector("input").required = true;
  const invitation = field("Private invitation JSON", "invitation", "textarea", "", "This single-use capability stays in the local ceremony, not chat history."); invitation.querySelector("textarea").required = true;
  const upload = field("Or choose an invitation file", "invitation-file", "file"); upload.querySelector("input").accept = "application/json,.json";
  let generation = 0; const area = invitation.querySelector("textarea");
  area.addEventListener("input", () => { generation++; });
  upload.querySelector("input").onchange = async (event) => { const revision = ++generation, file = event.target.files[0]; area.value = ""; if (!file) return; if (file.size > 4096) { toast("Invitation files are limited to 4 KiB."); return; } try { const text = await file.text(); if (revision !== generation) return; JSON.parse(text); area.value = text; } catch { if (revision === generation) { area.value = ""; toast("Could not read a valid invitation JSON file. Choose another or paste it."); } } };
  modal("Join a room", [name, bind, roleField(joinDraft.role), upload, invitation], "Start secure pairing", async (data) => { const name = data.get("name").trim(); joinDraft = {name, bind:data.get("bind").trim(), role:data.get("role")}; const result = await act({ action: "join", ...joinDraft, invitation: data.get("invitation"), accept_files: false }); area.value = ""; upload.querySelector("input").value = ""; joinDrafts.set(result.operation, {...joinDraft}); select(name); });
}
function retryJoin(operation) { if (!joinDrafts.has(operation.id)) return; joinDraft = {...joinDrafts.get(operation.id)}; dismissedOperations.add(operation.id); joinDrafts.delete(operation.id); joinDialog(); }
function inviteDialog() {
  const target = room(), roles = admissionRoles(target); if (!roles.length) { toast("Inviting is unavailable with this device's cached data rights."); return; }
  const bind = field("Pairing listen IP:port", "bind", "text", "", "Use a different port from this room's data port."); bind.querySelector("input").required = true;
  try { const url = new URL("http://" + target.status.address); url.port = String(Number(url.port) < 65535 ? Number(url.port) + 1 : Number(url.port) - 1); bind.querySelector("input").value = url.host; } catch {}
  modal("Invite a member", [bind, roleField("participant", roles), el("p", "dialog-note", "The other device must choose the same accepted role. Available roles reflect cached data rights, not fresh authorization. Compare CONFIRM on both devices before approving.")], "Create single-use invitation", async (data) => { const latest = room(); if (latest?.name !== target.name || !admissionRoles(latest).includes(data.get("role"))) throw new Error("Room or admission rights changed. Reopen this invitation."); await act({ action: "invite", name: target.name, bind: data.get("bind").trim(), role: data.get("role") }); });
}
function controllersDialog() {
  const target = room(); if (!roomActionAllowed(target, "control")) { toast("Controller changes require administrative data rights."); return; }
  const list = el("div"); for (const peer of target.status.members) { const row = el("label", "field"); const input = el("input"); input.type = "checkbox"; input.name = "peers"; input.value = peer; input.checked = target.status.voters.includes(peer); row.title = peer; row.append(input, el("span", "", memberChoice(target, peer))); list.append(row); }
  modal("Choose voting controllers", [el("p", "dialog-note", "Only current data members are eligible; approving the set requires administrative data rights. A vote does not grant publishing or moderation rights. Retire a member's vote before removing its data access. A majority must remain available."), list], "Approve exact controller set", async (data) => { const latest = room(); if (latest?.name !== target.name || !roomActionAllowed(latest, "control")) throw new Error("Room or administrative rights changed. Reopen controller selection."); await act({ action: "controllers", name: target.name, peers: data.getAll("peers") }); toast("Controller set committed. Data access is unchanged."); });
}
function removeMemberDialog(target, peer) {
  const latest = room();
  if (latest?.name !== target.name || !roomActionAllowed(latest, "remove")) { toast("Removing data access requires moderation rights."); return; }
  if (latest.status.voters.includes(peer)) { toast("Retire this member's controller vote before removing data access."); return; }
  modal("Remove data membership?", [el("p", "dialog-note", "This rotates the private group epoch. Names are presentation only; verify the exact device fingerprint. Previously received data cannot be recalled."), el("code", "fingerprint", peer)], "Remove data access", async () => {
    const currentRoom = room();
    if (currentRoom?.name !== target.name || !roomActionAllowed(currentRoom, "remove") || currentRoom.status.voters.includes(peer) || !currentRoom.status.members.includes(peer)) throw new Error("Room, member or controller state changed. Review the current member list.");
    await act({action:"remove", name:target.name, peer});
    toast("Data membership removal accepted and applied locally. Previously received data cannot be recalled.");
  });
}
function addressDialog() {
  const target = room(); if (!target) return;
  const bind = field("New reachable local IP:port", "bind", "text", target.status?.address || ""); bind.querySelector("input").required = true;
  modal("Move this device's address", [bind, el("p", "dialog-note", "Stop this room first, then start it with the new address. The device key, data rights and voter set stay unchanged. Publication requires a reachable quorum.")], "Start with authenticated new address", async (data) => { if (target.status) throw new Error("Stop this room before rebinding its socket."); await act({ action: "serve", name: target.name, bind: data.get("bind").trim(), accept_files: $("allow-files").checked }); });
}
function fileDialog(label, send) {
  const target = room(); if (!target?.status) { toast("Start the room before sending files."); return; }
  const wrapper = el("label", "field", "Explicit recipient"); const peers = el("select"); peers.name = "to";
  for (const peer of target.status.members.filter((peer) => peer !== target.status.owner)) { const option = el("option", "", memberChoice(target, peer)); option.value = peer; option.title = peer; peers.append(option); }
  if (!peers.options.length) { toast("Invite a recipient first."); return; } wrapper.append(peers);
  const privacy = el("label", "check"); const sensitive = el("input"); sensitive.type = "checkbox"; sensitive.name = "sensitive"; sensitive.checked = $("sensitive").checked; privacy.append(sensitive, el("span", "", "Sensitive local receipt"));
  modal(label, [wrapper, privacy, el("p", "dialog-note", "The recipient must explicitly consent to file storage. Retrying the same content verifies its prefix; a receipt is not a future availability promise.")], "Send to this recipient", async (data) => { const result = await send(target.name, data.get("to"), data.get("sensitive") === "on"); if (!result.ok) throw new Error(result.code === "DENIED" ? "Transfer not allowed. Check your room permissions and ask the recipient to enable incoming files." : result.result?.error || result.code || "The recipient refused the file."); toast("Verified durable recipient storage."); });
}
async function loadFiles() {
  const name = selected; if (!name || !room()) return;
  try { const files = await api("/api/rooms/" + encodeURIComponent(name) + "/files"); if (name !== selected) return;
    $("file-problem").hidden = true; $("file-problem").textContent = "";
    $("files").replaceChildren(...files.map((file, index) => { const card = el("div", "file-card"); card.append(el("span", "file-icon", "F"), el("span", "", "Received file " + (index + 1) + " / " + Math.ceil(file.bytes / 1024) + " KiB"));
      const download = el("button", "outline compact", "Download"); download.onclick = async () => { try { const response = await fetch("/api/rooms/" + encodeURIComponent(name) + "/files/" + encodeURIComponent(file.id), { headers: { Authorization: "Bearer " + token } }); if (!response.ok) throw new Error("Inbox download refused."); const url = URL.createObjectURL(await response.blob()); const anchor = el("a"); anchor.href = url; anchor.download = file.id; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); } catch (error) { toast(error.message); } };
      const forward = el("button", "outline compact", "Forward copy"); forward.onclick = () => fileDialog("Forward this stored copy", async (room, to, sensitive) => { const response = await fetch("/api/rooms/" + encodeURIComponent(room) + "/files/" + encodeURIComponent(file.id) + "/forward?to=" + encodeURIComponent(to) + "&sensitive=" + sensitive, { method: "POST", headers: { Authorization: "Bearer " + token } }); return response.json(); });
      const share = el("button", "outline compact", "Offer copy"); share.onclick = () => shareCopyDialog(name, file.id);
      const withdraw = el("button", "outline compact", "Withdraw offer"); withdraw.onclick = async () => { try { const digest = file.id.slice(65, -4); const reply = await copyRequest(name, {operation:"unshare", sha256:digest}); toast(reply.withdrawn ? "Offer withdrawn; stored bytes are unchanged." : "No active offer for this copy."); } catch(error) { toast(error.message); } };
      card.append(download, forward, share, withdraw); return card; }));
    $("files-panel").hidden = !files.length;
  } catch (error) { if (name !== selected) return; $("file-problem").textContent = "Inbox could not refresh; stored bytes are unchanged. " + error.message; $("file-problem").hidden = false; $("files-panel").hidden = false; }
}
function copyPeers(target) {
  const list = el("div");
  for (const peer of target.status.members.filter(peer => peer !== target.status.owner)) { const row = el("label", "field"), checkbox = el("input"); checkbox.type = "checkbox"; checkbox.name = "peers"; checkbox.value = peer; row.title = peer; row.append(checkbox, el("span", "", memberChoice(target, peer))); list.append(row); }
  return list;
}
async function copyRequest(name, command) {
  const reply = await act({action:"copies", name, command});
  if (!reply.ok && !(command.operation === "locate" && reply.code === "PARTIAL_AVAILABILITY")) throw new Error(reply.result?.error || reply.code || "Copy operation refused.");
  return reply.result;
}
function shareCopyDialog(name, id) {
  const target = room(); if (target?.name !== name || !target.status) { toast("Start this room before offering a copy."); return; }
  modal("Offer this stored copy", [el("p", "dialog-note", "Only checked peers may locate or fetch this copy. The offer ends on stop, withdrawal or membership epoch change. Stored bytes are never deleted."), el("code", "fingerprint", id.slice(65,-4)), copyPeers(target)], "Publish to the exact selected audience", async data => {
    const peers = data.getAll("peers"); if (!peers.length) throw new Error("Choose at least one intended recipient.");
    await copyRequest(name, {operation:"share", id, peers}); toast("Copy offered to the selected peers for this service and membership epoch.");
  });
}
function findCopiesDialog() {
  const target = room(); if (!target?.status) { toast("Start this room before finding copies."); return; }
  const name = target.name, digest = field("Known content SHA-256", "sha256"); digest.querySelector("input").required = true; digest.querySelector("input").pattern = "[a-fA-F0-9]{64}";
  modal("Ask selected copy holders", [digest, copyPeers(target), el("p", "dialog-note", "The content digest is sent only to checked peers. Advertisements are hints; fetching still requires incoming-file consent and verifies the bytes.")], "Find authorized copies", async data => {
    const peers = data.getAll("peers"); if (!peers.length) throw new Error("Choose which peers may receive this digest.");
    const sha256 = data.get("sha256").trim().toLowerCase(), report = await copyRequest(name, {operation:"locate", sha256, peers});
    if (selected !== name) return;
    $("copy-summary").textContent = report.advertised.length + " advertised / " + report.unavailable.length + " unavailable / " + report.failed.length + " unknown. Fetch verifies current bytes; no future availability is promised.";
    $("copies").replaceChildren(...report.advertised.map(hint => { const card = el("div", "file-card"); card.title = hint.peer; card.append(el("span", "", memberChoice(target, hint.peer)), el("span", "", Math.ceil(hint.bytes/1024) + " KiB / advertised")); const fetch = el("button", "outline compact", "Fetch and verify"); fetch.onclick = async () => { fetch.disabled = true; try { await copyRequest(name, {operation:"fetch", sha256, peer:hint.peer}); toast("Verified durable copy stored in " + name + "."); await loadFiles(); } catch(error) { toast(error.message); } finally { fetch.disabled = false; } }; card.append(fetch); return card; }));
    $("copy-panel").hidden = false;
  });
}

// Only this authenticated tuple identifies a publication. A UI sequence, author
// label or equal body is never evidence that a live event is a stored row.
function referenceKey(reference) {
  const bytes = value => Array.isArray(value) && value.length === 32 && value.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255);
  if (!reference || !bytes(reference.room) || !bytes(reference.member) || !bytes(reference.wire_sha256) || !Number.isSafeInteger(reference.epoch) || reference.epoch < 0 || !["OutgoingPrepared", "IncomingProcessed"].includes(reference.event)) return null;
  return JSON.stringify([reference.room, reference.member, reference.epoch, reference.event, reference.wire_sha256]);
}
function messageTarget(reference) {
  if (reference?.event !== undefined && !referenceKey(reference)) return null;
  if (!referenceKey({...reference, event:"IncomingProcessed"})) return null;
  return {room:[...reference.room], member:[...reference.member], epoch:reference.epoch, wire_sha256:[...reference.wire_sha256]};
}
function targetKey(target) {
  const value = messageTarget(target);
  return value ? JSON.stringify([value.room, value.member, value.epoch, value.wire_sha256]) : null;
}
const reactionNames = ["like", "love", "laugh", "surprised", "sad", "thanks"];
function checkedRelation(message) {
  const relation = message.relation;
  if (!relation) return null;
  const target = messageTarget(relation.target);
  if (!target || !message.reference || JSON.stringify(target.room) !== JSON.stringify(message.reference.room)
      || target.epoch > message.reference.epoch || !["reply", "reaction"].includes(relation.kind)
      || (relation.kind === "reaction" && (!reactionNames.includes(relation.value) || typeof relation.added !== "boolean"))) throw new Error("Invalid authenticated conversation relation.");
  return {...relation, target};
}
/**
 * Interpret the backend's complete-recipient queue acknowledgement, never human reading.
 * @param {object|null|undefined} delivery The room IPC reply and its delivery summary.
 * @returns {boolean} True only for explicit successful preparation and all-recipient acknowledgement.
 * @example deliveryComplete({ok:true,result:{all_acknowledged:true}}) // true
 * @example deliveryComplete({ok:true,result:{all_acknowledged:false}}) // false
 */
function deliveryComplete(delivery) { return delivery?.ok === true && delivery.result?.all_acknowledged === true; }
async function conversationAction(action) {
  if (conversationPending) throw new Error("A reply or reaction is already pending.");
  if (room()?.name !== action.name || !roomActionAllowed(room(), "publish")) throw new Error("Room changed or publishing rights are unavailable.");
  conversationPending = true;
  try { render(); const result = await act(action); if (!result.delivery?.ok) throw new Error(result.delivery?.code || "Operation outcome unavailable; inspect the room before retrying.");
    toast(deliveryComplete(result.delivery) ? "Acknowledged by recipient event queues, not human reading." : "Partial delivery; inspect peer outcomes before retrying."); return result;
  } finally { conversationPending = false; if (current) render(); }
}
function replyDialog(message, target) {
  const reference = messageTarget(message.reference), name = target.name;
  if (!reference || message.room !== name) throw new Error("Authenticated message reference unavailable for this room.");
  const body = field("Reply", "reply-text", "textarea"); body.querySelector("textarea").maxLength = 65536;
  const sensitive = field("Sensitive reply / exclude from history and notifications", "reply-sensitive", "checkbox"); sensitive.querySelector("input").checked = message.sensitive === true;
  modal("Reply / " + (target.title || name), [el("p", "dialog-note", "Reply to " + memberName(target, message.sender) + ". The original text is not copied. A reference is not a read receipt or remote history permission."), body, sensitive], "Send reply", async data => {
    if (selected !== name || !roomActionAllowed(room(), "publish")) throw new Error("Room changed, stopped or publishing rights are unavailable. Reopen this reply from its original room.");
    const text = data.get("reply-text"); if (!text) throw new Error("Write a reply first.");
    await conversationAction({action:"reply", name, target:reference, text, sensitive:data.get("reply-sensitive") === "on"});
  });
}
function relationLabel(message, target) {
  const relation = checkedRelation(message);
  if (!relation) return null;
  const known = current.messages.some(entry => entry.room === message.room && targetKey(entry.reference) === targetKey(relation.target));
  const label = relation.kind === "reply" ? "Reply to " + memberName(target, relation.target.member.map(byte=>byte.toString(16).padStart(2,"0")).join("")) : (relation.added ? "Added " : "Removed ") + relation.value;
  return el("small", "muted conversation-relation", label + (known ? " / authenticated message reference" : " / original message unavailable in this view"));
}
function messageActions(item, message, target) {
  let actions = item.querySelector(".message-actions");
  if (!actions) { actions = el("div", "message-actions"); item.append(actions); }
  const reference = messageTarget(message.reference); if (!reference) return;
  const summaries = (current.reactions || []).filter(summary => targetKey(summary.target) === targetKey(reference));
  const publish = roomActionAllowed(target, "publish"), view = JSON.stringify([publish, conversationPending, current.identity, message.message_id, message.kind, message.sensitive, summaries]);
  if (actions.dataset.view === view) return;
  const active = document.activeElement, focus = actions.contains(active) ? active?.dataset.action : null;
  const detailsOpen = actions.querySelector("details")?.open;
  actions.dataset.view = view; actions.replaceChildren();
  const reply = el("button", "outline compact", "Reply"); reply.disabled = !publish || conversationPending;
  reply.dataset.action = "reply";
  reply.onclick = () => { try { replyDialog(message, target); } catch(error) { toast(error.message); } }; actions.append(reply);
  if (message.message_id) { const details = el("details", "technical-details"); details.open = detailsOpen; details.append(el("summary", "", "Message ID"), el("code", "fingerprint", message.message_id)); actions.append(details); }
  if (message.kind === "reaction") { if (focus) [...actions.querySelectorAll("button")].find(button=>button.dataset.action === focus)?.focus(); return; }
  if (message.sensitive) {
    const button = el("button", "outline compact", "Sensitive reaction"); button.disabled = !publish || conversationPending; button.dataset.action = "react-sensitive";
    button.onclick = () => {
      const choice = el("label", "field", "Reaction"), select = el("select"); select.name = "reaction";
      for (const value of reactionNames) { const option = el("option", "", value); option.value = value; select.append(option); } choice.append(select);
      const remove = field("Remove your own reaction", "reaction-remove", "checkbox");
      modal("Sensitive reaction / " + (target.title || target.name), [choice, remove, el("p", "dialog-note", "This activity is excluded from persistent history, OS notifications and public reaction counts. No original text is copied.")], "Apply sensitive reaction", async data => {
        if (selected !== target.name || !roomActionAllowed(room(), "publish")) throw new Error("Room changed, stopped or publishing rights are unavailable.");
        await conversationAction({action:"react", name:target.name, target:reference, value:data.get("reaction"), added:data.get("reaction-remove") !== "on", sensitive:true});
      });
    }; actions.append(button);
  } else for (const value of reactionNames) {
    const summary = summaries.find(entry=>entry.value === value), members = summary?.members || [];
    const own = members.some(member=>member.map(byte=>byte.toString(16).padStart(2,"0")).join("") === current.identity);
    const button = el("button", "outline compact", value + (members.length ? " " + members.length : "")); button.disabled = !publish || conversationPending;
    button.dataset.action = "react-" + value;
    button.setAttribute("aria-pressed", String(own)); button.title = (own ? "Remove your " : "Add your ") + value + " reaction; counts cover buffered nonsensitive activity";
    button.onclick = async () => { try { await conversationAction({action:"react", name:target.name, target:reference, value, added:!own, sensitive:message.sensitive === true}); } catch(error) { toast(error.message); } };
    actions.append(button);
  }
  if (focus) [...actions.querySelectorAll("button")].find(button=>button.dataset.action === focus)?.focus();
}
function historyEntries(rows, live) {
  const entries = new Map();
  for (const row of rows) {
    const key = referenceKey(row.reference), record = row.record;
    if (!key || !record || record.event !== row.reference.event || record.epoch !== row.reference.epoch || JSON.stringify(record.wire_sha256) !== JSON.stringify(row.reference.wire_sha256) || typeof record.body !== "string") throw new Error("Invalid authenticated history reference; records not displayed.");
    const entry = {key, text:record.body, relation:record.relation || null, kind:record.relation?.kind || "text", reference:row.reference, stored:true, sender:row.reference.member.map(byte => byte.toString(16).padStart(2, "0")).join(""), sensitive:false};
    checkedRelation(entry); entries.set(key, entry);
  }
  for (const message of live) {
    const key = referenceKey(message.reference);
    if (key && entries.has(key)) continue;
    entries.set(key || "live:" + message.id, {...message, key:key || "live:" + message.id, stored:false});
  }
  return [...entries.values()];
}
function historyStateText(summary) {
  if (summary === "Off") return "OFF / not enrolled. No automatic archive.";
  const enrolled = summary?.Enrolled;
  if (!enrolled) return "UNKNOWN / refresh public status. Unknown is not empty history.";
  return (enrolled.enabled ? "CAPTURE ON" : "CAPTURE OFF") + (enrolled.unlocked ? " / existing key unlocked" : " / LOCKED: key unavailable, not empty history") + ". Cached public metadata, not a verified row count or current quorum.";
}
async function historyRequest(name, command) {
  const response = await api("/api/action", {action:"history", name, command});
  return response.result;
}
function renderHistoryEntries(view) {
  if (historyView !== view || !view.list.isConnected) return;
  const live = (current?.messages || []).filter(message => message.room === view.name);
  const entries = historyEntries(view.rows, live);
  view.list.replaceChildren(...entries.map(entry => {
    const item = el("article", "history-record"), event = referenceKey(entry.reference) ? entry.reference.event : null;
    const fact = event === "OutgoingPrepared" ? "Outgoing prepared / outcome uncertain" : event === "IncomingProcessed" ? "Incoming processed" : "Live only / authenticated stable reference unavailable";
    item.append(el("small", "muted", memberName(room(), entry.sender) + " / " + fact + (entry.stored ? " / local encrypted record" : " / bounded memory")), el("div", "message-body", entry.sensitive ? "Sensitive / excluded from stored history; reveal only in live chat." : entry.kind === "reaction" ? "Reaction activity" : entry.text));
    if (!entry.sensitive) { const relation = relationLabel({...entry,room:view.name}, room()); if (relation) item.append(relation); }
    messageActions(item, {...entry,room:view.name}, room());
    return item;
  }));
  if (!entries.length) view.list.append(el("p", "muted", view.searched ? "No disclosable records on this page." : "Submit a search to read stored records. Live messages are shown separately until an authenticated reference is available."));
}
function historyDialog() {
  const target = room(); if (!target?.status) { toast("Start the room before using its local history actor."); return; }
  const name = target.name, status = el("p", "dialog-note", "Status not requested yet."), notice = el("p", "muted", "Off by default. Enrollment captures future non-sensitive text only. These are local encrypted records, not delivery or human read receipts. No files, media or old-message import."), controls = el("div", "history-controls");
  const query = field("Literal Unicode substring (blank lists a page)", "history-query"); query.querySelector("input").maxLength = 256;
  const paging = el("p", "muted", "Stored history has not been queried."), list = el("div", "history-records"), next = el("button", "outline compact", "Next stored page"); next.type = "button"; next.disabled = true;
  const view = {name, rows:[], query:"", cursor:0, searched:false, list, pending:false}; historyView = view;
  const alive = () => historyView === view && selected === name && token;
  const report = error => { if (alive()) { status.textContent = "Unavailable / outcome uncertain. No empty-history claim. Refresh status before retrying a mutation. " + error.message; next.disabled = true; } };
  const button = (label, handler) => { const node = el("button", "outline compact", label); node.type = "button"; node.onclick = async () => { if (view.pending) return; view.pending = true; node.disabled = true; try { await handler(); } catch(error) { report(error); } finally { view.pending = false; node.disabled = false; } }; controls.append(node); return node; };
  const getStatus = async operation => { const summary = await historyRequest(name, {operation}); if (alive()) status.textContent = historyStateText(summary); };
  button("Refresh current status", () => getStatus("refresh"));
  const mutation = (operation, explanation, label, extra = []) => {
    modal(label, [el("p", "dialog-note", explanation + " A timeout or lost reply may occur after commit. Refresh status and inspect records before explicitly retrying."), ...extra], label, async data => {
      const command = {operation, confirm:true}; if (operation === "recover") command.expected_recipient = data.get("expected-recipient").trim();
      await historyRequest(name, command); if (alive()) historyDialog(); return false;
    });
  };
  button("Enable future capture", async () => mutation("enable", "Explicitly enroll or re-enable this profile's history key. Capture future non-sensitive text only; do not import previous chat. Existing orphan keys require separate public-recipient recovery.", "Enable local capture"));
  button("Disable capture", async () => { await historyRequest(name, {operation:"disable"}); await getStatus("status"); });
  button("Unlock existing key", async () => { await historyRequest(name, {operation:"open"}); await getStatus("status"); });
  button("Clear stored records", async () => mutation("clear", "Delete retained encrypted history records for this profile and room? This cannot be undone. The enrolled key, room state and live in-memory messages remain. A timeout may occur after deletion commits.", "Confirm clear records"));
  button("Recover orphan key", async () => { const expected = field("Expected PUBLIC age recipient", "expected-recipient", "text", "", "Obtain independently from trusted enrollment metadata. Never enter a private key or a filesystem path."); expected.querySelector("input").required = true; expected.querySelector("input").maxLength = 128; mutation("recover", "Explicit orphan recovery requires capture off and the exact expected public recipient. This cannot replace an enrolled recipient or repair a missing key.", "Confirm orphan recovery", [expected]); });
  const page = async after => {
    if (view.pending) return;
    view.pending = true; next.disabled = true;
    try {
      const result = await historyRequest(name, {operation:"search", text:view.query, after_id:after, limit:20});
      if (!alive()) return;
      if (!Array.isArray(result.rows) || !Number.isSafeInteger(result.next_after) || result.next_after < after || typeof result.has_more !== "boolean" || !Number.isSafeInteger(result.withheld) || result.withheld < 0 || (result.has_more && result.next_after <= after)) throw new Error("Invalid bounded page response.");
      historyEntries(result.rows, []); view.rows = result.rows; view.cursor = result.next_after; view.searched = true;
      paging.textContent = result.rows.length + " stored records / " + result.withheld + " withheld on this page / cursor " + result.next_after + (result.has_more ? " / more available" : " / end of scan") + ". Live-only entries below do not affect this cursor.";
      renderHistoryEntries(view); next.disabled = !result.has_more;
    } catch(error) { if (alive()) { view.rows = []; list.replaceChildren(); report(error); } }
    finally { view.pending = false; }
  };
  next.onclick = () => page(view.cursor);
  modal("Local encrypted history / " + (target.title || name), [notice, status, controls, query, paging, next, list], "Search stored records", async data => { view.query = data.get("history-query"); await page(0); });
  // Search is an in-panel operation, not a dialog dismissal.
  $("dialog-form").onsubmit = async event => { event.preventDefault(); if (view.pending) return; view.query = query.querySelector("input").value; await page(0); };
  renderHistoryEntries(view);
  getStatus("status").catch(report);
}
function render() {
  if (!current) return;
  applyAppearance(current.settings.appearance);
  $("workspace").dataset.sidebar = current.settings.sidebar_mode || "expanded";
  $("version").textContent = current.version; $("profile-name").textContent = current.settings.display_name; $("identity").textContent = brief(current.identity); $("identity").title = current.identity || "Not initialized";
  $("welcome").hidden = Boolean(current.identity); $("create").disabled = !current.identity; $("join").disabled = !current.identity;
  if (!selected && current.rooms.length) selected = current.rooms[0].name;
  $("rooms").replaceChildren(...current.rooms.map((entry) => { const button = el("button", "room-link" + (entry.name === selected ? " selected" : "")); button.setAttribute("aria-label", entry.title || entry.name); button.title = entry.title || entry.name; const text = el("span"); text.append(el("strong", "", entry.title || entry.name), el("small", "", entry.status ? "foreground / online" : entry.problem ? "recovery required" : "offline")); button.append(el("span", "room-icon", "#"), text); button.onclick = () => select(entry.name); return button; }));
  const target = room(); $("room-view").hidden = !current.identity || !target; $("choose-room").hidden = !current.identity || Boolean(target);
  if (target) {
    const namesError = target.presentation_error || target.status?.profile_names_error;
    $("room-title").textContent = target.title || target.name; $("room-id").textContent = target.id || "Unavailable"; $("room-problem").textContent = target.problem || (namesError ? "Member names unavailable: " + namesError : ""); $("room-problem").hidden = !target.problem && !namesError;
    $("room-state").textContent = target.status ? "FOREGROUND / ONLINE" : (current.phases[target.name]?.state || "OFFLINE").toUpperCase();
    $("start").hidden = Boolean(target.status); $("stop").hidden = !target.status; $("room-history").disabled = !target.status; $("invite").disabled = !admissionRoles(target).length; $("controllers").disabled = !roomActionAllowed(target, "control"); $("find-copies").disabled = !target.status; $("share-media").disabled = !roomActionAllowed(target, "publish") || !mediaControl; $("receive-media").disabled = !target.status || !mediaControl;
    $("text").disabled = !roomActionAllowed(target, "publish"); $("text").placeholder = $("text").disabled ? "Publishing unavailable with this device's cached room rights." : "Write to the current room members..."; $("send").disabled = $("text").disabled; $("attach").disabled = $("text").disabled; $("allow-files").disabled = fileConsentPending;
    if (target.status?.files && !fileConsentPending) $("allow-files").checked = target.status.files.accepted;
    $("quorum").textContent = target.status ? target.status.members.length + " MEMBERS / " + target.status.voters.length + " VOTERS / EPOCH " + target.status.epoch : "PERSISTENT KEYS / NO BACKGROUND SERVICE";
    const openPeers = new Set([...$("peers").querySelectorAll(".peer")].filter(card => card.querySelector("details")?.open).map(card => card.dataset.peer));
    $("peers").replaceChildren(...(target.status?.members || []).map((peer) => { const card = el("div", "peer"), self = peer === target.status.owner; card.dataset.peer = peer; const label = el("span"); const name = memberName(target, peer); const details = el("details", "technical-details"); details.open = openPeers.has(peer); details.append(el("summary", "", "Fingerprint"), el("code", "fingerprint", peer)); label.append(el("strong", "", name), el("small", "", (self ? "This device / " : "") + (target.status.voters.includes(peer) ? "Voting controller" : "Data member")), details); card.append(el("span", "peer-avatar", name === "Unnamed member" ? "?" : Array.from(name.trim()).slice(0, 2).join("").toUpperCase()), label);
      label.append(el("small", "muted", memberProvenance(target, peer)));
      if (!self) { const nickname = el("button", "outline compact", "Nickname"); nickname.onclick = () => nicknameDialog(target, peer); card.append(nickname); const remove = el("button", "peer-remove", "x"); remove.disabled = !roomActionAllowed(target, "remove") || target.status.voters.includes(peer); remove.title = target.status.voters.includes(peer) ? "Retire this member's controller vote before removing data access" : "Remove data membership / requires moderation rights"; remove.setAttribute("aria-label", remove.title); remove.onclick = () => removeMemberDialog(target, peer); card.append(remove); } return card; }));
    mediaControl?.refreshLabels();
    if (shownRoom !== selected) { shownRoom = selected; rendered.clear(); revealed.clear(); $("messages").replaceChildren(); $("files").replaceChildren(); }
    const messages = current.messages.filter((message) => message.room === selected);
    if (!messages.length && !$("messages").querySelector(".empty-message")) { const empty = el("div", "empty-message"); empty.append(el("span", "line-art"), el("h2", "", "A direct conversation."), el("p", "", "Live messages use bounded memory. Room actions / Local history opens the optional encrypted archive.")); $("messages").append(empty); }
    for (const message of messages) {
      if (rendered.has(message.id)) { const item = rendered.get(message.id), meta = item.querySelector(".message-meta"); meta.textContent = (message.outgoing ? memberName(target, target.status?.owner) : memberName(target, message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED"); messageActions(item, message, target); continue; }
      $("messages").querySelector(".empty-message")?.remove(); const item = el("article", "message" + (message.outgoing ? " outgoing" : ""));
      const meta = el("div", "message-meta", (message.outgoing ? memberName(target, target.status?.owner) : memberName(target, message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED")); meta.title = message.sender; item.append(meta);
      const body = el("div", "message-body" + (message.sensitive ? " sensitive" : ""));
      if (message.sensitive) { body.append(el("span", "", "Hidden on this device / " + message.bytes + " bytes")); const reveal = el("button", "", "Reveal"); reveal.onclick = async () => { try { const value = await act({ action: "reveal", id: message.id }); body.textContent = value.text; const relation = relationLabel({...message, relation:value.relation}, target); if (relation) body.append(relation); revealed.set(message.id, true); } catch (error) { toast(error.message); } }; body.append(reveal); }
      else { body.textContent = message.kind === "reaction" ? "Reaction activity" : message.text; const relation = relationLabel(message, target); if (relation) body.append(relation); } item.append(body); messageActions(item, message, target); rendered.set(message.id, item); $("messages").append(item); $("messages").scrollTop = $("messages").scrollHeight;
    }
    const retained = new Set(messages.map((message) => message.id)); for (const [id, node] of rendered) if (!retained.has(id)) { node.remove(); rendered.delete(id); revealed.delete(id); }
  }
  renderOperations();
  if (historyView) renderHistoryEntries(historyView);
}
function renderOperations() {
  const operationIds = new Set(current.operations.map((operation) => operation.id));
  for (const operation of current.operations) if (operation.kind === "join" && ["joined", "stopped"].includes(operation.state)) joinDrafts.delete(operation.id);
  for (const id of joinDrafts.keys()) if (!operationIds.has(id)) joinDrafts.delete(id);
  for (const id of dismissedOperations) if (!operationIds.has(id)) dismissedOperations.delete(id);
  const visibleOperations = current.operations.filter(operation => !dismissedOperations.has(operation.id));
  $("action-count").textContent = String(visibleOperations.length);
  const view = JSON.stringify([visibleOperations, [...joinDrafts.keys()]]);
  if (view === operationView) return;
  operationView = view;
  const active = document.activeElement, focusedCard = active?.closest?.(".operation");
  const focusKey = focusedCard && $("operations").contains(active) ? [focusedCard.dataset.operation, active.dataset.action] : null;
  $("operations").replaceChildren(...visibleOperations.map((operation) => {
    const card = el("section", "operation"); card.dataset.operation = String(operation.id); card.append(el("h3", "", operation.room + " / " + operation.state));
    if (operation.error) card.append(el("p", "error", operation.error));
    if (["failed", "declined", "complete", "stopped", "joined"].includes(operation.state) && !operation.proposal && !operation.invitation_ready) { if (operation.kind === "join" && joinDrafts.has(operation.id)) { const retry = el("button", "primary compact", "Edit joining details"); retry.dataset.action = "retry"; retry.onclick = () => retryJoin(operation); card.append(el("p", "", "The invitation is not retained. Paste or select a valid private invitation again."), retry); } const dismiss = el("button", "outline compact", "Dismiss"); dismiss.dataset.action = "dismiss"; dismiss.onclick = () => { dismissedOperations.add(operation.id); joinDrafts.delete(operation.id); render(); }; card.append(dismiss); }
    if (operation.proposal) { card.append(el("p", "", "Compare this CONFIRM value on both devices."), el("div", "sas", operation.proposal.confirmation), el("code", "", operation.proposal.fingerprint)); const actions = el("div", "operation-actions"); for (const accept of [true, false]) { const button = el("button", accept ? "primary compact" : "outline compact", accept ? "Matches / approve" : "Reject"); button.dataset.action = accept ? "approve" : "reject"; button.onclick = async () => { try { await act({ action: "confirm", id: operation.id, accept }); } catch (error) { toast(error.message); } }; actions.append(button); } card.append(actions); }
    else if (operation.invitation_ready && !["failed", "working", "declined"].includes(operation.state)) { card.append(el("p", "", "Send this private single-use JSON to the intended device. A public fingerprint alone is not an invitation. It does not grant a vote.")); const button = el("button", "outline compact", "Download invitation"); button.dataset.action = "download"; button.onclick = async () => { try { const response = await fetch("/api/operations/" + operation.id + "/invitation", { headers: { Authorization: "Bearer " + token } }); if (!response.ok) throw new Error("Invitation is unavailable or expired."); const url = URL.createObjectURL(await response.blob()); const anchor = el("a"); anchor.href = url; anchor.download = "kk-invitation.json"; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); } catch (error) { toast(error.message); } }; card.append(button); }
    return card;
  }));
  if (focusKey) {
    const card = [...$("operations").children].find(node => node.dataset.operation === focusKey[0]);
    const replacement = card && [...card.querySelectorAll("button")].find(node => node.dataset.action === focusKey[1]);
    (replacement || $("action-center").querySelector("summary"))?.focus();
  }
}
async function refresh() { if (!token || refreshing) return; refreshing = true; try { current = await api("/api/state"); notificationControl?.observe(current); $("connection").textContent = "LOCAL / CONNECTED"; render(); await loadFiles(); } catch (error) { $("connection").textContent = "LOCAL / UNAVAILABLE"; mediaControl?.stopAll(); notificationControl?.close(); } finally { refreshing = false; } }
function applyAppearance(appearance) {
  const style = document.documentElement.style;
  const keys = ["--bg", "--surface", "--ink", "--muted", "--line", "--accent", "--accent-soft"];
  if (appearance.hue === null && appearance.clarity === 50 && appearance.saturation === 20) { for (const key of keys) style.removeProperty(key); return; }
  const hue = appearance.hue ?? 0, saturation = appearance.hue === null ? 0 : appearance.saturation;
  const clarity = appearance.clarity;
  const background = clarity <= 50 ? 3 + clarity * 7 / 50 : 10 + (clarity - 50) * 87 / 50;
  // Taper the surface separation towards the white end; keep a common
  // foreground readable on both surfaces, even for highly saturated hues.
  let surface = Math.min(100, background + 4 * (100 - clarity) / 50);
  const luminance = light => {
    const s = saturation / 100, l = light / 100, c = (1 - Math.abs(2 * l - 1)) * s;
    const h = hue / 60, x = c * (1 - Math.abs(h % 2 - 1)), m = l - c / 2;
    const rgb = h < 1 ? [c,x,0] : h < 2 ? [x,c,0] : h < 3 ? [0,c,x] : h < 4 ? [0,x,c] : h < 5 ? [x,0,c] : [c,0,x];
    const linear = v => { v += m; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2]);
  };
  let a = luminance(background), b = luminance(surface);
  let white = Math.min(1.05 / (a + 0.05), 1.05 / (b + 0.05));
  let black = Math.min((a + 0.05) / 0.05, (b + 0.05) / 0.05);
  if (Math.max(white, black) < 4.5) { surface = background; b = a; white = 1.05 / (a + 0.05); black = (a + 0.05) / 0.05; }
  const ink = white >= black ? "#fff" : "#000";
  const color = light => `hsl(${hue} ${saturation}% ${light}%)`;
  style.setProperty("--bg", color(background)); style.setProperty("--surface", color(surface));
  style.setProperty("--ink", ink); style.setProperty("--muted", ink); style.setProperty("--accent", ink);
  style.setProperty("--line", color(Math.max(0, Math.min(100, background + (ink === "#fff" ? 16 : -16)))));
  style.setProperty("--accent-soft", color(surface));
}
$("theme").onclick = () => {
  const baseline = {revision:current.settings.revision ?? "0", display_name:current.settings.display_name, share_display_name:current.settings.share_display_name === true, message_notifications:current.settings.message_notifications === true, sidebar_mode:current.settings.sidebar_mode || "expanded", appearance:{...current.settings.appearance}};
  const settings = current.settings, name = field("Display name", "display_name", "text", settings.display_name); name.querySelector("input").required = true; name.querySelector("input").maxLength = 64;
  const share = field("Share this display name with room members", "share_display_name", "checkbox"); share.querySelector("input").checked = settings.share_display_name === true;
  const notifications = field("Content-free system message notifications", "message_notifications", "checkbox"); notifications.querySelector("input").checked = settings.message_notifications === true;
  const allowNotifications = el("button", "outline compact", "Allow notifications in this browser"); allowNotifications.type = "button";
  const notificationStatus = el("p", "dialog-note", "Browser permission: " + notificationControl.permission());
  notificationStatus.role = "status";
  allowNotifications.disabled = notificationControl.permission() === "unsupported";
  allowNotifications.onclick = async () => {
    const revision = modalRevision;
    allowNotifications.disabled = true; notificationStatus.textContent = "Waiting for the browser's notification permission. No profile change is saved yet.";
    try { await notificationControl.allow(); if (revision !== modalRevision || !$("dialog").open) return; notifications.querySelector("input").checked = true; notificationStatus.textContent = "Browser allowed. Save settings to enable this profile's alerts."; }
    catch (error) { if (revision === modalRevision && $("dialog").open) notificationStatus.textContent = error.message; }
    finally { if (revision === modalRevision && $("dialog").open) allowNotifications.disabled = notificationControl.permission() === "unsupported"; }
  };
  const sidebar = el("label", "field", "Sidebar mode"), sidebarSelect = el("select"); sidebarSelect.name = "sidebar_mode";
  for (const mode of ["expanded", "compact", "auto"]) { const option = el("option", "", mode); option.value = mode; sidebarSelect.append(option); }
  sidebarSelect.value = settings.sidebar_mode || "expanded"; sidebar.append(sidebarSelect);
  const hue = field("Hue (0-350 in steps of 10; blank for neutral)", "hue", "number", settings.appearance.hue === null ? "" : String(settings.appearance.hue)); const hueInput = hue.querySelector("input"); hueInput.min = 0; hueInput.max = 350; hueInput.step = 10;
  const clarity = field("Clarity (0-100)", "clarity", "range", String(settings.appearance.clarity)); const saturation = field("Saturation (0-100)", "saturation", "range", String(settings.appearance.saturation));
  for (const wrapper of [clarity, saturation]) { const input = wrapper.querySelector("input"); input.min = 0; input.max = 100; input.step = 10; const value = el("small", "muted", input.value); input.oninput = () => { value.textContent = input.value; }; wrapper.append(value); }
  modal("Profile settings", [name, share, sidebar, el("p", "dialog-note", "Sharing is off by default. When off, your own name remains visible only here. Previously shared names cannot be erased from other members' knowledge or local nicknames. Names are not verified human identities."), notifications, allowNotifications, notificationStatus, el("p", "dialog-note", "Alerts are off by default, only while this UI is open. No text, names or sensitive-message alerts are sent to the OS; alerts may appear on the lock screen. Browser permission is separate and may need renewal when the local origin changes."), hue, clarity, saturation, el("p", "dialog-note", "Neutral hue, clarity 50 and saturation 20 restore the original dark grey palette. Settings belong to the running profile.")], "Save settings", async data => {
    const raw = data.get("hue"), action = {action:"settings", expected_revision:baseline.revision};
    const values = {display_name:data.get("display_name"), share_display_name:data.get("share_display_name") === "on", message_notifications:data.get("message_notifications") === "on", sidebar_mode:data.get("sidebar_mode")};
    for (const key of Object.keys(values)) if (values[key] !== baseline[key]) action[key] = values[key];
    if (action.message_notifications === true && notificationControl.permission() !== "granted") throw new Error("Allow notifications in this browser first, or keep system alerts off.");
    const appearance = {hue:raw === "" ? null : Number(raw), clarity:Number(data.get("clarity")), saturation:Number(data.get("saturation"))};
    if (Object.keys(appearance).some(key => appearance[key] !== baseline.appearance[key])) action.appearance = appearance;
    try { await act(action); } catch (error) {
      if (error.code !== "SETTINGS_CONFLICT") throw error;
      let latest;
      try { latest = (await api("/api/state")).settings; }
      catch { throw new Error("Settings changed elsewhere, but current consent could not be refreshed. No changes were saved. Retry to reload consent before saving."); }
      current.settings = latest;
      if (values.display_name === baseline.display_name) name.querySelector("input").value = latest.display_name;
      if (values.sidebar_mode === baseline.sidebar_mode) sidebarSelect.value = latest.sidebar_mode;
      for (const [key, wrapper] of [["hue", hue], ["clarity", clarity], ["saturation", saturation]]) {
        if (appearance[key] === baseline.appearance[key]) { const input = wrapper.querySelector("input"); input.value = latest.appearance[key] === null ? "" : String(latest.appearance[key]); input.oninput?.(); }
      }
      share.querySelector("input").checked = latest.share_display_name === true;
      notifications.querySelector("input").checked = latest.message_notifications === true;
      Object.assign(baseline, {revision:latest.revision, display_name:latest.display_name, share_display_name:latest.share_display_name === true, message_notifications:latest.message_notifications === true, sidebar_mode:latest.sidebar_mode, appearance:{...latest.appearance}});
      throw new Error("Settings changed elsewhere. Current sharing and notification consent has been reloaded; review it and your retained edits, then save again.");
    }
  });
};
$("profiles").onclick = async () => { try {
  const profiles = await act({action:"profiles"}), list = el("div");
  for (const profile of profiles) { const row = el("div", "file-card"); row.append(el("strong", "", profile.settings.display_name), el("span", "muted", profile.id + (profile.selected ? " / selected for next launch" : ""))); const choose = el("button", "outline compact", "Select for restart"); choose.onclick = async () => { try { await act({action:"profile-select", id:profile.id}); toast("Selection saved. Stop this foreground UI and restart it to use the selected profile; running rooms remain on this identity."); $("dialog").close(); } catch (error) { $("dialog-error").textContent = error.message; } }; row.append(choose); if (profile.id !== "default") { const remove = el("button", "outline compact", "Unregister"); remove.onclick = async () => { try { await act({action:"profile-unregister", id:profile.id}); $("dialog").close(); toast("Registration removed; profile files remain untouched."); } catch(error) { $("dialog-error").textContent = error.message; } }; row.append(remove); } list.append(row); }
  const id = field("New profile ID", "id"), display = field("New display name", "display_name"), directory = field("Existing profile directory (register only)", "directory");
  modal("Profiles", [el("p", "dialog-note", "Selection affects the next foreground launch only. Existing running rooms never change identity."), list, id, display, directory], "Create or register", async data => { const id = data.get("id").trim(), directory = data.get("directory").trim(); await act(directory ? {action:"profile-register", id, directory} : {action:"profile-create", id, display_name:data.get("display_name")}); toast("Profile registered. Select it and restart to use it."); });
} catch(error) { toast(error.message); } };
$("menu").onclick = async () => {
  if (matchMedia("(max-width: 700px)").matches) {
    const open = !$("sidebar").classList.contains("open");
    if (!open) { closeSidebar(); return; }
    $("sidebar").classList.add("open"); $("sidebar").setAttribute("role", "dialog"); $("sidebar").setAttribute("aria-modal", "true"); $("sidebar-backdrop").hidden = false; $("menu").setAttribute("aria-expanded", "true"); const main = $("workspace")?.querySelector("main"); if (main) main.inert = true; $("sidebar").querySelector("a, button")?.focus();
  } else {
    const modes = ["expanded", "compact", "auto"], mode = modes[(modes.indexOf(current.settings.sidebar_mode || "expanded") + 1) % modes.length];
    try { await act({action:"settings", expected_revision:current.settings.revision, sidebar_mode:mode}); toast("Sidebar: " + mode); } catch (error) { toast(error.message); }
  }
};
$("sidebar-backdrop").onclick = () => { closeSidebar(); $("menu").focus(); };
addEventListener("keydown", event => {
  if (!$("sidebar").classList.contains("open") || $("dialog")?.open) return;
  if (event.key === "Escape") { event.preventDefault(); closeSidebar(); $("menu").focus(); }
  else if (event.key === "Tab" && matchMedia("(max-width: 700px)").matches) {
    const items = [$("sidebar-backdrop"), ...$("sidebar").querySelectorAll('a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])')].filter(item => !item.hidden && item.getClientRects().length);
    if (!items.length) return;
    const first = items[0], last = items.at(-1), active = document.activeElement;
    if (event.shiftKey && (active === first || !items.includes(active))) { event.preventDefault(); last.focus(); }
    else if (!event.shiftKey && (active === last || !items.includes(active))) { event.preventDefault(); first.focus(); }
  }
});
matchMedia("(max-width: 700px)").addEventListener?.("change", event => { if (!event.matches) closeSidebar(); });
$("dialog-close").onclick = () => $("dialog").close();
$("create").onclick = $("create-empty").onclick = createDialog; $("join").onclick = $("join-empty").onclick = joinDialog;
$("invite").onclick = inviteDialog; $("controllers").onclick = controllersDialog; $("address").onclick = addressDialog;
$("find-copies").onclick = findCopiesDialog; $("close-copies").onclick = () => { $("copy-panel").hidden = true; $("copies").replaceChildren(); };
$("room-history").onclick = historyDialog;
$("dialog").addEventListener?.("close", () => { if ($("dialog").open) return; modalRevision++; historyView = null; $("dialog-form").onsubmit = null; for (const input of $("dialog-content").querySelectorAll("input, textarea")) input.value = ""; $("dialog-content").replaceChildren(); });
$("initialize").onclick = async () => { try { await act({ action: "init" }); } catch (error) { toast(error.message); } };
$("start").onclick = async () => { try { await act({ action: "serve", name: selected, accept_files: $("allow-files").checked }); } catch (error) { toast(error.message); } };
$("stop").onclick = async () => { try { await act({ action: "stop", name: selected }); } catch (error) { toast(error.message); } };
$("allow-files").onchange = async () => {
  if (!room()?.status) return;
  fileConsentPending = true; $("allow-files").disabled = true;
  try { await act({ action: "file-consent", name: selected, accepted: $("allow-files").checked }); }
  catch (error) { toast(error.message); }
  finally { fileConsentPending = false; await refresh(); render(); }
};
$("composer").onsubmit = async (event) => { event.preventDefault(); const text = $("text").value; if (!text || !roomActionAllowed(room(), "publish")) return; $("send").disabled = true;
  try { const result = await act({ action: "send", name: selected, text, sensitive: $("sensitive").checked }); $("text").value = ""; $("send-status").textContent = deliveryComplete(result.delivery) ? "Acknowledged by the recipient event queues, not human reading." : "Partial or unconfirmed delivery. Inspect the peer state before retrying."; } catch (error) { toast(error.message); } finally { $("send").disabled = !roomActionAllowed(room(), "publish"); }
};
$("text").onkeydown = (event) => { if ((event.ctrlKey || event.metaKey) && event.key === "Enter") $("composer").requestSubmit(); };
$("share-media").onclick = () => mediaControl?.shareDialog(); $("receive-media").onclick = () => mediaControl?.receiveDialog(); $("stop-media").onclick = () => mediaControl?.stopAll();
addEventListener("pagehide", () => { mediaControl?.stopAll(); notificationControl?.retire(); });
addEventListener("focus", () => notificationControl?.close());
$("attach").onclick = () => $("file-input").click();
$("file-input").onchange = (event) => { const file = event.target.files[0]; if (!file) return; if (file.size > (current?.files.max_bytes || 0)) { toast("This file exceeds the configured upload limit."); return; }
  fileDialog("Send " + file.name, async (name, to, sensitive) => { const response = await fetch("/api/rooms/" + encodeURIComponent(name) + "/files?to=" + encodeURIComponent(to) + "&sensitive=" + sensitive, { method: "POST", headers: { Authorization: "Bearer " + token, "Content-Type": "application/octet-stream" }, body: file }); return response.json(); }); event.target.value = "";
};
notificationControl = messageNotifications(globalThis, {
  hidden: () => document.hidden, focused: () => document.hasFocus(), selected: () => selected,
  open: name => { window.focus(); if (current?.rooms.some(item => item.name === name)) { selected = name; sessionStorage.setItem("kk-room", selected); render(); } },
  unavailable: () => toast("System notifications are unavailable. Review browser permission in Settings; messages remain in the workspace."),
});
(async () => {
  try { const bootstrap = launchCode;
    if (bootstrap) { const response = await fetch("/api/bootstrap", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ token: bootstrap }) }); if (!response.ok) throw new Error("This private launch link expired or was already used."); token = (await response.json()).session; sessionStorage.setItem("kk-session", token); }
    if (!token) throw new Error("Start kk ui on this device and use its private browser launch. No credential is printed in the terminal.");
    current = await api("/api/state"); notificationControl.observe(current); $("gate").hidden = true; $("workspace").hidden = false;
    const media = await import("/media.js"); mediaControl = media.setupMedia({ token: () => token, room, memberName, memberChoice, config: () => current.media, toast, modal, el });
    render(); setInterval(refresh, 1000);
  } catch (error) { $("gate-text").textContent = error.message; }
})();
