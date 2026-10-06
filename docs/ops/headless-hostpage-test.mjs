// The laptop owner's settings page in headless Chrome over the DevTools protocol (called by headless-hostpage-test.sh).
// Drives the console page in headless Chrome over the DevTools protocol: login, Start, wait for WebRTC video,
// check decoded frames, quality change, Stop. Called by headless-browser-test.sh; needs Node 22 and google-chrome.
import { spawn } from "node:child_process";

const [, , url, debugPort = "9334", stateDir] = process.argv;
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

// The laptop owner's settings page: sign in, change and save settings, refused changes, sign out.
import { readFileSync, existsSync, writeFileSync } from "node:fs";
import { createHmac } from "node:crypto";
// The code an authenticator app would show for a base32 setup key.
const base32 = (text) => {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0, acc = 0; const out = [];
  for (const c of text.replace(/\s/g, "")) { acc = ((acc << 5) | alphabet.indexOf(c)) & 0xffff; bits += 5; if (bits >= 8) { bits -= 8; out.push((acc >> bits) & 255); } }
  return Buffer.from(out);
};
const appCode = (secret, seconds = Date.now() / 1000) => {
  const counter = Buffer.alloc(8); counter.writeBigUInt64BE(BigInt(Math.floor(seconds / 30)));
  const h = createHmac("sha1", base32(secret)).update(counter).digest();
  const o = h[19] & 15;
  return String((((h[o] & 127) << 24) | (h[o + 1] << 16) | (h[o + 2] << 8) | h[o + 3]) % 1000000).padStart(6, "0");
};
const sleep2 = sleep;
await cdp("Page.navigate", { url });
await sleep(1500);
const visible = (id) => js(`!document.getElementById(${JSON.stringify(id)}).hidden`);
check("the page opens on the sign-in view", (await visible("loginView")) && !(await visible("appView")) && !(await visible("savebar")));
check("nothing is readable before signing in", (await js("fetch('/host/state', {credentials: 'same-origin'}).then((r) => r.status)")) === 401);
const unnamed = await js(`[...document.querySelectorAll("button, select, input")].filter((el) => el.getClientRects().length && !(el.getAttribute("aria-label") || el.textContent.trim() || (el.closest("label") && el.closest("label").textContent.trim()))).map((el) => el.id || el.tagName)`);
check("every sign-in control has a name", unnamed.length === 0, JSON.stringify(unnamed));

const signIn = async (password) => {
  await js(`document.getElementById("password").value = ${JSON.stringify(password)}; document.getElementById("loginForm").requestSubmit(); true`);
  await sleep(1200);
};
await signIn("not the password");
check("a wrong password is refused with a message", (await js("document.getElementById('loginMsg').textContent")).length > 0 && !(await visible("appView")));
await signIn("hostpass");
check("the right password opens the settings", (await visible("appView")) && (await visible("savebar")) && !(await visible("loginView")));
check("the password field is cleared", (await js("document.getElementById('password').value")) === "");
check("the status names the laptop's state", /Ready/.test(await js("document.getElementById('statusLine').textContent")));
check("the internet card says the laptop is home-network only", /Home network only/.test(await js("document.getElementById('internetLine').textContent")));
check("nothing is changed yet, so Save is off", (await js("document.getElementById('save').disabled")) === true);
check("defaults are shown", (await js("[document.querySelector('[data-key=allow_private]').checked, document.querySelector('[data-key=allow_shared]').checked, document.querySelector('[data-key=approval]').value, document.querySelector('[data-key=max_fps]').value].join()")) === "true,true,never,0");
check("the command-line address is shown as the current value", (await js("document.querySelector('[data-key=http_listen]').value")) === "127.0.0.1:18095");
const unnamed2 = await js(`[...document.querySelectorAll("button, select, input")].filter((el) => el.getClientRects().length && !(el.getAttribute("aria-label") || el.textContent.trim() || (el.closest("label") && el.closest("label").textContent.trim()))).map((el) => el.id || el.tagName)`);
check("every settings control has a name", unnamed2.length === 0, JSON.stringify(unnamed2));

