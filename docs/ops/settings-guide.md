# Settings guide

Two places, two jobs.

- **The client page** (the tablet or phone): how *this device* connects. Choices are saved on the device, one set per
  laptop address, and can only go as far as the laptop owner allows.
- **The host page** (on the laptop): what the laptop allows. See "Host settings" below.

Open the console page and sign in. The connect screen shows the laptop's name and state, two modes and a **Settings**
button. **Reset to defaults** forgets this device's choices and goes back to the laptop's defaults (`profile.json`, which
only supplies the starting values). A browser that cannot keep site data (some private modes) falls back to saving the
choices on the laptop. Anything the owner has limited is greyed out, with a line saying "Set by the laptop owner".

## Modes

| | Private (default) | Shared |
|---|---|---|
| Laptop panel | blank (a virtual monitor replaces it) | unchanged, you see what the tablet sees |
| Laptop keyboard and touchpad | blocked | work as usual |
| Screen lock when the session ends | on | off |
| Needs the restore watchdog and input grant | yes | no |

Pick a mode, then change any single setting; the mode label turns to "Custom" when the mix matches neither preset.

## Session (applied at Connect)

| Setting | What it does | Range |
|---|---|---|
| Blank the laptop screen | Private mode's virtual monitor and blank panel | on/off |
| Block laptop keyboard and touchpad | Grab the built-in input devices | on/off |
| Lock the laptop when I disconnect | Lock the screen when the session ends, whatever ended it. Off is the Shared default | on/off |
| Resolution | Size of the private screen: the laptop's own, match this device, or 1280x720 / 1920x1080 / 2560x1440 (the server accepts 640-3840 x 360-2160) | blank screen only |
| Laptop pointer in the picture | Draw the laptop's own pointer into the video; the page hides its pointer marker | on/off |
| Laptop sound | Send the laptop's sound to the tablet (WebRTC video only) | on/off |
| Frame rate limit | 0 follows the quality level | 5-60 fps |
| Bitrate | 0 follows the quality level | 300-30000 kbit/s |
| Heartbeat timeout ("link lost after") | Seconds of browser silence before the session ends | 5-120 s |
| Idle limit | Minutes without remote input before the session ends; 0 = never | up to 1440 |
| Session length limit | Hours a session may last; 0 = no limit | up to 72 |

Frame rate and bitrate also change live: open **Settings** from the session menu and pick new values.

## This device (more)

Quality level, picture scale (fit, stretch, 1:1 with scrolling to follow the pointer), volume, the pointer marker
over the picture, touch mode (trackpad or direct), the keyboard (key presses for a US layout, or characters typed with
keysyms for any laptop layout; Command acts as Control for Mac and iPad keyboards).

## Installing the page as an app

The connect screen offers **Install** when the browser allows it. Browsers only offer this on a trusted https address
(a Tailscale certificate or Let's Encrypt, see `internet-access.md`) or on `localhost`; with the self-signed certificate
the page works but cannot be installed. On iPhone and iPad use Share, then Add to Home Screen. The service worker caches
nothing: it only makes the app installable and shows a "can't reach the laptop" page that retries every 5 seconds.

## Host settings (the laptop owner)

Open the indicator menu and choose **Host settings...**, or browse to `http://localhost:8090/` on the laptop. The page is
reachable from the laptop only (a loopback address; other machines and web pages cannot reach it) and asks for the laptop
account's password. Everything is saved to `host.json` next to `profile.json` and takes effect when the console restarts
(**Save and restart the console**; a running session ends, and the page asks first). Settings in `host.json` win over
command-line flags.

| Section | What you set |
|---|---|
| Who may connect | allow Private and/or Shared sessions; approving each connection: **Never ask** (default, so remote use works while you are away) or **Ask** (Accept or Deny on the laptop from a notification or the indicator menu; no answer in 30 seconds is a Deny) |
| Limits a client cannot exceed | lock on disconnect forced on or off (or the client chooses), longest session, longest idle time, highest frame rate and bitrate |
| What a client may use | laptop sound, clipboard, typing text, which sound output is sent |
| Network | the plain-http and https addresses, a certificate and key from a real authority, internet mode (`--public`). Read-only status of access from outside; to set it up run `blackroom internet` on the laptop (`docs/ops/internet-access.md`) |
| How clients sign in | **Linux password + authenticator code + key** (recommended: a normal sign-in page on the tablet) or the one-time address with a token. Choosing the first needs the login authority: press **Set up the login authority** once under Credentials (it shows the QR, key and recovery codes once), then choose it, Save and restart. If the authority is not running when the console starts, the one-time address is used and the page says so |
| Authenticator app | **Add or replace the authenticator** in two steps: scan the QR code (or type the setup key by hand, shown in groups of four with the account name), then type one 6-digit code from the app. Only a right code stores the new secret; until then the old authenticator keeps working. Asks for your password to start; five wrong codes or ten minutes end the setup; the login authority is restarted afterwards so it uses the new secret |
| Login and credentials | status and trusted devices, new Remote Access Key, new recovery codes, new authenticator + key + codes, sign out every remote login, switch remote access off or on, forget a trusted device. Each change asks for your password again and shows new secrets once; they are not stored on the page |
| Remote use while locked | a switch for the lock-screen extension (asks for your password to turn on; while on, locking the laptop no longer ends a remote session). The tray menu has the same switch | 
| This laptop | notifications, running time in the top bar, start the console when you log in (applies at once; only the idle service starts, nothing connects by itself) |

A client that asks for something outside these limits is refused (a mode that is not allowed) or held to the limit (numbers,
sound), and its page shows what is available. "Typing text" off refuses text input even if a modified page sends it.
If `host.json` exists but cannot be read or is invalid, the console uses the defaults except that every connection asks for
your approval, and the host page shows the reason; saving the page writes a valid file again.

### What is not here yet

Saved servers (several laptops) and named connection profiles in the client; the client's "remember this device" is the
existing trusted-browser login.
