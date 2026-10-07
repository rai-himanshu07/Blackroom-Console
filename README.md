# Blackroom Console

Use your laptop from a tablet or phone. Open a web browser to see the laptop's screen, move its pointer and type, without
installing a client app. You can leave the laptop's screen visible or make it blank while you work remotely.

**Version 0.1.0 is a technical preview.** It is built for one owner, one laptop and one remote device at a time. Read the
requirements below before installing it. Keep access to the laptop while you try it for the first time.

![Connect screen](docs/screenshots/client-1-connect-tablet.png)

## Before you begin

You need:

- A compatible laptop. The tested setup is **Ubuntu 26.04, GNOME 50 on Wayland, with an NVIDIA GPU**. Other Linux desktops
  and Windows/macOS hosts are not supported. Ubuntu's **Settings > System > About** shows your system version; ask for help
  checking compatibility if you are unsure.
- **Only the laptop's built-in screen connected.** Disconnect external monitors before a session.
- A tablet or phone with a browser. **Chrome on Android is tested**; Safari/iPhone/iPad and Firefox are not verified.
- Your laptop account password, an authenticator app on a device you can access, and a password manager or another secure
  place for the Remote Access Key and recovery codes. Examples of authenticator apps: Aegis, Google Authenticator,
  Microsoft Authenticator and 1Password.
- For your first connection, both devices on the same trusted Wi-Fi network. The laptop must be awake and someone must
  already be logged in. Prevent sleep in Ubuntu's power settings while using it remotely; sleep and lid-close recovery
  have not been verified.

The detailed [compatibility table](docs/ops/compatibility-matrix.md) separates observed results from untested features.
This preview has no file transfer and no multi-user or multi-controller support.

## 1. Install on the laptop

