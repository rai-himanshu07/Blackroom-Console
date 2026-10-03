//! Test peer for the console clipboard, run only against a throwaway `--headless` Shell on a private bus
//! (`docs/ops/headless-clipboard-test.sh`): a second RemoteDesktop session that owns or reads the clipboard.
//!   clip_peer serve <text> <seconds>   offer <text> and answer every paste until the time is up
//!   clip_peer read                     print the current clipboard text
//! Prints only synthetic test text.

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use blackroom_console::clipboard::{self, TEXT_MIMES};
use blackroom_console::display;
use blackroom_gnome::mutter::remote_desktop::{
    RemoteDesktopSession, SelectionEvent, listen_selection_events,
};
use zbus::blocking::Connection;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let conn = Connection::session().context("session bus")?;
    display::require_headless_shell(&conn)?;
    let events = listen_selection_events(&conn).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut session = RemoteDesktopSession::create(&conn).map_err(|e| anyhow::anyhow!("{e}"))?;
    session
        .enable_clipboard()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    session.start().map_err(|e| anyhow::anyhow!("{e}"))?;
    match args.first().map(String::as_str) {
        Some("serve") => {
            let text = args.get(1).context("text")?.clone();
            let seconds: u64 = args.get(2).context("seconds")?.parse()?;
            session
                .set_selection(&TEXT_MIMES)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("serving");
            let own = session.object_path().to_string();
            let deadline = Instant::now() + Duration::from_secs(seconds);
            let mut served = 0;
            while Instant::now() < deadline {
                let Ok(event) = events.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };
                if let SelectionEvent::Transfer {
                    path,
                    mime_type,
                    serial,
                } = event
                    && path == own
                {
                    let ok = TEXT_MIMES.contains(&mime_type.as_str())
                        && session.selection_write(serial).is_ok_and(|fd| {
                            clipboard::write_with_timeout(
                                fd,
                                text.clone().into_bytes(),
                                Duration::from_secs(4),
                            )
                        });
                    let _ = session.selection_write_done(serial, ok);
                    served += 1;
                }
            }
            println!("served {served}");
        }
        Some("read") => {
            // The owner announcement may still be on its way.
            let mut last = None;
            for _ in 0..10 {
                for mime in TEXT_MIMES {
                    if let Ok(fd) = session.selection_read(mime)
                        && let Ok(text) = clipboard::read_with_timeout(fd, Duration::from_secs(4))
                    {
                        println!("{text}");
                        return Ok(());
                    }
                    last = Some(mime);
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            bail!("no clipboard text could be read (last mime {last:?})");
        }
        _ => bail!("usage: clip_peer serve <text> <seconds> | read"),
    }
    Ok(())
}
