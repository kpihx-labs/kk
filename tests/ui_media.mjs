// Synthetic browser adapter evidence only: no devices, network or native platform.
import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Packetizer } from "../src/ui/assets/rtp.js";

const source = (await readFile(new URL("../src/ui/assets/media.js", import.meta.url), "utf8"))
  .replace('"/rtp.js"', JSON.stringify(new URL("../src/ui/assets/rtp.js", import.meta.url).href));
const { setupMedia } = await import("data:text/javascript;base64," + Buffer.from(source).toString("base64"));

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.dataset = {}; this.hidden = true; this.draws = 0; }
  append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  remove() { if (this.parent) this.parent.children.splice(this.parent.children.indexOf(this), 1); this.parent = null; }
  get options() { return this.children; }
  getContext() { return { drawImage: () => this.draws++ }; }
}
class Socket {
  static OPEN = 1;
  static all = [];
  constructor(url, protocols) { this.url = url; this.protocols = protocols; this.readyState = 1; this.bufferedAmount = 0; this.events = new Map(); this.sent = []; Socket.all.push(this); }
  addEventListener(name, fn) { if (!this.events.has(name)) this.events.set(name, new Set()); this.events.get(name).add(fn); }
  removeEventListener(name, fn) { this.events.get(name)?.delete(fn); }
  emit(name, data) { const event = { data }; this["on" + name]?.(event); for (const fn of [...(this.events.get(name) || [])]) fn(event); }
  close() { if (this.readyState === 3) return; this.readyState = 3; this.emit("close"); }
  send(packet) { this.sent.push(packet); }
}
class Decoder {
  static all = [];
  constructor(callbacks) { this.callbacks = callbacks; this.state = "unconfigured"; this.decodeQueueSize = 0; this.chunks = []; Decoder.all.push(this); }
  configure() { this.state = "configured"; }
  decode(chunk) { assert.equal(this.state, "configured"); this.chunks.push(chunk); this.callbacks.output({ close() {} }); }
  close() { this.state = "closed"; }
}
class Encoder {
  static all = [];
  static async isConfigSupported() { return { supported: true }; }
  constructor(callbacks) { this.callbacks = callbacks; this.state = "unconfigured"; this.encodeQueueSize = 0; Encoder.all.push(this); }
  configure() { this.state = "configured"; }
  close() { this.state = "closed"; }
}
const deferred = () => { let resolve; const promise = new Promise((done) => { resolve = done; }); return { promise, resolve }; };
const tick = async () => { await new Promise((resolve) => setImmediate(resolve)); };
function harness() {
  Socket.all = []; Decoder.all = []; Encoder.all = [];
  const panel = new Element("section"), tiles = new Element("div"), toasts = [], tracks = [], captures = [], contexts = [];
  let room = { name: "alpha", status: { owner: "local", members: ["local", "peer-a", "peer-b", "peer-c", "peer-d", "peer-e"] } }, submit;
  let permission = null, playback = null, controls = [], entries = [], enumerations = 0, surface;
  function stream() { const track = { stopped: false, getSettings: () => ({ displaySurface: surface }), stop() { this.stopped = true; } }; tracks.push(track); return { getTracks: () => [track], getVideoTracks: () => [track] }; }
  const capture = (kind, constraints) => { captures.push({ kind, constraints }); return permission ? permission.promise : Promise.resolve(stream()); };
  const globals = { document: { getElementById: (id) => id === "media-panel" ? panel : tiles, createElement: (tag) => new Element(tag) },
    location: { href: "http://localhost:8080/", protocol: "http:" }, navigator: { mediaDevices: { enumerateDevices: async () => { enumerations++; return entries; }, getSupportedConstraints: () => ({ deviceId: true }), getDisplayMedia: (constraints) => capture("screen", constraints), getUserMedia: (constraints) => capture("device", constraints) } },
    WebSocket: Socket, VideoDecoder: Decoder, VideoEncoder: Encoder, VideoFrame: class { close() {} }, EncodedVideoChunk: class { constructor(data) { Object.assign(this, data); } },
    AudioDecoder: Decoder, AudioEncoder: Encoder, AudioData: class {}, EncodedAudioChunk: class {},
    AudioContext: class { constructor() { contexts.push(this); this.audioWorklet = { addModule: async () => {} }; } resume() { return playback?.promise || Promise.resolve(); } close() { this.closed = true; } },
    AudioWorkletNode: class { connect() {} disconnect() {} },
    MediaStreamTrackProcessor: class { constructor() { const pending = deferred(); this.readable = { getReader: () => ({ read: () => pending.promise, cancel: () => pending.resolve({ done: true }) }) }; } },
    OffscreenCanvas: Element };
  const originals = new Map();
  for (const [key, value] of Object.entries(globals)) { originals.set(key, Object.getOwnPropertyDescriptor(globalThis, key)); Object.defineProperty(globalThis, key, { configurable: true, writable: true, value }); }
  originals.set("window", Object.getOwnPropertyDescriptor(globalThis, "window")); Object.defineProperty(globalThis, "window", { configurable: true, value: globalThis });
  const cfg = { max_tracks: 4, max_subscriptions: 4, max_publications: 3, max_users: 8, fps: 15, width: 640, height: 360, packet_bytes: 1024, frame_bytes: 262144, packet_age_ms: 1000, datagram_buffer_bytes: 65536 };
  const names = new Map();
  const control = setupMedia({ room: () => room, token: () => "synthetic-private-capability", config: () => cfg, memberName: (_room, id) => names.get(id) || "Name " + id, memberChoice: (_room, id) => (names.get(id) || "Name " + id) + " / " + id,
    toast: (message) => toasts.push(message), el: (tag, cls, text) => Object.assign(new Element(tag), { textContent: text }), modal: (...args) => { controls = args[1]; submit = args.at(-1); } });
  const data = (kind, from, choices = {}) => new Map([["kind", kind], ["from", from], ["quality", 800000], ...Object.entries(choices)]);
  return { panel, tiles, captures, tracks, toasts, control, cfg, stream, contexts, names,
    share(kind = "screen", choices) { control.shareDialog(); return submit(data(kind, null, choices)); },
    openShare() { control.shareDialog(); },
    submit(kind = "screen", choices) { return submit(data(kind, null, choices)); },
    field(name) { const walk = (nodes) => nodes.flatMap((node) => [node, ...walk(node.children)]); return walk(controls).find((node) => node.name === name); },
    controls: () => controls,
    devices(value) { entries = value; }, enumerations: () => enumerations,
    surface(value) { surface = value; },
    listen(peer, kind = "screen") { control.receiveDialog(); return submit(data(kind, peer)); },
    staleShare() { control.shareDialog(); const callback = submit; return () => callback(data("screen")); },
    permission() { permission = deferred(); return permission; },
    playback() { playback = deferred(); return playback; },
    switchRoom(name = "beta") { control.stopAll(); room = { ...room, name }; },
    stop(card) { card.children[0].children.at(-1).onclick(); },
    restore() { control.stopAll(); for (const [key, descriptor] of originals) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key]; } } };
}
function frame(socket, id = 1, timestamp = 0) {
  const bytes = new Uint8Array([0, 0, 0, 0x9d, 1, 0x2a, 0x80, 2, 0x68, 1]);
  const sender = socket.packetizer ??= new Packetizer("screen", 1024);
  for (const packet of sender.encode({ byteLength: bytes.length, timestamp, copyTo: (target) => target.set(bytes) })) {
    const envelope = new Uint8Array(24 + packet.length); new DataView(envelope.buffer).setBigUint64(0, BigInt(Date.now())); envelope.fill(id, 8, 24); envelope.set(packet, 24); socket.emit("message", envelope.buffer);
  }
}

