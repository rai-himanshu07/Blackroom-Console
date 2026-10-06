# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/); versions: SemVer.

## [0.1.0] - unreleased (technical preview)

### Added
- Remote console: WebRTC H.264 (NVENC, software fallback) and MJPEG, pointer, keyboard, touch, scroll, Opus laptop sound, text
  clipboard, Private and Shared sessions, per-device settings, installable web app.
- Safety: display restore, lock on disconnect (when set), input-grab release, a restore timer after a crash, an emergency chord
  with a durable stop marker, a labelled End session control, a warning that stays until Disconnect is confirmed, and a session
  menu block showing what the session confirmed doing to the laptop.
- Login: Linux password, authenticator, Remote Access Key or trusted browser; rate limits; revocation.
- Laptop side: host settings page, top-bar indicator, lock-screen extension, approve-each-connection, `blackroom` command
  (`setup`, `reset`, `repair`, `internet`, credentials), `.deb`.
- Outside access: `blackroom internet` (private VPN recommended, Home only, Direct), certificate checks and renewal reminders.
- F8 opens the session menu from the keyboard; the menu and End session buttons can be dragged and stay where they are put (per device).
- Host settings drive setup: a First steps card, "Keyboard blocking" (a password dialog on the laptop through polkit), ports as plain numbers with a clear refusal when another program holds one.
- Host settings: one "Enable editing" bar (laptop password once, valid for 5 minutes, Lock now) replaces the password field in each section; a sticky section menu with grouped sections; trusted browsers show readable dates and hide forgotten ones.

### Known limits
See the support table in `docs/ops/compatibility-matrix.md`: Safari/iOS, certificate renewal, other hardware and a soak are untested.
