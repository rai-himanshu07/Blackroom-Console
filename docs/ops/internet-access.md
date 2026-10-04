# Using the console from outside your home network

Run `blackroom internet` on the laptop. It asks what you have, checks everything on this laptop, shows what it would change
and saves only after you agree. `blackroom setup` offers it as its last step (the default answer is "no, home network only").
`blackroom internet --check` prints the current state at any time. Nothing here changes your router or runs sudo for you, and
every check is local: only a phone on mobile data can prove the connection works.

## Which way fits you?

The question is not "static or dynamic IP" but **"can anything on the internet open a connection to your router, and on to
this laptop?"**

| Your situation | Use |
|---|---|
| Not sure, no access to the router, your provider shares one address between customers (CGNAT), or you want nothing exposed | **A VPN such as Tailscale** (way 1). Works with every provider. |
| The router accepts incoming connections **and** you have a name (a dynamic-DNS name, or your own domain) | **Direct, with a real certificate** (way 2a). |
| The router accepts incoming connections and you have **only a static IP address**, no name | **Direct, with the console's own self-signed certificate** (way 2b): works, but weaker. A free dynamic-DNS name makes it way 2a. |
| You only use it at home | Do nothing: the default. |

How to tell whether your router can accept incoming connections: open the router's status page and read its **WAN** (internet)
address, then compare it with what a "what is my IP" website shows. If they differ, or the WAN address is in `100.64.0.0` to
`100.127.255.255` or is a private address (`10.x`, `172.16-31.x`, `192.168.x`), a second device or your provider sits in front
of you and no direct connection can work: use the VPN. A static IP does not help against CGNAT, an ISP that blocks inbound
ports, or a second router that you forget to forward through.

## Way 1: a VPN (Tailscale)

Nothing is opened to the internet, so internet mode stays off. Media inside a tailnet is a direct WireGuard path: no TURN, no
port forwarding, and it works behind CGNAT.

1. Install Tailscale on the laptop and on the phone or tablet, sign in to the same account, and in the Tailscale admin console
   switch on MagicDNS and HTTPS certificates.
2. Run `blackroom internet`, choose 1, type the laptop's Tailscale name (like `laptop.tailnet-name.ts.net`). If no certificate
   exists yet it prints the commands (they need sudo, which you run yourself):
   ```
   mkdir -p ~/.config/blackroom/tls && chmod 700 ~/.config/blackroom/tls
   sudo tailscale cert --cert-file ~/.config/blackroom/tls/cert.pem --key-file ~/.config/blackroom/tls/key.pem laptop.tailnet-name.ts.net
   sudo chown $USER: ~/.config/blackroom/tls/*.pem && chmod 600 ~/.config/blackroom/tls/*.pem
   ```
   Run `blackroom internet` again. Renew about every 60 days with the same command; the console picks the new files up within
   six hours, but only if they are valid and still cover the name.
3. On the phone (Tailscale connected, Wi-Fi off) open `https://laptop.tailnet-name.ts.net:8443/`.

Any other VPN works the same way; only Tailscale can issue the certificate. Without a certificate, press Enter at the name
question: the console keeps its own self-signed certificate and you accept the browser warning once.

## Way 2: directly over the internet

Reachable by strangers' scanners, so it is stricter: internet mode (`public`) refuses to start unless login goes through
hostd (password + authenticator + key), https is on, the plain-http port is on 127.0.0.1 only, and the certificate checks
out. Cookies are `Secure`; HSTS is sent only with a real certificate.

**On the router and the laptop (the wizard prints the exact table for your settings):**
- Give the laptop a fixed LAN address (a DHCP reservation in the router), so the forward does not break.
- Forward **TCP 8443** (or external 443 to internal 8443; the console never needs to run as root) to the laptop.
- For WebRTC video also forward the **UDP range** the wizard sets (default 50000-50100). Without it, video falls back to MJPEG
  over the same https port: slower, no laptop sound, but it works.
- If a firewall is active on the laptop (`sudo ufw status`), allow the same ports: `sudo ufw allow 8443/tcp` and
  `sudo ufw allow 50000:50100/udp`.
- Do **not** switch on UPnP for this; forward by hand.
- IPv6: the https listener is IPv4 (`0.0.0.0:8443`). If your phone's network is IPv6-only, add an AAAA record only after
  allowing inbound 8443 in the router's IPv6 firewall; the console does not do that for you.
- Test from the phone on **mobile data** with Wi-Fi off. From your own Wi-Fi the address often fails on routers without NAT
  loopback ("hairpin"); that is not a console problem.

### 2a. You have a name and a real certificate

1. A name that follows your address (a dynamic-DNS service, or your own domain with an A record).
2. A certificate for that name, for example with Let's Encrypt (run as root; port 80 must reach this laptop while it is
   issued, or use a DNS plugin):
   ```
   sudo certbot certonly --standalone -d home.example.org
   ```
   Put copies where the console reads them, now and on every renewal (a certbot deploy hook,
   `/etc/letsencrypt/renewal-hooks/deploy/blackroom.sh`):
   ```
   install -d -m 700 -o YOURUSER /home/YOURUSER/.config/blackroom/tls
   install -m 644 -o YOURUSER /etc/letsencrypt/live/home.example.org/fullchain.pem /home/YOURUSER/.config/blackroom/tls/cert.pem
   install -m 600 -o YOURUSER /etc/letsencrypt/live/home.example.org/privkey.pem  /home/YOURUSER/.config/blackroom/tls/key.pem
   ```
3. `blackroom internet`, choose 2, answer yes to "can your router accept incoming connections", choose 1 (a name), type the
   name. The check reads the certificate: it must be in date, cover the name, belong to the key, and not be self-signed.

