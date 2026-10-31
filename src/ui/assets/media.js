import { Packetizer, parse, VideoFrames, dimensions, RtpClock } from "/rtp.js";

export function setupMedia(ui) {
  const sessions = new Map();
  const videoQualities = [["Low bandwidth / 256 kbit/s", 256000], ["Balanced / 800 kbit/s", 800000], ["Detailed / 1.5 Mbit/s", 1500000]];
  let generation = 0;
  function transportReason(event) {
    if (event?.code === 1008) return "The server closed media for a policy refusal. Refresh room permissions and consent before trying again.";
    if (event?.code === 1009) return "The media transport rejected an oversized message.";
    if (event?.code === 1013) return "The media service reported temporary overload. Stop unused sources before retrying.";
    return "Media disconnected or was refused. The browser did not expose the server's reason; no specific permission or quorum failure can be inferred.";
  }
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
    const cfg = ui.config(), limit = direction === "publish" ? cfg.max_publications : cfg.max_subscriptions;
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
    fail(reason) { if (!closed) { ui.toast(reason); session.stop(); } },
    active() { const current = ui.room(); if (current?.name !== room.name || !current?.status || current.status.owner !== room.status.owner || (direction === "listen" && !current.status.members.includes(peer))) session.stop(); return !closed; },
    cleanup(fn) { if (closed) fn(); else cleanups.push(fn); },
    transport(value) { socket = value; socket.onclose = (event) => { if (!closed && event?.code && event.code !== 1000) ui.toast(transportReason(event)); session.stop(); }; socket.onerror = () => session.fail(transportReason()); if (closed) socket.close(); },
    get closed() { return closed; } };
    sessions.set(key, session); card.button.onclick = session.stop;
    return session;
  }
  async function opened(socket) {
    return new Promise((resolve, reject) => {
      function finish(error, message) { clearTimeout(timer); socket.removeEventListener("message", ready); socket.removeEventListener("close", closed); error ? reject(error) : resolve(message); }
      function ready(event) { if (typeof event.data !== "string") return; try { const message = JSON.parse(event.data); if (message.event === "ready") { if (!Array.isArray(message.peers) || message.peers.some((peer) => typeof peer !== "string")) throw new Error(); finish(null, message); } else if (message.event === "ended") finish(new Error("The media source ended before setup completed. No capture is still shared.")); } catch { finish(new Error("Invalid media setup response; the sharing source was stopped.")); } }
      function closed(event) { finish(new Error(transportReason(event))); }
      const timer = setTimeout(() => finish(new Error("Media setup timed out.")), 30000);
      socket.addEventListener("message", ready); socket.addEventListener("close", closed, { once: true });
    });
  }
  async function publish(room, kind, quality, consentGeneration, choice, refreshDevices) {
    const session = reserve(room, "publish", room?.status?.owner, kind, consentGeneration); if (!session) return;
    const cfg = ui.config();
    try {
    if (kind === "audio" ? !window.AudioEncoder || !window.AudioContext || !window.AudioWorkletNode || !window.AudioData : !window.VideoEncoder || !window.VideoFrame || !window.MediaStreamTrackProcessor || !window.OffscreenCanvas) throw new Error("This browser lacks the required media encoding APIs. No capture was requested.");
    if (kind === "screen" ? !navigator.mediaDevices?.getDisplayMedia : !navigator.mediaDevices?.getUserMedia) throw new Error("Capture of this source is unavailable in this browser context.");
    if (!videoQualities.some(([, value]) => value === quality)) throw new Error("Choose a listed video quality.");
    const surface = { any: null, monitor: "monitor", window: "window", browser: "browser" };
    if (!Object.hasOwn(surface, choice.surface)) throw new Error("Choose a listed display source.");
    if (choice.device && !navigator.mediaDevices.getSupportedConstraints?.().deviceId) throw new Error("This browser cannot select a specific capture device. Choose Default device.");
    const device = choice.device ? { deviceId: { exact: choice.device } } : {};
    // These calls run directly from the user's gesture and use native browser
    // permission dialogs, including the Wayland screen-selection portal.
    // displaySurface is a preference, never an exact/min constraint or a promise
    // about the OS chooser. No title, device ID or requested surface enters RTP.
    const request = kind === "screen" ? navigator.mediaDevices.getDisplayMedia({ video: { frameRate: cfg.fps, ...(surface[choice.surface] ? { displaySurface: surface[choice.surface] } : {}) }, audio: false })
      : navigator.mediaDevices.getUserMedia(kind === "audio" ? { audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1, ...device }, video: false }
        : { video: { width: cfg.width, height: cfg.height, frameRate: cfg.fps, ...device }, audio: false });
    let stream;
    try { stream = await request; }
    catch (error) {
      if (["NotFoundError", "OverconstrainedError"].includes(error.name)) throw new Error("The selected capture device/source is unavailable. Refresh devices or choose Default device; no substitute was opened.");
      if (["NotAllowedError", "SecurityError"].includes(error.name)) throw new Error("Capture was cancelled or permission was denied. Nothing is shared; submit again only when ready.");
      if (error.name === "NotReadableError") throw new Error("The browser/OS could not read the selected source. It may be busy or unavailable; no substitute was opened.");
      if (error.name === "AbortError") throw new Error("The browser/OS aborted capture setup. Nothing is shared; select the source again when ready.");
      throw error;
    }
    session.cleanup(() => { for (const track of stream.getTracks()) { track.onended = null; track.stop(); } });
    if (!session.active()) return;
    if (kind !== "screen") void refreshDevices();
    const sourceEnded = () => {
      if (!session.active()) return;
      session.stop();
      ui.modal(kind.toUpperCase() + " sharing ended", [ui.el("p", "dialog-note", "The capture device/source ended. This publication, its encoder and transport are stopped. Other sharing and playback continue. Nothing will restart automatically; choose a source and authorize capture again to restart.")], "Choose source to restart", () => {
        const current = ui.room();
        if (generation !== consentGeneration || current?.name !== room.name || current?.status?.owner !== room.status.owner) throw new Error("The room/session changed. Open sharing controls in the current room instead.");
        shareDialog(kind); return false;
      });
    };
    for (const track of stream.getTracks()) track.onended = sourceEnded;
    if (stream.getTracks().some((track) => track.readyState === "ended")) { sourceEnded(); return; }
    const privacy = ui.el("button", "outline compact", kind === "audio" ? "Mute" : "Hide video");
    privacy.type = "button"; privacy.setAttribute?.("aria-pressed", "false");
    let muted = false;
    privacy.onclick = () => {
      if (!session.active()) return;
      muted = !muted;
      for (const track of stream.getTracks()) track.enabled = !muted;
      privacy.textContent = muted ? (kind === "audio" ? "Unmute" : "Show video") : (kind === "audio" ? "Mute" : "Hide video");
      privacy.setAttribute?.("aria-pressed", String(muted));
      session.card.element.dataset.muted = String(muted);
    };
    session.card.element.append(privacy, ui.el("small", "muted", "Mute/hide sends silence/black while capture stays open. Stop releases the source; changing it requires fresh consent."));
    // Never silently move to another device when the selected input disappears.
    // Only explicit non-default IDs are reconciled: enumeration may conceal IDs
    // before permission and display sources are governed by track.onended.
    if (kind !== "screen" && choice.device && navigator.mediaDevices.addEventListener) {
      let checking = false;
      const changed = async () => {
        if (checking || !session.active()) return;
        checking = true;
        try {
          const entries = await navigator.mediaDevices.enumerateDevices();
          if (session.active() && !entries.some((entry) => entry.deviceId === choice.device && entry.kind === (kind === "camera" ? "videoinput" : "audioinput"))) {
            ui.toast("The selected device was removed. Sharing stopped; choose a source again to authorize capture."); session.stop();
          }
        } catch { /* A transient enumeration failure is not proof of removal. */ }
        finally { checking = false; }
      };
      navigator.mediaDevices.addEventListener("devicechange", changed);
      session.cleanup(() => navigator.mediaDevices.removeEventListener("devicechange", changed));
    }
    if (kind === "screen") {
      const actual = stream.getVideoTracks()[0]?.getSettings?.().displaySurface;
      const label = { monitor: "Entire screen", window: "Window/application window", browser: "Browser tab" };
      session.card.element.append(ui.el("small", "muted", "Captured source: " + (Object.hasOwn(label, actual) ? label[actual] : "Not reported by browser")));
    }
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
        const encoder = new AudioEncoder({ output: (chunk) => transmit(chunk), error: () => session.fail("Opus encoding failed. Audio sharing stopped and the source was released.") });
        const settings = { codec: "opus", sampleRate: 48000, numberOfChannels: 1, bitrate: cfg.audio_bitrate };
        session.cleanup(() => { if (encoder.state !== "closed") encoder.close(); });
        const supported = await AudioEncoder.isConfigSupported(settings); if (!session.active()) return;
        if (!supported.supported) throw new Error("Opus encoding is unavailable in this browser.");
        encoder.configure(settings);
        const source = context.createMediaStreamSource(stream), worklet = new AudioWorkletNode(context, "kk-capture", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1] });
        source.connect(worklet); worklet.connect(context.destination); session.cleanups.push(() => { source.disconnect(); worklet.disconnect(); }); let frames = 0;
        worklet.port.onmessage = (event) => { if (!session.active()) return; if (!(event.data instanceof ArrayBuffer) || event.data.byteLength !== 960 * Float32Array.BYTES_PER_ELEMENT) { session.fail("Audio capture produced an invalid PCM block. Sharing stopped and the source was released."); return; } const timestamp = frames * 1000000 / 48000; frames += 960; if (encoder.state !== "configured" || encoder.encodeQueueSize > 2) return;
          let audio;
          try { audio = new AudioData({ format: "f32", sampleRate: 48000, numberOfFrames: 960, numberOfChannels: 1, timestamp, data: event.data }); encoder.encode(audio); }
          catch { session.fail("Audio encoding failed. Sharing stopped and the capture source was released; restart requires fresh consent."); }
          finally { audio?.close(); } };
      } else {
        if (!window.VideoEncoder || !window.MediaStreamTrackProcessor) throw new Error("This browser lacks the native video encoding APIs. Use the CLI or a supported browser.");
        const encoder = new VideoEncoder({ output: (chunk) => { if (!transmit(chunk)) forceKey = true; }, error: () => session.fail("VP8 encoding failed. Video sharing stopped and the source was released.") });
        const settings = { codec: "vp8", width: cfg.width, height: cfg.height, bitrate: quality, framerate: cfg.fps, latencyMode: "realtime" };
        session.cleanup(() => { if (encoder.state !== "closed") encoder.close(); });
        const supported = await VideoEncoder.isConfigSupported(settings); if (!session.active()) return;
        if (!supported.supported) throw new Error("VP8 encoding is unavailable in this browser.");
        encoder.configure(settings);
        const canvas = new OffscreenCanvas(cfg.width, cfg.height), drawing = canvas.getContext("2d", { alpha: false });
        const preview = document.createElement("video"); preview.muted = true; preview.autoplay = true; preview.playsInline = true; preview.srcObject = stream; card.element.append(preview); session.cleanup(() => { preview.srcObject = null; });
        const reader = new MediaStreamTrackProcessor({ track: stream.getVideoTracks()[0] }).readable.getReader(); session.cleanups.push(() => reader.cancel());
        let frameNumber = 0, last = -Infinity, forceKey = true;
        const liveQuality = ui.el("select"), qualityLabel = ui.el("label", "", "Video quality");
        for (const [label, value] of videoQualities) { const option = ui.el("option", "", label); option.value = String(value); liveQuality.append(option); }
        liveQuality.value = String(quality); qualityLabel.append(liveQuality); card.element.append(qualityLabel);
        let changingQuality = false;
        liveQuality.onchange = async () => {
          if (!session.active() || changingQuality) return;
          const bitrate = Number(liveQuality.value), previous = settings.bitrate;
          if (!videoQualities.some(([, value]) => value === bitrate)) { liveQuality.value = String(previous); ui.toast("Choose a listed video quality. The current quality was kept."); return; }
          if (bitrate === previous) return;
          changingQuality = true; liveQuality.disabled = true;
          try {
            const next = { ...settings, bitrate }, supported = await VideoEncoder.isConfigSupported(next);
            if (!session.active()) return;
            if (!supported.supported) { liveQuality.value = String(previous); ui.toast("This quality is unavailable. The current quality and capture were kept."); return; }
            if (encoder.state !== "configured") throw new Error("Encoder is no longer configured.");
            // WebCodecs queues configure after existing encode work. Preserve
            // capture, dimensions and RTP clock; the next input is a keyframe.
            encoder.configure(next); settings.bitrate = bitrate; forceKey = true;
          } catch {
            liveQuality.value = String(previous);
            if (session.active()) {
              if (encoder.state === "configured") ui.toast("Quality change was refused. The current quality and capture were kept.");
              else session.fail("The video encoder stopped during quality selection. Sharing stopped; authorize a new publication to restart.");
            }
          } finally { changingQuality = false; liveQuality.disabled = false; }
        };
        (async () => { try { while (session.active()) { const { value: frame, done } = await reader.read(); if (done) break;
          if (!session.active()) { frame.close(); break; }
          if (encoder.encodeQueueSize > 2 || frame.timestamp - last < 1000000 / cfg.fps) { frame.close(); continue; }
          let scaled;
          try { last = frame.timestamp; drawing.drawImage(frame, 0, 0, cfg.width, cfg.height); scaled = new VideoFrame(canvas, { timestamp: frame.timestamp }); }
          finally { frame.close(); }
          try { const keyFrame = forceKey || frameNumber++ % (cfg.fps * 2) === 0; forceKey = false; encoder.encode(scaled, { keyFrame }); }
          finally { scaled.close(); }
        } } catch { if (!session.closed) ui.toast("Native capture ended."); } finally { session.stop(); } })();
      }
      if (session.active()) {
        const change = ui.el("button", "outline compact", "Change source…"); change.type = "button";
        change.onclick = () => {
          if (!session.active()) return;
          // Capture replacement needs a fresh publication/track and RTP clock;
          // never reuse this session's consent or silently substitute a device.
          ui.modal("Change this capture source", [ui.el("p", "dialog-note", "A new source requires a new publication. Confirming stops only this publication, then opens fresh source selection and consent. If you cancel or the new source fails, this sharing stays stopped. Other sharing and playback continue.")], "Stop this publication and choose again", () => {
            if (!session.active()) throw new Error("This publication already ended. Open sharing controls to authorize a new source.");
            session.stop(); shareDialog(kind); return false;
          });
        };
        card.element.append(change);
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
      worklet = new AudioWorkletNode(context, "kk-playback", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [1] }); session.cleanup(() => worklet.disconnect());
      const gain = context.createGain(); gain.gain.value = 1; session.cleanup(() => gain.disconnect());
      worklet.connect(gain); gain.connect(context.destination);
      const volume = ui.el("input"), volumeText = ui.el("span", "", "Volume: 100%"), volumeLabel = ui.el("label");
      volume.type = "range"; volume.min = "0"; volume.max = "100"; volume.step = "1"; volume.value = "100";
      volume.setAttribute?.("aria-label", "Volume for this received source"); card.element.dataset.volume = "100";
      volume.oninput = () => {
        if (!session.active()) return;
        const percent = Number(volume.value); if (!Number.isFinite(percent) || percent < 0 || percent > 100) return;
        gain.gain.setTargetAtTime(percent / 100, context.currentTime, 0.01);
        volumeText.textContent = "Volume: " + percent + "%"; card.element.dataset.volume = String(percent);
      };
      volumeLabel.append(volumeText, volume); card.element.append(volumeLabel);
      const mute = ui.el("button", "outline compact", "Mute playback"); mute.type = "button";
      let muted = false; mute.setAttribute?.("aria-pressed", "false");
      mute.onclick = () => {
        if (!session.active()) return;
        muted = !muted; worklet.port.postMessage({ muted });
        mute.textContent = muted ? "Unmute playback" : "Mute playback";
        mute.setAttribute?.("aria-pressed", String(muted)); card.element.dataset.muted = String(muted);
      };
      card.element.append(mute, ui.el("small", "muted", "Playback mute is local only. Stop withdraws this source's receive consent."));
      // Output IDs stay local to this playback context. Silent QA sinks cannot
      // be changed to physical outputs by this control.
      if (context.setSinkId && typeof context.sinkId === "string" && navigator.mediaDevices?.enumerateDevices) {
        const chooseOutput = ui.el("button", "outline compact", "Audio output…"), outputLabel = ui.el("small", "muted", "Audio output: system default");
        chooseOutput.type = "button"; let outputId = context.sinkId, switching = false, recheckOutput = false;
        async function defaultOutput() {
          if (!session.active() || switching) return;
          switching = true; chooseOutput.disabled = true;
          try {
            await context.setSinkId("");
            if (!session.active()) return;
            if (context.sinkId !== "") throw new Error("Default output was not confirmed.");
            outputId = ""; outputLabel.textContent = "Audio output: system default (previous output unavailable)";
            ui.toast("The selected audio output is unavailable. Reception continues on the browser's default output; no other device was selected.");
          } catch {
            if (!session.active()) return;
            muted = true; worklet.port.postMessage({ muted }); mute.textContent = "Unmute playback"; mute.setAttribute?.("aria-pressed", "true"); card.element.dataset.muted = "true";
            outputLabel.textContent = "Audio output unavailable / reception kept, playback muted";
            ui.toast("The default audio output could not be opened. Reception remains connected and muted; choose an output explicitly.");
          } finally { switching = false; chooseOutput.disabled = false; if (recheckOutput) { recheckOutput = false; void checkOutput(); } }
        }
        async function checkOutput() {
          if (!session.active() || !outputId) return;
          if (switching) { recheckOutput = true; return; }
          const selected = outputId;
          try {
            const entries = await navigator.mediaDevices.enumerateDevices();
            if (session.active() && !switching && outputId === selected && !entries.some((entry) => entry.kind === "audiooutput" && entry.deviceId === selected)) await defaultOutput();
          } catch { /* Listing failure alone is not evidence that an output disappeared. */ }
        }
        const sinkChanged = () => { if (!switching && outputId && context.sinkId !== outputId) void defaultOutput(); };
        const deviceChanged = () => checkOutput();
        context.addEventListener?.("sinkchange", sinkChanged); session.cleanup(() => context.removeEventListener?.("sinkchange", sinkChanged));
        navigator.mediaDevices.addEventListener?.("devicechange", deviceChanged); session.cleanup(() => navigator.mediaDevices.removeEventListener?.("devicechange", deviceChanged));
        chooseOutput.onclick = async () => {
          if (!session.active()) return;
          try {
            const entries = (await navigator.mediaDevices.enumerateDevices()).slice(0, 64).filter((entry) => entry.kind === "audiooutput" && typeof entry.deviceId === "string" && entry.deviceId.length > 0 && entry.deviceId.length <= 1024);
            if (!session.active()) return;
            const choices = new Map([["", "System default"]]);
            for (const [index, entry] of entries.entries()) if (entry.deviceId !== "default") choices.set(entry.deviceId, String(entry.label || "Audio output " + (index + 1)).slice(0, 160));
            const select = ui.el("select"); select.name = "output";
            for (const [id, label] of choices) { const option = ui.el("option", "", label); option.value = id; select.append(option); }
            select.value = choices.has(outputId) ? outputId : "";
            ui.modal("Choose local audio output", [ui.el("p", "dialog-note", "Only this source changes output. No microphone is opened. If the output disappears, reception continues on the browser default with a warning, never on another automatically selected device."), select], "Use this output", async (data) => {
              if (!session.active()) throw new Error("Playback ended. Accept the source again before selecting its output.");
              const selected = data.get("output"); if (!choices.has(selected)) throw new Error("Choose a listed audio output.");
              switching = true;
              try {
                await context.setSinkId(selected);
                if (!session.active()) return;
                if (context.sinkId !== selected) throw new Error("Output selection was not confirmed.");
                outputId = selected; outputLabel.textContent = "Audio output: " + choices.get(selected);
              } catch (error) {
                if (error.name === "NotFoundError") { switching = false; await defaultOutput(); return; }
                session.stop(); throw new Error("The selected audio output could not be opened. Playback stopped; no fallback output was selected.");
              }
              finally { switching = false; if (recheckOutput) { recheckOutput = false; void checkOutput(); } }
            });
          } catch { if (session.active()) ui.toast("Audio output listing is unavailable. The current output was not changed."); }
        };
        card.element.append(chooseOutput, outputLabel);
      }
    } else if (!window.VideoDecoder || !window.EncodedVideoChunk) throw new Error("VP8 playback is unavailable in this browser.");
    } catch (error) { session.stop(); throw error; }
    let socket;
    try { socket = websocket(room.name, kind, "listen", "?from=" + encodeURIComponent(from)); session.transport(socket); }
    catch (error) { session.stop(); throw error; }
    try {
    let decoder = null, track = "", clock = null, keyNeeded = true, revision = 0;
    let frames = new VideoFrames(cfg.frame_bytes, cfg.packet_age_ms), canvas = null, drawing = null;
    card.element.dataset.decoded = "0";
    if (kind !== "audio") { canvas = document.createElement("canvas"); canvas.width = cfg.width; canvas.height = cfg.height; drawing = canvas.getContext("2d", { alpha: false }); card.element.append(canvas); }
    function configure() {
      const configuredRevision = ++revision;
      if (kind === "audio") worklet.port.postMessage({ reset: true });
      if (decoder && decoder.state !== "closed") decoder.close(); clock = new RtpClock(kind === "audio" ? 48000 : 90000); keyNeeded = true; frames = new VideoFrames(cfg.frame_bytes, cfg.packet_age_ms);
      if (kind === "audio") {
        decoder = new AudioDecoder({ output: (audio) => {
          try {
            if (!session.active() || configuredRevision !== revision || !audio.numberOfFrames || audio.numberOfFrames > 9600) return;
            const samples = new Float32Array(audio.numberOfFrames); audio.copyTo(samples, { planeIndex: 0, format: "f32-planar" });
            worklet.port.postMessage(samples.buffer, [samples.buffer]); card.element.dataset.decoded = String(Number(card.element.dataset.decoded) + 1);
          } catch { if (configuredRevision === revision) session.fail("Audio playback failed. This receiver stopped; choose the source again to restart."); }
          finally { audio.close(); }
        }, error: () => { if (configuredRevision === revision) session.fail("Opus decoding failed. Playback stopped; choose the source again to restore consent."); } });
        decoder.configure({ codec: "opus", sampleRate: 48000, numberOfChannels: 1 });
      } else {
        decoder = new VideoDecoder({ output: (frame) => {
          try { if (session.active() && configuredRevision === revision) { drawing.drawImage(frame, 0, 0, canvas.width, canvas.height); card.element.dataset.decoded = String(Number(card.element.dataset.decoded) + 1); } }
          catch { if (configuredRevision === revision) session.fail("Video rendering failed. This receiver stopped; choose the source again to restart."); }
          finally { frame.close(); }
        }, error: () => { if (configuredRevision === revision) keyNeeded = true; } });
        decoder.configure({ codec: "vp8", optimizeForLatency: true });
      }
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
      try {
        if (kind === "audio") { const timestamp = clock.next(packet.timestamp); if (timestamp === null) return; decoder.decode(new EncodedAudioChunk({ type: "key", timestamp, data: packet.payload })); }
        else { const dropped = frames.dropped; const frame = frames.push(packet); if (frames.dropped > dropped) keyNeeded = true; if (!frame) return;
          if (frame.key) { if (!dimensions(frame.bytes, cfg.width, cfg.height)) return; if (decoder.state !== "configured") configure(); keyNeeded = false; }
          if (keyNeeded) return; const timestamp = clock.next(frame.timestamp); if (timestamp === null) return;
          decoder.decode(new EncodedVideoChunk({ type: frame.key ? "key" : "delta", timestamp, data: frame.bytes }));
        }
        card.status.textContent = "Encrypted / live / volatile buffers";
      } catch { keyNeeded = true; }
    };
    } catch (error) { session.stop(); throw error; }
  }
  function shareDialog(initialKind) {
    const room = ui.room(), consentGeneration = generation;
    if (!navigator.mediaDevices) { ui.toast("Native capture is unavailable in this browser context."); return; }
    const kind = ui.el("select"); kind.name = "kind"; for (const value of ["audio", "camera", "screen"]) { const option = ui.el("option", "", value); option.value = value; kind.append(option); }
    if (["audio", "camera", "screen"].includes(initialKind)) kind.value = initialKind;
    const quality = ui.el("select"); quality.name = "quality"; for (const [label, value] of videoQualities) { const option = ui.el("option", "", label); option.value = value; option.selected = value === 800000; quality.append(option); }
    const surface = ui.el("select"); surface.name = "surface";
    for (const [value, label] of [["any", "Browser/OS chooser (any source)"], ["monitor", "Entire screen (preference)"], ["window", "Window/application window (preference)"], ["browser", "Browser tab (preference)"]]) { const option = ui.el("option", "", label); option.value = value; surface.append(option); }
    const device = ui.el("select"); device.name = "device";
    const feedback = ui.el("p", "dialog-note"), refresh = ui.el("button", "outline compact", "Refresh devices"); refresh.type = "button";
    const field = (label, control) => { const wrapper = ui.el("label", "", label); wrapper.append(control); return wrapper; };
    const surfaceField = field("Display source preference", surface), deviceField = field("Local capture device", device);
    let devices = [], revision = 0;
    function renderDevices() {
      const previous = device.value, type = kind.value === "camera" ? "videoinput" : "audioinput";
      const fallback = ui.el("option", "", "Default device (browser/OS)"); fallback.value = ""; device.replaceChildren(fallback);
      for (const [index, entry] of devices.filter((entry) => entry.kind === type).entries()) { const option = ui.el("option", "", entry.label || "Unlabelled " + (type === "videoinput" ? "camera " : "microphone ") + (index + 1)); option.value = entry.deviceId; device.append(option); }
      device.value = [...device.options].some((option) => option.value === previous) ? previous : "";
      surfaceField.hidden = kind.value !== "screen"; deviceField.hidden = kind.value === "screen"; refresh.hidden = kind.value === "screen";
      quality.disabled = kind.value === "audio";
    }
    async function refreshDevices() {
      const request = ++revision;
      if (!navigator.mediaDevices.enumerateDevices) { feedback.textContent = "Device listing is unavailable. Default device remains available if capture is supported."; return; }
      try {
        const entries = await navigator.mediaDevices.enumerateDevices();
        if (request !== revision || generation !== consentGeneration) return;
        // Browser IDs belong only to this dialog/origin session, never profile storage.
        devices = entries.slice(0, 64).filter((entry) => ["audioinput", "videoinput"].includes(entry.kind) && typeof entry.deviceId === "string" && entry.deviceId.length > 0 && entry.deviceId.length <= 1024).map((entry) => ({ kind: entry.kind, deviceId: entry.deviceId, label: String(entry.label || "").slice(0, 160) }));
        renderDevices(); feedback.textContent = "Device labels may be hidden until you explicitly grant capture. Default device needs no listed ID. Showing at most 64 browser entries.";
      } catch { if (request === revision) feedback.textContent = "Device listing failed. Use Default device or refresh after checking browser permissions."; }
    }
    kind.onchange = renderDevices; refresh.onclick = () => { void refreshDevices(); }; renderDevices(); void refreshDevices();
    ui.modal("Share a native source", [ui.el("p", "dialog-note", "Only peers that explicitly accept this source can receive it. Browser/OS permission is still required. No media recording is enabled."), field("Source kind", kind), surfaceField, deviceField, refresh, feedback, ui.el("p", "dialog-note", "Display choices are hints only; the browser/OS makes the final choice. A window means one chosen window, not all windows of an application. No custom window enumeration or previews are available."), field("Video quality", quality)], "Choose source and authorize capture", async (data) => {
      const selectedKind = data.get("kind"), selectedDevice = selectedKind === "screen" ? "" : (data.get("device") || "");
      if (selectedDevice && !devices.some((entry) => entry.deviceId === selectedDevice && entry.kind === (selectedKind === "camera" ? "videoinput" : "audioinput"))) throw new Error("The selected device is no longer listed. Refresh devices or choose Default device.");
      return publish(room, selectedKind, selectedKind === "audio" ? 800000 : Number(data.get("quality")), consentGeneration, { surface: data.get("surface") || "any", device: selectedDevice }, refreshDevices);
    });
  }
  function receiveDialog() {
    const room = ui.room(), consentGeneration = generation; if (!room?.status) return;
    const kind = ui.el("select"); kind.name = "kind"; for (const value of ["audio", "camera", "screen"]) { const option = ui.el("option", "", value); option.value = value; kind.append(option); }
    const peer = ui.el("select"); peer.name = "from"; for (const id of room.status.members.filter((id) => id !== room.status.owner)) { const option = ui.el("option", "", ui.memberChoice(room, id)); option.value = id; option.title = id; peer.append(option); }
    if (!peer.options.length) { ui.toast("Invite a sender first."); return; }
    ui.modal("Consent to a source", [ui.el("p", "dialog-note", "This opens local playback only for the chosen member and source kind. Stop withdraws consent; no camera or microphone is opened here."), peer, kind], "Accept this source", async (data) => listen(room, data.get("kind"), data.get("from"), consentGeneration));
  }
  return { shareDialog, receiveDialog, refreshLabels() { const room = ui.room(); for (const session of [...sessions.values()]) { if (!session.active()) continue; const {direction, peer, kind} = session.card.element.dataset; if (direction === "listen" && room?.name === session.room) session.card.title.textContent = kind.toUpperCase() + " / " + ui.memberName(room, peer); } }, stopAll() { generation++; for (const session of [...sessions.values()]) session.stop(); } };
}
