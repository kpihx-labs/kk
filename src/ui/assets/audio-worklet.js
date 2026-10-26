// Bounded volatile PCM only. No recording, network or permission decisions.
class Capture extends AudioWorkletProcessor {
  constructor() { super(); this.buffer = new Float32Array(960); this.used = 0; }
  process(inputs, outputs) {
    for (const output of outputs) for (const channel of output) channel.fill(0);
    const input = inputs[0]; if (!input?.length) return true;
    for (let index = 0; index < input[0].length; index++) {
      let sample = 0; for (const channel of input) sample += channel[index] / input.length;
      this.buffer[this.used++] = sample;
      if (this.used === this.buffer.length) { this.port.postMessage(this.buffer.buffer, [this.buffer.buffer]); this.buffer = new Float32Array(960); this.used = 0; }
    }
    return true;
  }
}
class Playback extends AudioWorkletProcessor {
  constructor() { super(); this.queue = []; this.offset = 0; this.samples = 0; this.port.onmessage = (event) => {
    const samples = new Float32Array(event.data); if (samples.length > 9600) return;
    if (this.samples + samples.length > 12000) { this.queue = []; this.offset = 0; this.samples = 0; }
    this.queue.push(samples); this.samples += samples.length;
  }; }
  process(inputs, outputs) {
    for (const output of outputs) for (const channel of output) channel.fill(0);
    const output = outputs[0]?.[0]; if (!output) return true;
    for (let index = 0; index < output.length; index++) {
      if (!this.queue.length) break;
      output[index] = this.queue[0][this.offset++]; this.samples--;
      if (this.offset === this.queue[0].length) { this.queue.shift(); this.offset = 0; }
    }
    return true;
  }
}
registerProcessor("kk-capture", Capture); registerProcessor("kk-playback", Playback);
