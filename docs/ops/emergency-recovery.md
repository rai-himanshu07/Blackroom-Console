# Emergency recovery

For whoever sits at the laptop when a remote session went wrong. Read the first section, then the one that matches.
Keep a second device with an SSH login to this laptop ready before you rely on Private mode: SSH is the backup, never the only
way out.

## What the console does by itself

| Situation | What happens without you | Observed |
|---|---|---|
| The browser closes or the network drops | After the heartbeat timeout (30 s by default, 5 to 120 s in the settings) the console stops the session: the screen is restored, the laptop is locked **if the session's lock setting is on** (default for Private, off for Shared), and the keyboard and touchpad grab is released after the lock | yes, headless and live (owner-reported) |
| The console process dies mid-session | The kernel drops the input grab with the daemon. A 60 s restore timer restores the display and locks. If systemd restarts the console, start-up recovery restores the display and locks in about 5 s | the 60 s timer and an immediate lock after a killed console: yes, once on this laptop; the restart path: headless only |
| The input-grab daemon freezes | systemd kills it after 10 s and the kernel drops the grab | yes, once (a test unit, not the shipped file) |
| Emergency chord, see below | The grab is released and the console ends the session and every browser login | yes, once with the packaged console on the real desktop (owner-reported 2026-10-06: keyboard and screen came back, the session ended, login was closed until the marker was cleared) |

A step that fails during a stop (restore, lock, release) is reported as a warning on the page and in the top-bar menu. It is
never shown as success.

## The emergency chord

Hold **Left Ctrl + Left Shift + Left Alt + Esc** on the laptop's built-in keyboard for **2 seconds**.

- It works **only while the console holds the keyboard grab**: a Private session with "Block this laptop's keyboard and
  touchpad" on. With that setting off, in Shared mode or with no session, it does nothing, because your keyboard is not
  grabbed and you can use the tray menu (Disconnect) or the lock screen instead.
- The packaged daemon releases the grab at once. The console notices within about 2 s, runs its normal stop (restore the
  display, lock if the lock setting is on, release) and refuses every browser login that existed before the chord.
- The daemon then refuses new grabs until it is restarted on purpose: run `systemctl --user restart
  blackroom-console.service` (this also restarts the daemon) before the next session.
- The packaged daemon is given the login authority's state directory, so the chord also writes a durable stop marker
  (`emergency-stop`) there and bumps the security epoch: **every remote login stays refused, also after a restart, until you
  clear the marker locally** (below). It is not given `--lock-on-emergency`, so the chord itself does not lock the screen: the
  lock comes from the console's own stop and only when the session's lock setting is on. A Shared session ended by the chord
  leaves the laptop unlocked unless you lock it. Locking from the daemon waits for one supervised live run.
- What is **not** guaranteed: that the chord works if the keyboard that you press is not one of the grabbed built-in nodes
  (an external USB keyboard is not covered), or if the daemon is frozen (the watchdog then needs up to 10 s).
- The chord worked once with the packaged console on the real desktop (owner-reported 2026-10-06). Treat it as a second line, not the first.

## Stuck Private session: the panel is black and the keyboard is dead

1. Wait 30 s: a silent browser ends the session by itself. If you are in the room and the tablet is fine, press **Disconnect**.
2. Hold the chord (above), 2 s.
3. From another device over SSH (set the runtime directory first if the shell does not have it):
   ```
   export XDG_RUNTIME_DIR=/run/user/$(id -u)
   systemctl --user stop blackroom-console.service       # runs the normal stop: restore, lock per setting, release
   systemctl --user stop blackroom-console-emergencyd.service   # releases the keyboard and touchpad at once
   ```
4. Panel still black after the console stopped:
   ```
   systemctl --user start blackroom-console.service      # start-up recovery restores the display and locks
   # or, by hand, from the display backup of the session that was running:
   /usr/lib/blackroom/exp07_restore --keep-live-virtual --lock-after --backup "$XDG_RUNTIME_DIR/blackroom-console/backup.json"
   ```
5. Last resort, a keyboard that is still dead: `pkill -KILL -x remote-emergenc` (the daemon's name is cut to 15 characters).

**Do not unlock the screen to "fix" a black panel.** After a stop or a restore the laptop is normally *locked*, so you see the
lock screen: type your password there. `loginctl unlock-session` over SSH removes the lock for everyone in the room: use it only
when you are at the laptop and the lock screen itself cannot be used. A dead console never needs it.

## After the chord: remote login is closed until you reopen it

Do this at the laptop, once you know why the chord was needed. Removing the marker is the only way back; nothing clears it
automatically, and `blackroom reset full` refuses while it exists.
```
systemctl --user stop remote-hostd.service blackroom-console.service
ls ~/.local/share/blackroom-console/hostd          # look for emergency-stop (and recovery-pending)
rm ~/.local/share/blackroom-console/hostd/emergency-stop
systemctl --user start blackroom-console.service    # starts the login authority and the grab daemon too
```
Old browser sessions stay void (the epoch was bumped); log in again. Offline test only: the marker removal and restart were
checked against the store, not on the real desktop.

## Network lost

- Laptop side: nothing to do. The session ends after the heartbeat timeout as above.
- Client side: the page reconnects by itself for about 30 s. After that, open the address again and start a new session. A failed
  **Disconnect** (for example with no network) leaves the session running until the timeout; the laptop ends it then.
- Through a VPN: check that the VPN is connected on both devices before suspecting the console. Laptop sleep and lid close with
  the VPN were not tried.

## Turn remote access off now

```
blackroom --state-dir ~/.local/share/blackroom-console/hostd disable     # ends every session; `enable` re-opens it
systemctl --user stop blackroom-console.service
```

## After a recovery

Run `blackroom repair` (read-only report). Check `blackroom --state-dir ~/.local/share/blackroom-console/hostd logs` for what
the console recorded. If the chord was used, restart the console unit before the next session.
