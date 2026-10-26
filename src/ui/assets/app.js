"use strict";
const launchCode = location.hash.slice(1); history.replaceState(null, "", location.pathname);
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => { const node = document.createElement(tag); if (cls) node.className = cls; if (text !== undefined) node.textContent = text; return node; };
let token = sessionStorage.getItem("kk-session") || "";
let current = null, selected = sessionStorage.getItem("kk-room") || "", refreshing = false, mediaControl = null;
let shownRoom = "", rendered = new Map(), revealed = new Map(), toastTimer;
const dismissedOperations = new Set();
const brief = (fp) => fp ? fp.slice(0, 8) + ":" + fp.slice(-8) : "";
function toast(text) { $("toast").textContent = text; $("toast").hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => { $("toast").hidden = true; }, 5500); }
async function api(path, body) {
  const headers = { Authorization: "Bearer " + token };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, { method: body === undefined ? "GET" : "POST", headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store" });
  if (response.status === 401 || response.status === 403) {
    token = ""; sessionStorage.removeItem("kk-session"); mediaControl?.stopAll();
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
function select(name) { if (selected !== name) { mediaControl?.stopAll(); $("copy-panel").hidden = true; $("copies").replaceChildren(); } selected = name; sessionStorage.setItem("kk-room", name); $("sidebar").classList.remove("open"); render(); }
function field(label, name, type = "text", value = "", note) {
  const wrapper = el("label", "field", label); const input = el(type === "textarea" ? "textarea" : "input"); input.name = name;
  if (type !== "textarea") input.type = type; if (value) input.value = value;
  wrapper.append(input); if (note) wrapper.append(el("small", "muted", note)); return wrapper;
}
function roleField() { const wrapper = el("label", "field", "Accepted data role"); const input = el("select"); input.name = "role"; for (const role of ["participant", "reader", "inviter", "moderator"]) { const option = el("option", "", role); option.value = role; input.append(option); } wrapper.append(input); return wrapper; }
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
  const bind = field("Local listen IP:port", "bind", "text", "", "Use an address reachable by the devices you invite."); bind.querySelector("input").placeholder = "192.168.1.10:45462"; bind.querySelector("input").required = true;
  modal("Create a room", [name, title, bind], "Create and start", async (data) => { const name = data.get("name").trim(); await act({ action: "create", name, title: data.get("title"), bind: data.get("bind").trim() }); selected = name; await act({ action: "serve", name, accept_files: false }); });
}
function joinDialog() {
  if (!current?.identity) { toast("Initialize this device first."); return; }
  const name = field("Local room name", "name"); name.querySelector("input").required = true;
  const bind = field("This device's reachable IP:port", "bind"); bind.querySelector("input").required = true;
  const invitation = field("Private invitation JSON", "invitation", "textarea", "", "This single-use capability stays in the local ceremony, not chat history."); invitation.querySelector("textarea").required = true;
  const upload = field("Or choose an invitation file", "invitation-file", "file"); upload.querySelector("input").accept = "application/json,.json";
  upload.querySelector("input").onchange = async (event) => { const file = event.target.files[0]; if (file && file.size <= 4096) invitation.querySelector("textarea").value = await file.text(); else toast("Invitation files are limited to 4 KiB."); };
  modal("Join a room", [name, bind, roleField(), upload, invitation], "Start secure pairing", async (data) => { const name = data.get("name").trim(); await act({ action: "join", name, bind: data.get("bind").trim(), role: data.get("role"), invitation: data.get("invitation"), accept_files: false }); selected = name; });
}
function inviteDialog() {
  const target = room(); if (!target?.status) return;
  const bind = field("Pairing listen IP:port", "bind", "text", "", "Use a different port from this room's data port."); bind.querySelector("input").required = true;
  try { const url = new URL("http://" + target.status.address); url.port = String(Number(url.port) < 65535 ? Number(url.port) + 1 : Number(url.port) - 1); bind.querySelector("input").value = url.host; } catch {}
  modal("Invite a member", [bind, roleField(), el("p", "dialog-note", "The other device must choose the same accepted role. Compare CONFIRM on both devices before approving.")], "Create single-use invitation", async (data) => { await act({ action: "invite", name: target.name, bind: data.get("bind").trim(), role: data.get("role") }); });
}
function controllersDialog() {
  const target = room(); if (!target?.status) return;
  const list = el("div"); for (const peer of target.status.members) { const row = el("label", "field"); const input = el("input"); input.type = "checkbox"; input.name = "peers"; input.value = peer; input.checked = target.status.voters.includes(peer); row.append(input, el("code", "", brief(peer))); list.append(row); }
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
  for (const peer of target.status.members.filter((peer) => peer !== target.status.owner)) { const option = el("option", "", brief(peer)); option.value = peer; peers.append(option); }
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
  for (const peer of target.status.members.filter(peer => peer !== target.status.owner)) { const row = el("label", "field"), checkbox = el("input"); checkbox.type = "checkbox"; checkbox.name = "peers"; checkbox.value = peer; row.append(checkbox, el("code", "", brief(peer))); list.append(row); }
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
    $("copies").replaceChildren(...report.advertised.map(hint => { const card = el("div", "file-card"); card.append(el("code", "", brief(hint.peer)), el("span", "", Math.ceil(hint.bytes/1024) + " KiB / advertised")); const fetch = el("button", "outline compact", "Fetch and verify"); fetch.onclick = async () => { fetch.disabled = true; try { await copyRequest(name, {operation:"fetch", sha256, peer:hint.peer}); toast("Verified durable copy stored in " + name + "."); await loadFiles(); } catch(error) { toast(error.message); } finally { fetch.disabled = false; } }; card.append(fetch); return card; }));
    $("copy-panel").hidden = false;
  });
}
function render() {
  if (!current) return;
  $("version").textContent = current.version; $("identity").textContent = brief(current.identity); $("identity").title = current.identity || "Not initialized";
  $("welcome").hidden = Boolean(current.identity); $("create").disabled = !current.identity; $("join").disabled = !current.identity;
  if (!selected && current.rooms.length) selected = current.rooms[0].name;
  $("rooms").replaceChildren(...current.rooms.map((entry) => { const button = el("button", "room-link" + (entry.name === selected ? " selected" : "")); const text = el("span"); text.append(el("strong", "", entry.title || entry.name), el("small", "", entry.status ? "foreground / online" : entry.problem ? "recovery required" : "offline")); button.append(el("span", "room-icon", "#"), text); button.onclick = () => select(entry.name); return button; }));
  const target = room(); $("room-view").hidden = !current.identity || !target; $("choose-room").hidden = !current.identity || Boolean(target);
  if (target) {
    $("room-title").textContent = target.title || target.name; $("room-id").textContent = target.id || target.problem || "";
    $("room-state").textContent = target.status ? "FOREGROUND / ONLINE" : (current.phases[target.name]?.state || "OFFLINE").toUpperCase();
    $("start").hidden = Boolean(target.status); $("stop").hidden = !target.status; $("invite").disabled = !target.status; $("controllers").disabled = !target.status; $("find-copies").disabled = !target.status; $("share-media").disabled = !target.status || !mediaControl; $("receive-media").disabled = !target.status || !mediaControl;
    $("text").disabled = !target.status; $("send").disabled = !target.status; $("attach").disabled = !target.status; $("allow-files").disabled = Boolean(target.status);
    $("quorum").textContent = target.status ? target.status.members.length + " MEMBERS / " + target.status.voters.length + " VOTERS / EPOCH " + target.status.epoch : "PERSISTENT KEYS / NO BACKGROUND SERVICE";
    $("peers").replaceChildren(...(target.status?.members || []).map((peer) => { const card = el("div", "peer"); const label = el("span"); label.append(el("code", "", brief(peer)), el("small", "", (peer === target.status.owner ? "This device / " : "") + (target.status.voters.includes(peer) ? "Voting controller" : "Data member"))); card.title = peer; card.append(el("span", "peer-avatar", peer.slice(0, 2).toUpperCase()), label);
      if (peer !== target.status.owner) { const remove = el("button", "peer-remove", "x"); remove.title = "Remove data membership"; remove.onclick = () => modal("Remove data membership?", [el("p", "dialog-note", "Retire this peer's vote separately if necessary. Data removal rotates the private group epoch."), el("code", "fingerprint", peer)], "Remove data access", async () => { await act({ action: "remove", name: target.name, peer }); }); card.append(remove); } return card; }));
    if (shownRoom !== selected) { shownRoom = selected; rendered.clear(); revealed.clear(); $("messages").replaceChildren(); $("files").replaceChildren(); }
    const messages = current.messages.filter((message) => message.room === selected);
    if (!messages.length && !$("messages").querySelector(".empty-message")) { const empty = el("div", "empty-message"); empty.append(el("span", "line-art"), el("h2", "", "A direct conversation."), el("p", "", "Messages are kept only in this workspace's bounded memory.")); $("messages").append(empty); }
    for (const message of messages) {
      if (rendered.has(message.id)) continue;
      $("messages").querySelector(".empty-message")?.remove(); const item = el("article", "message" + (message.outgoing ? " outgoing" : ""));
      item.append(el("div", "message-meta", (message.outgoing ? "YOU" : brief(message.sender)) + (message.sensitive ? " / SENSITIVE" : " / ENCRYPTED")));
      const body = el("div", "message-body" + (message.sensitive ? " sensitive" : ""));
      if (message.sensitive) { body.append(el("span", "", "Hidden on this device / " + message.bytes + " bytes")); const reveal = el("button", "", "Reveal"); reveal.onclick = async () => { try { const value = await act({ action: "reveal", id: message.id }); body.textContent = value.text; revealed.set(message.id, true); } catch (error) { toast(error.message); } }; body.append(reveal); }
      else body.textContent = message.text; item.append(body); rendered.set(message.id, item); $("messages").append(item); $("messages").scrollTop = $("messages").scrollHeight;
    }
    const retained = new Set(messages.map((message) => message.id)); for (const [id, node] of rendered) if (!retained.has(id)) { node.remove(); rendered.delete(id); revealed.delete(id); }
  }
  const operationIds = new Set(current.operations.map((operation) => operation.id));
  for (const id of dismissedOperations) if (!operationIds.has(id)) dismissedOperations.delete(id);
  $("operations").replaceChildren(...current.operations.filter((operation) => !dismissedOperations.has(operation.id) && !["complete", "stopped", "joined"].includes(operation.state)).map((operation) => {
    const card = el("section", "operation"); card.append(el("h3", "", operation.room + " / " + operation.state));
    if (operation.error) card.append(el("p", "error", operation.error));
    if (["failed", "declined"].includes(operation.state)) { const dismiss = el("button", "outline compact", "Dismiss"); dismiss.onclick = () => { dismissedOperations.add(operation.id); render(); }; card.append(dismiss); }
    if (operation.proposal) { card.append(el("p", "", "Compare this CONFIRM value on both devices."), el("div", "sas", operation.proposal.confirmation), el("code", "", operation.proposal.fingerprint)); const actions = el("div", "operation-actions"); for (const accept of [true, false]) { const button = el("button", accept ? "primary compact" : "outline compact", accept ? "Matches / approve" : "Reject"); button.onclick = async () => { try { await act({ action: "confirm", id: operation.id, accept }); } catch (error) { toast(error.message); } }; actions.append(button); } card.append(actions); }
    else if (operation.invitation_ready && !["failed", "working", "declined"].includes(operation.state)) { card.append(el("p", "", "Deliver the private single-use invitation to the intended device. It does not grant a vote.")); const button = el("button", "outline compact", "Download invitation"); button.onclick = async () => { try { const response = await fetch("/api/operations/" + operation.id + "/invitation", { headers: { Authorization: "Bearer " + token } }); if (!response.ok) throw new Error("Invitation is unavailable or expired."); const url = URL.createObjectURL(await response.blob()); const anchor = el("a"); anchor.href = url; anchor.download = "kk-invitation.json"; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); } catch (error) { toast(error.message); } }; card.append(button); }
    return card;
  }));
}
async function refresh() { if (!token || refreshing) return; refreshing = true; try { current = await api("/api/state"); $("connection").textContent = "LOCAL / CONNECTED"; render(); await loadFiles(); } catch (error) { $("connection").textContent = "LOCAL / UNAVAILABLE"; mediaControl?.stopAll(); } finally { refreshing = false; } }
function setTheme(theme) { document.documentElement.dataset.theme = theme; localStorage.setItem("kk-theme", theme); $("theme").textContent = theme === "dark" ? "Light" : "Dark"; }
$("theme").onclick = () => setTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
setTheme(localStorage.getItem("kk-theme") || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"));
$("menu").onclick = () => $("sidebar").classList.toggle("open"); $("dialog-close").onclick = () => $("dialog").close();
$("create").onclick = $("create-empty").onclick = createDialog; $("join").onclick = $("join-empty").onclick = joinDialog;
$("invite").onclick = inviteDialog; $("controllers").onclick = controllersDialog; $("address").onclick = addressDialog;
$("find-copies").onclick = findCopiesDialog; $("close-copies").onclick = () => { $("copy-panel").hidden = true; $("copies").replaceChildren(); };
$("initialize").onclick = async () => { try { await act({ action: "init" }); } catch (error) { toast(error.message); } };
$("start").onclick = async () => { try { await act({ action: "serve", name: selected, accept_files: $("allow-files").checked }); } catch (error) { toast(error.message); } };
$("stop").onclick = async () => { try { await act({ action: "stop", name: selected }); } catch (error) { toast(error.message); } };
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
    const media = await import("/media.js"); mediaControl = media.setupMedia({ token: () => token, room, config: () => current.media, toast, modal, el });
    render(); setInterval(refresh, 1000);
  } catch (error) { $("gate-text").textContent = error.message; }
})();
