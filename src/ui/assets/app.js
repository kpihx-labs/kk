"use strict";
const launchCode = location.hash.slice(1); history.replaceState(null, "", location.pathname);
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => { const node = document.createElement(tag); if (cls) node.className = cls; if (text !== undefined) node.textContent = text; return node; };
let token = sessionStorage.getItem("kk-session") || "";
let current = null, selected = sessionStorage.getItem("kk-room") || "", refreshing = false, mediaControl = null;
let shownRoom = "", rendered = new Map(), revealed = new Map(), toastTimer;
let fileConsentPending = false;
let joinDraft = {name:"", bind:"", role:"participant"};
const joinDrafts = new Map();
const dismissedOperations = new Set();
const brief = (fp) => fp ? fp.slice(0, 8) + ":" + fp.slice(-8) : "";
function toast(text) { $("toast").textContent = text; $("toast").hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => { $("toast").hidden = true; }, 5500); }
async function api(path, body) {
  const headers = { Authorization: "Bearer " + token };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, { method: body === undefined ? "GET" : "POST", headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store" });
  if (response.status === 401 || response.status === 403) {
    token = ""; sessionStorage.removeItem("kk-session"); mediaControl?.stopAll(); joinDrafts.clear(); joinDraft = {name:"", bind:"", role:"participant"}; $("dialog").close(); $("dialog-content").replaceChildren();
    const message = "This private session expired or the local service restarted. Relaunch kk ui and use its new private browser launch.";
    $("gate-text").textContent = message; $("gate").hidden = false; $("workspace").hidden = true;
    throw new Error(message);
  }
  let value; try { value = await response.json(); } catch { throw new Error("The local service returned an invalid response."); }
  if (!response.ok || value.ok === false) throw new Error(value.error || value.code || "Operation refused.");
  return value;
}
async function act(action) { const value = await api("/api/action", action); await refresh(); return value.result; }
function room() { return current?.rooms.find((entry) => entry.name === selected); }
function memberName(target, id) { return target?.presentation?.find(member => member.id === id)?.display_name || "Unnamed member"; }
function memberChoice(target, id) { return memberName(target, id) + " / " + brief(id); }
function nicknameDialog(target, peer) {
  const entry = target.presentation?.find(member => member.id === peer);
  const name = field("Local nickname (leave blank to clear)", "display_name", "text", entry?.source === "local_override" ? entry.display_name : "");
  name.querySelector("input").maxLength = 64;
  modal("Local member nickname", [el("p", "dialog-note", "This nickname is LOCAL ONLY. It is not a verified remote profile name, is not announced to the member, and does not change trust or permissions."), el("code", "fingerprint", peer), name], "Save nickname", async data => { const value = data.get("display_name").trim(); await act({action:"member-label", name:target.name, member:peer, display_name:value || null}); });
}
function select(name) { if (selected !== name) { mediaControl?.stopAll(); $("copy-panel").hidden = true; $("copies").replaceChildren(); } selected = name; sessionStorage.setItem("kk-room", name); $("sidebar").classList.remove("open"); render(); }
function field(label, name, type = "text", value = "", note) {
  const wrapper = el("label", "field", label); const input = el(type === "textarea" ? "textarea" : "input"); input.name = name;
  if (type !== "textarea") input.type = type; if (value) input.value = value;
  wrapper.append(input); if (note) wrapper.append(el("small", "muted", note)); return wrapper;
}
function roleField(value = "participant") { const wrapper = el("label", "field", "Accepted data role"); const input = el("select"); input.name = "role"; for (const role of ["participant", "reader", "inviter", "moderator"]) { const option = el("option", "", role); option.value = role; input.append(option); } input.value = value; wrapper.append(input); return wrapper; }
function modal(title, controls, submit, handler) {
  $("dialog-title").textContent = title; $("dialog-content").replaceChildren(...controls); $("dialog-error").textContent = ""; $("dialog-submit").textContent = submit;
  $("dialog-form").onsubmit = async (event) => { event.preventDefault(); const button = $("dialog-submit"); button.disabled = true;
    try { await handler(new FormData(event.currentTarget)); $("dialog").close(); } catch (error) { $("dialog-error").textContent = error.message; } finally { button.disabled = false; }
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
  const target = room(); if (!target?.status) return;
  const bind = field("Pairing listen IP:port", "bind", "text", "", "Use a different port from this room's data port."); bind.querySelector("input").required = true;
  try { const url = new URL("http://" + target.status.address); url.port = String(Number(url.port) < 65535 ? Number(url.port) + 1 : Number(url.port) - 1); bind.querySelector("input").value = url.host; } catch {}
  modal("Invite a member", [bind, roleField(), el("p", "dialog-note", "The other device must choose the same accepted role. Compare CONFIRM on both devices before approving.")], "Create single-use invitation", async (data) => { await act({ action: "invite", name: target.name, bind: data.get("bind").trim(), role: data.get("role") }); });
}
function controllersDialog() {
  const target = room(); if (!target?.status) return;
  const list = el("div"); for (const peer of target.status.members) { const row = el("label", "field"); const input = el("input"); input.type = "checkbox"; input.name = "peers"; input.value = peer; input.checked = target.status.voters.includes(peer); row.title = peer; row.append(input, el("span", "", memberChoice(target, peer))); list.append(row); }
  modal("Choose voting controllers", [el("p", "dialog-note", "Only administrative members are eligible. This changes the consensus voter set, not anyone's data role. A majority must remain available."), list], "Approve exact controller set", async (data) => { await act({ action: "controllers", name: target.name, peers: data.getAll("peers") }); });
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
  modal(label, [wrapper, privacy, el("p", "dialog-note", "The recipient must explicitly consent to file storage. Retrying the same content verifies its prefix; a receipt is not a future availability promise.")], "Send to this recipient", async (data) => { const result = await send(target.name, data.get("to"), data.get("sensitive") === "on"); if (!result.ok) throw new Error(result.result?.error || result.code || "The recipient refused the file."); toast("Verified durable recipient storage."); });
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
function render() {
  if (!current) return;
  applyAppearance(current.settings.appearance);
  $("version").textContent = current.version; $("profile-name").textContent = current.settings.display_name; $("identity").textContent = brief(current.identity); $("identity").title = current.identity || "Not initialized";
  $("welcome").hidden = Boolean(current.identity); $("create").disabled = !current.identity; $("join").disabled = !current.identity;
  if (!selected && current.rooms.length) selected = current.rooms[0].name;
  $("rooms").replaceChildren(...current.rooms.map((entry) => { const button = el("button", "room-link" + (entry.name === selected ? " selected" : "")); const text = el("span"); text.append(el("strong", "", entry.title || entry.name), el("small", "", entry.status ? "foreground / online" : entry.problem ? "recovery required" : "offline")); button.append(el("span", "room-icon", "#"), text); button.onclick = () => select(entry.name); return button; }));
  const target = room(); $("room-view").hidden = !current.identity || !target; $("choose-room").hidden = !current.identity || Boolean(target);
  if (target) {
    $("room-title").textContent = target.title || target.name; $("room-id").textContent = target.id || "Unavailable"; $("room-problem").textContent = target.problem || (target.presentation_error ? "Local member names unavailable: " + target.presentation_error : ""); $("room-problem").hidden = !target.problem && !target.presentation_error;
    $("room-state").textContent = target.status ? "FOREGROUND / ONLINE" : (current.phases[target.name]?.state || "OFFLINE").toUpperCase();
    $("start").hidden = Boolean(target.status); $("stop").hidden = !target.status; $("invite").disabled = !target.status; $("controllers").disabled = !target.status; $("find-copies").disabled = !target.status; $("share-media").disabled = !target.status || !mediaControl; $("receive-media").disabled = !target.status || !mediaControl;
    $("text").disabled = !target.status; $("send").disabled = !target.status; $("attach").disabled = !target.status; $("allow-files").disabled = fileConsentPending;
    if (target.status?.files && !fileConsentPending) $("allow-files").checked = target.status.files.accepted;
    $("quorum").textContent = target.status ? target.status.members.length + " MEMBERS / " + target.status.voters.length + " VOTERS / EPOCH " + target.status.epoch : "PERSISTENT KEYS / NO BACKGROUND SERVICE";
    const openPeers = new Set([...$("peers").querySelectorAll(".peer")].filter(card => card.querySelector("details")?.open).map(card => card.dataset.peer));
    $("peers").replaceChildren(...(target.status?.members || []).map((peer) => { const card = el("div", "peer"), self = peer === target.status.owner; card.dataset.peer = peer; const label = el("span"); const name = memberName(target, peer); const details = el("details", "technical-details"); details.open = openPeers.has(peer); details.append(el("summary", "", "Fingerprint"), el("code", "fingerprint", peer)); label.append(el("strong", "", name), el("small", "", (self ? "This device / " : "") + (target.status.voters.includes(peer) ? "Voting controller" : "Data member")), details); card.append(el("span", "peer-avatar", name === "Unnamed member" ? "?" : Array.from(name.trim()).slice(0, 2).join("").toUpperCase()), label);
      if (!self) { const nickname = el("button", "outline compact", "Nickname"); nickname.onclick = () => nicknameDialog(target, peer); card.append(nickname); const remove = el("button", "peer-remove", "x"); remove.title = "Remove data membership"; remove.onclick = () => modal("Remove data membership?", [el("p", "dialog-note", "Retire this peer's vote separately if necessary. Data removal rotates the private group epoch."), el("code", "fingerprint", peer)], "Remove data access", async () => { await act({ action: "remove", name: target.name, peer }); }); card.append(remove); } return card; }));
    mediaControl?.refreshLabels();
    if (shownRoom !== selected) { shownRoom = selected; rendered.clear(); revealed.clear(); $("messages").replaceChildren(); $("files").replaceChildren(); }
    const messages = current.messages.filter((message) => message.room === selected);
    if (!messages.length && !$("messages").querySelector(".empty-message")) { const empty = el("div", "empty-message"); empty.append(el("span", "line-art"), el("h2", "", "A direct conversation."), el("p", "", "Messages are kept only in this workspace's bounded memory.")); $("messages").append(empty); }
    for (const message of messages) {
      if (rendered.has(message.id)) { const meta = rendered.get(message.id).querySelector(".message-meta"); meta.textContent = (message.outgoing ? memberName(target, target.status?.owner) : memberName(target, message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED"); continue; }
      $("messages").querySelector(".empty-message")?.remove(); const item = el("article", "message" + (message.outgoing ? " outgoing" : ""));
      const meta = el("div", "message-meta", (message.outgoing ? memberName(target, target.status?.owner) : memberName(target, message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED")); meta.title = message.sender; item.append(meta);
      const body = el("div", "message-body" + (message.sensitive ? " sensitive" : ""));
      if (message.sensitive) { body.append(el("span", "", "Hidden on this device / " + message.bytes + " bytes")); const reveal = el("button", "", "Reveal"); reveal.onclick = async () => { try { const value = await act({ action: "reveal", id: message.id }); body.textContent = value.text; revealed.set(message.id, true); } catch (error) { toast(error.message); } }; body.append(reveal); }
      else body.textContent = message.text; item.append(body); rendered.set(message.id, item); $("messages").append(item); $("messages").scrollTop = $("messages").scrollHeight;
    }
    const retained = new Set(messages.map((message) => message.id)); for (const [id, node] of rendered) if (!retained.has(id)) { node.remove(); rendered.delete(id); revealed.delete(id); }
  }
  const operationIds = new Set(current.operations.map((operation) => operation.id));
  for (const operation of current.operations) if (operation.kind === "join" && ["joined", "stopped"].includes(operation.state)) joinDrafts.delete(operation.id);
  for (const id of joinDrafts.keys()) if (!operationIds.has(id)) joinDrafts.delete(id);
  for (const id of dismissedOperations) if (!operationIds.has(id)) dismissedOperations.delete(id);
  $("operations").replaceChildren(...current.operations.filter((operation) => !dismissedOperations.has(operation.id) && !["complete", "stopped", "joined"].includes(operation.state)).map((operation) => {
    const card = el("section", "operation"); card.append(el("h3", "", operation.room + " / " + operation.state));
    if (operation.error) card.append(el("p", "error", operation.error));
    if (["failed", "declined"].includes(operation.state)) { if (operation.kind === "join" && joinDrafts.has(operation.id)) { const retry = el("button", "primary compact", "Edit joining details"); retry.onclick = () => retryJoin(operation); card.append(el("p", "", "The invitation is not retained. Paste or select a valid private invitation again."), retry); } const dismiss = el("button", "outline compact", "Dismiss"); dismiss.onclick = () => { dismissedOperations.add(operation.id); joinDrafts.delete(operation.id); render(); }; card.append(dismiss); }
    if (operation.proposal) { card.append(el("p", "", "Compare this CONFIRM value on both devices."), el("div", "sas", operation.proposal.confirmation), el("code", "", operation.proposal.fingerprint)); const actions = el("div", "operation-actions"); for (const accept of [true, false]) { const button = el("button", accept ? "primary compact" : "outline compact", accept ? "Matches / approve" : "Reject"); button.onclick = async () => { try { await act({ action: "confirm", id: operation.id, accept }); } catch (error) { toast(error.message); } }; actions.append(button); } card.append(actions); }
    else if (operation.invitation_ready && !["failed", "working", "declined"].includes(operation.state)) { card.append(el("p", "", "Send this private single-use JSON to the intended device. A public fingerprint alone is not an invitation. It does not grant a vote.")); const button = el("button", "outline compact", "Download invitation"); button.onclick = async () => { try { const response = await fetch("/api/operations/" + operation.id + "/invitation", { headers: { Authorization: "Bearer " + token } }); if (!response.ok) throw new Error("Invitation is unavailable or expired."); const url = URL.createObjectURL(await response.blob()); const anchor = el("a"); anchor.href = url; anchor.download = "kk-invitation.json"; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); } catch (error) { toast(error.message); } }; card.append(button); }
    return card;
  }));
}
async function refresh() { if (!token || refreshing) return; refreshing = true; try { current = await api("/api/state"); $("connection").textContent = "LOCAL / CONNECTED"; render(); await loadFiles(); } catch (error) { $("connection").textContent = "LOCAL / UNAVAILABLE"; mediaControl?.stopAll(); } finally { refreshing = false; } }
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
  const settings = current.settings, name = field("Display name", "display_name", "text", settings.display_name); name.querySelector("input").required = true; name.querySelector("input").maxLength = 64;
  const hue = field("Hue (0-350 in steps of 10; blank for neutral)", "hue", "number", settings.appearance.hue === null ? "" : String(settings.appearance.hue)); const hueInput = hue.querySelector("input"); hueInput.min = 0; hueInput.max = 350; hueInput.step = 10;
  const clarity = field("Clarity (0-100)", "clarity", "range", String(settings.appearance.clarity)); const saturation = field("Saturation (0-100)", "saturation", "range", String(settings.appearance.saturation));
  for (const wrapper of [clarity, saturation]) { const input = wrapper.querySelector("input"); input.min = 0; input.max = 100; input.step = 10; const value = el("small", "muted", input.value); input.oninput = () => { value.textContent = input.value; }; wrapper.append(value); }
  modal("Profile settings", [name, hue, clarity, saturation, el("p", "dialog-note", "Neutral hue, clarity 50 and saturation 20 restore the original dark grey palette. Settings belong to the running profile.")], "Save settings", async data => { const raw = data.get("hue"); await act({action:"settings", display_name:data.get("display_name"), appearance:{hue:raw === "" ? null : Number(raw), clarity:Number(data.get("clarity")), saturation:Number(data.get("saturation"))}}); });
};
$("profiles").onclick = async () => { try {
  const profiles = await act({action:"profiles"}), list = el("div");
  for (const profile of profiles) { const row = el("div", "file-card"); row.append(el("strong", "", profile.settings.display_name), el("span", "muted", profile.id + (profile.selected ? " / selected for next launch" : ""))); const choose = el("button", "outline compact", "Select for restart"); choose.onclick = async () => { try { await act({action:"profile-select", id:profile.id}); toast("Selection saved. Stop this foreground UI and restart it to use the selected profile; running rooms remain on this identity."); $("dialog").close(); } catch (error) { $("dialog-error").textContent = error.message; } }; row.append(choose); if (profile.id !== "default") { const remove = el("button", "outline compact", "Unregister"); remove.onclick = async () => { try { await act({action:"profile-unregister", id:profile.id}); $("dialog").close(); toast("Registration removed; profile files remain untouched."); } catch(error) { $("dialog-error").textContent = error.message; } }; row.append(remove); } list.append(row); }
  const id = field("New profile ID", "id"), display = field("New display name", "display_name"), directory = field("Existing profile directory (register only)", "directory");
  modal("Profiles", [el("p", "dialog-note", "Selection affects the next foreground launch only. Existing running rooms never change identity."), list, id, display, directory], "Create or register", async data => { const id = data.get("id").trim(), directory = data.get("directory").trim(); await act(directory ? {action:"profile-register", id, directory} : {action:"profile-create", id, display_name:data.get("display_name")}); toast("Profile registered. Select it and restart to use it."); });
} catch(error) { toast(error.message); } };
$("menu").onclick = () => $("sidebar").classList.toggle("open"); $("dialog-close").onclick = () => $("dialog").close();
$("create").onclick = $("create-empty").onclick = createDialog; $("join").onclick = $("join-empty").onclick = joinDialog;
$("invite").onclick = inviteDialog; $("controllers").onclick = controllersDialog; $("address").onclick = addressDialog;
$("find-copies").onclick = findCopiesDialog; $("close-copies").onclick = () => { $("copy-panel").hidden = true; $("copies").replaceChildren(); };
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
$("composer").onsubmit = async (event) => { event.preventDefault(); const text = $("text").value; if (!text || !room()?.status) return; $("send").disabled = true;
  try { const result = await act({ action: "send", name: selected, text, sensitive: $("sensitive").checked }); $("text").value = ""; $("send-status").textContent = result.delivery.ok ? "Acknowledged by the recipient event queues, not human reading." : "Partial delivery. Inspect the peer state before retrying."; } catch (error) { toast(error.message); } finally { $("send").disabled = !room()?.status; }
};
$("text").onkeydown = (event) => { if ((event.ctrlKey || event.metaKey) && event.key === "Enter") $("composer").requestSubmit(); };
$("share-media").onclick = () => mediaControl?.shareDialog(); $("receive-media").onclick = () => mediaControl?.receiveDialog(); $("stop-media").onclick = () => mediaControl?.stopAll();
addEventListener("pagehide", () => mediaControl?.stopAll());
$("attach").onclick = () => $("file-input").click();
$("file-input").onchange = (event) => { const file = event.target.files[0]; if (!file) return; if (file.size > (current?.files.max_bytes || 0)) { toast("This file exceeds the configured upload limit."); return; }
  fileDialog("Send " + file.name, async (name, to, sensitive) => { const response = await fetch("/api/rooms/" + encodeURIComponent(name) + "/files?to=" + encodeURIComponent(to) + "&sensitive=" + sensitive, { method: "POST", headers: { Authorization: "Bearer " + token, "Content-Type": "application/octet-stream" }, body: file }); return response.json(); }); event.target.value = "";
};
(async () => {
  try { const bootstrap = launchCode;
    if (bootstrap) { const response = await fetch("/api/bootstrap", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ token: bootstrap }) }); if (!response.ok) throw new Error("This private launch link expired or was already used."); token = (await response.json()).session; sessionStorage.setItem("kk-session", token); }
    if (!token) throw new Error("Start kk ui on this device and use its private browser launch. No credential is printed in the terminal.");
    current = await api("/api/state"); $("gate").hidden = true; $("workspace").hidden = false;
    const media = await import("/media.js"); mediaControl = media.setupMedia({ token: () => token, room, memberName, memberChoice, config: () => current.media, toast, modal, el });
    render(); setInterval(refresh, 1000);
  } catch (error) { $("gate-text").textContent = error.message; }
})();