### 2b. You have only a static IP address

No certificate authority issues a normal certificate for a bare address that this project has verified (Let's Encrypt has
begun to issue certificates for IP addresses; this has not been tried here, so check its current documentation if you want
that route). The console can instead make its own self-signed certificate for the address, by your explicit choice:

- Choose 2, "yes" to the router question, then 2 (only an IP address), type the IP, and answer yes to the self-signed
  question.
- After the restart, `blackroom internet --check` (or the host page) shows the certificate **fingerprint**. On the first
  visit, open the browser's certificate details and compare the SHA-256 fingerprint with it before you continue.
- A first visit that you accept without comparing is trust-on-first-use. If the warning appears again later, the
  certificate changed: do not continue until you have compared the new fingerprint. HSTS is not sent, and installing the page
  as an app needs a trusted certificate (the page still works in the browser).
- A name with a real certificate (2a) is safer, and free dynamic-DNS names exist.

### Optional: a TURN relay

Only for mobile networks that block direct media. Without it the https video fallback is used. If you want one, install
coturn on this laptop or a small server and use a `turnserver.conf` like this (if coturn itself sits behind NAT, also add
`external-ip=PUBLIC-ADDRESS/PRIVATE-ADDRESS`; read coturn's documentation about IPv6 peers):
```
listening-port=3478
fingerprint
use-auth-secret
static-auth-secret=<20 or more random characters>
realm=home.example.org
min-port=50200
max-port=50400
no-cli
no-multicast-peers
denied-peer-ip=0.0.0.0-0.255.255.255
denied-peer-ip=10.0.0.0-10.255.255.255
denied-peer-ip=100.64.0.0-100.127.255.255
denied-peer-ip=127.0.0.0-127.255.255.255
denied-peer-ip=169.254.0.0-169.254.255.255
denied-peer-ip=172.16.0.0-172.31.255.255
denied-peer-ip=192.168.0.0-192.168.255.255
```
Forward TCP and UDP 3478 and UDP 50200-50400 to it. Store the same secret for the console (`umask 077; printf '%s' 'THE-SECRET' >
~/.config/blackroom/turn-secret`) and give `blackroom internet` its address
(`turn:home.example.org:3478?transport=udp`) and that path. The secret file must be a regular file of yours with mode 600; it
is never written to host.json, the page or the logs. Clients on very restrictive networks may need `turns:` over TCP 443,
which this setup does not provide.

## What the wizard writes

All of it goes to `~/.local/share/blackroom-console/host.json` (owner only), the same file as the host settings page, and
takes effect after the console restarts (`systemctl --user restart blackroom-console.service`; it ends a running session):

| Key | Meaning |
|---|---|
| `public` | internet mode: the strict start-up rules above |
| `public_name` | the name or IP clients type |
| `public_cert` | `ca` (files from an authority) or `self_signed` (the console's own, for a bare IP) |
| `tls_cert`, `tls_key` | certificate and key files (absolute paths) |
| `stun` | STUN servers (default the wizard offers: `stun:stun.l.google.com:19302`: it only sees your public address) |
| `turn`, `turn_secret_file`, `turn_ttl_secs` | TURN relays, the secret's file, credential lifetime |
| `ice_ports` | UDP range for media, `MIN-MAX` |

If `host.json` exists but cannot be read, the console starts with its network listeners on this laptop only (127.0.0.1),
asks you to approve every connection, and shows the reason on the host settings page; save the page, or fix the file, then
restart. The host settings page shows the state read-only and refuses to save an internet mode that the next start would
refuse. The older command-line flags (`--public`, `--tls-cert`, `--stun`, `--turn`, `--ice-port-range` and the `BR_*`
variables of `docs/ops/console.sh`) still work; what `host.json` sets wins.

## Before you expose anything

- `blackroom ... status` shows `remote_access: enabled` and your credentials exist; run `rotate-key` and `recovery-codes` and
  store them.
- Use a strong Linux password; it is guessed online. The authenticator code and the key are **not phishing-proof**: check the
  address in the bar.
- `sudo journalctl -t unix_chkpwd -t pam-auth-helper --since today` and `blackroom ... logs --tail 50` show failed attempts.
- `blackroom ... disable` closes remote access at once from anywhere (SSH works); `enable` re-opens it.
- The panel stays blank and the keyboard grabbed while a Private session runs; keep a second-device SSH session as before.

## What this does not protect against

Direct internet access makes this login discoverable by scanners. Authentication and rate limits reduce, but do not
eliminate, password guessing, lockout or denial-of-service risks. The password, authenticator code and Remote Access Key are
not phishing-proof: check the exact https address and never bypass a certificate warning. This does not protect against a
compromised laptop or browser. The checks are local and do not prove internet connectivity.

## If it does not connect

- The login page loads but video fails on mobile data: media needs STUN, and TURN behind some carriers. The status chip shows
  "MJPEG" when WebRTC failed; that fallback works over https alone.
- Nothing loads from outside: the forward, the firewall, CGNAT or a second router; test with Wi-Fi off. `blackroom internet
  --check` proves only the laptop's side.
- A certificate warning on a real name: the file must be the full chain (`fullchain.pem`), the name must match, the clock must
  be correct. The check names what is wrong.
- The console does not start after a change: `journalctl --user -u blackroom-console -n 30` says what internet mode refused;
  `blackroom internet --check` lists the same problems. Turn internet mode off with `blackroom internet` (choose 3) or in the
  host settings page.
- `refusing to start in --public mode`: the message lists what to fix.
