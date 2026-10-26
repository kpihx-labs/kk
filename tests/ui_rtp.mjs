import test from "node:test";
import assert from "node:assert/strict";
import { Packetizer, parse, VideoFrames, dimensions } from "../src/ui/assets/rtp.js";

function chunk(bytes, timestamp = 0) { return { byteLength: bytes.length, timestamp, copyTo(target) { target.set(bytes); } }; }
function keyframe(length = 3000) {
  const bytes = new Uint8Array(length); bytes.set([0, 0, 0, 0x9d, 1, 0x2a, 0x80, 2, 0x68, 1]);
  for (let index = 10; index < bytes.length; index++) bytes[index] = index & 255;
  return bytes;
}
test("VP8 RTP fragments preserve bytes across reordered packets and sequence wrap", () => {
  const sender = new Packetizer("screen", 1024); sender.sequence = 65534;
  const bytes = keyframe(), packets = sender.encode(chunk(bytes)); assert.equal(packets.length, 3);
  assert.ok(packets.every((packet) => packet.length <= 1024));
  const frames = new VideoFrames(256 * 1024, 250); let complete;
  for (const index of [2, 0, 1]) complete = frames.push(parse(packets[index], "screen")) || complete;
  assert.deepEqual(complete.bytes, bytes); assert.equal(complete.key, true);
  assert.equal(frames.frames.size, 0); assert.equal(dimensions(complete.bytes, 640, 360), true);
});
test("Opus RTP is unfragmented and never acquires a VP8 descriptor", () => {
  const bytes = new Uint8Array(200).fill(0x5a), sender = new Packetizer("audio", 1024);
  const packets = sender.encode(chunk(bytes)); assert.equal(packets.length, 1);
  assert.deepEqual(parse(packets[0], "audio").payload, bytes);
  assert.equal(parse(packets[0], "screen"), null);
  assert.equal(sender.encode(chunk(new Uint8Array(2048))).length, 0);
});
test("incomplete video frames and malformed descriptors stay bounded", () => {
  const sender = new Packetizer("camera", 1024), frames = new VideoFrames(1000, 250);
  assert.equal(frames.push(parse(sender.encode(chunk(keyframe()))[0], "camera")), null);
  assert.ok(frames.frames.size <= 1);
  for (let index = 0; index < 50; index++) frames.push(parse(sender.encode(chunk(keyframe(), index * 66000))[0], "camera"));
  assert.ok(frames.frames.size <= 2);
  assert.equal(frames.push({ ssrc: 1, timestamp: 1, sequence: 1, marker: true, payload: new Uint8Array([0x80]) }), null);
  assert.equal(dimensions(keyframe(), 320, 200), false);
  const invalid = keyframe(); invalid[3] = 0; assert.equal(dimensions(invalid, 640, 360), false);
});