const set = (key, value) => js(`(() => { const el = document.querySelector('[data-key="${key}"]'); if (el.type === 'checkbox') el.checked = ${JSON.stringify(value)}; else el.value = ${JSON.stringify(value)}; el.dispatchEvent(new Event('change', {bubbles: true})); })()`);
if (process.env.BR_SHOT) {
  await cdp("Emulation.setDeviceMetricsOverride", { width: 900, height: 2100, deviceScaleFactor: 1, mobile: false });
  await sleep(400);
  const shot = await cdp("Page.captureScreenshot", { format: "png" });
  (await import("node:fs")).writeFileSync(process.env.BR_SHOT, Buffer.from(shot.result.data, "base64"));
  await cdp("Emulation.clearDeviceMetricsOverride");
}
await set("max_fps", "30");
await set("approval", "ask");
await set("allow_text", false);
check("a change turns Save on", (await js("!document.getElementById('save').disabled")) === true);
await js("document.getElementById('save').click()");
await sleep(1200);
check("Save reports success", /Saved/.test(await js("document.getElementById('saveNote').textContent")));
const state = await js("fetch('/host/state', {credentials: 'same-origin'}).then((r) => r.json())");
check("the laptop holds the new settings", state.config.max_fps === 30 && state.config.approval === "ask" && state.config.allow_text === false, JSON.stringify([state.config.max_fps, state.config.approval]));
check("it says they are not in effect yet", state.restart_needed === true && /restart/i.test(await js("document.getElementById('hint').textContent")));
check("host.json is on disk, owner-only", existsSync(`${stateDir}/host.json`) && (JSON.parse(readFileSync(`${stateDir}/host.json`, "utf8")).max_fps === 30));
check("Save is off again after saving", (await js("document.getElementById('save').disabled")) === true);

// The access switch: home only, a private VPN, or direct; each mode's settings are remembered.
await js("window.confirm = () => true; true");
const setAccess = (id, value) => js(`(() => { const el = document.getElementById(${JSON.stringify(id)}); el.value = ${JSON.stringify(value)}; el.dispatchEvent(new Event("change", { bubbles: true })); return true; })()`);
const config = () => js("fetch('/host/state', {credentials: 'same-origin'}).then((r) => r.json()).then((s) => s.config)");
// BR_SHOT_DIR=<dir>: also save screenshots of the whole page and of the access card for a design review.
const shotOf = async (name, selector) => {
  const dir = process.env.BR_SHOT_DIR;
  if (!dir) return;
  await cdp("Emulation.setDeviceMetricsOverride", { width: 1000, height: 5200, deviceScaleFactor: 1, mobile: false });
  await sleep(400);
  const box = await js(`(() => { const r = ${selector ? `document.querySelector(${JSON.stringify(selector)})` : "document.documentElement"}.getBoundingClientRect(); return { x: r.x, y: r.y + scrollY, width: r.width, height: r.height }; })()`);
  const r = await cdp("Page.captureScreenshot", { format: "png", captureBeyondViewport: true, clip: { ...box, scale: 1 } });
  writeFileSync(`${dir}/${name}.png`, Buffer.from(r.result.data, "base64"));
  await cdp("Emulation.clearDeviceMetricsOverride");
};
check("sign-in and credentials come before the limits, with a section menu", (await js("(() => { const ids = [...document.querySelectorAll('#appView > section')].map((s) => s.id); return ids.indexOf('credCard') < ids.indexOf('whoCard') && document.querySelectorAll('#appView nav.sections a').length >= 6 && [...document.querySelectorAll('#appView nav.sections a')].every((a) => document.querySelector(a.getAttribute('href'))); })()")) === true);
check("what is listening is shown by interface, not claimed private", /Listening now: plain http on 127\.0\.0\.1:\d+ \(this laptop only\)/.test(await js("document.getElementById('listenLine').textContent")));
await shotOf("host-1-full-page");
check("the access switch starts on home only, without a renewal banner", (await js("document.getElementById('accessMode').value")) === "home" && !(await visible("certBanner")));
await setAccess("accessMode", "vpn");
check("a pending choice is labelled apart from what is running", /Pending settings: a private VPN\. Currently running: home network only/.test(await js("document.getElementById('pendingAccess').textContent")) && (await visible("pendingAccess")));
check("choosing a VPN shows its fields and hides the direct ones", (await visible("accessVpn")) && !(await visible("accessDirect")));
await setAccess("vpnName", "laptop.tailnet.ts.net");
check("the VPN card has copyable commands for this host's name", /sudo tailscale cert --cert-file ~\/\.config\/blackroom\/tls\/cert\.pem --key-file ~\/\.config\/blackroom\/tls\/key\.pem laptop\.tailnet\.ts\.net/.test(await js("document.getElementById('vpnCommands').textContent")));
await shotOf("host-2-access-vpn", "#internetCard");
check("a mode change turns Save on", (await js("!document.getElementById('save').disabled")) === true);
await js("document.getElementById('save').click()");
await sleep(1200);
let held = await config();
check("the VPN mode is saved as a live name and remembered", held.public === false && held.public_name === "laptop.tailnet.ts.net" && held.saved_access?.vpn?.public_name === "laptop.tailnet.ts.net", JSON.stringify([held.public, held.public_name, held.saved_access]));
await setAccess("accessMode", "home");
await js("document.getElementById('save').click()");
await sleep(1200);
held = await config();
check("going back to home clears the live name and keeps the memory", held.public_name === null && held.tls_cert === null && held.saved_access?.vpn?.public_name === "laptop.tailnet.ts.net", JSON.stringify([held.public_name, held.saved_access]));
await setAccess("accessMode", "vpn");
check("switching back brings the remembered name back", (await js("document.getElementById('vpnName').value")) === "laptop.tailnet.ts.net");
await setAccess("accessMode", "direct");
await setAccess("directName", "203.0.113.7");
await setAccess("directCert", "self_signed");
check("choosing Direct also sets the safe listeners and the login", (await js("(() => { const c = collect(); return c.public === true && /^127\\.0\\.0\\.1:\\d+$/.test(c.http_listen) && c.tls_listen !== '' && c.login === 'hostd'; })()")) === true);
check("self-signed direct mode shows the fingerprint steps before any credentials", (await visible("fpSteps")) && /before/.test(await js("document.getElementById('fpSteps').textContent")));
check("a self-signed direct mode hides the certificate files", !(await visible("directFiles")) && (await visible("accessDirect")));
await shotOf("host-3-access-direct", "#internetCard");
check("switching to Direct asks for the laptop password", await visible("accessPasswordRow"));
await js("document.getElementById('save').click()");
await sleep(800);
check("saving Direct without the password is refused on the page", /laptop password/.test(await js("document.getElementById('saveNote').textContent")) && (await config()).public === false);
await js("document.getElementById('accessPassword').value = 'wrong'; document.getElementById('save').click()");
await sleep(1200);
check("a wrong password is refused by the laptop", /wrong password/.test(await js("document.getElementById('saveNote').textContent")) && (await config()).public === false);
await js("document.getElementById('accessPassword').value = 'hostpass'; document.getElementById('save').click()");
await sleep(1200);
check("direct mode is refused with the reasons while the setup is unsafe", /cannot be saved yet/.test(await js("document.getElementById('saveNote').textContent")) && (await config()).public === false, await js("document.getElementById('saveNote').textContent"));
await setAccess("accessMode", "home");
await js("document.getElementById('save').click()");
await sleep(1200);

