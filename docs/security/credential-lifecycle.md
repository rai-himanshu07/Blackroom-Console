# Credential lifecycle (Phase 11)

Every credential is generated here, shown once by the verb that creates it, and stored only as a
verifier. `blackroom --state-dir <dir> ...` is the operator surface (default state directory
`~/.local/share/blackroom-console/hostd`).

| Credential | Create | Stored as | Rotate / reset | Revoke |
|---|---|---|---|---|
| Linux password | not managed here | never stored (PAM checks it) | change it with Linux | n/a |
| Authenticator (TOTP) | `enroll --account A` (secret + otpauth URI once) | secret in `totp-credentials` (0600; HMAC needs the secret) plus last accepted step | re-enrol requires a new account name or a manual, local reset of the file (no remote reset exists, Doc 03 §14) | delete the account entry locally |
| Remote Access Key | `rotate-key --account A` (43 chars, 256 bits, base64url, once) | salted SHA-256 in `access-keys` | `rotate-key` again: the old key stops working at once | `rotate-key --revoke-devices` is the full reset |
| Recovery codes | `recovery-codes --account A` (10 codes, once) | salted SHA-256 in `recovery-codes`, with a used flag | running it again replaces the whole set | each code works once and is marked spent before login succeeds |
| Trusted device | login with the key plus a label (`trust_label`), credential returned once | salted SHA-256 in `trusted-devices` | register a new device | `revoke-device <id>`; `devices --account A` lists |
| Session | successful login | memory only | n/a | `revoke-session <id>`, `revoke-client`, `revoke-all`, `disable`, any epoch change, expiry |

Rules that hold everywhere:

- A secret is never logged, never part of a status or listing reply, and never in an error message
  (`RT-FILE-005` greps the audit log, status and session listing for every secret used in the flow).
- Rotation and revocation write the whole file atomically (temporary file, fsync, rename, directory
  fsync); a crash leaves the old content or the new, never a mix. Leftover temporary files are ignored.
- A damaged, symlinked, wrongly owned, wrongly moded, oversized, unknown-field or newer-schema file
  makes remote access **disabled** ("REMOTE ACCESS DISABLED") rather than guessing; an older schema is
  migrated step by step.
- Changing the Linux password needs no action here (PAM reads the live password). Rotating the key
  does not revoke trusted devices unless asked.
- `disable` writes its flag directly first, so it works when hostd is down; `enable` goes through
  hostd when it runs (so the optional polkit gate applies) and directly otherwise.

Not done: a built-in TOTP reset/re-enrol flow with recovery-code authorisation (resetting TOTP is a
local operator action for now), key escrow, and any encryption at rest beyond owner-only files.