test("two independent screen receivers decode while a local publication remains active", async () => {
  const h = harness();
  try {
    const publishing = h.share(); await tick();
    const publisher = Socket.all[0]; publisher.emit("message", JSON.stringify({ event: "ready", peers: ["peer-a", "peer-b"] })); await publishing;
    await h.listen("peer-a"); await h.listen("peer-b");
    const [local, a, b] = h.tiles.children; const [, first, second] = Socket.all;
    assert.equal(h.captures.length, 1); assert.equal(Socket.all.length, 3);
    assert.equal(first.url.searchParams.get("from"), "peer-a"); assert.equal(second.url.searchParams.get("from"), "peer-b");
    assert.equal(a.children[0].children[0].textContent, "SCREEN / Name peer-a");
    h.names.set("peer-a", "Local nickname"); h.control.refreshLabels();
    assert.equal(a.children[0].children[0].textContent, "SCREEN / Local nickname");
    assert.equal(b.children[0].children[0].textContent, "SCREEN / Name peer-b");
    assert.deepEqual(first.protocols, ["kk-v1", "kk-auth.synthetic-private-capability"]);
    frame(first); frame(second, 2);
    assert.equal(a.dataset.decoded, "1"); assert.equal(b.dataset.decoded, "1"); assert.equal(Decoder.all.length, 2);
    h.stop(a); assert.equal(first.readyState, 3); assert.equal(Decoder.all[0].state, "closed");
    assert.equal(second.readyState, 1); assert.equal(publisher.readyState, 1); assert.equal(h.tracks[0].stopped, false);
    Encoder.all[0].callbacks.output({ byteLength: 10, timestamp: 0, copyTo: (target) => target.fill(0) });
    assert.equal(publisher.sent.length, 1);
    frame(first); frame(second, 2, 66000); assert.equal(a.dataset.decoded, "1"); assert.equal(b.dataset.decoded, "2");
    assert.deepEqual(h.tiles.children, [local, b]);
    h.switchRoom(); assert.equal(h.tiles.children.length, 0); assert.equal(h.panel.hidden, true);
    assert.ok(Socket.all.every((socket) => socket.readyState === 3)); assert.equal(h.tracks[0].stopped, true);
    frame(second, 2, 132000); assert.equal(b.dataset.decoded, "2");
  } finally { h.restore(); }
});