await set("allow_private", false);
await set("allow_shared", false);
await js("document.getElementById('save').click()");
await sleep(1000);
check("a setting the laptop refuses is explained", /mode/.test(await js("document.getElementById('saveNote').textContent")));
check("and nothing was saved", JSON.parse(readFileSync(`${stateDir}/host.json`, "utf8")).allow_private === true);
await set("allow_private", true);
await set("allow_shared", true);

await js("document.getElementById('restart').click()");
await sleep(1500);
check("Restart saves and explains a console that was started by hand", /started by systemd|by hand|not started/i.test(await js("document.getElementById('saveNote').textContent")));

// A limit written by hand has no dropdown entry: it shows as itself and an unrelated save keeps it (it used to become "no limit").
const onDisk = readFileSync(`${stateDir}/host.json`, "utf8");
writeFileSync(`${stateDir}/host.json`, JSON.stringify({ ...JSON.parse(onDisk), max_session_hours: 3 }));
await js("load(true).then(() => true)");
check("a saved limit without a dropdown entry shows as itself", (await js("document.querySelector('[data-key=max_session_hours]').value")) === "3");
await set("approval", "never");
await js("document.getElementById('save').click()");
await sleep(1000);
check("an unrelated save keeps that limit", JSON.parse(readFileSync(`${stateDir}/host.json`, "utf8")).max_session_hours === 3);
// A damaged file is explained on the page, and every connection asks the owner meanwhile.
writeFileSync(`${stateDir}/host.json`, "{ nope");
await js("load(true).then(() => true)");
check("a damaged host.json is explained on the page", (await visible("configNote")) && /every connection asks the owner/.test(await js("document.getElementById('configNote').textContent")));
writeFileSync(`${stateDir}/host.json`, onDisk);
await js("load(true).then(() => true)");
check("the warning goes away once the file is valid again", !(await visible("configNote")));

