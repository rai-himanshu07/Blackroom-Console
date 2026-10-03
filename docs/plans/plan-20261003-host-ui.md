# Plan: host (laptop) settings, separate from the client (2026-10-03)

Owner's answers (asked, not assumed): the host gets its own settings page, local to the laptop; host limits are enforced and
the client can only choose inside them; the client keeps device choices on the device, the host keeps policy on the laptop.

## Decisions (from the owner)
- **Host page:** a local web page on `127.0.0.1` only, a separate port from the client, behind the Linux password (PAM).
  Opened from the indicator menu.
- **Host settings:** allowed modes, forced lock on disconnect, max session length and idle time, max fps and bitrate, allow
  laptop sound / clipboard / text typing, sound output device, network (ports, https certificate, internet mode), login method
  and credentials (full management, secrets shown once), approve-each-connection (Ask or Never ask), indicator options,
  start at login.
- **Approval:** a host setting, Ask or Never ask (default Never ask so remote use while away works); Ask means Accept/Deny on
  the laptop; no answer in 30 s = denied.
- **Applying changes:** every host change restarts the console (a running session ends; the page warns and asks first).
- **Client:** mode choice, picture, sound, scale and pointer, keyboard options. Saved on the device. "Remember this device" =
  the existing trusted-device login. Saved servers (several laptops) and connection profile names: skipped for now.

## Chunks
- [x] **T1. Host config and policy.** `host.rs` (`host.json` beside `profile.json`, 0600, atomic, validated), enforced in
  `start`, rates, audio, clipboard and text typing; `Status.policy` for the client. Check: unit tests, adversarial test,
  headless modes test with a restrictive policy.
- [x] **T2. Approval.** pending request, 30 s timeout deny, D-Bus `Pending`/`Approve`/`Deny`, indicator notice and menu rows,
  client "waiting for the laptop owner". Check: unit tests, headless Shell test.
- [ ] **T3. Host page.** loopback listener, PAM login, settings form (policy, sound device, network, indicator, approval),
  save and restart. Check: adversarial tests (origin, no anonymous access, loopback only), headless Chrome walkthrough.
- [ ] **T4. Credentials, autostart.** credentials section through the `blackroom` CLI verbs (status, rotate key, new
  authenticator, recovery codes, devices, revoke, disable remote access); start at login with systemd.
- [ ] **T5. Client split.** host-set limits greyed out with a note; device choices in localStorage; host-level controls leave
  the client settings sheet; waiting-for-approval state.
- [ ] **T6. Docs, gate, `.deb`, notes.**

## Risks
The host page can change network, login and credentials: it must not be reachable from any other machine or a web page in
the owner's browser (loopback only, Host and Origin checks, password, session cookie, CSP). Secrets shown on the page appear
on screen and in the browser's memory only; they are not stored or logged. Approval Ask blocks remote use when nobody is at
the laptop; the default is Never ask.