You need the **Blackroom Console `.deb` installer** from a source you trust. The source-code ZIP is not an installer.
If you only have the source code, use [Build from source](#build-from-source) below, or have someone build the installer
for you. Do not install an unknown copy: this application can control your desktop.

For a signed release, check the package with the supplied [verification script](docs/ops/verify-release.sh) before
installing it. It needs the package, `SHA256SUMS`, `SHA256SUMS.sig` and a trusted copy of the
[release public key](docs/release-signing.pub). A technically confident person can help with this check.

If Ubuntu offers **Open With > Software Install** for the downloaded `.deb`, open it that way and choose **Install**.
Otherwise, for version 0.1.0, put the installer in **Downloads**, open **Terminal** and enter these two lines:

```bash
cd ~/Downloads
sudo apt install ./blackroom-console_0.1.0-1_amd64.deb
```

Use the actual filename if your installer has another version. Enter your administrator password when asked; **Terminal
does not show password characters as you type**. Installing the package does not start remote access.

After the first installation, save your work, **log out of Ubuntu and log in again** so GNOME can find the top-bar extensions.

## 2. Set up remote sign-in

Do these steps **on the laptop**, not on the tablet:

1. Open **Blackroom Console** from Ubuntu's applications menu. Its settings page opens in your browser and the console
   icon appears in the top bar. The laptop-only settings address is `http://localhost:8090/`.
2. Sign in with your **laptop account password**. At the top of the page, enter it again under **Enable editing** to allow
   protected changes for five minutes. **Lock now** ends that editing permission; it does not lock Ubuntu's screen.
3. Under **Sign-in & security > Login and credentials**, choose **Set up remote sign-in**. Follow its prompts and securely
   record the new Remote Access Key and recovery codes. New secrets are shown only temporarily; record them before hiding
   them or leaving the page.
4. Add the laptop to your authenticator app using the setup QR code or key. When using **Add an authenticator**, type a code
   from the app and choose **Confirm**. Do not replace an authenticator that is already working just to repeat setup.
5. Check **Remote sign-in > Sign-in method** is **Password, code and key (recommended)**. Choose **Save and restart the
   console** when you have finished. A restart ends any running remote session; it does not reboot Ubuntu.

These are different things:

| Sign-in item | What to enter |
|---|---|
| Laptop password | The password you normally use to sign in to Ubuntu |
| Authenticator code | The current six-digit code from your authenticator app; it changes regularly |
| Remote Access Key | The separate key Blackroom created during setup, not your Wi-Fi password |
| Recovery code | One saved, single-use code, instead of an authenticator code when the app is unavailable |

Keep the key and recovery codes private. Do not photograph the QR code for sharing or include secrets in a support request.

## 3. Connect from the tablet or phone

1. Keep the laptop awake, with Ubuntu signed in, and connect both devices to the same Wi-Fi.
2. On the tablet or phone, open the **HTTPS address** shown under **First steps** on the laptop. It usually looks like
   `https://YOUR-LAPTOP-ADDRESS:8443/`. Use the address actually shown by your laptop; do not type this example literally.
   `localhost` always means the device you are holding, so it is not the laptop's remote address.
3. **Optional, recommended: check the certificate.** A first-visit browser warning is expected when Blackroom uses its own
   self-signed certificate. For extra assurance, you can compare its SHA-256 fingerprint with the laptop's using the
   [certificate-check steps](docs/ops/internet-access.md#2b-you-have-only-a-static-ip-address). A fingerprint identifies the
   certificate; matching values help confirm you are connecting to your laptop. If they differ or change unexpectedly
   later, check the address and certificate on the laptop before signing in.
4. Enter the laptop account name, password, current authenticator code and Remote Access Key. Choose **Trust this browser**
   only on your own device: it replaces the key on future visits, **not** the password or authenticator code.
5. For the first test, choose **Shared**, then **Connect**. You should see the same desktop on both devices. Move the
   pointer and try typing in a harmless document to check it works.
6. Open the session menu and choose **Disconnect**. Confirm that the laptop is usable before trying Private mode.

## 4. Choose how you want to work

| Mode | What happens on the laptop |
|---|---|
| **Shared** | Its screen stays visible and its keyboard/touchpad keep working. Someone beside it can see what you do. |
| **Private** | With the default options, its screen goes blank and its built-in keyboard/touchpad are blocked. You work from the remote device. |

Before Private mode, open **This laptop > Keyboard blocking** in the laptop's settings and choose **Allow keyboard
blocking**. Answer the administrator password dialog **on the laptop**. This permission is needed again after each reboot
unless you deliberately enable the permanent-access option below. Shared mode does not need it.

During a session, the remote page's menu has picture quality, sound, clipboard and keyboard controls. With a physical
keyboard, **F8** opens that menu; F8 is reserved for Blackroom and is not sent to the laptop. Clipboard sharing is text only
and happens when you press its send/fetch buttons.

Use **Disconnect**, or the visible **End session** control in Private mode, when finished. The console restores the screen
and releases blocked input. It locks Ubuntu **only when the session's lock setting is on**: on by default for Private,
off by default for Shared, unless the laptop owner overrides it. Warnings about a failed step need attention.

### Optional: connect while Ubuntu is locked

Under **This laptop**, enable **Allow remote sessions on the lock screen** only if you need it. You still type the laptop
password on Ubuntu's lock screen from the remote device. This is not passwordless unlocking.

**While this switch is on, locking Ubuntu no longer ends a remote session.** Other programs running as your Ubuntu user
can also open remote sessions on the lock screen. Use Disconnect or switch remote access off when you need it closed.

### Optional: reach the laptop after reboot

**Come back after a restart** enables automatic Ubuntu login, a subsequent screen lock, permanent built-in input access
and lock-screen remote access. It requires administrator approval.

**Leave this off unless you accept the trade-off:** the desktop can be visible for a few seconds before the automatic lock,
and other programs running as your user can read the permitted input devices. Disk-encryption or BIOS passwords still
need someone at the laptop. See the [settings guide](docs/ops/settings-guide.md) before enabling it.

## 5. Use it away from home

**Start with a private VPN such as Tailscale.** Install and sign in to it on both devices, then follow the
[outside-access guide](docs/ops/internet-access.md). In Blackroom's **Access > Access from outside**, choose **Private VPN**
and enter the name and certificate settings from that guide. A VPN is separate software; selecting it here does not
install it or configure your router or firewall.

Save and restart the console. Test with the phone's Wi-Fi off, using mobile data, while you can still reach the laptop.
Tailscale from mobile data has been tried; other VPNs and relayed connections have not been verified.

**Direct internet access is advanced and riskier.** It lets strangers reach the sign-in page and needs router forwarding
and certificate checks. It was tried on one router, not broadly tested. Do not forward ports as part of basic setup.
A self-signed certificate is not automatically trusted; the fingerprint check still matters.

## Change the default connection port

The port is the number after the colon in an address. Blackroom normally uses **8443 for HTTPS** (the tablet or phone
connection) and **8080 for plain HTTP** (local access on the laptop). You can change them if another application already
uses one, or you prefer a different number. Changing the port does not replace sign-in or make public access safer.

1. On the laptop, open **Blackroom Console**, sign in, and choose **Enable editing** with your laptop password.
2. Open **Access > Network**. Keep **Enable HTTPS** switched on. In **https: address and port**, change only the number
   after the final colon. For example, change `0.0.0.0:8443` to `0.0.0.0:8444`; leave the address before the colon unchanged.
3. If you also need another plain-HTTP port, change `127.0.0.1:8080` to `127.0.0.1:8081`. Keep `127.0.0.1` so plain HTTP
   stays on the laptop only. Choose an unused port between **1024 and 65535**. If a port is already in use, the page refuses
   to save and explains which one to change.
4. Disconnect any remote session, then choose **Save and restart the console**. Check **Access from outside > Currently
   running** to confirm the new addresses. Restarting the console ends a session; it does not reboot the laptop.
5. On the tablet or phone, use the new HTTPS port, for example `https://YOUR-LAPTOP-ADDRESS:8444/`, using your laptop's actual
   address or VPN name. Update your bookmark too. For Direct internet access, update the router forwarding rule and any
   firewall rule for the HTTPS port, and use the corresponding outside port in the browser.

The laptop's **Host settings** page remains at `http://localhost:8090/`; these controls do not change its port. Video's
separate UDP ports are configured under **Access from outside**, not in the HTTPS field.

## Help and recovery

| Problem | What to do first |
|---|---|
| No picture or cannot connect | Check the laptop is awake, signed in and connected to the network. Reopen Blackroom on the laptop and use the address it shows. Guest Wi-Fi may prevent devices reaching each other. |
| Sign-in refused after several mistakes | Wait for the displayed lockout to end. Check your password and use a current authenticator code. Do not keep guessing. |
| Authenticator unavailable | Use one of your saved recovery codes in place of the app code. You still need the password and key or trusted browser. |
| Private mode cannot block the keyboard | On the laptop, choose **Allow keyboard blocking** again, especially after a reboot. |
| Lost tablet or key | From the laptop's settings, **Forget** the trusted browser, **Sign out every remote login**, or **Switch remote access off**. Create a new key if the old one may have been exposed. |
| Login authority unavailable, or the app will not start | The console will not weaken a configured three-factor login. At the laptop, run `blackroom repair` for a read-only report, then follow the [runbook](docs/ops/runbook.md). |
| Laptop stays blank after a connection is lost | Allow about 60 seconds for crash recovery. If it does not return, follow the [emergency recovery guide](docs/ops/emergency-recovery.md), including the external-keyboard or SSH options. |

**Emergency stop at the laptop:** while its built-in keyboard is blocked, hold **Left Ctrl + Left Shift + Left Alt + Esc
for two seconds**. This releases the grab, ends the session and closes remote logins until you complete the
[local recovery steps](docs/ops/emergency-recovery.md#after-the-chord-remote-login-is-closed-until-you-reopen-it). It only works while keyboard blocking is active and
**does not lock Ubuntu by itself**. Check and lock the laptop manually when it returns. Do not rely on the chord in Shared
mode or when keyboard blocking is off.

Closing the browser or losing the network ends a session after its configured silence timeout, about 30 seconds by
default. Use Disconnect when possible rather than waiting for that timeout.

## Update or uninstall

**Update:** disconnect first, install the newer trusted `.deb` the same way as above, and open Blackroom Console again.
Saved credentials and settings are kept. Package installation stops a running console before replacing it; it refuses
to continue if safety cleanup cannot finish. Do not force an upgrade past a recovery warning.

**Uninstall:** first switch **Come back after a restart** off if you enabled it. Then use Ubuntu's software manager, or:

```bash
sudo apt remove blackroom-console
```

Removing the package keeps your personal Blackroom settings and credentials. To erase those too, run `blackroom reset
full` **before** uninstalling and read its confirmation carefully: it deletes Blackroom credentials and cannot be undone.

## Privacy and security

There is no telemetry. Default configuration does not contact outside servers. Any STUN/TURN servers you configure, and
your separately installed VPN, add network connections. Logs contain operational and authentication events, not
keystrokes, clipboard text or passwords. Development token mode logs a secret URL; do not share its logs.

The packaged defaults keep laptop settings and plain HTTP local, while **HTTPS on port 8443 accepts connections on all
network interfaces**, behind sign-in. "Home network only" does not create a firewall rule; actual reachability depends on
your network and router. The settings page's **Currently running** block shows the active listeners.

Security reviews and tests reduce risk; **they do not guarantee that no bypass exists**. Phishing, a compromised Ubuntu
user account, and the optional automatic-login/lock-screen changes remain important risks. Read [SECURITY.md](SECURITY.md),
the [threat model](docs/security/threat-model.md) and the [security review](docs/security/red-team-report.md).

## Build from source

For developers or someone helping with installation. Run these commands in the project folder. You need the Rust
toolchain specified in [rust-toolchain.toml](rust-toolchain.toml), the GStreamer/PipeWire development headers, and Debian
packaging tools including `dpkg-deb` and `dpkg-shlibdeps`. Development and validation rules are in
[CONTRIBUTING.md](CONTRIBUTING.md).

```bash
CARGO_INCREMENTAL=0 docs/ops/build-deb.sh
sudo apt install ./target/deb/blackroom-console_0.1.0-1_amd64.deb
```

The build script builds the required release binaries; a separate workspace build is unnecessary. Do not run overlapping
Cargo commands. To verify a downloaded signed release from a directory containing all its files:

```bash
docs/ops/verify-release.sh ~/Downloads
```

Developer checks (no live display/input mutation):

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude gnome-session-agent && cargo test -p gnome-session-agent
cargo deny check && cargo audit
node docs/ops/indicator-logic-test.mjs
```

Real-desktop tests require a prepared host and operator participation. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence and details

GPL-3.0-or-later ([LICENSE](LICENSE)). No legal clearance is claimed for H.264 patents.

- Rust dependencies use licences allowed by the project's `cargo deny` policy: MIT, Apache-2.0 (including LLVM exception),
  BSD-2/3-Clause, ISC, Zlib, MPL-2.0, Unicode-3.0, IJG, LGPL-2.1-or-later and GPL-3.0-or-later. `cargo deny list` gives details.
- GStreamer, PipeWire, GNOME/Mutter, Linux-PAM and ACL tools are system dependencies, not bundled libraries.
- NVIDIA's encoder uses its proprietary driver; the software fallback uses the distribution's OpenH264 library.
- Fonts are system fonts; icons belong to this project.

Versions use SemVer (`v0.1.0`, package `0.1.0-1`). Checks are manual; there is no CI. Long-duration soak, certificate renewal,
sleep/lid-close recovery and untested browsers are not claimed as verified.

[SECURITY.md](SECURITY.md) · [CONTRIBUTING.md](CONTRIBUTING.md) · [CHANGELOG.md](CHANGELOG.md) · [docs/RELEASE_NOTES.md](docs/RELEASE_NOTES.md)
