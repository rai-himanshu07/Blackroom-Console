# Using the console from outside your LAN

The console is safe to expose only with the login through hostd (password + authenticator + key or trusted browser), https with a
real certificate, and the checks below. Pick **one** recipe. Recipe A is recommended: nothing is exposed to the internet.

| | A. Tailscale (WireGuard mesh) | B. Port forward + dynamic DNS | C. A VPN you already run |
|---|---|---|---|
| Ports opened on your router | none | TCP 8443 (https), UDP 50000-50100 (media), TURN ports | none |
| Client needs | the Tailscale app on the tablet/phone | only a browser | the VPN app |
| Certificate | `tailscale cert` (real, free) | Let's Encrypt (certbot) | self-signed is acceptable inside the VPN |
| Who can reach the login | only your own devices | anyone on the internet | VPN members |
| Works behind carrier-grade NAT on the client | yes | needs TURN | yes |

## What the console provides (all recipes)

`blackroom-console` flags (or the `BR_*` variables of `docs/ops/console.sh`):

| Flag | Variable | Purpose |
|---|---|---|
| `--tls-cert FILE --tls-key FILE` | `BR_TLS_CERT`, `BR_TLS_KEY` | real certificate for `--tls-listen`; re-read every 6 h, so renewals need no restart |
| `--public` | `BR_PUBLIC=1` | refuses to start unless login is through hostd, https uses a real certificate, and the plain-http port is loopback only; cookies are `Secure`, HSTS is sent |
| `--stun stun:host:port` | `BR_STUN` | STUN server for the server and the browser |
| `--turn turn:host:port?transport=udp` | `BR_TURN` | TURN relay (coturn) |
| `--turn-secret-file FILE` | `BR_TURN_SECRET_FILE` | coturn `static-auth-secret` (mode 600); TURN passwords are derived per login and expire (`--turn-ttl-secs`, default 1 h) |
| `--ice-port-range MIN-MAX` | `BR_ICE_PORTS` | UDP range used for media, so it can be forwarded |

Always on for https listeners: `Secure` HttpOnly SameSite=Strict cookies, a Content-Security-Policy, `X-Frame-Options: DENY`,
`nosniff`, no referrer. The browser fetches `/ice` after login, so TURN credentials are never in the page.

Online-guessing limits (hostd): 5 failures per source and account lock that source for 15 minutes (doubling to 4 h); 30 failures across
all sources lock the account for 5 minutes (doubling to 1 h), **except** for a trusted browser, which is not locked out by strangers.

## Recipe A: Tailscale (recommended)

1. Install Tailscale on the laptop and on the tablet/phone; sign in to the same tailnet. Enable MagicDNS and HTTPS certificates in the
   Tailscale admin console.
2. Get a certificate for the laptop's tailnet name (replace `laptop.tailnet-name.ts.net`):
   ```
   mkdir -p ~/.config/blackroom/tls && chmod 700 ~/.config/blackroom/tls
   sudo tailscale cert --cert-file ~/.config/blackroom/tls/cert.pem --key-file ~/.config/blackroom/tls/key.pem laptop.tailnet-name.ts.net
   sudo chown $USER: ~/.config/blackroom/tls/*.pem && chmod 600 ~/.config/blackroom/tls/*.pem
   ```
   Renew about every 60 days with the same command; the console picks it up within 6 hours.
3. Start the console (hostd must be running):
   ```
   BR_HOSTD=1 BR_TLS_CERT=$HOME/.config/blackroom/tls/cert.pem BR_TLS_KEY=$HOME/.config/blackroom/tls/key.pem \
   BR_STUN=stun:stun.l.google.com:19302 docs/ops/console.sh
   ```
4. On the tablet (Tailscale on, any network): open `https://laptop.tailnet-name.ts.net:8443/`.

Media inside a tailnet is a direct WireGuard path, so no TURN is needed. Leave `--public` off: nothing faces the internet.

