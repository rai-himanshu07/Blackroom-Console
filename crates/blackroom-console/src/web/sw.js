// Makes the page installable and shows a friendly page when the laptop cannot be reached. Nothing is cached:
// the console's pages and replies are private and must always come from the laptop.
const OFFLINE = `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="5"><title>Blackroom Console</title>
<style>body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0f1115;color:#e6e8ee;font:16px system-ui,sans-serif;text-align:center}
p{color:#9aa3b2;max-width:28em;padding:0 16px}</style>
<div><h1>Can't reach the laptop</h1><p>Check that it is on, awake and on the same network (or VPN). This page tries again every few seconds.</p></div>`;

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));
self.addEventListener("fetch", (event) => {
  if (event.request.mode !== "navigate") return;
  event.respondWith(fetch(event.request).catch(() => new Response(OFFLINE, {
    status: 503, headers: { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" },
  })));
});