test("duplicate actions reserve before capture and before asynchronous playback setup", async () => {
  const h = harness();
  try {
    const permission = h.permission(), pending = h.share();
    await h.share(); assert.equal(h.captures.length, 1); assert.equal(h.tiles.children.length, 1);
    await h.listen("peer-a"); await h.listen("peer-a");
    assert.equal(Socket.all.length, 1); assert.equal(h.tiles.children.length, 2); assert.equal(h.toasts.length, 2);
    h.stop(h.tiles.children[0]); const late = h.stream(); permission.resolve(late); await pending;
    assert.equal(h.tracks[0].stopped, true); assert.equal(Socket.all.length, 1);
    assert.equal(h.tiles.children[0].dataset.peer, "peer-a");
  } finally { h.restore(); }
});

test("room switching cancels pending permissions and rejects stale consent dialogs", async () => {
  const h = harness();
  try {
    const oldDialog = h.staleShare(), permission = h.permission(), pending = h.share();
    h.switchRoom(); await assert.rejects(oldDialog(), /room changed/);
    permission.resolve(h.stream()); await pending;
    assert.equal(h.tracks[0].stopped, true); assert.equal(Socket.all.length, 0); assert.equal(h.tiles.children.length, 0);
    await h.listen("peer-b"); assert.equal(Socket.all[0].url.pathname, "/api/rooms/beta/media/screen/listen");
  } finally { h.restore(); }
});

test("capacity feedback precedes device requests and stops release slots", async () => {
  const h = harness();
  try {
    for (const peer of ["peer-a", "peer-b", "peer-c", "peer-d"]) await h.listen(peer);
    await assert.rejects(h.listen("peer-e"), /capacity reached/); assert.equal(Socket.all.length, 4);
    h.cfg.max_users = 4; await assert.rejects(h.share(), /capacity reached/); assert.equal(h.captures.length, 0);
    h.stop(h.tiles.children[0]); await h.listen("peer-e"); assert.equal(h.tiles.children.length, 4);
    h.control.stopAll(); h.cfg.max_publications = 1;
    const permission = h.permission(), pending = h.share();
    await assert.rejects(h.share("camera"), /capacity reached/); assert.equal(h.captures.length, 1);
    h.control.stopAll(); permission.resolve(h.stream()); await pending;
  } finally { h.restore(); }
});