## Recipe B: port forward + dynamic DNS + Let's Encrypt

Use only if recipe A is not possible. The login becomes reachable by strangers; the three factors and the limits above are the defence.

1. A name that follows your home IP (a dynamic-DNS service) and a router that supports port forwarding. If your provider uses
   carrier-grade NAT (the router's WAN address is 100.64.x.x or differs from what a "what is my IP" site shows), forwarding cannot work: use recipe A.
2. Forward on the router to the laptop's LAN address: **TCP 8443 to 8443**, **UDP 50000-50100 to the same ports**.
3. Certificate (Let's Encrypt, run as root once, then renew automatically; replace the name):
   ```
   sudo certbot certonly --standalone -d home.example.org      # needs TCP 80 forwarded during issuance, or use a DNS-01 plugin
   ```
   Make the files readable by you only, for example in `/etc/letsencrypt/renewal-hooks/deploy/blackroom.sh`:
   ```
   install -m 600 -o YOURUSER /etc/letsencrypt/live/home.example.org/fullchain.pem /home/YOURUSER/.config/blackroom/tls/cert.pem
   install -m 600 -o YOURUSER /etc/letsencrypt/live/home.example.org/privkey.pem   /home/YOURUSER/.config/blackroom/tls/key.pem
   ```
4. TURN for clients on restrictive networks (mobile carriers): install coturn on the laptop or any small server and use this `turnserver.conf`:
   ```
   listening-port=3478
   fingerprint
   use-auth-secret
   static-auth-secret=<the same 20+ random characters as in your secret file>
   realm=home.example.org
   min-port=50200
   max-port=50400
   no-cli
   no-multicast-peers
   denied-peer-ip=10.0.0.0-10.255.255.255
   denied-peer-ip=172.16.0.0-172.31.255.255
   denied-peer-ip=192.168.0.0-192.168.255.255
   denied-peer-ip=127.0.0.0-127.255.255.255
   ```
   Forward TCP/UDP 3478 and UDP 50200-50400 to it. Store the secret: `umask 077; printf '%s' 'THE-SECRET' > ~/.config/blackroom/turn-secret`.
5. Start the console (`BR_PUBLIC=1` also puts the plain-http port on loopback and refuses weak settings):
   ```
   BR_HOSTD=1 BR_PUBLIC=1 BR_TLS_CERT=$HOME/.config/blackroom/tls/cert.pem BR_TLS_KEY=$HOME/.config/blackroom/tls/key.pem \
   BR_STUN=stun:stun.l.google.com:19302 BR_TURN="turn:home.example.org:3478?transport=udp turn:home.example.org:3478?transport=tcp" \
   BR_TURN_SECRET_FILE=$HOME/.config/blackroom/turn-secret BR_ICE_PORTS=50000-50100 docs/ops/console.sh
   ```
6. From a phone on mobile data open `https://home.example.org:8443/`.

## Before you expose anything

- `blackroom ... status` shows `remote_access: enabled` and your credentials exist; run `rotate-key` and `recovery-codes` and store them.
- Use a strong Linux password; it is guessed online, and the authenticator code and key are not phishing-proof (check the address in the bar).
- `sudo journalctl -t unix_chkpwd -t pam-auth-helper --since today` and `blackroom ... logs --tail 50` show failed attempts.
- `blackroom ... disable` closes remote access at once from anywhere (SSH works); `enable` re-opens it.
- The panel stays blank and the keyboard grabbed while a session runs; keep a second-device SSH session as before.

## If it does not connect

- Login page loads but video fails on mobile data: media needs STUN, and TURN behind carrier-grade NAT. Check the page's status chip: "MJPEG" means WebRTC failed and the
  slower fallback is used (works over https alone, but needs more bandwidth).
- Certificate warning on a real name: the file is the full chain (`fullchain.pem`), the name matches, the clock is correct.
- `refusing to start in --public mode`: the message lists what to fix.
