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
  let permission = null, playback = null;
  function stream() { const track = { stopped: false, stop() { this.stopped = true; } }; tracks.push(track); return { getTracks: () => [track], getVideoTracks: () => [track] }; }
  const capture = (kind) => { captures.push(kind); return permission ? permission.promise : Promise.resolve(stream()); };
  const globals = { document: { getElementById: (id) => id === "media-panel" ? panel : tiles, createElement: (tag) => new Element(tag) },
    location: { href: "http://localhost:8080/", protocol: "http:" }, navigator: { mediaDevices: { getDisplayMedia: () => capture("screen"), getUserMedia: () => capture("device") } },
    WebSocket: Socket, VideoDecoder: Decoder, VideoEncoder: Encoder, VideoFrame: class { close() {} }, EncodedVideoChunk: class { constructor(data) { Object.assign(this, data); } },
    AudioDecoder: Decoder, EncodedAudioChunk: class {},
    AudioContext: class { constructor() { contexts.push(this); this.audioWorklet = { addModule: async () => {} }; } resume() { return playback?.promise || Promise.resolve(); } close() { this.closed = true; } },
    AudioWorkletNode: class { connect() {} disconnect() {} },
    MediaStreamTrackProcessor: class { constructor() { const pending = deferred(); this.readable = { getReader: () => ({ read: () => pending.promise, cancel: () => pending.resolve({ done: true }) }) }; } },
    OffscreenCanvas: Element };
  const originals = new Map();
  for (const [key, value] of Object.entries(globals)) { originals.set(key, Object.getOwnPropertyDescriptor(globalThis, key)); Object.defineProperty(globalThis, key, { configurable: true, writable: true, value }); }
  originals.set("window", Object.getOwnPropertyDescriptor(globalThis, "window")); Object.defineProperty(globalThis, "window", { configurable: true, value: globalThis });
  const cfg = { max_tracks: 4, max_subscriptions: 4, max_users: 8, fps: 15, width: 640, height: 360, packet_bytes: 1024, frame_bytes: 262144, packet_age_ms: 1000, datagram_buffer_bytes: 65536 };
  const names = new Map();
  const control = setupMedia({ room: () => room, token: () => "synthetic-private-capability", config: () => cfg, memberName: (_room, id) => names.get(id) || "Name " + id, memberChoice: (_room, id) => (names.get(id) || "Name " + id) + " / " + id,
    toast: (message) => toasts.push(message), el: (tag, cls, text) => Object.assign(new Element(tag), { textContent: text }), modal: (...args) => { submit = args.at(-1); } });
  const data = (kind, from) => new Map([["kind", kind], ["from", from], ["quality", 800000]]);
  return { panel, tiles, captures, tracks, toasts, control, cfg, stream, contexts, names,
    share(kind = "screen") { control.shareDialog(); return submit(data(kind)); },
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
    h.control.stopAll(); h.cfg.max_tracks = 1;
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