test("unsupported capture APIs and invalid source kinds fail without requesting devices", async () => {
  const h = harness();
  try {
    globalThis.VideoEncoder = undefined;
    await assert.rejects(h.share(), /required media encoding APIs/); assert.equal(h.captures.length, 0);
    globalThis.VideoEncoder = Encoder; navigator.mediaDevices.getDisplayMedia = undefined;
    await assert.rejects(h.share(), /unavailable/); await assert.rejects(h.share("remote"), /Unknown media source/);
    globalThis.VideoDecoder = undefined; await assert.rejects(h.listen("peer-a"), /unavailable/);
    assert.equal(h.captures.length, 0); assert.equal(Socket.all.length, 0); assert.equal(h.tiles.children.length, 0); assert.equal(h.panel.hidden, true);
  } finally { h.restore(); }
});

test("duplicate audio consent is suppressed during suspended playback and room cleanup closes it", async () => {
  const h = harness();
  try {
    const playback = h.playback(), pending = h.listen("peer-a", "audio");
    await h.listen("peer-a", "audio"); assert.equal(h.contexts.length, 1); assert.equal(Socket.all.length, 0);
    h.switchRoom(); assert.equal(h.contexts[0].closed, true); playback.resolve(); await pending;
    assert.equal(Socket.all.length, 0); assert.equal(h.captures.length, 0); assert.equal(h.tiles.children.length, 0);
  } finally { h.restore(); }
});

test("stop-all invalidates an old dialog even when the same room is selected again", async () => {
  const h = harness();
  try {
    const oldDialog = h.staleShare(); h.switchRoom("alpha");
    await assert.rejects(oldDialog(), /room changed/); assert.equal(h.captures.length, 0);
    delete h.cfg.max_users; await assert.rejects(h.listen("peer-a"), /capacity is unavailable/);
    assert.equal(Socket.all.length, 0);
  } finally { h.restore(); }
});

test("replacing a source track closes its decoder and ignores delayed frames from that track", async () => {
  const h = harness();
  try {
    await h.listen("peer-a"); const socket = Socket.all[0], card = h.tiles.children[0];
    frame(socket, 1); const old = Decoder.all[0]; frame(socket, 2, 66000);
    assert.equal(old.state, "closed"); assert.equal(card.dataset.decoded, "2");
    let closed = false; old.callbacks.output({ close() { closed = true; } });
    assert.equal(closed, true); assert.equal(card.dataset.decoded, "2");
    h.stop(card); Decoder.all[1].callbacks.output({ close() {} }); assert.equal(card.dataset.decoded, "2");
  } finally { h.restore(); }
});

test("opening source controls only enumerates bounded devices and never prompts", async () => {
  const h = harness();
  try {
    h.devices(Array.from({ length: 80 }, (_, index) => ({ kind: "videoinput", deviceId: "camera-" + index, label: "" })));
    h.openShare(); await tick();
    const kind = h.field("kind"); kind.value = "camera"; kind.onchange();
    assert.equal(h.enumerations(), 1); assert.equal(h.captures.length, 0); assert.equal(Socket.all.length, 0);
    assert.equal(h.field("device").options.length, 65); assert.equal(h.field("device").options[0].value, "");
    assert.match(h.field("device").options[1].textContent, /Unlabelled camera/);
    assert.deepEqual(h.field("surface").options.map((option) => option.value), ["any", "monitor", "window", "browser"]);
  } finally { h.restore(); }
});

test("display choices send only nonmandatory surface hints and report actual surface", async () => {
  for (const preference of ["any", "monitor", "window", "browser"]) {
    const h = harness();
    try {
      h.surface("window"); const pending = h.share("screen", { surface: preference }); await tick();
      assert.deepEqual(h.captures[0].constraints, { video: { frameRate: 15, ...(preference === "any" ? {} : { displaySurface: preference }) }, audio: false });
      assert.match(h.tiles.children[0].children[1].textContent, /Captured source: Window\/application window/);
      const socket = Socket.all[0]; socket.emit("message", JSON.stringify({ event: "ready", peers: [] })); await pending;
      assert.equal(socket.url.search, ""); assert.equal(socket.sent.length, 0);
      assert.equal(h.tracks[0].stopped, false);
    } finally { h.restore(); }
  }
});

