// RFC3550 RTP and RFC7741 VP8 only. Identity, consent and routing stay in Rust.
export class Packetizer {
  constructor(kind, maximum) { this.kind = kind; this.maximum = maximum; const random = crypto.getRandomValues(new Uint32Array(2)); this.ssrc = random[0]; this.base = random[1]; this.sequence = random[1] & 65535; this.origin = null; }
  encode(chunk) {
    this.origin ??= chunk.timestamp;
    const timestamp = (this.base + Math.round((chunk.timestamp - this.origin) * (this.kind === "audio" ? 48000 : 90000) / 1000000)) >>> 0;
    const encoded = new Uint8Array(chunk.byteLength); chunk.copyTo(encoded);
    const video = this.kind !== "audio", payload = this.maximum - 12 - (video ? 1 : 0), packets = [];
    if (!video && encoded.length > payload) return packets;
    for (let offset = 0; offset < encoded.length; offset += payload) {
      const count = Math.min(payload, encoded.length - offset), last = offset + count === encoded.length;
      const bytes = new Uint8Array(12 + (video ? 1 : 0) + count), view = new DataView(bytes.buffer);
      bytes[0] = 0x80; bytes[1] = (video ? 97 : 96) | (video && last ? 0x80 : 0);
      view.setUint16(2, this.sequence); this.sequence = (this.sequence + 1) & 65535;
      view.setUint32(4, timestamp); view.setUint32(8, this.ssrc);
      if (video) bytes[12] = offset === 0 ? 0x10 : 0;
      bytes.set(encoded.subarray(offset, offset + count), video ? 13 : 12); packets.push(bytes);
    }
    return packets;
  }
}
export function parse(bytes, kind) {
  if (bytes.length < 13 || bytes[0] >>> 6 !== 2 || (bytes[1] & 127) !== (kind === "audio" ? 96 : 97)) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let header = 12 + (bytes[0] & 15) * 4;
  if (bytes[0] & 16) { if (header + 4 > bytes.length) return null; header += 4 + view.getUint16(header + 2) * 4; }
  const padding = bytes[0] & 32 ? bytes[bytes.length - 1] : 0;
  if (header >= bytes.length || padding > bytes.length - header || (bytes[0] & 32 && padding === 0)) return null;
  return { timestamp: view.getUint32(4), sequence: view.getUint16(2), ssrc: view.getUint32(8), marker: Boolean(bytes[1] & 128), payload: bytes.subarray(header, bytes.length - padding) };
}
// Unwrap at decoded-frame boundaries, not packet boundaries: VP8 fragments can
// arrive out of order. A fixed initial subtraction would stall hour-long calls
// and eventually wrap back to zero. Old/duplicate frames never rewind playback.
export class RtpClock {
  constructor(rate) { this.rate = rate; this.last = null; this.ticks = 0; }
  next(timestamp) {
    if (this.last === null) { this.last = timestamp; return 0; }
    const delta = (timestamp - this.last) | 0;
    if (delta <= 0) return null;
    this.ticks += delta; this.last = timestamp;
    return Math.round(this.ticks * 1000000 / this.rate);
  }
}
export class VideoFrames {
  constructor(maximum, age) { this.frames = new Map(); this.maximum = maximum; this.age = age; this.dropped = 0; }
  push(packet) {
    const payload = packet.payload; if (!payload.length) return null;
    let index = 1; const descriptor = payload[0];
    if (descriptor & 128) {
      if (index >= payload.length) return null; const extensions = payload[index++];
      if (extensions & 128) { if (index >= payload.length) return null; const picture = payload[index++]; if (picture & 128) index++; }
      if (extensions & 64) index++; if (extensions & 48) index++;
    }
    if (index >= payload.length) return null;
    const now = performance.now(), key = packet.ssrc + ":" + packet.timestamp;
    for (const [id, frame] of this.frames) if (now - frame.created > this.age) { this.frames.delete(id); this.dropped++; }
    if (!this.frames.has(key)) {
      if (this.frames.size >= 2) { this.frames.delete(this.frames.keys().next().value); this.dropped++; }
      this.frames.set(key, { created: now, parts: new Map(), start: null, end: null, bytes: 0 });
    }
    const frame = this.frames.get(key), part = payload.subarray(index);
    if (!frame.parts.has(packet.sequence)) { frame.parts.set(packet.sequence, part.slice()); frame.bytes += part.length; }
    if (frame.bytes > this.maximum || frame.parts.size > 512) { this.frames.delete(key); this.dropped++; return null; }
    if ((descriptor & 16) && (descriptor & 7) === 0) frame.start = packet.sequence;
    if (packet.marker) frame.end = packet.sequence;
    if (frame.start === null || frame.end === null) return null;
    const count = ((frame.end - frame.start) & 65535) + 1; if (count > 512 || count !== frame.parts.size) return null;
    const parts = []; let length = 0;
    for (let index = 0; index < count; index++) { const part = frame.parts.get((frame.start + index) & 65535); if (!part) return null; parts.push(part); length += part.length; }
    const bytes = new Uint8Array(length); let offset = 0; for (const part of parts) { bytes.set(part, offset); offset += part.length; }
    this.frames.delete(key); return { timestamp: packet.timestamp, bytes, key: !(bytes[0] & 1) };
  }
}
export function dimensions(bytes, maximumWidth, maximumHeight) {
  if (bytes.length < 10 || bytes[3] !== 0x9d || bytes[4] !== 1 || bytes[5] !== 0x2a) return false;
  const width = (bytes[6] | bytes[7] << 8) & 16383, height = (bytes[8] | bytes[9] << 8) & 16383;
  return width > 0 && height > 0 && width <= maximumWidth && height <= maximumHeight;
}
