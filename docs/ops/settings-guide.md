# Settings guide

Open the console page and sign in. The connect screen shows the laptop's name and state, two modes and a **Settings**
button. Settings are saved on the laptop (`profile.json` in `--profile-dir`, owner-only, written atomically), so every
browser you sign in with sees the same choices. **Reset to defaults** restores them; the picture and sound level of the
current browser follow the saved values.

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

## This device

Quality level, picture scale (fit, stretch, 1:1 with scrolling to follow the pointer), volume, the pointer marker
over the picture, touch mode (trackpad or direct), the keyboard (key presses for a US layout, or characters typed with
keysyms for any laptop layout; Command acts as Control for Mac and iPad keyboards).

## Installing the page as an app

The connect screen offers **Install** when the browser allows it. Browsers only offer this on a trusted https address
(a Tailscale certificate or Let's Encrypt, see `internet-access.md`) or on `localhost`; with the self-signed certificate
the page works but cannot be installed. On iPhone and iPad use Share, then Add to Home Screen. The service worker caches
nothing: it only makes the app installable and shows a "can't reach the laptop" page that retries every 5 seconds.
