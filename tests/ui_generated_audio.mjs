import test from "node:test";
import assert from "node:assert/strict";
import { installGeneratedAudioFixture } from "./support/generated_audio.mjs";

function fakeBrowser({ lies = false } = {}) {
  const contexts = []; let physicalCalls = 0;
  class Node { connect(target) { this.target = target; } disconnect() {} }
  class Context {
    constructor() { this.state = "suspended"; this.realDestination = new Node(); contexts.push(this); }
    get destination() { return this.realDestination; }
    async setSinkId(sink) { this.sinkId = lies ? "default" : sink; }
    async resume() { this.state = "running"; }
    async close() { this.state = "closed"; }
    createAnalyser() { return Object.assign(new Node(), { fftSize: 2048, getFloatTimeDomainData(samples) { samples.fill(0.25); } }); }
    createOscillator() { return Object.assign(new Node(), { frequency: {}, start() {}, stop() {} }); }
    createGain() { return Object.assign(new Node(), { gain: {} }); }
    createMediaStreamDestination() {
      const track = { readyState: "live", stop() { this.readyState = "ended"; } };
      return Object.assign(new Node(), { stream: { getTracks: () => [track] } });
    }
  }
  const getUserMedia = async () => { physicalCalls++; throw new Error("physical capture forbidden"); };
  const enumerateDevices = async () => [];
  const root = { AudioContext: Context, navigator: { mediaDevices: { getUserMedia, enumerateDevices } } };
  return { root, contexts, Context, getUserMedia, enumerateDevices, physicalCalls: () => physicalCalls };
}

test("generated audio fixture is inert until requested and restores exact APIs without physical fallback", async () => {
  const h = fakeBrowser(), fixture = installGeneratedAudioFixture(h.root, { frequency: 880 });
  assert.equal(h.contexts.length, 0);
  await assert.rejects(h.root.navigator.mediaDevices.getUserMedia({ audio: true, video: true }), /generated audio only/);
  await assert.rejects(h.root.navigator.mediaDevices.getUserMedia({ audio: { deviceId: { exact: "physical-id" } } }), /mismatch/);
  assert.equal(h.contexts.length, 0);
  const devices = await h.root.navigator.mediaDevices.enumerateDevices(); assert.equal(devices[0].deviceId, "kk-generated-audio");
  const stream = await h.root.navigator.mediaDevices.getUserMedia({ audio: true, video: false });
  assert.equal(fixture.snapshot().activeSources, 1);
  const playback = new h.root.AudioContext({ sampleRate: 48000 }); await playback.resume();
  assert.equal(fixture.snapshot().contexts[0].silentSink, true); assert.equal(fixture.snapshot().contexts[0].rms, 0.25);
  stream.getTracks()[0].stop(); assert.equal(fixture.snapshot().activeSources, 0);
  await fixture.restore(); await fixture.restore();
  assert.equal(h.root.AudioContext, h.Context); assert.equal(h.root.navigator.mediaDevices.getUserMedia, h.getUserMedia);
  assert.equal(h.root.navigator.mediaDevices.enumerateDevices, h.enumerateDevices);
  assert.equal(h.physicalCalls(), 0); assert.ok(h.contexts.every((context) => context.state === "closed"));
});

test("generated audio refuses missing or unconfirmed silent sinks, never audible fallback", async () => {
  const absent = fakeBrowser(); absent.root.AudioContext = class {};
  assert.throws(() => installGeneratedAudioFixture(absent.root), /silent sink/);
  assert.equal(absent.root.navigator.mediaDevices.getUserMedia, absent.getUserMedia);
  const h = fakeBrowser({ lies: true }), fixture = installGeneratedAudioFixture(h.root);
  await assert.rejects(h.root.navigator.mediaDevices.getUserMedia({ audio: true }), /did not confirm/);
  const playback = new h.root.AudioContext(); await assert.rejects(playback.resume(), /did not confirm/);
  assert.equal(fixture.snapshot().sources, 0); assert.equal(h.physicalCalls(), 0);
  await fixture.restore(); assert.ok(h.contexts.every((context) => context.state === "closed"));
});
