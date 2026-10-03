// Drives the console page in headless Chrome over the DevTools protocol: login, Start, wait for WebRTC video,
// check decoded frames, quality change, Stop. Called by headless-browser-test.sh; needs Node 22 and google-chrome.
import { spawn } from "node:child_process";

const [, , url, debugPort = "9333"] = process.argv;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chrome = spawn("google-chrome", [
  "--headless=new", "--no-sandbox", "--disable-gpu", "--ignore-certificate-errors",
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
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
const cdp = (method, params = {}) => new Promise((r) => { const id = nextId++; pending.set(id, r); ws.send(JSON.stringify({ id, method, params })); });
const js = async (expression) => {
  const r = await cdp("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 300));
  return r.result.result.value;
};

let failed = false;
const check = (name, ok, detail = "") => { console.log(`${ok ? "ok  " : "FAIL"} ${name} ${detail}`); if (!ok) failed = true; };

await cdp("Page.enable");
await cdp("Page.navigate", { url });
await sleep(2500);
check("page loaded", (await js("document.getElementById('start') !== null")) === true);

await js("document.getElementById('quality').value = 'high'; document.getElementById('quality').dispatchEvent(new Event('change', {bubbles: true}))");
await js("document.getElementById('start').click()");
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
check("Stop button is visible while running", (await js("getComputedStyle(document.getElementById('stop')).display")) !== "none");
await sleep(3000);
const stats = await js(`(async () => { const r = await fetch('/status', {credentials: 'same-origin'}); const s = await r.json(); const q = document.getElementById('rtc'); return {s, frames: q.getVideoPlaybackQuality ? q.getVideoPlaybackQuality().totalVideoFrames : -1, chip: document.getElementById('chip').textContent}; })()`);
console.log("status:", JSON.stringify(stats).slice(0, 400));
check("frames decoded", stats.frames > 5, `(${stats.frames})`);
check("server reports the encoder", typeof stats.s.webrtc_encoder === "string", `(${stats.s.webrtc_encoder})`);
check("quality applied", stats.s.quality === "high");
check("no webrtc pipeline error", !stats.s.webrtc_error, `(${stats.s.webrtc_error})`);

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
const end = await js("({chip: document.getElementById('chip').textContent, msg: document.getElementById('msg').textContent})");
check("stopped", end.chip === "idle", JSON.stringify(end));
check("display restored", /Display restored: true/.test(end.msg));
console.log(failed ? "BROWSER TEST FAILED" : "BROWSER TEST OK");
done(failed ? 1 : 0);
