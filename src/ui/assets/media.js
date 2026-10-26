import { Packetizer, parse, VideoFrames, dimensions } from "/rtp.js";

export function setupMedia(ui) {
  const sessions = new Set();
  const panel = document.getElementById("media-panel"), tiles = document.getElementById("media-tiles");
  function tile(label) {
    panel.hidden = false; const element = ui.el("article", "media-tile"), heading = ui.el("div", "media-heading");
    const title = ui.el("strong", "", label), status = ui.el("small", "muted", "Connecting...");
    const button = ui.el("button", "outline compact", "Stop"); heading.append(title, status, button); element.append(heading); tiles.append(element);
    return { element, status, button };
  }
  function websocket(room, kind, direction, query = "") {
    const url = new URL("/api/rooms/" + encodeURIComponent(room) + "/media/" + kind + "/" + direction + query, location.href); url.protocol = "ws:";
    const socket = new WebSocket(url, ["kk-v1", "kk-auth." + ui.token()]); socket.binaryType = "arraybuffer"; return socket;
  }
  function own(card, socket, stream) {
    let closed = false; const cleanups = [];
    const session = { stop() {
      if (closed) return; closed = true;
      for (const track of stream?.getTracks() || []) track.stop();
      for (const cleanup of cleanups) { try { cleanup(); } catch {} }
      socket?.close(); card.element.remove(); sessions.delete(session); panel.hidden = sessions.size === 0;
    }, cleanups, get closed() { return closed; } };
    sessions.add(session); card.button.onclick = session.stop; if (socket) { socket.onclose = session.stop; socket.onerror = () => { ui.toast("Media transport refused or disconnected. Check consent, roles and the quorum."); session.stop(); }; }
    for (const track of stream?.getTracks() || []) track.onended = session.stop;
    return session;
  }
  async function opened(socket) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Media setup timed out.")), 30000);
      socket.addEventListener("message", function ready(event) { if (typeof event.data !== "string") return; const message = JSON.parse(event.data); if (message.event === "ready") { clearTimeout(timer); socket.removeEventListener("message", ready); resolve(message); } });
      socket.addEventListener("close", () => { clearTimeout(timer); reject(new Error("The peer did not consent to this source.")); }, { once: true });
    });
  }
  async function publish(kind, quality) {
    const room = ui.room(); if (!room?.status) throw new Error("Start the room before publishing media.");
    const cfg = ui.config();
    // These calls run directly from the user's gesture and use native browser
    // permission dialogs, including the Wayland screen-selection portal.
    const request = kind === "screen" ? navigator.mediaDevices.getDisplayMedia({ video: { frameRate: cfg.fps }, audio: false })
      : navigator.mediaDevices.getUserMedia(kind === "audio" ? { audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1 }, video: false }
        : { video: { width: cfg.width, height: cfg.height, frameRate: cfg.fps }, audio: false });
    const stream = await request; let session;
    try {
      const card = tile("SHARING / " + kind.toUpperCase()), socket = websocket(room.name, kind, "publish"); session = own(card, socket, stream);
      const ready = await opened(socket); card.status.textContent = ready.peers.length + " accepting peers / encrypted transport";
      const packetizer = new Packetizer(kind, cfg.packet_bytes);
      const transmit = (chunk) => {
        if (session.closed || socket.readyState !== WebSocket.OPEN || socket.bufferedAmount > cfg.datagram_buffer_bytes || chunk.byteLength > cfg.frame_bytes) return false;
        for (const packet of packetizer.encode(chunk)) socket.send(packet);
        return true;
      };
      if (kind === "audio") {
        if (!window.AudioEncoder) throw new Error("This browser does not provide an Opus AudioEncoder. Use the native CLI.");
        const context = new AudioContext({ sampleRate: 48000, latencyHint: "interactive" }); session.cleanups.push(() => context.close()); await context.resume(); await context.audioWorklet.addModule("/audio-worklet.js");
        const encoder = new AudioEncoder({ output: (chunk) => transmit(chunk), error: () => session.stop() });
        const settings = { codec: "opus", sampleRate: 48000, numberOfChannels: 1, bitrate: cfg.audio_bitrate };
        if (!(await AudioEncoder.isConfigSupported(settings)).supported) throw new Error("Opus encoding is unavailable in this browser.");
        encoder.configure(settings); session.cleanups.push(() => { if (encoder.state !== "closed") encoder.close(); });
        const source = context.createMediaStreamSource(stream), worklet = new AudioWorkletNode(context, "kk-capture", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1] });
        source.connect(worklet); worklet.connect(context.destination); session.cleanups.push(() => { source.disconnect(); worklet.disconnect(); }); let frames = 0;
        worklet.port.onmessage = (event) => { const timestamp = frames * 1000000 / 48000; frames += 960; if (session.closed || encoder.state !== "configured" || encoder.encodeQueueSize > 2) return;
          const audio = new AudioData({ format: "f32", sampleRate: 48000, numberOfFrames: 960, numberOfChannels: 1, timestamp, data: event.data }); encoder.encode(audio); audio.close(); };
      } else {
        if (!window.VideoEncoder || !window.MediaStreamTrackProcessor) throw new Error("This browser lacks the native video encoding APIs. Use the CLI or a supported browser.");
        const encoder = new VideoEncoder({ output: (chunk) => { if (!transmit(chunk)) forceKey = true; }, error: () => session.stop() });
        const settings = { codec: "vp8", width: cfg.width, height: cfg.height, bitrate: quality, framerate: cfg.fps, latencyMode: "realtime" };
        if (!(await VideoEncoder.isConfigSupported(settings)).supported) throw new Error("VP8 encoding is unavailable in this browser.");
        encoder.configure(settings); session.cleanups.push(() => { if (encoder.state !== "closed") encoder.close(); });
        const canvas = new OffscreenCanvas(cfg.width, cfg.height), drawing = canvas.getContext("2d", { alpha: false });
        const preview = document.createElement("video"); preview.muted = true; preview.autoplay = true; preview.playsInline = true; preview.srcObject = stream; card.element.append(preview);
        const reader = new MediaStreamTrackProcessor({ track: stream.getVideoTracks()[0] }).readable.getReader(); session.cleanups.push(() => reader.cancel());
        let frameNumber = 0, last = -Infinity, forceKey = true;
        (async () => { try { while (!session.closed) { const { value: frame, done } = await reader.read(); if (done) break;
          if (encoder.encodeQueueSize > 2 || frame.timestamp - last < 1000000 / cfg.fps) { frame.close(); continue; }
          last = frame.timestamp; drawing.drawImage(frame, 0, 0, cfg.width, cfg.height); const scaled = new VideoFrame(canvas, { timestamp: frame.timestamp }); frame.close();
          const keyFrame = forceKey || frameNumber++ % (cfg.fps * 2) === 0; forceKey = false; encoder.encode(scaled, { keyFrame }); scaled.close();
        } } catch { if (!session.closed) ui.toast("Native capture ended."); } finally { session.stop(); } })();
      }
    } catch (error) { if (session) session.stop(); else for (const track of stream.getTracks()) track.stop(); throw error; }
  }
  async function listen(kind, from) {
    const room = ui.room(); if (!room?.status) throw new Error("Start the room before accepting media.");
    const cfg = ui.config(), card = tile(kind.toUpperCase() + " / " + from.slice(0, 8));
    let context = null, worklet = null;
    try { if (kind === "audio") {
      if (!window.AudioDecoder) { card.element.remove(); throw new Error("Opus AudioDecoder is unavailable in this browser."); }
      context = new AudioContext({ sampleRate: 48000, latencyHint: "interactive" }); await context.resume(); await context.audioWorklet.addModule("/audio-worklet.js");
      worklet = new AudioWorkletNode(context, "kk-playback", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [1] }); worklet.connect(context.destination);
    } else if (!window.VideoDecoder) { card.element.remove(); throw new Error("VP8 VideoDecoder is unavailable in this browser."); }
    } catch (error) { if (context) await context.close(); card.element.remove(); panel.hidden = sessions.size === 0; throw error; }
    const socket = websocket(room.name, kind, "listen", "?from=" + encodeURIComponent(from)), session = own(card, socket);
    if (context) session.cleanups.push(() => { worklet.disconnect(); context.close(); });
    let decoder = null, track = "", base = null, keyNeeded = true;
    let frames = new VideoFrames(cfg.frame_bytes, cfg.packet_age_ms), canvas = null, drawing = null;
    card.element.dataset.decoded = "0";
    if (kind !== "audio") { canvas = document.createElement("canvas"); canvas.width = cfg.width; canvas.height = cfg.height; drawing = canvas.getContext("2d", { alpha: false }); card.element.append(canvas); }
    function configure() {
      if (decoder && decoder.state !== "closed") decoder.close(); base = null; keyNeeded = true; frames = new VideoFrames(cfg.frame_bytes, cfg.packet_age_ms);
      if (kind === "audio") { decoder = new AudioDecoder({ output: (audio) => { if (audio.numberOfFrames > 9600) { audio.close(); return; } const samples = new Float32Array(audio.numberOfFrames); audio.copyTo(samples, { planeIndex: 0, format: "f32-planar" }); audio.close(); worklet.port.postMessage(samples.buffer, [samples.buffer]); card.element.dataset.decoded = String(Number(card.element.dataset.decoded) + 1); }, error: () => session.stop() }); decoder.configure({ codec: "opus", sampleRate: 48000, numberOfChannels: 1 }); }
      else { decoder = new VideoDecoder({ output: (frame) => { drawing.drawImage(frame, 0, 0, canvas.width, canvas.height); frame.close(); card.element.dataset.decoded = String(Number(card.element.dataset.decoded) + 1); }, error: () => { keyNeeded = true; } }); decoder.configure({ codec: "vp8", optimizeForLatency: true }); }
    }
    session.cleanups.push(() => { if (decoder && decoder.state !== "closed") decoder.close(); });
    socket.onmessage = (event) => {
      if (typeof event.data === "string") { const message = JSON.parse(event.data); card.status.textContent = message.event === "ready" ? "Consented / waiting for source" : "Stream ended"; return; }
      const bytes = new Uint8Array(event.data); if (bytes.length < 37 || bytes.length > cfg.packet_bytes + 24) return;
      const stamp = Number(new DataView(bytes.buffer).getBigUint64(0)); if (Date.now() < stamp || Date.now() - stamp > cfg.packet_age_ms) return;
      const id = Array.from(bytes.subarray(8, 24), (byte) => byte.toString(16).padStart(2, "0")).join("");
      if (id !== track) { track = id; configure(); }
      const packet = parse(bytes.subarray(24), kind); if (!packet || (kind === "audio" && decoder.state !== "configured")) return;
      if (decoder.decodeQueueSize > 2) { keyNeeded = true; return; }
      base ??= packet.timestamp; const offset = (packet.timestamp - base) >>> 0, rate = kind === "audio" ? 48000 : 90000;
      if (offset > rate * 3600) return; const timestamp = offset * 1000000 / rate;
      try {
        if (kind === "audio") decoder.decode(new EncodedAudioChunk({ type: "key", timestamp, data: packet.payload }));
        else { const dropped = frames.dropped; const frame = frames.push(packet); if (frames.dropped > dropped) keyNeeded = true; if (!frame) return;
          if (frame.key) { if (!dimensions(frame.bytes, cfg.width, cfg.height)) return; if (decoder.state !== "configured") { configure(); base = frame.timestamp; } keyNeeded = false; }
          if (keyNeeded) return; decoder.decode(new EncodedVideoChunk({ type: frame.key ? "key" : "delta", timestamp: ((frame.timestamp - base) >>> 0) * 1000000 / 90000, data: frame.bytes }));
        }
        card.status.textContent = "Encrypted / live / volatile buffers";
      } catch { keyNeeded = true; }
    };
  }
  function shareDialog() {
    if (!navigator.mediaDevices) { ui.toast("Native capture is unavailable in this browser context."); return; }
    const kind = ui.el("select"); kind.name = "kind"; for (const value of ["audio", "camera", "screen"]) { const option = ui.el("option", "", value); option.value = value; kind.append(option); }
    const quality = ui.el("select"); quality.name = "quality"; for (const [label, value] of [["Low bandwidth / 256 kbit/s", 256000], ["Balanced / 800 kbit/s", 800000], ["Detailed / 1.5 Mbit/s", 1500000]]) { const option = ui.el("option", "", label); option.value = value; option.selected = value === 800000; quality.append(option); }
    ui.modal("Share a native source", [ui.el("p", "dialog-note", "Only peers that explicitly accept this source can receive it. Browser/OS permission is still required. No media recording is enabled."), kind, quality], "Choose source and authorize capture", async (data) => publish(data.get("kind"), Number(data.get("quality"))));
  }
  function receiveDialog() {
    const room = ui.room(); if (!room?.status) return;
    const kind = ui.el("select"); kind.name = "kind"; for (const value of ["audio", "camera", "screen"]) { const option = ui.el("option", "", value); option.value = value; kind.append(option); }
    const peer = ui.el("select"); peer.name = "from"; for (const id of room.status.members.filter((id) => id !== room.status.owner)) { const option = ui.el("option", "", id.slice(0, 8) + ":" + id.slice(-8)); option.value = id; peer.append(option); }
    if (!peer.options.length) { ui.toast("Invite a sender first."); return; }
    ui.modal("Consent to a source", [ui.el("p", "dialog-note", "This opens local playback only for the chosen member and source kind. Stop withdraws consent; no camera or microphone is opened here."), peer, kind], "Accept this source", async (data) => listen(data.get("kind"), data.get("from")));
  }
  return { shareDialog, receiveDialog, stopAll() { for (const session of [...sessions]) session.stop(); } };
}