// Credentials: the page asks the laptop's own command; a change needs the password again; secrets are shown once.
await js("window.confirm = () => true; true");
check("the credential status is shown", /stub status/.test(await js("document.getElementById('credStatus').textContent")));
check("trusted browsers are rows with a Forget button", (await js("[...document.querySelectorAll('#deviceList li')].map((li) => li.textContent).join('|')")).includes("Chrome on tablet") && (await js("document.querySelectorAll('#deviceList button').length")) === 1);
await js("document.getElementById('credPassword').value = 'hostpass'; document.querySelector('#deviceList button').click(); true");
await sleep(1200);
check("Forget runs the fixed revoke command for that device", /revoke-device dev-1/.test(readFileSync(`${stateDir}/cli-calls`, "utf8")) || /revoke_device|revoke-device/.test(readFileSync(`${stateDir}/cli-calls`, "utf8")));
await js("document.getElementById('credKey').click()");
await sleep(500);
check("a change without the password does nothing", /password/i.test(await js("document.getElementById('saveNote').textContent")) && (await js("document.getElementById('secretBox').hidden")) === true);
await js("document.getElementById('credPassword').value = 'wrong'; document.getElementById('credKey').click(); true");
await sleep(900);
check("a wrong password is refused", /wrong/i.test(await js("document.getElementById('saveNote').textContent")) && (await js("document.getElementById('secretBox').hidden")) === true);
check("the error also shows inside the credentials card", /wrong/i.test(await js("document.querySelector('#credCard .actionnote').textContent")) && (await js("!document.querySelector('#credCard .actionnote').hidden")) === true);
await js("document.getElementById('credPassword').value = 'hostpass'; document.getElementById('credKey').click(); true");
await sleep(1500);
check("the new key is shown once", /ABCD-EFGH-IJKL/.test(await js("document.getElementById('credOut').textContent")) && (await js("document.getElementById('secretBox').hidden")) === false);
check("the password field is emptied", (await js("document.getElementById('credPassword').value")) === "");
check("the laptop ran the fixed command", /rotate-key --account \S+/.test(readFileSync(`${stateDir}/cli-calls`, "utf8")));
const rotations = () => (readFileSync(`${stateDir}/cli-calls`, "utf8").match(/rotate-key/g) || []).length;
const rotatedBefore = rotations();
await js("document.getElementById('credPassword').value = 'hostpass'; document.getElementById('credKey').click(); document.getElementById('credKey').click(); true");
await sleep(1500);
check("a double click runs the command once", rotations() - rotatedBefore === 1, String(rotations() - rotatedBefore));
await js("document.getElementById('credHide').click()");
check("Hide removes the secret from the page", (await js("document.getElementById('credOut').textContent")) === "" && (await js("document.getElementById('secretBox').hidden")) === true);

// Authenticator app: scan the QR code or type the key, then one right code stores it.
check("the page says no authenticator exists yet", /No authenticator/.test(await js("document.getElementById('totpStatus').textContent")));
await js("document.getElementById('totpStart').click()");
await sleep(600);
check("starting without the password shows nothing", (await js("document.getElementById('totpBox').hidden")) === true);
await js("document.getElementById('totpPassword').value = 'hostpass'; document.getElementById('totpStart').click(); true");
await sleep(1200);
check("the setup box opens", (await js("document.getElementById('totpBox').hidden")) === false);
check("the QR code is drawn", (await js("new Promise((r) => { const i = document.getElementById('totpQr'); if (i.complete) r(i.naturalWidth); else i.onload = () => r(i.naturalWidth); })")) > 100);
const manualKey = await js("document.getElementById('totpSecret').textContent");
check("the key to type by hand is shown in groups", /^([A-Z2-7]{4} )+[A-Z2-7]{1,4}$/.test(manualKey), manualKey);
check("the account name to type is shown", /Blackroom Console:\S+/.test(await js("document.getElementById('totpAccount').textContent")));
if (process.env.BR_SHOT) {
  await cdp("Emulation.setDeviceMetricsOverride", { width: 900, height: 900, deviceScaleFactor: 1, mobile: false });
  await js("document.getElementById('totpCard').scrollIntoView(); true");
  await sleep(400);
  const shot2 = await cdp("Page.captureScreenshot", { format: "png" });
  (await import("node:fs")).writeFileSync(process.env.BR_SHOT.replace(/\.png$/, "-totp.png"), Buffer.from(shot2.result.data, "base64"));
  await cdp("Emulation.clearDeviceMetricsOverride");
}
await js("document.getElementById('totpCode').value = '000000'; document.getElementById('totpVerify').click(); true");
await sleep(900);
check("a wrong code is refused and nothing is stored", /not right/.test(await js("document.getElementById('totpMsg').textContent")) && !existsSync(`${stateDir}/hostd/totp-credentials`));
await js(`document.getElementById('totpCode').value = ${JSON.stringify(appCode(manualKey))}; document.getElementById('totpVerify').click(); true`);
await sleep(1200);
check("one right code from the app confirms it", /confirmed and saved/.test(await js("document.getElementById('saveNote').textContent")) && (await js("document.getElementById('totpBox').hidden")) === true);
check("the secret is now stored, owner-only", existsSync(`${stateDir}/hostd/totp-credentials`) && readFileSync(`${stateDir}/hostd/totp-credentials`, "utf8").includes(manualKey.replace(/\s/g, "")));
check("the login authority was asked to pick it up", /--user try-restart remote-hostd/.test(readFileSync(`${stateDir}/systemctl-calls`, "utf8")));
check("the page now says an authenticator exists", /An authenticator is set up/.test(await js("document.getElementById('totpStatus').textContent")));
check("the key is gone from the page", (await js("document.getElementById('totpSecret').textContent")) === "" && (await js("document.getElementById('totpQr').getAttribute('src')")) === null);

