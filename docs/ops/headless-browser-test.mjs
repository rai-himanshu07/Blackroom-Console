// Drives the console page in headless Chrome over the DevTools protocol: login, Start, wait for WebRTC video,
// check decoded frames, quality change, Stop. Called by headless-browser-test.sh; needs Node 22 and google-chrome.
import { spawn } from "node:child_process";

const [, , url, debugPort = "9333"] = process.argv;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chrome = spawn("google-chrome", [
  "--headless=new", "--no-sandbox", "--disable-gpu", "--ignore-certificate-errors", "--autoplay-policy=no-user-gesture-required",
  `--remote-debugging-port=${debugPort}`, `--user-data-dir=/tmp/br-chrome-${process.pid}`, "about:blank",
], { stdio: "ignore" });
const done = (code) => { chrome.kill("SIGKILL"); process.exit(code); };

let target;
for (let i = 0; i < 50 && !target; i++) {
  try { target = (await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json()).find((t) => t.type === "page"); } catch { /* not up yet */ }
  if (!target) await sleep(200);
}
if (!target) { console.log("FAIL: Chrome did not start"); done(1); }

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => { ws.onopen = r; });
let nextId = 1;
const pending = new Map();
const pageErrors = [];
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  else if (d.method === "Runtime.exceptionThrown") pageErrors.push(d.params.exceptionDetails.text + " " + (d.params.exceptionDetails.exception?.description || "").slice(0, 120));
};
const cdp = (method, params = {}) => new Promise((r) => { const id = nextId++; pending.set(id, r); ws.send(JSON.stringify({ id, method, params })); });
const js = async (expression) => {
  const r = await cdp("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 300));
  return r.result.result.value;
};

let failed = false;
const check = (name, ok, detail = "") => { console.log(`${ok ? "ok  " : "FAIL"} ${name} ${detail}`); if (!ok) failed = true; };

await cdp("Page.enable");
await cdp("Runtime.enable");
await cdp("Page.navigate", { url });
await sleep(2500);
check("page loaded on the connect screen", (await js("document.getElementById('connect') !== null && document.body.dataset.view")) === "home");
await sleep(500);
check("the connect screen names the laptop", (await js("document.getElementById('hostname').textContent.trim().length > 0")) === true);
const unnamedHome = await js(`[...document.querySelectorAll("button, select, textarea, input, [role=button]")].filter((el) => el.getClientRects().length && !(el.getAttribute("aria-label") || el.textContent.trim() || el.getAttribute("title") || el.getAttribute("placeholder") || (el.closest("label") && el.closest("label").textContent.trim()))).map((el) => el.id || el.tagName)`);
check("every connect-screen control has an accessible name", unnamedHome.length === 0, JSON.stringify(unnamedHome));

// Presets and the settings sheet (saved on the laptop).
await js("document.querySelector('.mode[data-preset=shared]').click()");
check("the Shared preset leaves screen and input alone", (await js("!settings.session.blank_panel && !settings.session.block_local_input && currentPreset() === 'shared'")) === true);
await js("document.querySelector('.mode[data-preset=private]').click()");
check("the Private preset blanks and blocks", (await js("settings.session.blank_panel && settings.session.block_local_input && currentPreset() === 'private'")) === true);
await js("document.getElementById('openSettings').click()");
check("the settings sheet opens", (await js("!document.getElementById('sheet').hidden")) === true);
await js("document.querySelector('.tabs [data-tab=display]').click()");
check("tabs switch panes", (await js("!document.querySelector('[data-pane=display]').hidden && document.querySelector('[data-pane=conn]').hidden")) === true);
await js("(() => { const el = document.querySelector('[data-key=\"client.scale\"]'); el.value = 'actual'; el.dispatchEvent(new Event('change', {bubbles: true})); })()");
await sleep(900);
const savedScale = await js("fetch('/settings', {credentials: 'same-origin'}).then((r) => r.json()).then((s) => s.client.scale)");
check("a changed setting is saved on the laptop", savedScale === "actual" && (await js("stageScale")) === "actual");
await js("(() => { const el = document.querySelector('[data-key=\"client.scale\"]'); el.value = 'fit'; el.dispatchEvent(new Event('change', {bubbles: true})); })()");
await js("document.getElementById('sheetClose').click()");
check("the settings sheet closes", (await js("document.getElementById('sheet').hidden")) === true);
await sleep(600);

const withAudio = process.env.BR_AUDIO_TEST === "1";
await js("settings.client.quality = 'high'; applyAll()");
if (withAudio) {
  await js("settings.session.audio = true; applyAll()");
}
await js("document.getElementById('connect').click()");
let state = {};
for (let i = 0; i < 60; i++) {
  await sleep(500);
  state = await js(`({chip: document.getElementById('chip').textContent, w: document.getElementById('rtc').videoWidth, shown: getComputedStyle(document.getElementById('rtc')).display, msg: document.getElementById('msg').textContent})`);
  if (state.w > 0 && /WebRTC \d+ fps/.test(state.chip)) break;
  if (i === 8) console.log("server:", await js("fetch('/status',{credentials:'same-origin'}).then(r=>r.text())"));
  if (i === 8) console.log("peer:", await js("pc ? pc.getStats().then((r) => { let o = {c: pc.connectionState}; r.forEach((e) => { if (e.type === 'inbound-rtp') o.inbound = {packets: e.packetsReceived, bytes: e.bytesReceived, frames: e.framesDecoded, dropped: e.framesDropped, keyFrames: e.keyFramesDecoded, decoder: e.decoderImplementation, pli: e.pliCount, err: e.lastPacketReceivedTimestamp}; }); return JSON.stringify(o); }) : 'no peer'"));
}
console.log("state:", JSON.stringify(state));
check("webrtc video decoded at 1920 wide", state.w === 1920, `(videoWidth ${state.w})`);
check("webrtc element shown", state.shown === "block");
check("the view switched to the session", (await js("document.body.dataset.view")) === "session");
check("the menu opens from its button", (await js("(() => { document.getElementById('menuBtn').click(); return !document.getElementById('menu').hidden; })()")) === true);
await sleep(3000);
const stats = await js(`(async () => { const r = await fetch('/status', {credentials: 'same-origin'}); const s = await r.json(); const q = document.getElementById('rtc'); return {s, frames: q.getVideoPlaybackQuality ? q.getVideoPlaybackQuality().totalVideoFrames : -1, chip: document.getElementById('chip').textContent}; })()`);
console.log("status:", JSON.stringify(stats).slice(0, 400));
check("frames decoded", stats.frames > 5, `(${stats.frames})`);
check("server reports the encoder", typeof stats.s.webrtc_encoder === "string", `(${stats.s.webrtc_encoder})`);
check("quality applied", stats.s.quality === "high");
check("no webrtc pipeline error", !stats.s.webrtc_error, `(${stats.s.webrtc_error})`);
if (withAudio) {
  await sleep(2000);
  const audio = await js(`pc.getStats().then((r) => { let a = null; r.forEach((e) => { if (e.type === 'inbound-rtp' && e.kind === 'audio') a = e; }); return a && { bytes: a.bytesReceived, packets: a.packetsReceived, energy: a.totalAudioEnergy || 0 }; })`);
  console.log("audio:", JSON.stringify(audio));
  check("audio packets arrive", !!audio && audio.packets > 20, JSON.stringify(audio));
  // Decode what arrived: a 440 Hz tone is played into the stand-in sound output.
  const tone = await js(`(async () => {
    const track = rtcVideo.srcObject && rtcVideo.srcObject.getAudioTracks()[0];
    if (!track) return { error: "no audio track" };
    const context = new AudioContext();
    await context.resume();
    const analyser = context.createAnalyser(); analyser.fftSize = 8192;
    context.createMediaStreamSource(new MediaStream([track])).connect(analyser);
    await new Promise((r) => setTimeout(r, 1500));
    const bins = new Float32Array(analyser.frequencyBinCount); analyser.getFloatFrequencyData(bins);
    let best = 0; for (let i = 1; i < bins.length; i++) if (bins[i] > bins[best]) best = i;
    return { hz: Math.round(best * context.sampleRate / analyser.fftSize), db: Math.round(bins[best]), muted: rtcVideo.muted, state: context.state };
  })()`);
  console.log("tone:", JSON.stringify(tone));
  check("the laptop's 440 Hz tone arrives and decodes", !tone.error && Math.abs(tone.hz - 440) < 20 && tone.db > -70, JSON.stringify(tone));
  const live = await js("fetch('/status', {credentials: 'same-origin'}).then((r) => r.json())");
  check("server reports audio on", live.audio === "on", `(${live.audio} ${live.audio_note})`);
}

// Input over the data channel: accepted by the server without a single HTTP /input batch.
const statusJson = "fetch('/status',{credentials:'same-origin'}).then(r=>r.json())";
check("input data channel is open", (await js("!!inputChannel && inputChannel.readyState === 'open'")) === true);
const acceptedBefore = (await js(statusJson)).input_accepted;
await js("globalThis.__httpBatches = 0; const orig = post; post = (path, body) => { if (path === '/input' && body && body.length) __httpBatches++; return orig(path, body); }; tap('ShiftLeft')");
await sleep(1500);
const acceptedAfter = (await js(statusJson)).input_accepted;
check("data-channel input accepted", acceptedAfter - acceptedBefore >= 2, `(${acceptedBefore} -> ${acceptedAfter})`);
check("no HTTP input batch was needed", (await js("__httpBatches")) === 0);

await js("document.getElementById('quality').value = 'low'; document.getElementById('quality').dispatchEvent(new Event('change', {bubbles: true}))");
await sleep(1500);
check("quality changed live", (await js("fetch('/status',{credentials:'same-origin'}).then(r=>r.json()).then(s=>s.quality)")) === "low");

// The CSP lists hashes of the page's own script only: a script injected into the DOM must not run.
const injected = await js(`(() => { const el = document.createElement("script"); el.textContent = "window.__injected = 1"; document.head.appendChild(el); return window.__injected === 1; })()`);
check("an injected inline script is blocked by the CSP", injected === false);

// Accessibility and diagnostics (the menu is open): every control has a name; the status chip opens a diagnostics panel.
const unnamed = await js(`[...document.querySelectorAll("button, select, textarea, input, [role=button]")].filter((el) => el.getClientRects().length && !(el.getAttribute("aria-label") || el.textContent.trim() || el.getAttribute("title") || el.getAttribute("placeholder"))).map((el) => el.id || el.tagName)`);
check("every visible control has an accessible name", unnamed.length === 0, JSON.stringify(unnamed));
await js("document.getElementById('chip').click()");
const diag = await js("document.getElementById('diag').textContent");
check("diagnostics panel shows link, video and browser", /link: ok/.test(diag) && /video: WebRTC H\.264/.test(diag) && /keyboard capture/.test(diag), JSON.stringify(diag));
await js("document.getElementById('chip').click()");
check("diagnostics panel closes", (await js("getComputedStyle(document.getElementById('diag')).display")) === "none");
// A browser without Keyboard Lock or any Fullscreen API (Safari on iPhone): capabilities say so and the Fullscreen button is hidden.
const safari = await js(`(() => {
  const saved = [Object.getOwnPropertyDescriptor(Navigator.prototype, "keyboard"), Object.getOwnPropertyDescriptor(Document.prototype, "fullscreenEnabled"), Object.getOwnPropertyDescriptor(Document.prototype, "webkitFullscreenEnabled")];
  Object.defineProperty(Navigator.prototype, "keyboard", { configurable: true, get() { return undefined; } });
  Object.defineProperty(Document.prototype, "fullscreenEnabled", { configurable: true, get() { return false; } });
  Object.defineProperty(Document.prototype, "webkitFullscreenEnabled", { configurable: true, get() { return false; } });
  const result = { caps: capabilities(), note: keyboardNote() };
  Object.defineProperty(Navigator.prototype, "keyboard", saved[0]);
  Object.defineProperty(Document.prototype, "fullscreenEnabled", saved[1]);
  if (saved[2]) Object.defineProperty(Document.prototype, "webkitFullscreenEnabled", saved[2]);
  return result;
})()`);
check("Safari-like browser: no keyboard lock, no fullscreen, a clear note", !safari.caps.keyboardLock && !safari.caps.fullscreen && /cannot capture/.test(safari.note), JSON.stringify(safari));

// Clipboard buttons: shown while running, send the box text, fetch it back (this headless Shell has no other
// clipboard owner, so the text set from the page is what comes back).
check("clipboard row is shown", (await js("getComputedStyle(document.getElementById('clip')).display")) === "flex");
await js("document.getElementById('cliptext').value = 'page clipboard test'; document.getElementById('clipsend').click()");
await sleep(1500);
check("clipboard send reports success", /Sent 19 characters/.test(await js("document.getElementById('clipmsg').textContent")));
await js("document.getElementById('cliptext').value = ''; document.getElementById('clipget').click()");
await sleep(1500);
check("clipboard get fills the box", (await js("document.getElementById('cliptext').value")) === "page clipboard test");

// A dropped link: the page notices after two failed status polls and rebuilds the video when the link returns,
// without a new login and without the session ending.
await js("window.__pc0 = pc; true");
await cdp("Network.enable");
await cdp("Network.emulateNetworkConditions", { offline: true, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
await sleep(6500);
check("link loss is noticed", (await js("linkLost")) === true);
await cdp("Network.emulateNetworkConditions", { offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
await sleep(7000);
check("link recovery rebuilt the video", (await js("linkLost === false && usingRtc === true && pc !== window.__pc0 && rtcVideo.videoWidth > 0")) === true);
check("session survived the drop", (await js("fetch('/status',{credentials:'same-origin'}).then(r=>r.json()).then(s=>s.phase)")) === "running");

await js("document.getElementById('transport').value = 'mjpeg'; document.getElementById('transport').dispatchEvent(new Event('change', {bubbles: true}))");
await sleep(3000);
const mj = await js("({shown: getComputedStyle(document.getElementById('mjpeg')).display, w: document.getElementById('mjpeg').naturalWidth})");
check("MJPEG fallback shows frames", mj.shown === "block" && mj.w === 1920, JSON.stringify(mj));

await js("document.getElementById('stop').click()");
await sleep(4000);
const end = await js("({view: document.body.dataset.view, pill: document.getElementById('hoststate').textContent, note: document.getElementById('hostnote').textContent})");
check("back on the connect screen after Disconnect", end.view === "home" && end.pill === "Ready", JSON.stringify(end));
check("the screen was restored", /screen was restored/.test(end.note), JSON.stringify(end));
check("no uncaught page errors", pageErrors.length === 0, JSON.stringify(pageErrors));
console.log(failed ? "BROWSER TEST FAILED" : "BROWSER TEST OK");
done(failed ? 1 : 0);
