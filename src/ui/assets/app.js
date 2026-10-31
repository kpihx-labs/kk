"use strict";
const launchCode = location.hash.slice(1); history.replaceState(null, "", location.pathname);
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => { const node = document.createElement(tag); if (cls) node.className = cls; if (text !== undefined) node.textContent = text; return node; };
let token = sessionStorage.getItem("kk-session") || "";
let current = null, selected = sessionStorage.getItem("kk-room") || "", refreshing = false, mediaControl = null, notificationControl = null;
let shownRoom = "", rendered = new Map(), revealed = new Map(), toastTimer;
let fileConsentPending = false;
let conversationPending = false;
let localAvailable = true;
let mediaSessionFence = null;
const composerDrafts = new Map(), pendingSends = new Map(), replyDrafts = new Map();
let composerScope = null, draftIdentity = null, draftGeneration = 0, draftError = false;
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
function clientRequestWaitMs() {
  const value = current?.client?.request_timeout_ms;
  return Number.isSafeInteger(value) && value > 0 && value <= 0x7fffffff ? value : null;
}
function clientWaitError() {
  const error = new Error("Client wait ended. The operation may still complete; this is not SDK cancellation. Inspect the room before retrying; duplicates are possible.");
  error.code = "CLIENT_WAIT_ENDED"; return error;
}
function clientWait(start, milliseconds, parentSignal) {
  const controller = new AbortController(); let finished = false, rejectWait;
  const deadline = Date.now() + milliseconds, expired = new Promise((_resolve, reject) => { rejectWait = reject; });
  const expire = () => { if (!finished) { controller.abort(); rejectWait(clientWaitError()); } };
  const timer = setTimeout(expire, milliseconds);
  parentSignal?.addEventListener("abort", expire, {once:true});
  let work;
  try { if (parentSignal?.aborted) { expire(); work = expired; } else work = Promise.resolve(start(controller.signal)); }
  catch(error) { work = Promise.reject(error); }
  const promise = Promise.race([work, expired]).finally(() => { finished = true; clearTimeout(timer); parentSignal?.removeEventListener("abort", expire); });
  return {promise, expire, deadline};
}
function expirePendingSends() {
  for (const attempt of pendingSends.values()) if (Date.now() >= attempt.deadline) attempt.expire?.();
}
async function api(path, body, options = {}) {
  if (body !== undefined && !localAvailable) throw new Error("Local service unavailable. Wait for a successful refresh before trying again; the previous outcome may be uncertain.");
  const perform = async signal => {
    if (signal?.aborted) throw clientWaitError();
    const headers = { Authorization: "Bearer " + token };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    const response = await fetch(path, { method: body === undefined ? "GET" : "POST", headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store", signal });
    if (signal?.aborted) throw clientWaitError();
    if (response.status === 401 || response.status === 403) {
      token = ""; sessionStorage.removeItem("kk-session"); mediaControl?.stopAll(); notificationControl?.retire(); historyView = null; joinDrafts.clear(); retireDrafts(); joinDraft = {name:"", bind:"", role:"participant"}; $("dialog").close(); $("dialog-content").replaceChildren();
      const message = "This private session expired or the local service restarted. Relaunch kk ui and use its new private browser launch.";
      $("gate-text").textContent = message; $("gate").hidden = false; $("workspace").hidden = true;
      throw new Error(message);
    }
    let value; try { value = await response.json(); } catch { if (signal?.aborted) throw clientWaitError(); throw new Error("The local service returned an invalid response."); }
    if (signal?.aborted) throw clientWaitError();
    if (!response.ok || value.ok === false) { const error = new Error(value.error || value.code || "Operation refused."); error.code = typeof value.code === "string" ? value.code : undefined; throw error; }
    return value;
  };
  const milliseconds = clientRequestWaitMs();
  return milliseconds === null ? perform(options.signal) : clientWait(perform, milliseconds, options.signal).promise;
}
async function act(action, options = {}) { if (["stop", "serve", "join"].includes(action.action) && action.name === selected) mediaControl?.stopAll(); const value = await api("/api/action", action, options); if (options.refresh !== false) await refresh(); return value.result; }
function room() { return current?.rooms.find((entry) => entry.name === selected); }
function roomActionAllowed(target, action) { return (typeof localAvailable === "undefined" || localAvailable) && target?.status?.active === true && target.status.capabilities?.[action] === true; }
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
function draftPolicy() {
  const policy = current?.drafts;
  return policy && Number.isSafeInteger(policy.max_rooms) && policy.max_rooms > 0 && Number.isSafeInteger(policy.max_text_bytes) && policy.max_text_bytes > 0 ? policy : null;
}
function draftId(value) {
  if (typeof value === "string" && /^[a-f0-9]{64}$/i.test(value)) return value.toLowerCase();
  if (Array.isArray(value) && value.length === 32 && value.every(byte=>Number.isInteger(byte)&&byte>=0&&byte<=255)) return value.map(byte=>byte.toString(16).padStart(2,"0")).join("");
  return null;
}
function draftRoomKey(name) {
  const identity = draftId(current?.identity), id = draftId(current?.rooms?.find(entry=>entry.name===name)?.id);
  return identity && id ? JSON.stringify([draftGeneration, identity, name, id]) : null;
}
function clearComposer() { $("text").value = ""; $("sensitive").checked = false; $("send-status").textContent = "Always encrypted in transit."; draftError = false; }
function retireDrafts() { draftGeneration++; draftIdentity = null; composerScope = null; composerDrafts.clear(); for (const attempt of pendingSends.values()) attempt.expire?.(); pendingSends.clear(); replyDrafts.clear(); clearComposer(); }
function closeRoomViews() { mediaControl?.stopAll(); historyView = null; $("dialog").close(); $("dialog-content").replaceChildren(); $("copy-panel").hidden = true; $("copies").replaceChildren(); }
function reconcileDrafts() {
  const identity = draftId(current?.identity);
  if (identity !== draftIdentity) { const previous = composerScope; retireDrafts(); draftIdentity = identity; if (previous) closeRoomViews(); }
  const allowed = new Set((current?.rooms || []).map(entry=>draftRoomKey(entry.name)).filter(Boolean));
  for (const key of composerDrafts.keys()) if (!allowed.has(key)) composerDrafts.delete(key);
  for (const key of pendingSends.keys()) if (!allowed.has(key)) { pendingSends.get(key)?.expire?.(); pendingSends.delete(key); }
  for (const [key, draft] of replyDrafts) if (!allowed.has(draft.scope)) replyDrafts.delete(key);
  const key = draftRoomKey(selected);
  if (composerScope !== key) {
    if (composerScope && !allowed.has(composerScope)) closeRoomViews();
    composerScope = key; clearComposer(); const draft = composerDrafts.get(key);
    if (draft) { $("text").value = draft.text; $("sensitive").checked = draft.sensitive; $("send-status").textContent = draft.status; }
  }
}
function saveComposerDraft() {
  if (!composerScope) return true;
  if (composerScope !== draftRoomKey(selected)) { reconcileDrafts(); return true; }
  const policy = draftPolicy(), text = $("text").value, sensitive = $("sensitive").checked;
  if (!policy || new TextEncoder().encode(text).byteLength > policy.max_text_bytes || (!composerDrafts.has(composerScope) && (text || sensitive) && composerDrafts.size >= policy.max_rooms)) {
    draftError = true; $("send-status").textContent = "Draft limit: keep editing here; shorten this draft or clear another saved room draft before switching. Nothing was sent."; toast($("send-status").textContent); return false;
  }
  if (draftError) { $("send-status").textContent = "Always encrypted in transit."; draftError = false; }
  if (!text && !sensitive) { composerDrafts.delete(composerScope); return true; }
  const draft = composerDrafts.get(composerScope) || {};
  Object.assign(draft, {scope:composerScope, text, sensitive, status:$("send-status").textContent.slice(0, policy.max_text_bytes)}); composerDrafts.set(composerScope, draft); return true;
}
function select(name) { if (selected !== name) { if (!saveComposerDraft()) return; closeRoomViews(); } selected = name; sessionStorage.setItem("kk-room", name); const wasOpen = $("sidebar").classList.contains("open"); closeSidebar(); if (wasOpen) $("menu").focus(); render(); }
function field(label, name, type = "text", value = "", note) {
  const wrapper = el("label", "field", label); const input = el(type === "textarea" ? "textarea" : "input"); input.name = name;
  if (type !== "textarea") input.type = type; if (value) input.value = value;
  wrapper.append(input); if (note) wrapper.append(el("small", "muted", note)); return wrapper;
}
function roleField(value = "participant", roles = ["participant", "reader", "inviter", "moderator"]) { const wrapper = el("label", "field", "Accepted data role"); const input = el("select"); input.name = "role"; for (const role of roles) { const option = el("option", "", role); option.value = role; input.append(option); } input.value = roles.includes(value) ? value : roles[0] || ""; wrapper.append(input); return wrapper; }
function modal(title, controls, submit, handler) {
  const revision = ++modalRevision, active = () => revision === modalRevision && $("dialog").open;
  $("dialog-title").textContent = title; $("dialog-content").replaceChildren(...controls); $("dialog-error").textContent = ""; $("dialog-submit").textContent = submit;
  $("dialog-submit").disabled = false; $("dialog-submit").ariaBusy = "false";
  let pending = false;
  $("dialog-form").onsubmit = async (event) => { event.preventDefault(); if (pending) return; pending = true; const button = $("dialog-submit"), label = button.textContent; button.disabled = true; button.ariaBusy = "true"; button.textContent = "Working...";
    try { const result = await handler(new FormData(event.currentTarget)); if (active() && result !== false) $("dialog").close(); } catch (error) { if (active()) $("dialog-error").textContent = error.message; } finally { pending = false; if (revision === modalRevision) { button.disabled = false; button.ariaBusy = "false"; if (button.textContent === "Working...") button.textContent = label; } }
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
  let generation = 0; const area = invitation.querySelector("textarea"), invitationMaxBytes = 4096;
  area.addEventListener("input", () => { generation++; });
  upload.querySelector("input").onchange = async (event) => { const revision = ++generation, file = event.target.files[0]; area.value = ""; if (!file) return; if (file.size > invitationMaxBytes) { toast("Invitation files are limited to 4 KiB."); return; } try { const text = await file.text(); if (revision !== generation) return; JSON.parse(text); area.value = text; } catch { if (revision === generation) { area.value = ""; toast("Could not read a valid invitation JSON file. Choose another or paste it."); } } };
  const paste = el("button", "outline compact", "Paste invitation from clipboard"); paste.type = "button";
  paste.disabled = !globalThis.navigator?.clipboard?.readText;
  const dialogRevision = modalRevision + 1, identity = current.identity;
  paste.onclick = async () => {
    if (paste.disabled) return;
    if (area.value) { toast("Clear the invitation field intentionally before replacing it."); return; }
    const revision = ++generation; paste.disabled = true;
    try { const text = await navigator.clipboard.readText();
      if (revision !== generation || modalRevision !== dialogRevision || !$("dialog").open || current?.identity !== identity) return;
      if (new Blob([text]).size > invitationMaxBytes) throw new Error("Invitation is too large.");
      JSON.parse(text); area.value = text;
    } catch { if (revision === generation && modalRevision === dialogRevision) toast("Clipboard invitation unavailable or invalid. Use your system Paste shortcut or choose one invitation file; no joining request was sent."); }
    finally { paste.disabled = !globalThis.navigator?.clipboard?.readText; }
  };
  modal("Join a room", [name, bind, roleField(joinDraft.role), upload, invitation, paste], "Start secure pairing", async (data) => { const name = data.get("name").trim(); joinDraft = {name, bind:data.get("bind").trim(), role:data.get("role")}; const result = await act({ action: "join", ...joinDraft, invitation: data.get("invitation"), accept_files: false }); area.value = ""; upload.querySelector("input").value = ""; joinDrafts.set(result.operation, {...joinDraft}); select(name); });
}
function retryJoin(operation) { if (!joinDrafts.has(operation.id)) return; joinDraft = {...joinDrafts.get(operation.id)}; dismissedOperations.add(operation.id); joinDrafts.delete(operation.id); joinDialog(); }
function inviteDialog() {
  const target = room(), roles = admissionRoles(target); if (!roles.length) { toast("Inviting is unavailable with this device's cached data rights."); return; }
  const bind = field("Pairing listen IP:port", "bind", "text", "", "Use a different port from this room's data port."); bind.querySelector("input").required = true;
  try { const url = new URL("http://" + target.status.address); url.port = String(Number(url.port) < 65535 ? Number(url.port) + 1 : Number(url.port) - 1); bind.querySelector("input").value = url.host; } catch {}
  modal("Invite a member", [bind, roleField("participant", roles), el("p", "dialog-note", "The other device must choose the same accepted role. Available roles reflect cached data rights, not fresh authorization. Compare CONFIRM on both devices before approving.")], "Create single-use invitation", async (data) => { const latest = room(); if (latest?.name !== target.name || !admissionRoles(latest).includes(data.get("role"))) throw new Error("Room or admission rights changed. Reopen this invitation."); await act({ action: "invite", name: target.name, bind: data.get("bind").trim(), role: data.get("role") }); });
}
function controllersDialog() {
  const target = room(); if (!roomActionAllowed(target, "control") || !foregroundInstance(target)) { toast("Controller changes require a current foreground owner and administrative data rights."); return; }
  const session = recoverySession(target), selectionState = status => JSON.stringify([status?.epoch, [...(status?.members || [])].sort(), [...(status?.voters || [])].sort()]);
  const original = selectionState(target.status);
  const list = el("div"); for (const peer of target.status.members) { const row = el("label", "field"); const input = el("input"); input.type = "checkbox"; input.name = "peers"; input.value = peer; input.checked = target.status.voters.includes(peer); row.title = peer; row.append(input, el("span", "", memberChoice(target, peer))); list.append(row); }
  modal("Choose voting controllers", [el("p", "dialog-note", "Only current data members are eligible; approving the set requires administrative data rights. A vote does not grant publishing or moderation rights. Retire a member's vote before removing its data access. A majority must remain available."), list], "Approve exact controller set", async (data) => {
    const latest = room();
    if (latest?.name !== target.name || !roomActionAllowed(latest, "control") || recoverySession(latest) !== session || selectionState(latest.status) !== original) throw new Error("Room, foreground owner, membership, votes or administrative rights changed. Reopen controller selection before approving.");
    await act({ action: "controllers", name: target.name, peers: data.getAll("peers") });
    if (room()?.name !== target.name || recoverySession(room()) !== session) throw new Error("Foreground changed while changing votes. Inspect current controllers; the previous outcome must not be retried blindly.");
    toast("Controller set committed. Data access is unchanged.");
  });
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
function foregroundInstance(target) {
  const id = target?.status?.foreground_instance;
  return Array.isArray(id) && id.length === 16 && id.every(byte=>Number.isInteger(byte)&&byte>=0&&byte<=255) ? id : null;
}
function recoverySession(target) {
  return JSON.stringify([current.identity, target?.name, target?.id, target?.status?.active, current.phases?.[target?.name]?.revision ?? null, foregroundInstance(target)]);
}
function recoveryDialog() {
  const target = room(); if (!foregroundInstance(target) || !localAvailable) { toast("Start this room with a current foreground owner before inspecting retained recovery metadata."); return; }
  const name = target.name, session = recoverySession(target);
  const sameSession = () => localAvailable && selected === name && room()?.name === name && room()?.status && recoverySession(room()) === session;
  const notice = el("p", "dialog-note", "Inspection is explicit, not an automatic poll. Only a public operation ID, change kind and prior epoch are requested. No prepared proposal, Welcome, key or invitation is displayed.");
  modal("Inspect prepared change / " + (target.title || name), [notice], "Inspect retained metadata", async () => {
    if (!sameSession()) throw new Error("Room or foreground session changed. Reopen recovery from the intended room.");
    const revision = modalRevision, response = await api("/api/action", {action:"pending-mutation", name});
    if (revision !== modalRevision || !$("dialog").open) return false;
    if (!sameSession()) throw new Error("Room or foreground session changed during inspection. Reopen recovery.");
    const pending = response.result;
    if (pending === null) { notice.textContent = "This foreground owner reports no prepared change now. This is not an invitation or recipient-delivery receipt. Inspect again only explicitly."; return false; }
    if (!pending || !Array.isArray(pending.id) || pending.id.length !== 32 || !pending.id.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255) || !["admission", "removal"].includes(pending.kind) || typeof pending.member !== "string" || !/^[a-f0-9]{64}$/.test(pending.member) || !Number.isSafeInteger(pending.epoch_before) || pending.epoch_before < 0) throw new Error("Invalid public recovery metadata. No retry was issued.");
    const expected = [...pending.id], confirmation = field("I confirm retrying this exact retained signed change", "confirm-recovery", "checkbox"); confirmation.querySelector("input").required = true;
    const member = el("details", "technical-details"); member.append(el("summary", "", "Affected member full ID"), el("code", "fingerprint", pending.member));
    const meaning = "Control acceptance is not invitation resumption or recipient delivery. Closing or timing out while retrying is not cancellation; inspect current metadata before another explicit attempt.";
    modal("Confirm exact control retry / " + (target.title || name), [el("p", "dialog-note", pending.kind + " / prior epoch " + pending.epoch_before + ". The owner retries its exact stored signed proposal, never a new one."), el("p", "dialog-note", "Affected member / " + memberChoice(target, pending.member) + " / " + memberProvenance(target, pending.member)), member, el("code", "fingerprint", expected.map(byte=>byte.toString(16).padStart(2,"0")).join("")), el("p", "dialog-note", "Public operation identifier / not an invitation or capability. " + meaning), confirmation], "Retry exact prepared change", async data => {
      if (data.get("confirm-recovery") !== "on") throw new Error("Explicit confirmation is required. No retry was issued.");
      if (!sameSession()) throw new Error("Room or foreground session changed. Inspect metadata again before confirming.");
      const revision = modalRevision; let result;
      try { result = (await api("/api/action", {action:"retry-prepared", name, expected, confirm:true})).result; }
      catch(error) { throw new Error("Retry outcome uncertain; a timeout or closed view does not cancel acceptance. Inspect retained metadata before retrying. " + error.message); }
      if (revision !== modalRevision || !$("dialog").open) return false;
      if (!sameSession()) throw new Error("Foreground session changed while retrying; inspect the intended owner before another attempt.");
      if (!result || result.meaning !== "control-acceptance-not-invitation-resumption-or-delivery" || typeof result.accepted !== "boolean" || typeof result.replayed !== "boolean" || !Number.isSafeInteger(result.epoch) || result.epoch < 0) throw new Error("Public recovery result unavailable or invalid; outcome uncertain. Inspect the owner before retrying.");
      modal("Control retry result / " + (target.title || name), [el("p", "dialog-note", (result.accepted ? "Owner reports control acceptance" : "Owner did not report control acceptance") + " / epoch " + result.epoch + " / replayed " + result.replayed + "."), el("p", "dialog-note", meaning)], "Close", async () => {});
      return false;
    });
    return false;
  });
}
function fileDialog(label, send, storedCopy = false, attachment = null) {
  if (!localAvailable) { toast("Local service unavailable. Reconnect before sending a file."); return; }
  const target = room(); if (!roomActionAllowed(target, "publish") || !foregroundInstance(target)) { toast("Start a current room owner and verify publishing rights before sending files."); return; }
  const session = recoverySession(target);
  const wrapper = el("label", "field", "Explicit recipient"); const peers = el("select"); peers.name = "to";
  for (const peer of target.status.members.filter((peer) => peer !== target.status.owner)) { const option = el("option", "", memberChoice(target, peer)); option.value = peer; option.title = peer; peers.append(option); }
  if (!peers.options.length) { toast("Invite a recipient first."); return; } wrapper.append(peers);
  const privacy = el("label", "check"); const sensitive = el("input"); sensitive.type = "checkbox"; sensitive.name = "sensitive"; sensitive.checked = storedCopy || $("sensitive").checked; privacy.append(sensitive, el("span", "", "Sensitive local receipt"));
  if (storedCopy) privacy.append(el("small", "muted", "Original sensitivity was not recorded. This new transfer defaults to sensitive; change only intentionally."));
  const selection = [], access = el("p", "dialog-note"); access.role = "status";
  let picker, revision, starting = false, submitted = false, cancelledLocally = false, retired = false, observer;
  const permitted = recipient => { const latest = room(); return localAvailable && latest?.name === target.name && recoverySession(latest) === session && roomActionAllowed(latest, "publish") && latest.status?.members.includes(recipient); };
  const live = () => !retired && revision === modalRevision && $("dialog").open;
  const updateAdmission = () => {
    if (!live()) return;
    const allowed = permitted(peers.value);
    if (!allowed && starting && !submitted) cancelledLocally = true;
    const disabled = starting || !allowed;
    if ($("dialog-submit").disabled !== disabled) $("dialog-submit").disabled = disabled;
    if (picker) picker.disabled = disabled;
    peers.disabled = starting; sensitive.disabled = starting;
    access.hidden = allowed && !submitted && !cancelledLocally;
    access.textContent = !allowed ? submitted ? "Access changed after admission was submitted. It may still complete; no cancellation or delivery is confirmed. Inspect File transfers before retrying." : "Publication unavailable. The local draft and attachment selection are kept. Admission is blocked; nothing retries automatically." : submitted ? "An admission was submitted. Inspect File transfers before another attempt; closing or replacing a selection does not cancel it." : "Local admission stopped before dispatch. Review and submit again explicitly; no automatic retry.";
  };
  if (attachment) {
    const summary = el("p", "dialog-note", "Selected attachment / " + attachment.file.name + " / " + attachment.file.size + " bytes");
    const replace = el("label", "field", "Replace attachment before starting (optional)"); picker = el("input"); picker.type = "file"; replace.append(picker); selection.push(summary, replace, el("p", "dialog-note", "Before starting, Close discards this local selection. Replacing or closing never cancels an earlier submitted transfer; inspect File transfers if its outcome is uncertain."));
    picker.onchange = () => {
      const next = picker.files?.[0]; picker.value = "";
      if (!next || starting || revision !== modalRevision || !$("dialog").open) return;
      if (!localAvailable || room()?.name !== target.name || recoverySession(room()) !== session) { toast("Room or foreground changed. Reopen the attachment selection."); return; }
      if (!Number.isSafeInteger(next.size) || next.size < 0 || next.size > (current?.files?.max_bytes || 0)) { toast("Replacement exceeds the upload limit; the current attachment was kept."); return; }
      attachment.file = next; summary.textContent = "Selected attachment / " + next.name + " / " + next.size + " bytes"; $("dialog-title").textContent = "Send " + next.name;
    };
  }
  modal(label, [wrapper, privacy, ...selection, access, el("p", "dialog-note", "The recipient must explicitly consent to file storage. Starting a transfer is not delivery. Progress and explicit cancellation are available in File transfers; closing a dialog does not cancel it. Only a verified receipt confirms durable recipient storage, not a human read or future availability.")], "Send to this recipient", async (data) => {
    const latest = room();
    if (!localAvailable || latest?.name !== target.name || recoverySession(latest) !== session || !roomActionAllowed(latest, "publish") || !latest.status?.members.includes(data.get("to"))) throw new Error("Room, foreground owner, recipient, publishing rights or local connection changed. Reopen the transfer from its original room.");
    const file = attachment?.file;
    if (attachment && (!file || file.size > (current?.files?.max_bytes || 0))) throw new Error("Select an attachment within the upload limit before starting.");
    starting = true; cancelledLocally = false; updateAdmission();
    try {
      await Promise.resolve();
      if (cancelledLocally || !live() || !permitted(data.get("to"))) throw new Error("Local admission stopped before dispatch. No transfer request was sent. The draft/selection stays local while this view is open; submit again only explicitly.");
      submitted = true; updateAdmission();
      const result = await send(target.name, data.get("to"), data.get("sensitive") === "on", file);
      if (result?.ok !== true) throw new Error(result?.code === "DENIED" ? "Transfer not allowed. Check your room permissions and ask the recipient to enable incoming files." : result?.result?.error || result?.error || result?.code || "The recipient refused the file.");
      if (room()?.name !== target.name || recoverySession(room()) !== session) throw new Error("Foreground changed during transfer admission. Inspect the original owner's File transfers before retrying; delivery is not inferred.");
      if (!result.result?.id || !result.result?.progress) throw new Error("Transfer outcome unavailable. Inspect File transfers before retrying.");
      if (!live()) return;
      transferDialog(target.name, session, result.result);
    } finally { starting = false; updateAdmission(); }
  });
  revision = modalRevision;
  const dialog = $("dialog"), retire = () => { if (!live()) { retired = true; observer?.disconnect(); if (attachment) attachment.file = null; dialog.removeEventListener("close", retire); } };
  dialog.addEventListener("close", retire); peers.onchange = updateAdmission;
  if (typeof MutationObserver === "function") { observer = new MutationObserver(() => { retire(); updateAdmission(); }); observer.observe($("send"), {attributes:true,attributeFilter:["disabled"]}); observer.observe($("room-state"), {childList:true}); observer.observe($("dialog-submit"), {attributes:true,attributeFilter:["disabled"]}); }
  updateAdmission();
}
function transferTerminal(job) { return ["completed", "cancelled", "uncertain", "failed"].includes(job?.progress?.phase); }
function transferText(job) {
  const progress = job?.progress;
  if (!progress || typeof job.id !== "string" || !["preparing", "negotiating", "transferring", "verifying", "publishing", "awaiting-receipt", "completed", "cancelled", "uncertain", "failed"].includes(progress.phase)) throw new Error("Invalid transfer status; no delivery is inferred.");
  const policyHint = progress.error === "DENIED" ? " Check room permissions and the recipient's incoming-file consent." : "";
  if (progress.phase === "completed") {
    const receipt = job.receipt;
    return receipt && Number.isSafeInteger(receipt.bytes) && receipt.bytes >= 0 && Number.isSafeInteger(receipt.resumed_from) && receipt.resumed_from >= 0 && receipt.resumed_from <= receipt.bytes
      ? "Verified durable recipient storage / " + receipt.bytes + " bytes" + (receipt.resumed_from ? " / verified resume prefix " + receipt.resumed_from + " bytes" : "") + ". Not a human read or future availability promise."
      : "Completion receipt unavailable; delivery remains unconfirmed.";
  }
  if (progress.phase === "uncertain") return "Remote outcome uncertain. Local work ended after handoff; inspect recipient storage before retrying." + (progress.error ? " Reported reason: " + progress.error + "." : "") + policyHint;
  if (progress.phase === "cancelled") return "Local transfer cancelled. Previously handed-off bytes or remote publication are not undone.";
  if (progress.phase === "failed") return "Transfer failed / " + (progress.error || "outcome unavailable") + policyHint;
  return progress.phase + " / " + progress.source_bytes + " source bytes submitted locally / " + progress.resumed_from + " bytes of verified resume prefix" + (progress.total_bytes === null ? "" : " / " + progress.total_bytes + " total bytes") + ". Not a storage receipt." + (job.cancel_requested ? " Cancellation requested, not yet acknowledged." : "");
}
function transferDialog(name, session, initial) {
  if (!localAvailable || room()?.name !== name || recoverySession(room()) !== session) { toast("Original foreground owner changed or is unavailable. Reopen transfers from its room; old IDs are not reused."); return; }
  let job = initial, busy = false, timer;
  const status = el("p", "dialog-note", transferText(job)), cancel = el("button", "outline compact", "Request cancellation"), update = el("button", "outline compact", "Refresh status");
  cancel.type = update.type = "button";
  modal("File transfer", [status, cancel, update, el("p", "dialog-note", "Closing stops this view, not the transfer. Reopen File transfers in this same foreground room to inspect it.")], "Close", async () => {});
  const revision = modalRevision, live = () => revision === modalRevision && $("dialog").open && room()?.name === name && recoverySession(room()) === session;
  const draw = () => { status.textContent = transferText(job); cancel.disabled = transferTerminal(job) || job.cancel_requested || busy; };
  const request = async (cancelling = false) => {
    if (!live() || busy) return; clearTimeout(timer); busy = true; draw();
    try {
      const path = "/api/rooms/" + encodeURIComponent(name) + "/transfers/" + encodeURIComponent(job.id);
      const response = await api(path + (cancelling ? "/cancel" : ""), cancelling ? {} : undefined);
      if (!live()) return;
      job = cancelling ? response.result.job : response.result; draw();
      if (cancelling && response.result.acknowledged !== true) status.textContent += " Cancellation requested, not yet acknowledged.";
      if (!transferTerminal(job)) timer = setTimeout(() => request(), 1000);
    } catch (error) { if (live()) status.textContent = "Status unavailable; the transfer may still complete. " + error.message + " Use Refresh status, not a duplicate send."; }
    finally { busy = false; if (live()) cancel.disabled = transferTerminal(job) || job.cancel_requested; }
  };
  cancel.onclick = () => request(true); update.onclick = () => request(); draw();
  if (!transferTerminal(job)) timer = setTimeout(() => request(), 1000);
  $("dialog").addEventListener("close", () => { if (!live()) clearTimeout(timer); }, {once:true});
}
async function transfersDialog() {
  const target = room(); if (!localAvailable || !target?.status || !foregroundInstance(target)) { toast("Start the original foreground room before inspecting transfers."); return; }
  const name = target.name, session = recoverySession(target);
  try {
    const response = await api("/api/rooms/" + encodeURIComponent(name) + "/transfers");
    if (room()?.name !== name || recoverySession(room()) !== session) return;
    if (!Array.isArray(response.result)) throw new Error("Transfer list unavailable.");
    const list = el("div");
    for (const job of response.result) { const button = el("button", "outline compact", transferText(job)); button.type = "button"; button.onclick = () => transferDialog(name, session, job); list.append(button); }
    if (!response.result.length) list.append(el("p", "dialog-note", "No retained transfers for this foreground owner. Restarting retires its transfer IDs; it does not prove remote nonpublication."));
    modal("File transfers", [list], "Close", async () => {});
  } catch(error) { toast(error.message); }
}
async function readOwnedInbox(response, expected, limit, signal, progress) {
  if (!Number.isSafeInteger(expected) || expected < 0 || !Number.isSafeInteger(limit) || limit < 0 || expected > limit) throw new Error("File exceeds the configured local read bound.");
  if (!response.body?.getReader) throw new Error("Bounded streaming is unavailable in this browser.");
  const reader = response.body.getReader(), chunks = []; let received = 0;
  try {
    const length = response.headers.get("Content-Length");
    if (!length || !/^\d+$/.test(length) || Number(length) !== expected) throw new Error("File length changed or is unavailable. Refresh the inbox before retrying.");
    for (;;) {
      if (signal.aborted) throw new Error("Local file read stopped.");
      const {done, value} = await reader.read(); if (done) break;
      if (!(value instanceof Uint8Array) || value.byteLength > expected - received || value.byteLength > limit - received) throw new Error("File exceeded its verified length; no browser copy was prepared.");
      chunks.push(value); received += value.byteLength; progress(received);
    }
    if (received !== expected) throw new Error("Incomplete local file read; no browser copy was prepared.");
    return new Blob(chunks, {type:"application/octet-stream"});
  } catch (error) { await reader.cancel().catch(failure => console.warn("Local file reader could not stop:", failure.name)); throw error; }
  finally { reader.releaseLock(); }
}
async function saveInboxFile(name, file, preview = false) {
  const textPreview = preview === "text", imagePreview = preview === true;
  const limit = textPreview ? Math.min(current?.files?.preview_max_bytes, current?.drafts?.max_text_bytes) : imagePreview ? current?.files?.preview_max_bytes : current?.files?.max_bytes;
  if (selected !== name || !localAvailable || !Number.isSafeInteger(file.bytes) || file.bytes < 0 || !Number.isSafeInteger(limit) || file.bytes > limit) { toast(preview ? "Preview is unavailable at this size or policy. Save copy remains available." : "Reconnect and refresh this room's inbox before saving a bounded file."); return; }
  const controller = new AbortController(), status = el("p", "muted", "Verifying the stored inbox object before reading..."), progress = el("progress"), output = el("div");
  status.setAttribute("role", "status");
  progress.max = Math.max(file.bytes, 1); progress.value = 0; progress.setAttribute("aria-label", "Bytes read from the local inbox"); let finished = false, retired = false, url;
  modal(textPreview ? "Explicit UTF-8 text preview" : imagePreview ? "Explicit image preview" : "Save received file", [status, progress, output, el("p", "dialog-note", "This reads a stored object from your local inbox, not a new peer transfer. The browser controls the save destination. Original filename, type and sensitivity provenance are unavailable." + (textPreview ? " You explicitly requested a bounded UTF-8 interpretation, not a document renderer. HTML and links are inert text. Unsupported or oversized files remain downloadable." : imagePreview ? " You explicitly requested a bounded PNG/JPEG reveal. Unsupported or oversized images remain downloadable." : ""))], "Stop local download", async () => { if (!finished) { controller.abort(); return false; } });
  const revision = modalRevision, dialog = $("dialog"), live = () => !retired && revision === modalRevision && dialog.open && selected === name && token;
  const retire = () => { retired = true; controller.abort(); if (url) { URL.revokeObjectURL(url); url = null; } output.replaceChildren(); dialog.removeEventListener("close", closed); globalThis.removeEventListener?.("pagehide", retire); if (revision === modalRevision && dialog.open) dialog.close?.(); };
  const closed = () => { if (revision !== modalRevision || !dialog.open) retire(); };
  dialog.addEventListener("close", closed);
  globalThis.addEventListener?.("pagehide", retire, {once:true});
  if (preview) { const fallback = el("button", "outline compact", "Save without preview"); fallback.type = "button"; fallback.onclick = () => saveInboxFile(name, file); output.append(fallback); }
  try {
    const response = await fetch("/api/rooms/" + encodeURIComponent(name) + "/files/" + encodeURIComponent(file.id) + (imagePreview ? "?preview=true" : ""), {headers:{Authorization:"Bearer " + token}, signal:controller.signal});
    if (!response.ok) { if ([401,403].includes(response.status)) await refresh(); throw new Error(preview ? "Preview unsupported or outside its bounds. Save copy remains available; stored bytes are unchanged." : "Verified inbox read refused. Stored bytes are unchanged."); }
    const mime = response.headers.get("Content-Type");
    if (imagePreview && !["image/png", "image/jpeg"].includes(mime)) throw new Error("Preview type is not an allowed raster. Save copy remains available.");
    const blob = await readOwnedInbox(response, file.bytes, limit, controller.signal, bytes => { if (live()) { progress.value = bytes; status.textContent = bytes + " / " + file.bytes + " bytes read from the local inbox."; } });
    if (!live()) return;
    if (textPreview) {
      let text; try { text = new TextDecoder("utf-8", {fatal:true}).decode(await blob.arrayBuffer()); }
      catch { throw new Error("This file is not valid UTF-8 text. Save copy remains available; stored bytes are unchanged."); }
      if (!live()) return;
      if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(text)) throw new Error("Binary control characters are outside text preview. Save copy remains available.");
      output.append(el("pre", "message-body", text));
    }
    url = URL.createObjectURL(imagePreview ? blob.slice(0, blob.size, mime) : blob); const save = el("a", "outline compact", "Save verified copy"); save.href = url; save.download = file.id;
    if (imagePreview) { const image = el("img", "file-preview"); image.alt = "Explicit local PNG/JPEG preview; original sensitivity is unknown"; image.decoding = "async"; image.onerror = () => { if (live()) status.textContent = "Browser image preview could not render. Save copy remains available; stored bytes are unchanged."; }; image.src = url; output.append(image); }
    save.onclick = () => { status.textContent = "Save requested. Your browser controls the destination and disk-write completion."; };
    output.append(save); status.textContent = "Local read complete / " + file.bytes + " bytes. Choose Save verified copy.";
  } catch(error) { const stopped = controller.signal.aborted; controller.abort(); if (live()) status.textContent = stopped ? "Local download stopped. The stored inbox file is unchanged; no peer transfer was cancelled." : error.message; }
  finally { finished = true; if (live()) $("dialog-submit").textContent = "Close"; }
}
async function loadFiles() {
  const name = selected; if (!name || !room()) return;
  try { const files = await api("/api/rooms/" + encodeURIComponent(name) + "/files"); if (name !== selected) return;
    $("file-problem").hidden = true; $("file-problem").textContent = "";
    $("files").replaceChildren(...files.map((file, index) => { const card = el("div", "file-card"); card.append(el("span", "file-icon", "F"), el("span", "", "Received file " + (index + 1) + " / " + Math.ceil(file.bytes / 1024) + " KiB"));
      const download = el("button", "outline compact", "Save copy"); download.onclick = () => saveInboxFile(name, file);
      const preview = el("button", "outline compact", "Reveal image preview"); preview.type = "button"; preview.disabled = !Number.isSafeInteger(current.files?.preview_max_bytes) || file.bytes > current.files.preview_max_bytes; preview.title = preview.disabled ? "Outside preview byte bound; Save copy is still available" : "Explicit PNG/JPEG reveal only; original type and sensitivity are unknown"; preview.onclick = () => saveInboxFile(name, file, true);
      const textLimit = Math.min(current.files?.preview_max_bytes, current.drafts?.max_text_bytes), text = el("button", "outline compact", "Reveal text preview"); text.type = "button"; text.disabled = !Number.isSafeInteger(textLimit) || file.bytes > textLimit; text.title = "Explicit bounded UTF-8 text only; no HTML execution or original-type claim"; text.onclick = () => saveInboxFile(name, file, "text");
      const forward = el("button", "outline compact", "Forward copy"); forward.onclick = () => fileDialog("Forward this stored copy", async (room, to, sensitive) => { const response = await fetch("/api/rooms/" + encodeURIComponent(room) + "/files/" + encodeURIComponent(file.id) + "/forward?to=" + encodeURIComponent(to) + "&sensitive=" + sensitive + "&background=true", { method: "POST", headers: { Authorization: "Bearer " + token } }); return response.json(); }, true);
      const share = el("button", "outline compact", "Offer copy"); share.onclick = () => shareCopyDialog(name, file.id);
      const withdraw = el("button", "outline compact", "Withdraw offer"); withdraw.onclick = async () => { try { const digest = file.id.slice(65, -4); const reply = await copyRequest(name, {operation:"unshare", sha256:digest}); toast(reply.withdrawn ? "Offer withdrawn; stored bytes are unchanged." : "No active offer for this copy."); } catch(error) { toast(error.message); } };
      card.append(download, preview, text, forward, share, withdraw); return card; }));
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
function checkedRelation(message) {
  const relation = message.relation;
  if (!relation) return null;
  const target = messageTarget(relation.target);
  if (!target || !message.reference || JSON.stringify(target.room) !== JSON.stringify(message.reference.room)
      || target.epoch > message.reference.epoch || !["reply", "reaction"].includes(relation.kind)
       || (relation.kind === "reaction" && (!["like", "love", "laugh", "surprised", "sad", "thanks"].includes(relation.value) || typeof relation.added !== "boolean"))) throw new Error("Invalid authenticated conversation relation.");
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
  if (action.action !== "reply") throw new Error("Only authenticated replies are available in this workspace.");
  if (conversationPending) throw new Error("A reply is already pending.");
  if (room()?.name !== action.name || !roomActionAllowed(room(), "publish") || !foregroundInstance(room())) throw new Error("Room changed, foreground identity is unavailable or publishing rights are unavailable.");
  const session = recoverySession(room());
  conversationPending = true;
  try { render(); const result = await act(action); if (recoverySession(room()) !== session) throw new Error("Foreground changed while replying; inspect the outcome before retrying."); if (!result.delivery?.ok) throw new Error(result.delivery?.code || "Operation outcome unavailable; inspect the room before retrying.");
    toast(deliveryComplete(result.delivery) ? "Acknowledged by recipient event queues, not human reading." : "Partial delivery; inspect peer outcomes before retrying."); return result;
  } finally { conversationPending = false; if (current) render(); }
}
function replyDialog(message, target) {
  const reference = messageTarget(message.reference), name = target.name;
  if (!reference || message.room !== name) throw new Error("Authenticated message reference unavailable for this room.");
  const scope = draftRoomKey(name); if (!scope) throw new Error("Profile or room identity unavailable; no reply draft was opened.");
  const key = JSON.stringify([scope, targetKey(reference)]), draft = replyDrafts.get(key) || {scope, text:"", sensitive:message.sensitive === true}, draftOnly = !roomActionAllowed(room(), "publish") || !foregroundInstance(room()), session = recoverySession(target);
  replyDrafts.set(key, draft);
  const body = field("Reply", "reply-text", "textarea"); body.querySelector("textarea").maxLength = 65536;
  const sensitive = field("Sensitive reply / exclude from history and notifications", "reply-sensitive", "checkbox"); sensitive.querySelector("input").checked = draft.sensitive;
  body.querySelector("textarea").value = draft.text;
  body.querySelector("textarea").oninput = () => { draft.text = body.querySelector("textarea").value; };
  sensitive.querySelector("input").onchange = () => { draft.sensitive = sensitive.querySelector("input").checked; };
  modal("Reply / " + (target.title || name), [el("p", "dialog-note", "Reply to " + memberName(target, message.sender) + ". The original text is not copied. Unsent replies stay only in this tab when closed; reopen the same message to continue. A reference is not a read receipt or remote history permission." + (draftOnly ? " Publishing is unavailable now; Keep draft sends nothing. Reopen after reconnecting or restoring rights to send." : "")), body, sensitive], draftOnly ? "Keep draft" : "Send reply", async data => {
    const text = data.get("reply-text"), hidden = data.get("reply-sensitive") === "on"; draft.text = text; draft.sensitive = hidden;
    if (draftOnly) return;
    if (selected !== name || draftRoomKey(name) !== scope || recoverySession(room()) !== session || !roomActionAllowed(room(), "publish")) throw new Error("Room changed, stopped or publishing rights are unavailable. Reopen this reply from its original room.");
    if (!text) throw new Error("Write a reply first.");
    const result = await conversationAction({action:"reply", name, target:reference, text, sensitive:hidden});
    if (deliveryComplete(result.delivery) && replyDrafts.get(key) === draft && draft.text === text && draft.sensitive === hidden) replyDrafts.delete(key);
  });
}
function focusMessageReference(reference, quiet = false) {
  const key = targetKey(reference), roots = historyView?.list?.isConnected ? [historyView.list, $("messages")] : [$("messages")];
  for (const root of roots) {
    const item = [...root.querySelectorAll("[data-reference]")].find(node => node.dataset.reference === key);
    if (item) { item.tabIndex = -1; item.scrollIntoView({block:"center"}); item.focus({preventScroll:true}); return true; }
  }
  if (!quiet) toast("Original message is not loaded in this view. Search local history if it was explicitly captured."); return false;
}
async function openMessageReference(message, reference) {
  if (focusMessageReference(reference, true)) return;
  const row = await historyRequest(message.room, {operation:"resolve", target:reference});
  if (!row) { toast("Original message is unavailable in retained local history."); return; }
  if (targetKey(row.reference) !== targetKey(reference) || typeof row.record?.body !== "string") throw new Error("Invalid resolved history reference.");
  modal("Original message", [el("p", "dialog-note", "Authenticated local history record. This does not fetch from a peer or create a read receipt."), el("pre", "message-text", row.record.body)], "Close", async () => {});
}
async function copyMessageText(message, name) {
  if (selected !== name || message.sensitive || typeof message.text !== "string") throw new Error("Only visible nonsensitive text from this room can be copied here.");
  if (!globalThis.navigator?.clipboard?.writeText) throw new Error("Clipboard unavailable. Select the visible text and use your browser's Copy command.");
  await navigator.clipboard.writeText(message.text); toast("Message text copied to the system clipboard.");
}
function relationLabel(message, target) {
  const relation = checkedRelation(message);
  if (!relation || relation.kind !== "reply") return null;
  const known = current.messages.some(entry => entry.room === message.room && targetKey(entry.reference) === targetKey(relation.target));
  const label = "Reply to " + memberName(target, relation.target.member.map(byte=>byte.toString(16).padStart(2,"0")).join(""));
  const link = el("button", "outline compact conversation-relation", label + (known ? " / show original" : " / find local original"));
  link.type = "button"; link.onclick = async () => { try { await openMessageReference(message, relation.target); } catch(error) { toast(error.message); } }; return link;
}
function messageActions(item, message, target) {
  if (message.kind === "reaction") return;
  let actions = item.querySelector(".message-actions");
  if (!actions) { actions = el("div", "message-actions"); item.append(actions); }
  const reference = messageTarget(message.reference); if (!reference) return;
  const publish = roomActionAllowed(target, "publish") && foregroundInstance(target), view = JSON.stringify([publish, conversationPending, current.identity, message.message_id, message.kind, message.sensitive]);
  if (actions.dataset.view === view) return;
  const active = document.activeElement, focus = actions.contains(active) ? active?.dataset.action : null;
  const detailsOpen = actions.querySelector("details")?.open;
  actions.dataset.view = view; actions.replaceChildren();
  const reply = el("button", "outline compact", publish ? "Reply" : "Draft reply"); reply.disabled = conversationPending;
  reply.type = "button";
  reply.dataset.action = "reply";
  reply.onclick = () => { try { replyDialog(message, target); } catch(error) { toast(error.message); } }; actions.append(reply);
  if (!message.sensitive && typeof message.text === "string") { const copy = el("button", "outline compact", "Copy text"); copy.type = "button"; copy.dataset.action = "copy"; copy.onclick = async () => { try { await copyMessageText(message, target.name); } catch(error) { toast(error.message); } }; actions.append(copy); }
  if (message.message_id) { const details = el("details", "technical-details"); details.open = detailsOpen; details.append(el("summary", "", "Message ID"), el("code", "fingerprint", message.message_id)); actions.append(details); }
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
  const target = room(), renderedView = JSON.stringify([entries, target?.presentation, current.identity, roomActionAllowed(target, "publish"), foregroundInstance(target), conversationPending, view.searched]);
  if (view.renderedView === renderedView) return;
  const active = globalThis.document?.activeElement, record = active?.closest?.(".history-record");
  const focus = record && view.list.contains?.(active) ? {key:record.dataset.reference, action:active.dataset.action, article:active === record} : null;
  view.renderedView = renderedView;
  view.list.replaceChildren(...entries.map(entry => {
    const item = el("article", "history-record"), event = referenceKey(entry.reference) ? entry.reference.event : null;
    item.dataset.reference = targetKey(entry.reference) || "";
    const fact = event === "OutgoingPrepared" ? "Outgoing prepared / outcome uncertain" : event === "IncomingProcessed" ? "Incoming processed" : "Live only / authenticated stable reference unavailable";
    item.append(el("small", "muted", memberName(room(), entry.sender) + " / " + fact + (entry.stored ? " / local encrypted record" : " / bounded memory")), el("div", "message-body", entry.sensitive ? "Sensitive / excluded from stored history; reveal only in live chat." : entry.text || (entry.kind === "reaction" ? "Legacy conversation metadata / retained record" : "")));
    if (!entry.sensitive) { const relation = relationLabel({...entry,room:view.name}, room()); if (relation) item.append(relation); }
    messageActions(item, {...entry,room:view.name}, room());
    return item;
  }));
  if (!entries.length) view.list.append(el("p", "muted", view.searched ? "No disclosable records on this page." : "Submit a search to read stored records. Live messages are shown separately until an authenticated reference is available."));
  if (focus) {
    const replacement = [...view.list.querySelectorAll(".history-record")].find(item => item.dataset.reference === focus.key);
    if (focus.article && replacement) { replacement.tabIndex = -1; replacement.focus({preventScroll:true}); }
    else [...(replacement?.querySelectorAll("[data-action]") || [])].find(node => node.dataset.action === focus.action)?.focus({preventScroll:true});
  }
}
function historyDialog() {
  const target = room(); if (!target?.status) { toast("Start the room before using its local history actor."); return; }
  const name = target.name, status = el("p", "dialog-note", "Status not requested yet."), notice = el("p", "muted", "Off by default. Enrollment captures future non-sensitive text only. These are local encrypted records, not delivery or human read receipts. No files, media or old-message import."), controls = el("div", "history-controls");
  const query = field("Literal Unicode substring (blank lists a page)", "history-query"); query.querySelector("input").maxLength = 256;
  const paging = el("p", "muted", "Stored history has not been queried."), list = el("div", "history-records"), next = el("button", "outline compact", "Next stored page"); next.type = "button"; next.disabled = true;
  const view = {name, rows:[], query:"", cursor:0, searched:false, list, pending:false}; historyView = view;
  const alive = () => historyView === view && selected === name && token && $("dialog").open;
  const report = error => { if (alive()) { status.textContent = "Unavailable / outcome uncertain. No empty-history claim. Refresh status before retrying a mutation. " + error.message; view.hasMore = false; next.disabled = true; } };
  const setPending = pending => { view.pending = pending; for (const node of controls.querySelectorAll("button")) node.disabled = pending; $("dialog-submit").disabled = pending; next.disabled = pending || !view.hasMore; };
  const button = (label, handler) => { const node = el("button", "outline compact", label); node.type = "button"; node.onclick = async () => { if (view.pending || !alive()) return; setPending(true); try { await handler(); } catch(error) { report(error); } finally { if (alive()) setPending(false); } }; controls.append(node); return node; };
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
    if (view.pending || !alive()) return;
    setPending(true);
    try {
      const result = await historyRequest(name, {operation:"search", text:view.query, after_id:after, limit:20});
      if (!alive()) return;
      if (!Array.isArray(result.rows) || !Number.isSafeInteger(result.next_after) || result.next_after < after || typeof result.has_more !== "boolean" || !Number.isSafeInteger(result.withheld) || result.withheld < 0 || (result.has_more && result.next_after <= after)) throw new Error("Invalid bounded page response.");
      historyEntries(result.rows, []); view.rows = result.rows; view.cursor = result.next_after; view.searched = true;
      paging.textContent = result.rows.length + " stored records / " + result.withheld + " withheld on this page / cursor " + result.next_after + (result.has_more ? " / more available" : " / end of scan") + ". Live-only entries below do not affect this cursor.";
      view.hasMore = result.has_more; renderHistoryEntries(view);
    } catch(error) { if (alive()) { view.hasMore = false; view.rows = []; view.renderedView = null; list.replaceChildren(); report(error); } }
    finally { if (alive()) setPending(false); }
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
  $("connection-problem").hidden = localAvailable;
  applyAppearance(current.settings.appearance);
  $("workspace").dataset.sidebar = current.settings.sidebar_mode || "expanded";
  $("version").textContent = current.version; $("profile-name").textContent = current.settings.display_name; $("identity").textContent = brief(current.identity); $("identity").title = current.identity || "Not initialized";
  $("welcome").hidden = Boolean(current.identity); $("create").disabled = !current.identity; $("join").disabled = !current.identity;
  if (!selected && current.rooms.length) selected = current.rooms[0].name;
  reconcileDrafts();
  reconcileMediaSession();
  $("rooms").replaceChildren(...current.rooms.map((entry) => { const button = el("button", "room-link" + (entry.name === selected ? " selected" : "")); button.setAttribute("aria-label", entry.title || entry.name); button.title = entry.title || entry.name; const text = el("span"); text.append(el("strong", "", entry.title || entry.name), el("small", "", !localAvailable ? "status stale" : entry.status ? (entry.status.active ? "foreground / online" : "access inactive") : entry.problem ? "recovery required" : "offline")); button.append(el("span", "room-icon", "#"), text); button.onclick = () => select(entry.name); return button; }));
  const target = room(); $("room-view").hidden = !current.identity || !target; $("choose-room").hidden = !current.identity || Boolean(target);
  if (target) {
    const namesError = target.presentation_error || target.status?.profile_names_error;
    $("room-title").textContent = target.title || target.name; $("room-id").textContent = target.id || "Unavailable";
    $("room-problem").textContent = [target.problem, namesError ? "Member names unavailable: " + namesError : "", !roomActionAllowed(target, "publish") ? "Publishing unavailable. Local drafts are kept; no automatic retry or new upload admission." : ""].filter(Boolean).join(" / "); $("room-problem").hidden = !$("room-problem").textContent;
    $("room-state").textContent = !localAvailable ? "LOCAL UNAVAILABLE / STATUS STALE" : target.status ? (target.status.active ? "FOREGROUND / ONLINE" : "ACCESS INACTIVE / CACHED STATE") : (current.phases[target.name]?.state || "OFFLINE").toUpperCase();
    $("start").hidden = Boolean(target.status); $("stop").hidden = !target.status; $("leave").hidden = !target.status?.active; $("room-history").disabled = !target.status; $("invite").disabled = !admissionRoles(target).length; $("controllers").disabled = !roomActionAllowed(target, "control"); $("find-copies").disabled = !target.status; $("share-media").disabled = !roomActionAllowed(target, "publish") || !mediaControl; $("receive-media").disabled = !target.status || !mediaControl;
    for (const id of ["start", "stop", "address", "theme", "profiles", "initialize"]) $(id).disabled = !localAvailable;
    for (const id of ["room-history", "find-copies", "receive-media"]) $(id).disabled ||= !localAvailable;
    $("recover-pending").disabled = !foregroundInstance(target) || !localAvailable;
    $("share-media").disabled ||= !foregroundInstance(target); $("receive-media").disabled ||= !foregroundInstance(target);
    renderComposer(target); $("allow-files").disabled = fileConsentPending || !localAvailable;
    if (target.status?.files && !fileConsentPending) $("allow-files").checked = target.status.files.accepted;
    $("quorum").textContent = target.status ? target.status.members.length + " MEMBERS / " + target.status.voters.length + " VOTERS / EPOCH " + target.status.epoch : "PERSISTENT KEYS / NO BACKGROUND SERVICE";
    const openPeers = new Set([...$("peers").querySelectorAll(".peer")].filter(card => card.querySelector("details")?.open).map(card => card.dataset.peer));
    $("peers").replaceChildren(...(target.status?.members || []).map((peer) => { const card = el("div", "peer"), self = peer === target.status.owner; card.dataset.peer = peer; const label = el("span"); const name = memberName(target, peer); const details = el("details", "technical-details"); details.open = openPeers.has(peer); details.append(el("summary", "", "Fingerprint"), el("code", "fingerprint", peer)); label.append(el("strong", "", name), el("small", "", (self ? "This device / " : "") + (target.status.voters.includes(peer) ? "Voting controller" : "Data member")), details); card.append(el("span", "peer-avatar", name === "Unnamed member" ? "?" : Array.from(name.trim()).slice(0, 2).join("").toUpperCase()), label);
      label.append(el("small", "muted", memberProvenance(target, peer)));
      if (!self) { const nickname = el("button", "outline compact", "Nickname"); nickname.onclick = () => nicknameDialog(target, peer); card.append(nickname); const remove = el("button", "peer-remove", "x"); remove.disabled = !roomActionAllowed(target, "remove") || target.status.voters.includes(peer); remove.title = target.status.voters.includes(peer) ? "Retire this member's controller vote before removing data access" : "Remove data membership / requires moderation rights"; remove.setAttribute("aria-label", remove.title); remove.onclick = () => removeMemberDialog(target, peer); card.append(remove); } return card; }));
    mediaControl?.refreshLabels();
    if (shownRoom !== selected) { shownRoom = selected; rendered.clear(); revealed.clear(); $("messages").replaceChildren(); $("files").replaceChildren(); }
    const messages = current.messages.filter((message) => message.room === selected && message.kind !== "reaction");
    if (!messages.length && !$("messages").querySelector(".empty-message")) { const empty = el("div", "empty-message"); empty.append(el("span", "line-art"), el("h2", "", "A direct conversation."), el("p", "", "Live messages use bounded memory. Room actions / Local history opens the optional encrypted archive.")); $("messages").append(empty); }
    for (const message of messages) {
      if (rendered.has(message.id)) { const item = rendered.get(message.id), meta = item.querySelector(".message-meta"); meta.textContent = (message.outgoing ? memberName(target, target.status?.owner) : memberName(target, message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED"); messageActions(item, message, target); continue; }
      $("messages").querySelector(".empty-message")?.remove(); const item = el("article", "message" + (message.outgoing ? " outgoing" : "")); item.dataset.reference = targetKey(message.reference) || ""; item.tabIndex = $("messages").querySelector('.message[tabindex="0"]') ? -1 : 0;
      const meta = el("div", "message-meta", (message.outgoing ? memberName(target, target.status?.owner) : memberName(target, message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED")); meta.title = message.sender; item.append(meta);
      const body = el("div", "message-body" + (message.sensitive ? " sensitive" : ""));
      if (message.sensitive) { body.append(el("span", "", "Hidden on this device / " + message.bytes + " bytes")); const reveal = el("button", "", "Reveal"); reveal.onclick = async () => { try { const value = await act({ action: "reveal", id: message.id }); body.textContent = value.text; const relation = relationLabel({...message, relation:value.relation}, target); if (relation) body.append(relation); revealed.set(message.id, true); } catch (error) { toast(error.message); } }; body.append(reveal); }
      else { body.textContent = message.text; const relation = relationLabel(message, target); if (relation) body.append(relation); } item.append(body); messageActions(item, message, target); rendered.set(message.id, item); $("messages").append(item); $("messages").scrollTop = $("messages").scrollHeight;
    }
    const retained = new Set(messages.map((message) => message.id)); for (const [id, node] of rendered) if (!retained.has(id)) { node.remove(); rendered.delete(id); revealed.delete(id); }
    if (!$("messages").querySelector('.message[tabindex="0"]')) { const first = $("messages").querySelector(".message"); if (first) first.tabIndex = 0; }
  }
  renderOperations();
  if (historyView) renderHistoryEntries(historyView);
}
function invitationAvailable(id, identity = current?.identity) {
  return Boolean(token && localAvailable && current?.identity === identity && current?.operations.some(operation => operation.id === id && operation.kind === "invite" && operation.state === "waiting" && operation.invitation_ready === true && !operation.proposal));
}
async function saveInvitation(id) {
  const identity = current?.identity;
  const available = () => invitationAvailable(id, identity);
  if (!available()) throw new Error("Invitation unavailable or expired. No download was started.");
  const controller = new AbortController(), status = el("p", "dialog-note", "Preparing the private invitation..."), output = el("div");
  modal("Save private invitation", [status, output, el("p", "dialog-note", "Choose Save invitation once, then complete your browser's Save/Open choice. Keep this dialog open until the file is saved. The file is a private single-use capability, not a public fingerprint.")], "Close", async () => {});
  const revision = modalRevision, dialog = $("dialog"); let retired = false, url, observer;
  const live = () => !retired && revision === modalRevision && dialog.open && available();
  const retire = () => { retired = true; controller.abort(); if (url) { URL.revokeObjectURL(url); url = null; } output.replaceChildren(); observer?.disconnect(); dialog.removeEventListener("close", closed); globalThis.removeEventListener?.("pagehide", retire); };
  const closed = () => { if (!live()) retire(); };
  dialog.addEventListener("close", closed); globalThis.addEventListener?.("pagehide", retire, {once:true});
  observer = new MutationObserver(closed); observer.observe($("dialog-content"), {childList:true}); observer.observe($("operations"), {childList:true});
  try {
    const response = await fetch("/api/operations/" + id + "/invitation", {headers:{Authorization:"Bearer " + token},signal:controller.signal,cache:"no-store"});
    if (!response.ok) throw new Error("Invitation unavailable or expired. Nothing was saved.");
    if (!live()) return;
    const blob = await response.blob(); if (!live()) return;
    url = URL.createObjectURL(blob);
    const save = el("a", "outline compact", "Save invitation"); save.href = url; save.download = "kk-invitation.json";
    save.onclick = event => { if (!live()) { event.preventDefault(); retire(); return; } status.textContent = "Save requested. Complete the browser's Save/Open choice before closing this dialog. This does not approve or complete pairing."; };
    output.append(save); status.textContent = "Private invitation ready. Choose Save invitation once; the browser controls the destination and completion.";
  } catch(error) { if (live()) status.textContent = error.message; }
}
async function copyInvitation(id) {
  if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable. Use the single invitation download instead.");
  const identity = current?.identity;
  const active = () => invitationAvailable(id, identity);
  if (!localAvailable || !active()) throw new Error("This invitation is no longer available. Create a fresh invitation intentionally.");
  const response = await fetch("/api/operations/" + id + "/invitation", {headers:{Authorization:"Bearer " + token},cache:"no-store"});
  if (!response.ok) throw new Error("Invitation unavailable or expired. Nothing was copied.");
  const text = await response.text();
  if (!token || !localAvailable || !active()) throw new Error("Invitation changed while reading. Nothing was copied.");
  await navigator.clipboard.writeText(text);
  toast("Private single-use invitation copied. Paste only into the intended device's Join form; clear your clipboard afterwards.");
}
function renderOperations() {
  const terminalStates = ["failed", "declined", "complete", "stopped", "joined"], identity = current.identity;
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
    if (terminalStates.includes(operation.state)) {
      if (operation.kind === "invite" && ["failed", "declined", "stopped"].includes(operation.state)) {
        const retry = el("button", "primary compact", "Create fresh invitation"); retry.dataset.action = "retry-invitation";
        const id = operation.id, name = operation.room;
        retry.onclick = () => {
          const latest = current.operations.find(item => item.id === id);
          if (!token || !localAvailable || current.identity !== identity || latest?.kind !== "invite" || latest.room !== name || !["failed", "declined", "stopped"].includes(latest.state)) { toast("Operation changed. Inspect its current state before creating an invitation."); return; }
          if (!current.rooms.some(item => item.name === name)) { toast("The original room is unavailable. No invitation was created."); return; }
          if (selected !== name) select(name);
          if (selected === name && room()?.name === name) inviteDialog();
        };
        card.append(el("p", "", "This attempt cannot be resumed. Create and confirm a new single-use invitation; the old JSON and confirmation are not reused."), retry);
      }
      if (operation.kind === "join" && joinDrafts.has(operation.id)) { const retry = el("button", "primary compact", "Edit joining details"); retry.dataset.action = "retry"; retry.onclick = () => retryJoin(operation); card.append(el("p", "", "The invitation is not retained. Paste or select a valid private invitation again."), retry); }
      const dismiss = el("button", "outline compact", "Dismiss"); dismiss.dataset.action = "dismiss"; dismiss.onclick = () => { if (current.identity !== identity || !current.operations.some(item => item.id === operation.id && terminalStates.includes(item.state))) return; dismissedOperations.add(operation.id); joinDrafts.delete(operation.id); render(); }; card.append(dismiss);
    }
    if (operation.state === "waiting" && operation.proposal) { const confirmation = operation.proposal.confirmation, fingerprint = operation.proposal.fingerprint; card.append(el("p", "", "Compare this CONFIRM value on both devices."), el("div", "sas", confirmation), el("code", "", fingerprint)); const actions = el("div", "operation-actions"); for (const accept of [true, false]) { const button = el("button", accept ? "primary compact" : "outline compact", accept ? "Matches / approve" : "Reject"); button.dataset.action = accept ? "approve" : "reject"; button.onclick = async () => { try { const latest = current.operations.find(item => item.id === operation.id); if (!token || !localAvailable || current.identity !== identity || latest?.state !== "waiting" || latest.proposal?.confirmation !== confirmation || latest.proposal?.fingerprint !== fingerprint) throw new Error("Confirmation changed or retired. Inspect the current operation before approving."); await act({ action: "confirm", id: operation.id, accept }); } catch (error) { toast(error.message); } }; actions.append(button); } card.append(actions); }
    else if (invitationAvailable(operation.id, identity)) { card.append(el("p", "", "Send this private single-use JSON only to the intended device. Copy avoids a downloaded file but leaves a private capability on the clipboard; clear it afterwards. A public fingerprint alone is not an invitation. It does not grant a vote.")); const copy = el("button", "outline compact", "Copy invitation"); copy.dataset.action = "copy-invitation"; copy.onclick = async () => { try { await copyInvitation(operation.id); } catch(error) { toast(error.message); } }; const button = el("button", "outline compact", "Download invitation"); button.dataset.action = "download"; button.onclick = async () => { try { await saveInvitation(operation.id); } catch (error) { toast(error.message); } }; card.append(copy, button); }
    return card;
  }));
  if (focusKey) {
    const card = [...$("operations").children].find(node => node.dataset.operation === focusKey[0]);
    const replacement = card && [...card.querySelectorAll("button")].find(node => node.dataset.action === focusKey[1]);
    (replacement || $("action-center").querySelector("summary"))?.focus();
  }
}
function renderComposer(target) {
  expirePendingSends();
  const key = draftRoomKey(target.name), policy = draftPolicy(), ready = key && policy && (composerDrafts.has(key) || composerDrafts.size < policy.max_rooms);
  const publish = roomActionAllowed(target, "publish") && foregroundInstance(target), pending = pendingSends.has(key);
  // Drafting is a local edit, not a room capability. Never lock unsent text on an outage.
  $("text").disabled = !ready; $("sensitive").disabled = !ready; if (policy) $("text").maxLength = policy.max_text_bytes;
  $("text").placeholder = !localAvailable ? "Write a local draft. Reconnect before sending." : !target.status ? "Write a local draft. Start this room to send." : !foregroundInstance(target) ? "Write a local draft. Reopen this room with a current foreground owner before sending." : !publish ? "Write a local draft. Publishing is unavailable with your room rights." : "Write to the current room members...";
  if (!ready) $("text").placeholder = "Room identity or draft capacity unavailable. Clear another room's draft or refresh before drafting here.";
  $("send").disabled = !ready || !publish || pending; $("send").textContent = pending ? "Sending..." : "Send"; $("attach").disabled = !publish;
}
function reconcileMediaSession() {
  const target = room();
  const fence = JSON.stringify([current?.identity, selected, target?.id, localAvailable && target?.status?.active === true, current?.phases?.[selected]?.revision ?? null, foregroundInstance(target)]);
  if (mediaSessionFence !== null && mediaSessionFence !== fence) mediaControl?.stopAll();
  mediaSessionFence = fence;
}
async function refresh() { if (!token || refreshing) return; refreshing = true; $("retry-connection").disabled = true; try { current = await api("/api/state"); localAvailable = true; notificationControl?.observe(current); $("connection").textContent = "LOCAL / CONNECTED"; render(); await loadFiles(); } catch (error) { localAvailable = false; $("connection").textContent = "LOCAL / UNAVAILABLE"; mediaControl?.stopAll(); notificationControl?.close(); if (current && token) render(); } finally { refreshing = false; $("retry-connection").disabled = false; } }
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
$("messages").addEventListener("focusin", event => {
  if ($("dialog").open) return;
  const message = event.target.closest?.(".message"); if (!message) return;
  for (const item of $("messages").querySelectorAll(".message")) item.tabIndex = item === message ? 0 : -1;
});
$("messages").addEventListener("keydown", event => {
  if (event.defaultPrevented || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || $("dialog").open || $("sidebar").classList.contains("open")) return;
  const target = event.target;
  if (!(target instanceof HTMLElement) || target.isContentEditable || target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) return;
  const message = target.closest(".message"); if (!message || !$("messages").contains(message)) return;
  if (target !== message && !target.closest(".message-actions") && !target.matches("button, a, summary")) return;
  if (event.key === "Enter" && target === message) {
    const reply = message.querySelector('button[data-action="reply"]:not([disabled])');
    if (reply) { event.preventDefault(); if (!event.repeat) reply.click(); }
    return;
  }
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
  const items = [...$("messages").querySelectorAll(".message")], index = items.indexOf(message);
  const next = items[Math.max(0, Math.min(items.length - 1, index + (event.key === "ArrowUp" ? -1 : 1)))];
  if (next) { event.preventDefault(); next.focus({preventScroll:true}); next.scrollIntoView({block:"nearest"}); }
});
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
$("retry-connection").onclick = () => refresh();
$("invite").onclick = inviteDialog; $("controllers").onclick = controllersDialog; $("address").onclick = addressDialog;
$("find-copies").onclick = findCopiesDialog; $("close-copies").onclick = () => { $("copy-panel").hidden = true; $("copies").replaceChildren(); };
$("room-history").onclick = historyDialog;
$("recover-pending").onclick = recoveryDialog;
$("dialog").addEventListener?.("close", () => { if ($("dialog").open) return; modalRevision++; historyView = null; $("dialog-form").onsubmit = null; for (const input of $("dialog-content").querySelectorAll("input, textarea")) input.value = ""; $("dialog-content").replaceChildren(); });
$("initialize").onclick = async () => { try { await act({ action: "init" }); } catch (error) { toast(error.message); } };
$("start").onclick = async () => { try { await act({ action: "serve", name: selected, accept_files: $("allow-files").checked }); } catch (error) { toast(error.message); } };
$("stop").onclick = async () => { try { await act({ action: "stop", name: selected }); } catch (error) { toast(error.message); } };
$("leave").onclick = () => modal("Leave room", [el("p", "dialog-note", "This removes only this profile's membership. It does not erase the profile, received files or retained local history. A timeout can leave the outcome uncertain; inspect status before retrying.")], "Leave room", async () => { await act({action:"leave-self", name:selected, confirm:true}); await refresh(); });
$("allow-files").onchange = async () => {
  if (!room()?.status) return;
  fileConsentPending = true; $("allow-files").disabled = true;
  try { await act({ action: "file-consent", name: selected, accepted: $("allow-files").checked }); }
  catch (error) { toast(error.message); }
  finally { fileConsentPending = false; await refresh(); render(); }
};
async function sendComposer(event) {
  event.preventDefault(); expirePendingSends(); reconcileDrafts(); const name = selected, key = draftRoomKey(name), text = $("text").value, sensitive = $("sensitive").checked;
  if (key && text && (!roomActionAllowed(room(), "publish") || !foregroundInstance(room()))) { $("send-status").textContent = "Publishing unavailable. Text and sensitivity are kept locally; nothing retries automatically. An earlier submitted request may still complete—inspect before retrying; duplicates are possible."; saveComposerDraft(); return; }
  if (!key || !text || pendingSends.has(key) || !roomActionAllowed(room(), "publish") || !foregroundInstance(room()) || !saveComposerDraft()) return;
  const milliseconds = clientRequestWaitMs();
  if (milliseconds === null) { $("send-status").textContent = "Client wait policy unavailable. Refresh or reopen this UI before sending; nothing was sent."; return; }
  const draft = composerDrafts.get(key), session = recoverySession(room()), generation = draftGeneration;
  const tracked = () => generation === draftGeneration && composerDrafts.get(key) === draft;
  const sameForeground = () => { const target = current?.rooms.find(entry=>entry.name===name); return target && recoverySession(target) === session; };
  const attempt = {deadline:Date.now() + milliseconds}; pendingSends.set(key, attempt);
  Object.assign(attempt, clientWait(signal => { renderComposer(room()); return act({action:"send", name, text, sensitive}, {signal,refresh:false}); }, milliseconds));
  try {
    const result = await attempt.promise;
    if (!tracked()) return;
    const accepted = result.delivery?.ok === true;
    draft.status = (sameForeground() ? deliveryComplete(result.delivery) ? "Acknowledged by the recipient event queues, not human reading." : "Partial or unconfirmed delivery. Inspect peer outcomes before explicitly retrying; duplicates are possible." : "Foreground changed during send. Draft retained; inspect the original outcome before retrying.").slice(0, draftPolicy()?.max_text_bytes || 0);
    if (composerScope === key) { if (sameForeground() && accepted && $("text").value === text && $("sensitive").checked === sensitive) $("text").value = ""; $("send-status").textContent = draft.status; saveComposerDraft(); }
    else if (sameForeground() && accepted && draft.text === text && draft.sensitive === sensitive) draft.text = "";
  } catch (error) {
    if (!tracked()) return;
    draft.status = ("Send outcome uncertain. Draft retained; another attempt can duplicate delivery. Inspect the room before retrying. " + error.message).slice(0, draftPolicy()?.max_text_bytes || 0);
    if (composerScope === key) $("send-status").textContent = draft.status;
    toast(error.message);
  } finally {
    if (pendingSends.get(key) === attempt) pendingSends.delete(key);
    if (current && token) {
      if (composerScope === key && room()) renderComposer(room());
      void refresh().catch(error => toast("Local view refresh unavailable. Inspect the room before retrying. " + error.message));
    }
  }
}
$("composer").onsubmit = sendComposer;
$("text").onkeydown = (event) => { if ((event.ctrlKey || event.metaKey) && event.key === "Enter") $("composer").requestSubmit(); };
$("share-media").onclick = () => mediaControl?.shareDialog(); $("receive-media").onclick = () => mediaControl?.receiveDialog(); $("stop-media").onclick = () => mediaControl?.stopAll();
addEventListener("pagehide", () => { mediaControl?.stopAll(); notificationControl?.retire(); });
addEventListener("focus", () => { expirePendingSends(); notificationControl?.close(); });
addEventListener("pageshow", expirePendingSends);
document.addEventListener("visibilitychange", () => { if (!document.hidden) expirePendingSends(); });
$("attach").onclick = () => $("file-input").click();
$("file-transfers").onclick = transfersDialog;
$("file-input").onchange = (event) => { const file = event.target.files[0]; if (!file) return; if (file.size > (current?.files.max_bytes || 0)) { toast("This file exceeds the configured upload limit."); return; }
  fileDialog("Send " + file.name, async (name, to, sensitive, selectedFile) => { const response = await fetch("/api/rooms/" + encodeURIComponent(name) + "/files?to=" + encodeURIComponent(to) + "&sensitive=" + sensitive + "&background=true", { method: "POST", headers: { Authorization: "Bearer " + token, "Content-Type": "application/octet-stream" }, body: selectedFile }); return response.json(); }, false, {file}); event.target.value = "";
};
notificationControl = messageNotifications(globalThis, {
  hidden: () => document.hidden, focused: () => document.hasFocus(), selected: () => selected,
  open: name => { window.focus(); if (current?.rooms.some(item => item.name === name)) select(name); },
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
