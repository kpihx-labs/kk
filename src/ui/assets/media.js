import { Packetizer, parse, VideoFrames, dimensions } from "/rtp.js";

export function setupMedia(ui) {
  const sessions = new Map();
  let generation = 0;
  const panel = document.getElementById("media-panel"), tiles = document.getElementById("media-tiles");
  function tile(label) {
    panel.hidden = false; const element = ui.el("article", "media-tile"), heading = ui.el("div", "media-heading");
    const title = ui.el("strong", "", label), status = ui.el("small", "muted", "Connecting...");
    const button = ui.el("button", "outline compact", "Stop"); heading.append(title, status, button); element.append(heading); tiles.append(element);
    return { element, title, status, button };
  }
  function websocket(room, kind, direction, query = "") {
    const url = new URL("/api/rooms/" + encodeURIComponent(room) + "/media/" + kind + "/" + direction + query, location.href); url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url, ["kk-v1", "kk-auth." + ui.token()]); socket.binaryType = "arraybuffer"; return socket;
  }
  function reserve(room, direction, peer, kind, consentGeneration) {
    if (!["audio", "camera", "screen"].includes(kind)) throw new Error("Unknown media source kind.");
    const current = ui.room();
    if (consentGeneration !== generation || !room?.status || !current?.status || current.name !== room.name || current.status.owner !== room.status.owner) throw new Error("The room changed. Open media controls again.");
    if (direction === "listen" && (!current.status.members.includes(peer) || peer === current.status.owner)) throw new Error("Choose another current room member as the source.");
    // A reservation includes permission/setup work, not just open transports.
    for (const session of sessions.values()) if (session.room !== room.name) session.stop();
    const key = JSON.stringify([room.name, direction, peer, kind]);
    if (sessions.has(key)) { ui.toast("This source is already active or awaiting permission. Use its tile to stop it."); return null; }
    const cfg = ui.config(), limit = direction === "publish" ? cfg.max_tracks : cfg.max_subscriptions;
    if (!Number.isSafeInteger(limit) || limit < 1 || !Number.isSafeInteger(cfg.max_users) || cfg.max_users < 1) throw new Error("Media capacity is unavailable. Refresh before trying again.");
    if (sessions.size >= cfg.max_users || [...sessions.values()].filter((session) => session.direction === direction).length >= limit) throw new Error("Media capacity reached. Stop a sharing or playback tile before adding another source.");
    const card = tile(direction === "publish" ? "SHARING / " + kind.toUpperCase() : kind.toUpperCase() + " / " + ui.memberName(room, peer));
    if (direction === "listen") card.title.title = peer;
    Object.assign(card.element.dataset, { room: room.name, direction, peer, kind });
    let closed = false, socket = null; const cleanups = [];
    const session = { stop() {
      if (closed) return; closed = true;
      for (const cleanup of cleanups) { try { cleanup(); } catch {} }
      socket?.close(); card.element.remove(); sessions.delete(key); panel.hidden = sessions.size === 0;
    }, room: room.name, direction, card, cleanups,
    active() { const current = ui.room(); if (current?.name !== room.name || !current?.status || current.status.owner !== room.status.owner || (direction === "listen" && !current.status.members.includes(peer))) session.stop(); return !closed; },
    cleanup(fn) { if (closed) fn(); else cleanups.push(fn); },
    transport(value) { socket = value; socket.onclose = session.stop; socket.onerror = () => { ui.toast("Media transport refused or disconnected. Check consent, roles, capacity and the quorum."); session.stop(); }; if (closed) socket.close(); },
    get closed() { return closed; } };
    sessions.set(key, session); card.button.onclick = session.stop;
    return session;
  }
  async function opened(socket) {
    return new Promise((resolve, reject) => {
      function finish(error, message) { clearTimeout(timer); socket.removeEventListener("message", ready); socket.removeEventListener("close", closed); error ? reject(error) : resolve(message); }
      function ready(event) { if (typeof event.data !== "string") return; try { const message = JSON.parse(event.data); if (message.event === "ready") finish(null, message); } catch { finish(new Error("Invalid media setup response.")); } }
      function closed() { finish(new Error("The source was stopped or refused. Check consent and media capacity.")); }
      const timer = setTimeout(() => finish(new Error("Media setup timed out.")), 30000);
      socket.addEventListener("message", ready); socket.addEventListener("close", closed, { once: true });
    });
  }
  async function publish(room, kind, quality, consentGeneration) {
    const session = reserve(room, "publish", room?.status?.owner, kind, consentGeneration); if (!session) return;
    const cfg = ui.config();
    try {
    if (kind === "audio" ? !window.AudioEncoder || !window.AudioContext || !window.AudioWorkletNode || !window.AudioData : !window.VideoEncoder || !window.VideoFrame || !window.MediaStreamTrackProcessor || !window.OffscreenCanvas) throw new Error("This browser lacks the required media encoding APIs. No capture was requested.");
    if (kind === "screen" ? !navigator.mediaDevices?.getDisplayMedia : !navigator.mediaDevices?.getUserMedia) throw new Error("Capture of this source is unavailable in this browser context.");
    // These calls run directly from the user's gesture and use native browser
    // permission dialogs, including the Wayland screen-selection portal.
    const request = kind === "screen" ? navigator.mediaDevices.getDisplayMedia({ video: { frameRate: cfg.fps }, audio: false })
      : navigator.mediaDevices.getUserMedia(kind === "audio" ? { audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1 }, video: false }
        : { video: { width: cfg.width, height: cfg.height, frameRate: cfg.fps }, audio: false });
    const stream = await request;
    session.cleanup(() => { for (const track of stream.getTracks()) { track.onended = null; track.stop(); } });
    if (!session.active()) return;
    for (const track of stream.getTracks()) track.onended = session.stop;
      const card = session.card, socket = websocket(room.name, kind, "publish"); session.transport(socket);
      const ready = await opened(socket); if (!session.active()) return; card.status.textContent = ready.peers.length + " accepting peers / encrypted transport";
      const packetizer = new Packetizer(kind, cfg.packet_bytes);
      const transmit = (chunk) => {
        if (!session.active() || socket.readyState !== WebSocket.OPEN || socket.bufferedAmount > cfg.datagram_buffer_bytes || chunk.byteLength > cfg.frame_bytes) return false;
        for (const packet of packetizer.encode(chunk)) socket.send(packet);
        return true;
      };
      if (kind === "audio") {
        if (!window.AudioEncoder) throw new Error("This browser does not provide an Opus AudioEncoder. Use the native CLI.");
        const context = new AudioContext({ sampleRate: 48000, latencyHint: "interactive" }); session.cleanup(() => context.close()); await context.resume(); if (!session.active()) return; await context.audioWorklet.addModule("/audio-worklet.js"); if (!session.active()) return;
        const encoder = new AudioEncoder({ output: (chunk) => transmit(chunk), error: () => session.stop() });
        const settings = { codec: "opus", sampleRate: 48000, numberOfChannels: 1, bitrate: cfg.audio_bitrate };
        session.cleanup(() => { if (encoder.state !== "closed") encoder.close(); });
        const supported = await AudioEncoder.isConfigSupported(settings); if (!session.active()) return;
        if (!supported.supported) throw new Error("Opus encoding is unavailable in this browser.");
        encoder.configure(settings);
        const source = context.createMediaStreamSource(stream), worklet = new AudioWorkletNode(context, "kk-capture", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1] });
        source.connect(worklet); worklet.connect(context.destination); session.cleanups.push(() => { source.disconnect(); worklet.disconnect(); }); let frames = 0;
        worklet.port.onmessage = (event) => { const timestamp = frames * 1000000 / 48000; frames += 960; if (!session.active() || encoder.state !== "configured" || encoder.encodeQueueSize > 2) return;
          const audio = new AudioData({ format: "f32", sampleRate: 48000, numberOfFrames: 960, numberOfChannels: 1, timestamp, data: event.data }); encoder.encode(audio); audio.close(); };
      } else {
        if (!window.VideoEncoder || !window.MediaStreamTrackProcessor) throw new Error("This browser lacks the native video encoding APIs. Use the CLI or a supported browser.");
        const encoder = new VideoEncoder({ output: (chunk) => { if (!transmit(chunk)) forceKey = true; }, error: () => session.stop() });
        const settings = { codec: "vp8", width: cfg.width, height: cfg.height, bitrate: quality, framerate: cfg.fps, latencyMode: "realtime" };
        session.cleanup(() => { if (encoder.state !== "closed") encoder.close(); });
        const supported = await VideoEncoder.isConfigSupported(settings); if (!session.active()) return;
        if (!supported.supported) throw new Error("VP8 encoding is unavailable in this browser.");
        encoder.configure(settings);
        const canvas = new OffscreenCanvas(cfg.width, cfg.height), drawing = canvas.getContext("2d", { alpha: false });
        const preview = document.createElement("video"); preview.muted = true; preview.autoplay = true; preview.playsInline = true; preview.srcObject = stream; card.element.append(preview); session.cleanup(() => { preview.srcObject = null; });
        const reader = new MediaStreamTrackProcessor({ track: stream.getVideoTracks()[0] }).readable.getReader(); session.cleanups.push(() => reader.cancel());
        let frameNumber = 0, last = -Infinity, forceKey = true;
        (async () => { try { while (session.active()) { const { value: frame, done } = await reader.read(); if (done) break;
          if (!session.active()) { frame.close(); break; }
          if (encoder.encodeQueueSize > 2 || frame.timestamp - last < 1000000 / cfg.fps) { frame.close(); continue; }
          last = frame.timestamp; drawing.drawImage(frame, 0, 0, cfg.width, cfg.height); const scaled = new VideoFrame(canvas, { timestamp: frame.timestamp }); frame.close();
          const keyFrame = forceKey || frameNumber++ % (cfg.fps * 2) === 0; forceKey = false; encoder.encode(scaled, { keyFrame }); scaled.close();
        } } catch { if (!session.closed) ui.toast("Native capture ended."); } finally { session.stop(); } })();
      }
    } catch (error) { session.stop(); throw error; }
  }
  async function listen(room, kind, from, consentGeneration) {
    const session = reserve(room, "listen", from, kind, consentGeneration); if (!session) return;
    const cfg = ui.config(), card = session.card;
    let context = null, worklet = null;
    try { if (kind === "audio") {
      if (!window.AudioDecoder || !window.AudioContext || !window.AudioWorkletNode || !window.EncodedAudioChunk) throw new Error("Opus playback is unavailable in this browser.");
      context = new AudioContext({ sampleRate: 48000, latencyHint: "interactive" }); session.cleanup(() => context.close()); await context.resume(); if (!session.active()) return; await context.audioWorklet.addModule("/audio-worklet.js"); if (!session.active()) return;
      worklet = new AudioWorkletNode(context, "kk-playback", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [1] }); session.cleanup(() => worklet.disconnect()); worklet.connect(context.destination);
    } else if (!window.VideoDecoder || !window.EncodedVideoChunk) throw new Error("VP8 playback is unavailable in this browser.");
    } catch (error) { session.stop(); throw error; }
    let socket;
    try { socket = websocket(room.name, kind, "listen", "?from=" + encodeURIComponent(from)); session.transport(socket); }
    catch (error) { session.stop(); throw error; }
    try {
    let decoder = null, track = "", base = null, keyNeeded = true, revision = 0;
    let frames = new VideoFrames(cfg.frame_bytes, cfg.packet_age_ms), canvas = null, drawing = null;
    card.element.dataset.decoded = "0";
    if (kind !== "audio") { canvas = document.createElement("canvas"); canvas.width = cfg.width; canvas.height = cfg.height; drawing = canvas.getContext("2d", { alpha: false }); card.element.append(canvas); }
    function configure() {
      const configuredRevision = ++revision;
      if (decoder && decoder.state !== "closed") decoder.close(); base = null; keyNeeded = true; frames = new VideoFrames(cfg.frame_bytes, cfg.packet_age_ms);
      if (kind === "audio") { decoder = new AudioDecoder({ output: (audio) => { if (!session.active() || configuredRevision !== revision || audio.numberOfFrames > 9600) { audio.close(); return; } const samples = new Float32Array(audio.numberOfFrames); audio.copyTo(samples, { planeIndex: 0, format: "f32-planar" }); audio.close(); worklet.port.postMessage(samples.buffer, [samples.buffer]); card.element.dataset.decoded = String(Number(card.element.dataset.decoded) + 1); }, error: () => { if (configuredRevision === revision) session.stop(); } }); decoder.configure({ codec: "opus", sampleRate: 48000, numberOfChannels: 1 }); }
      else { decoder = new VideoDecoder({ output: (frame) => { if (session.active() && configuredRevision === revision) { drawing.drawImage(frame, 0, 0, canvas.width, canvas.height); card.element.dataset.decoded = String(Number(card.element.dataset.decoded) + 1); } frame.close(); }, error: () => { if (configuredRevision === revision) keyNeeded = true; } }); decoder.configure({ codec: "vp8", optimizeForLatency: true }); }
    }
    session.cleanups.push(() => { if (decoder && decoder.state !== "closed") decoder.close(); });
    socket.onmessage = (event) => {
      if (!session.active()) return;
      if (typeof event.data === "string") { try { const message = JSON.parse(event.data); if (message.event === "ready") card.status.textContent = "Consented / waiting for source"; else if (message.event === "ended") session.stop(); } catch { session.stop(); } return; }
      const bytes = new Uint8Array(event.data); if (bytes.length < 37 || bytes.length > cfg.packet_bytes + 24) return;
      const stamp = Number(new DataView(bytes.buffer).getBigUint64(0)); if (Date.now() < stamp || Date.now() - stamp > cfg.packet_age_ms) return;
      const id = Array.from(bytes.subarray(8, 24), (byte) => byte.toString(16).padStart(2, "0")).join("");
      if (id !== track) { track = id; try { configure(); } catch { session.stop(); return; } }
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
    } catch (error) { session.stop(); throw error; }
  }
  function shareDialog() {
    const room = ui.room(), consentGeneration = generation;
    if (!navigator.mediaDevices) { ui.toast("Native capture is unavailable in this browser context."); return; }
    const kind = ui.el("select"); kind.name = "kind"; for (const value of ["audio", "camera", "screen"]) { const option = ui.el("option", "", value); option.value = value; kind.append(option); }
    const quality = ui.el("select"); quality.name = "quality"; for (const [label, value] of [["Low bandwidth / 256 kbit/s", 256000], ["Balanced / 800 kbit/s", 800000], ["Detailed / 1.5 Mbit/s", 1500000]]) { const option = ui.el("option", "", label); option.value = value; option.selected = value === 800000; quality.append(option); }
    ui.modal("Share a native source", [ui.el("p", "dialog-note", "Only peers that explicitly accept this source can receive it. Browser/OS permission is still required. No media recording is enabled."), kind, quality], "Choose source and authorize capture", async (data) => publish(room, data.get("kind"), Number(data.get("quality")), consentGeneration));
  }
  function receiveDialog() {
    const room = ui.room(), consentGeneration = generation; if (!room?.status) return;
    const kind = ui.el("select"); kind.name = "kind"; for (const value of ["audio", "camera", "screen"]) { const option = ui.el("option", "", value); option.value = value; kind.append(option); }
    const peer = ui.el("select"); peer.name = "from"; for (const id of room.status.members.filter((id) => id !== room.status.owner)) { const option = ui.el("option", "", ui.memberChoice(room, id)); option.value = id; option.title = id; peer.append(option); }
    if (!peer.options.length) { ui.toast("Invite a sender first."); return; }
    ui.modal("Consent to a source", [ui.el("p", "dialog-note", "This opens local playback only for the chosen member and source kind. Stop withdraws consent; no camera or microphone is opened here."), peer, kind], "Accept this source", async (data) => listen(room, data.get("kind"), data.get("from"), consentGeneration));
  }
  return { shareDialog, receiveDialog, refreshLabels() { const room = ui.room(); for (const session of sessions.values()) { const {direction, peer, kind} = session.card.element.dataset; if (direction === "listen" && room?.name === session.room) session.card.title.textContent = kind.toUpperCase() + " / " + ui.memberName(room, peer); } }, stopAll() { generation++; for (const session of [...sessions.values()]) session.stop(); } };
}
