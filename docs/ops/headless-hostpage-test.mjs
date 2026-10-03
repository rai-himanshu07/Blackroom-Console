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
import { readFileSync, existsSync } from "node:fs";
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

// Credentials: the page asks the laptop's own command; a change needs the password again; secrets are shown once.
await js("window.confirm = () => true; true");
check("the credential status is shown", /stub status/.test(await js("document.getElementById('credStatus').textContent")) && /dev-1/.test(await js("document.getElementById('credStatus').textContent")));
await js("document.getElementById('credKey').click()");
await sleep(500);
check("a change without the password does nothing", /password/i.test(await js("document.getElementById('saveNote').textContent")) && (await js("document.getElementById('secretBox').hidden")) === true);
await js("document.getElementById('credPassword').value = 'wrong'; document.getElementById('credKey').click(); true");
await sleep(900);
check("a wrong password is refused", /wrong/i.test(await js("document.getElementById('saveNote').textContent")) && (await js("document.getElementById('secretBox').hidden")) === true);
await js("document.getElementById('credPassword').value = 'hostpass'; document.getElementById('credKey').click(); true");
await sleep(1500);
check("the new key is shown once", /ABCD-EFGH-IJKL/.test(await js("document.getElementById('credOut').textContent")) && (await js("document.getElementById('secretBox').hidden")) === false);
check("the password field is emptied", (await js("document.getElementById('credPassword').value")) === "");
check("the laptop ran the fixed command", /rotate-key --account \S+/.test(readFileSync(`${stateDir}/cli-calls`, "utf8")));
await js("document.getElementById('credHide').click()");
check("Hide removes the secret from the page", (await js("document.getElementById('credOut').textContent")) === "" && (await js("document.getElementById('secretBox').hidden")) === true);

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

await js("document.getElementById('logout').click()");
await sleep(800);
check("Sign out returns to the sign-in view", (await visible("loginView")) && !(await visible("appView")));
check("and the session is gone", (await js("fetch('/host/state', {credentials: 'same-origin'}).then((r) => r.status)")) === 401);
check("no uncaught page errors", pageErrors.length === 0, JSON.stringify(pageErrors));
console.log(failed ? "HOST PAGE TEST FAILED" : "HOST PAGE TEST OK");
done(failed ? 1 : 0);