test("unreported display surface stays neutral and invalid preferences fail before capture", async () => {
  const h = harness();
  try {
    await assert.rejects(h.share("screen", { surface: "application-all-windows" }), /listed display source/);
    assert.equal(h.captures.length, 0);
    const pending = h.share("screen", { surface: "monitor" }); await tick();
    assert.equal(h.tiles.children[0].children[1].textContent, "Captured source: Not reported by browser");
    h.control.stopAll(); await assert.rejects(pending, /stopped or refused/);
  } finally { h.restore(); }
});

test("explicit listed camera IDs are local exact constraints and labels refresh after grant", async () => {
  const h = harness();
  try {
    h.devices([{ kind: "videoinput", deviceId: "origin-camera", label: "" }]); h.openShare(); await tick();
    h.devices([{ kind: "videoinput", deviceId: "origin-camera", label: "Granted camera" }]);
    const pending = h.submit("camera", { device: "origin-camera" }); await tick();
    assert.deepEqual(h.captures[0].constraints.video.deviceId, { exact: "origin-camera" });
    assert.equal(h.enumerations(), 2);
    h.field("kind").value = "camera"; h.field("kind").onchange();
    assert.equal(h.field("device").options[1].textContent, "Granted camera");
    const socket = Socket.all[0]; assert.ok(!socket.url.href.includes("origin-camera"));
    socket.emit("message", JSON.stringify({ event: "ready", peers: [] })); await pending;
    Encoder.all[0].callbacks.output({ byteLength: 10, timestamp: 0, copyTo: (target) => target.fill(0) });
    assert.ok(socket.sent.every((packet) => !new TextDecoder().decode(packet).includes("origin-camera")));
  } finally { h.restore(); }
});

test("microphone choice is exact only when selected; default capture has no device ID", async () => {
  const h = harness();
  try {
    h.devices([{ kind: "audioinput", deviceId: "origin-mic", label: "Microphone" }]); h.openShare(); await tick();
    const permission = h.permission(), pending = h.submit("audio", { device: "origin-mic" });
    assert.deepEqual(h.captures[0].constraints, { audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1, deviceId: { exact: "origin-mic" } }, video: false });
    h.control.stopAll(); permission.resolve(h.stream()); await pending;
    const defaultPending = h.share("camera"); await tick();
    assert.equal(Object.hasOwn(h.captures[1].constraints.video, "deviceId"), false);
    h.control.stopAll(); await assert.rejects(defaultPending, /stopped or refused/);
  } finally { h.restore(); }
});

test("wrong-kind, stale and unsupported device choices never trigger a prompt", async () => {
  const h = harness();
  try {
    h.devices([{ kind: "audioinput", deviceId: "mic-only", label: "Microphone" }]); h.openShare(); await tick();
    await assert.rejects(h.submit("camera", { device: "mic-only" }), /no longer listed/);
    await assert.rejects(h.submit("audio", { device: "old-origin-id" }), /no longer listed/);
    navigator.mediaDevices.getSupportedConstraints = () => ({});
    await assert.rejects(h.submit("audio", { device: "mic-only" }), /cannot select/);
    assert.equal(h.captures.length, 0); assert.equal(h.tiles.children.length, 0);
    h.devices([]); h.openShare(); await tick();
    await assert.rejects(h.submit("audio", { device: "mic-only" }), /no longer listed/);
  } finally { h.restore(); }
});

test("unavailable enumeration retains Default and device errors fail without substitution", async () => {
  const h = harness();
  try {
    navigator.mediaDevices.enumerateDevices = undefined; h.openShare();
    assert.equal(h.field("device").options.length, 1); assert.equal(h.captures.length, 0);
    navigator.mediaDevices.getUserMedia = async () => { throw Object.assign(new Error("missing"), { name: "NotFoundError" }); };
    await assert.rejects(h.submit("camera"), /no substitute was opened/);
    navigator.mediaDevices.getUserMedia = async () => { throw Object.assign(new Error("denied"), { name: "NotAllowedError" }); };
    await assert.rejects(h.submit("camera"), /permission was denied/);
    assert.equal(Socket.all.length, 0); assert.equal(h.tiles.children.length, 0);
  } finally { h.restore(); }
});
