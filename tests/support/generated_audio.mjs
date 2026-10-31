// Browser-owner fixture only: install explicitly in an owned QA tab. Never
// forwards a request to physical capture; playback must verify a silent sink.
export function installGeneratedAudioFixture(root = globalThis, { frequency = 440, amplitude = 0.1 } = {}) {
  const Native = root.AudioContext, devices = root.navigator?.mediaDevices;
  if (!Native?.prototype.setSinkId || !devices) throw new Error("Synthetic audio requires a verifiable AudioContext silent sink; no audible fallback.");
  if (!Number.isFinite(frequency) || frequency < 80 || frequency > 2000 || !Number.isFinite(amplitude) || amplitude <= 0 || amplitude > 0.3) throw new Error("Choose a bounded synthetic tone.");
  const contexts = [], sources = [], generated = [], saved = [];
  let restored = false;
  async function silent(context) {
    await context.setSinkId({ type: "none" });
    if (context.sinkId?.type !== "none") { await context.close(); throw new Error("The browser did not confirm the silent sink. Audio qualification refused."); }
  }
  class SilentContext extends Native {
    constructor(options) {
      super({ ...options, sinkId: { type: "none" } });
      this.fixtureMeter = this.createAnalyser(); this.fixtureMeter.fftSize = 2048;
      this.fixtureMeter.connect(super.destination); contexts.push(this);
    }
    get destination() { return this.fixtureMeter ?? super.destination; }
    async resume() { await silent(this); return super.resume(); }
  }
  function replace(object, name, value) {
    saved.push([object, name, Object.getOwnPropertyDescriptor(object, name)]);
    Object.defineProperty(object, name, { value, configurable: true, writable: true });
  }
  function restoreProperties() {
    for (const [object, name, descriptor] of saved.reverse()) {
      if (descriptor) Object.defineProperty(object, name, descriptor); else delete object[name];
    }
  }
  async function getUserMedia(constraints) {
    if (restored || !constraints?.audio || constraints.video) throw new Error("Fixture permits generated audio only; physical capture is never forwarded.");
    const selected = constraints.audio?.deviceId?.exact;
    if (selected && selected !== "kk-generated-audio") throw new Error("Fixture source ID mismatch; no substitute opened.");
    const context = new Native({ sampleRate: 48000, sinkId: { type: "none" } });
    generated.push(context);
    try { await silent(context); await context.resume(); if (restored) throw new Error("Fixture was restored during source setup."); }
    catch (error) { if (context.state !== "closed") await context.close(); throw error; }
    const oscillator = context.createOscillator(), gain = context.createGain(), output = context.createMediaStreamDestination();
    oscillator.frequency.value = frequency; gain.gain.value = amplitude;
    oscillator.connect(gain); gain.connect(output); oscillator.start();
    const source = { context, stopped: false, stream: output.stream }; sources.push(source);
    const tracks = output.stream.getTracks();
    for (const track of tracks) {
      const stop = track.stop.bind(track);
      track.stop = () => {
        stop();
        if (!source.stopped) { source.stopped = true; oscillator.stop(); gain.disconnect(); void context.close().catch(() => {}); }
      };
    }
    return output.stream;
  }
  try {
    replace(root, "AudioContext", SilentContext);
    replace(devices, "getUserMedia", getUserMedia);
    replace(devices, "enumerateDevices", async () => [{ kind: "audioinput", deviceId: "kk-generated-audio", groupId: "", label: "Generated QA tone (no microphone)" }]);
  } catch (error) { restoreProperties(); throw error; }
  return {
    snapshot() {
      return { frequency, sources: sources.length, activeSources: sources.filter((source) => !source.stopped).length,
        contexts: contexts.map((context) => {
          const samples = new Float32Array(context.fixtureMeter.fftSize); context.fixtureMeter.getFloatTimeDomainData(samples);
          let squares = 0, peak = 0; for (const sample of samples) { squares += sample * sample; peak = Math.max(peak, Math.abs(sample)); }
          return { state: context.state, silentSink: context.sinkId?.type === "none", rms: Math.sqrt(squares / samples.length), peak };
        }) };
    },
    async restore() {
      if (restored) return; restored = true;
      restoreProperties();
      for (const source of sources) for (const track of source.stream.getTracks()) track.stop();
      await Promise.allSettled([...contexts, ...generated].filter((context) => context.state !== "closed").map((context) => context.close()));
    },
  };
}
