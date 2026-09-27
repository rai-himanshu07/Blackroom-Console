# docs/protocol/

Wire protocol and IPC documentation for the gateway/browser and hostd/agent
interfaces defined in assessment §6.3 and Document 16. [state-machine.md](state-machine.md)
records the Phase 2 transitions and error catalogue. The production Plane A–D
message catalogue, HTTPS/WebSocket signalling and credential envelope remain
unimplemented.

The **loopback-only offline simulator** uses JSON at `/api/simulation`. `POST
/start` takes only `{ "demo_code": "SIMULATE" }` and returns a snapshot. When
that snapshot is `REMOTE_ACTIVE`, it includes a fresh, non-secret 32-hex-digit
`input_grant` and `next_sequence` starting at 1; both are null when locked.
`POST /input` requires `{ "grant_id": "...", "sequence": 1, "event": { "kind":
"key", "code": 30 } }`. Other fake event kinds are `move`, `click`, and
`scroll`. The gateway checks the current grant and exact next sequence before
dispatch and advances the sequence only after successful fake input. Repeats,
gaps, prior-grant commands and legacy unbound bodies are refused without
recording input. HTTP JSON is bounded by the core 64-KiB message limit, with
the stricter 512-byte limit for input. `input_grant` is a freshness marker,
not a credential; the public demo code is not user authentication or WebRTC
signalling. Neither endpoint can send live GNOME input.