// Sign-in method: the page explains where things stand and refuses a method the laptop cannot serve yet.
check("the page says how clients sign in now", /one-time address/.test(await js("document.getElementById('loginNote').textContent")) && /not running/.test(await js("document.getElementById('loginNote').textContent")));
await set("login", "hostd");
await js("document.getElementById('save').click()");
await sleep(1000);
check("hostd sign-in is refused until the login authority runs", /login authority/i.test(await js("document.getElementById('saveNote').textContent")));
await set("login", "");
await js("document.getElementById('credPassword').value = 'hostpass'; document.getElementById('loginSetup').click(); true");
await sleep(1500);
check("Set up the login authority runs setup first and shows its output once", /setup --state-dir/.test(readFileSync(`${stateDir}/cli-calls`, "utf8")) && (await js("document.getElementById('secretBox').hidden")) === false);
await js("document.getElementById('credHide').click()");

// Lock-screen access: the switch drives the extension, and turning it on needs the password.
check("the lock-screen switch starts off", (await js("document.getElementById('lockOn').checked")) === false && (await js("document.getElementById('lockOn').disabled")) === false);
await js("document.getElementById('lockOn').click()");
await sleep(1000);
check("turning it on without the password is refused", (await js("document.getElementById('lockOn').checked")) === false && !existsSync(`${stateDir}/ext-on`));
await js("document.getElementById('lockPassword').value = 'hostpass'; document.getElementById('lockOn').click(); true");
await sleep(1500);
check("with the password it turns on", existsSync(`${stateDir}/ext-on`) && /enable blackroom-locked-remote/.test(readFileSync(`${stateDir}/ext-calls`, "utf8")));
check("and the page says what that means", /On:/.test(await js("document.getElementById('lockNote').textContent")));
await js("document.getElementById('lockOn').click()");
await sleep(1500);
check("turning it off needs no password", !existsSync(`${stateDir}/ext-on`) && /Off:/.test(await js("document.getElementById('lockNote').textContent")));
// R23: "Off" is only pending while a remote session still runs.
await js("renderLock({lockscreen: {installed: true, enabled: false, active: false, pending_off: true}})");
check("off with a session running is shown as pending off", /Pending off/.test(await js("document.getElementById('lockNote').textContent")));
await js("renderLock({lockscreen: {installed: true, enabled: false, active: false, pending_off: false}})");
check("off with no session is plain off", /^Off:/.test(await js("document.getElementById('lockNote').textContent")));

// U15: unsaved changes are not lost to a stray Sign out.
await js("(() => { const el = document.querySelector('[data-key=\"allow_clipboard\"]'); el.checked = !el.checked; el.dispatchEvent(new Event('change', {bubbles: true})); })()");
await js("window.confirm = () => false; document.getElementById('logout').click(); true");
await sleep(500);
check("Sign out with unsaved changes asks first and stays when declined", (await visible("appView")) === true && (await js("!document.getElementById('save').disabled")) === true);
await js("window.confirm = () => true; true");
await js("document.getElementById('logout').click()");
await sleep(800);
check("Sign out returns to the sign-in view", (await visible("loginView")) && !(await visible("appView")));
check("and the session is gone", (await js("fetch('/host/state', {credentials: 'same-origin'}).then((r) => r.status)")) === 401);
check("no uncaught page errors", pageErrors.length === 0, JSON.stringify(pageErrors));
console.log(failed ? "HOST PAGE TEST FAILED" : "HOST PAGE TEST OK");
done(failed ? 1 : 0);
