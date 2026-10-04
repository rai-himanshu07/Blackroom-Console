//! `blackroom internet`: guided set-up for reaching the laptop from outside the home network, and `--check`.
//! The console binary owns the validation (`--check-internet`, `--apply-internet`); this module asks the questions,
//! shows what would change, and saves only after a dry run came back clean and the owner agreed. It never runs sudo,
//! never talks to the network, and never changes the router.

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

type Failure = (u8, String);

const LIB_DIRS: [&str; 2] = ["/usr/lib/blackroom", ".local/lib/blackroom"];
const DEFAULT_MEDIA_PORTS: &str = "50000-50100";
const DEFAULT_STUN: &str = "stun:stun.l.google.com:19302";
const CONSOLE_UNIT: &str = "blackroom-console.service";

pub const USAGE: &str = "usage: blackroom internet [--check] [--runtime-dir <abs path>] [--tls-dir <abs path>] [--console-bin <abs path>]\n  without --check: a guided set-up (VPN, or direct access with a name or a static IP); with --check: a read-only report";

const NOTICE: &str = "Direct internet access makes this login discoverable by scanners. Authentication and rate limits reduce, but do not eliminate, password guessing, lockout or denial-of-service risks. The password, authenticator code and Remote Access Key are not phishing-proof: check the exact https address and never bypass a certificate warning. This does not protect against a compromised laptop or browser. These checks do not prove internet connectivity.";
const SELF_SIGNED_NOTICE: &str = "A self-signed certificate is trusted only because you compared its fingerprint: the first visit is trust-on-first-use, and a changed certificate must be verified again.";

fn refuse(message: impl std::fmt::Display) -> Failure {
    (1, format!("refused: {message}"))
}

// ---- the console binary ----

pub trait Console {
    fn check(&self) -> Result<Value, String>;
    /// Validates `patch` against the saved settings and reports; saves it only when `save`.
    fn apply(&self, patch: &Value, save: bool) -> Result<Value, String>;
}

pub struct Binary {
    pub path: PathBuf,
    pub runtime: PathBuf,
}

impl Binary {
    /// `/usr/lib/blackroom`, `~/.local/lib/blackroom`, then next to this program (a development build).
    pub fn find() -> Option<PathBuf> {
        LIB_DIRS
            .iter()
            .filter_map(|dir| {
                if dir.starts_with('/') {
                    Some(PathBuf::from(dir))
                } else {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .filter(|home| home.is_absolute())
                        .map(|home| home.join(dir))
                }
            })
            .map(|dir| dir.join("blackroom-console"))
            .chain(
                std::env::current_exe()
                    .ok()
                    .map(|exe| exe.with_file_name("blackroom-console")),
            )
            .find(|path| path.is_file())
    }

    fn run(&self, extra: &[&str], stdin: Option<&str>) -> Result<Value, String> {
        // The packaged unit's own listen addresses, so the check sees what the service will run with.
        let mut child = Command::new(&self.path)
            .arg("--hostd-dir")
            .arg(&self.runtime)
            .args(["--listen", "127.0.0.1:8080", "--tls-listen", "0.0.0.0:8443"])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        if let Some(mut pipe) = child.stdin.take()
            && let Some(text) = stdin
        {
            let _ = pipe.write_all(text.as_bytes());
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&output.stdout);
        // Log lines may come first; the answer is the last JSON line.
        let answer = text
            .lines()
            .rev()
            .find(|line| line.starts_with('{'))
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .ok_or("the console gave no answer (is it installed? try --console-bin)")?;
        match answer.get("error").and_then(Value::as_str) {
            Some(error) => Err(error.to_string()),
            None => Ok(answer),
        }
    }
}

impl Console for Binary {
    fn check(&self) -> Result<Value, String> {
        self.run(&["--check-internet"], None)
    }

    fn apply(&self, patch: &Value, save: bool) -> Result<Value, String> {
        let mut extra = vec!["--apply-internet"];
        if !save {
            extra.push("--dry-run");
        }
        self.run(&extra, Some(&patch.to_string()))
    }
}

// ---- talking to the owner ----

pub struct Io<'a> {
    input: &'a mut dyn BufRead,
    out: &'a mut dyn Write,
}

impl<'a> Io<'a> {
    pub fn new(input: &'a mut dyn BufRead, out: &'a mut dyn Write) -> Self {
        Self { input, out }
    }

    fn say(&mut self, text: &str) -> Result<(), Failure> {
        writeln!(self.out, "{text}").map_err(refuse)
    }

    fn line(&mut self, prompt: &str) -> Result<String, Failure> {
        write!(self.out, "{prompt} ").map_err(refuse)?;
        self.out.flush().map_err(refuse)?;
        let mut text = String::new();
        if self.input.read_line(&mut text).map_err(refuse)? == 0 {
            return Err((1, "input ended: nothing was changed".into()));
        }
        Ok(text.trim().to_string())
    }

    pub(crate) fn yes_no(&mut self, prompt: &str, default: bool) -> Result<bool, Failure> {
        loop {
            let answer = self.line(&format!(
                "{prompt} [{}]",
                if default { "Y/n" } else { "y/N" }
            ))?;
            match answer.to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.say("Please answer y or n.")?,
            }
        }
    }

    /// 1-based index of the chosen option.
    fn choose(&mut self, prompt: &str, options: &[&str], default: usize) -> Result<usize, Failure> {
        self.say(prompt)?;
        for (index, option) in options.iter().enumerate() {
            self.say(&format!("  {}) {option}", index + 1))?;
        }
        loop {
            let answer = self.line(&format!("Choose 1-{} [{default}]", options.len()))?;
            if answer.is_empty() {
                return Ok(default);
            }
            if let Ok(number) = answer.parse::<usize>()
                && (1..=options.len()).contains(&number)
            {
                return Ok(number);
            }
            self.say("Please type one of the numbers.")?;
        }
    }
}

// ---- reports ----

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Plain-language rendering of a console report; true when nothing blocks.
pub fn print_report(io: &mut Io<'_>, answer: &Value) -> Result<bool, Failure> {
    let report = &answer["report"];
    let access = match report["access"].as_str() {
        Some("direct") => "from the internet (internet mode is on)",
        Some("private-network") => {
            "over a private network or VPN (a certificate is set, internet mode is off)"
        }
        _ => "on the home network only",
    };
    io.say(&format!("Reachable: {access}."))?;
    if let Some(url) = report["url"].as_str() {
        io.say(&format!("Address to open on the phone: {url}"))?;
    }
    if let Some(cert) = report.get("certificate").filter(|cert| cert.is_object()) {
        let names = cert["names"]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        io.say(&format!(
            "Certificate: covers {names}; {} day(s) left; {}.",
            report["days_left"].as_i64().unwrap_or(0),
            if cert["self_signed"].as_bool() == Some(true) {
                "self-signed"
            } else {
                "issued by an authority"
            }
        ))?;
        if let Some(print) = cert["fingerprint"].as_str() {
            io.say(&format!("Certificate fingerprint (SHA-256): {print}"))?;
        }
    }
    let forwards = strings(report, "forwards");
    if !forwards.is_empty() {
        io.say("Forward these on your router to this laptop:")?;
        for line in forwards {
            io.say(&format!("  - {line}"))?;
        }
    }
    let problems = strings(report, "problems");
    for line in &problems {
        io.say(&format!("PROBLEM {line}"))?;
    }
    for line in strings(report, "warnings") {
        io.say(&format!("WARNING {line}"))?;
    }
    for line in strings(report, "notes") {
        io.say(&format!("note: {line}"))?;
    }
    Ok(problems.is_empty())
}

// ---- the guided set-up ----

fn tls_files(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("cert.pem"), dir.join("key.pem"))
}

/// Shows what a change would do, asks, saves, and says what to do next. `public` means internet mode is being turned on.
fn commit(
    console: &dyn Console,
    io: &mut Io<'_>,
    patch: &Value,
    public: bool,
    restart: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    let dry = console.apply(patch, false).map_err(refuse)?;
    let clean = print_report(io, &dry)?;
    if !clean {
        io.say("Nothing was saved: fix the problems above and run `blackroom internet` again.")?;
        return Ok(());
    }
    if public {
        io.say("")?;
        io.say(NOTICE)?;
        if patch["public_cert"] == "self_signed" {
            io.say(SELF_SIGNED_NOTICE)?;
        }
        if io.line("Type yes to turn internet access on:")? != "yes" {
            io.say("Nothing was saved.")?;
            return Ok(());
        }
    } else if !io.yes_no("Save this?", true)? {
        io.say("Nothing was saved.")?;
        return Ok(());
    }
    console.apply(patch, true).map_err(refuse)?;
    io.say("Saved to host.json.")?;
    if io.yes_no(
        "Restart the console now so it takes effect? This ends a running remote session.",
        false,
    )? {
        if restart() {
            io.say("The console was restarted.")?;
        } else {
            io.say(&format!(
                "The restart did not work: run systemctl --user restart {CONSOLE_UNIT}"
            ))?;
        }
    } else {
        io.say(&format!(
            "It takes effect after: systemctl --user restart {CONSOLE_UNIT}"
        ))?;
    }
    Ok(())
}

fn certificate_help(
    io: &mut Io<'_>,
    kind: &str,
    name: &str,
    tls_dir: &Path,
) -> Result<(), Failure> {
    let dir = tls_dir.display();
    io.say(&format!(
        "No certificate and key were found in {dir} (cert.pem and key.pem)."
    ))?;
    io.say("Get one first, then run `blackroom internet` again. These commands need sudo; this program never runs it:")?;
    if kind == "tailscale" {
        io.say(&format!("  mkdir -p {dir} && chmod 700 {dir}"))?;
        io.say(&format!(
            "  sudo tailscale cert --cert-file {dir}/cert.pem --key-file {dir}/key.pem {name}"
        ))?;
    } else {
        io.say(&format!("  sudo certbot certonly --standalone -d {name}     # needs TCP 80 reachable while it is issued, or use a DNS plugin"))?;
        io.say(&format!("  mkdir -p {dir} && chmod 700 {dir}"))?;
        io.say(&format!("  sudo install -m 644 -o $USER /etc/letsencrypt/live/{name}/fullchain.pem {dir}/cert.pem"))?;
        io.say(&format!(
            "  sudo install -m 600 -o $USER /etc/letsencrypt/live/{name}/privkey.pem {dir}/key.pem"
        ))?;
        io.say("Make renewals copy the files again (a certbot deploy hook): docs/ops/internet-access.md shows one.")?;
    }
    Ok(())
}

fn phone_test(io: &mut Io<'_>, url: &str, vpn: bool) -> Result<(), Failure> {
    io.say("")?;
    io.say("Test it from the phone:")?;
    if vpn {
        io.say("  1. Turn Wi-Fi off (mobile data) and make sure the Tailscale app is connected.")?;
    } else {
        io.say("  1. Turn Wi-Fi off, so the phone uses mobile data (from your own Wi-Fi the address may fail because of the router).")?;
    }
    io.say(&format!("  2. Open {url}"))?;
    io.say("  3. Log in, connect, check the status chip says WebRTC (MJPEG also works), then disconnect.")?;
    io.say("Run `blackroom internet --check` any time for the local checks and the certificate fingerprint.")
}

fn vpn(
    console: &dyn Console,
    io: &mut Io<'_>,
    tls_dir: &Path,
    restart: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    io.say("Install Tailscale on this laptop and on the phone or tablet, sign in to the same account, and in the Tailscale admin console switch on MagicDNS and HTTPS certificates. Nothing is opened to the internet.")?;
    let name = io.line("This laptop's Tailscale name (like laptop.tailnet-name.ts.net), or Enter to keep the console's own certificate:")?;
    let patch = if name.is_empty() {
        json!({"public": false, "public_cert": null, "public_name": null, "tls_cert": null, "tls_key": null})
    } else {
        let (cert, key) = tls_files(tls_dir);
        if !cert.is_file() || !key.is_file() {
            return certificate_help(io, "tailscale", &name, tls_dir);
        }
        json!({"public": false, "public_cert": null, "public_name": name, "tls_cert": cert, "tls_key": key})
    };
    commit(console, io, &patch, false, restart)?;
    let port = console
        .check()
        .ok()
        .and_then(|answer| answer["report"]["https_port"].as_u64())
        .unwrap_or(8443);
    let host = if name.is_empty() {
        "this-laptop's-tailscale-name".to_string()
    } else {
        name
    };
    phone_test(io, &format!("https://{host}:{port}/"), true)
}

fn direct(
    console: &dyn Console,
    io: &mut Io<'_>,
    tls_dir: &Path,
    restart: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    io.say("Direct access works only when your router can accept connections from the internet.")?;
    loop {
        match io.choose(
            "Can your router accept incoming connections?",
            &[
                "Yes: I can forward ports, and my address is an ordinary public one (a static IP, or a changing one with a DNS name)",
                "I do not know",
                "No, or my provider shares one address between customers (CGNAT)",
            ],
            2,
        )? {
            3 => {
                io.say("Then a direct connection cannot work, whatever the settings. A VPN such as Tailscale does.")?;
                return if io.yes_no("Set up the VPN way instead?", true)? {
                    vpn(console, io, tls_dir, restart)
                } else {
                    io.say("Nothing was changed.")
                };
            }
            2 => io.say("Compare the WAN (internet) address in your router's status page with what a \"what is my IP\" website shows. If they differ, or the router shows 100.64.x.x to 100.127.x.x or a private address (10.x, 172.16-31.x, 192.168.x), there is no direct path: choose the VPN. Also forward ports only from the router you control; a second router behind it needs forwarding too.")?,
            _ => break,
        }
    }
    let how = io.choose(
        "How will your devices find this laptop?",
        &[
            "By a name: a dynamic-DNS name or my own domain (recommended: a real certificate is possible)",
            "Only by its static IP address",
        ],
        1,
    )?;
    let mut patch = if how == 1 {
        let name = io.line("The name (for example home.example.org):")?;
        let (cert, key) = tls_files(tls_dir);
        if !cert.is_file() || !key.is_file() {
            return certificate_help(io, "certbot", &name, tls_dir);
        }
        json!({"public": true, "public_name": name, "public_cert": "ca", "tls_cert": cert, "tls_key": key, "login": "hostd"})
    } else {
        let ip = io.line("The static IP address:")?;
        io.say("Without a name, no authority will issue a certificate you can use. The console can make its own self-signed one for that address: browsers warn on the first visit, and you must compare the certificate fingerprint yourself. A name with a real certificate is safer (free dynamic-DNS names exist).")?;
        if !io.yes_no("Use a self-signed certificate for this address?", false)? {
            return io.say("Nothing was changed. Get a name and run `blackroom internet` again.");
        }
        json!({"public": true, "public_name": ip, "public_cert": "self_signed", "tls_cert": null, "tls_key": null, "login": "hostd"})
    };
    let fields = patch
        .as_object_mut()
        .ok_or_else(|| refuse("internal: the change is not an object"))?;
    let state = console.check().map_err(refuse)?;
    let had_range = state["report"]["ice_ports"].is_string();
    if io.yes_no(
        &format!("Use a fixed UDP port range ({DEFAULT_MEDIA_PORTS}) for video, so it can be forwarded on the router?"),
        true,
    )? {
        fields.insert("ice_ports".into(), json!(DEFAULT_MEDIA_PORTS));
    } else if had_range {
        fields.insert("ice_ports".into(), Value::Null);
    }
    if io.yes_no(
        &format!("Use the public STUN server {DEFAULT_STUN} so WebRTC can learn your public address? It sees your address and nothing else."),
        true,
    )? {
        fields.insert("stun".into(), json!([DEFAULT_STUN]));
    } else {
        fields.insert("stun".into(), json!([]));
    }
    if state["report"]["turn_set"] == true {
        io.say("A TURN relay is already set and stays.")?;
    } else if io.yes_no(
        "Also set up a TURN relay? Only needed for mobile networks that block direct video; without it the slower https video is used.",
        false,
    )? {
        let urls = io.line("TURN addresses, separated by spaces (turn:home.example.org:3478?transport=udp):")?;
        let secret = io.line("Absolute path of the file holding the coturn static-auth-secret (mode 600):")?;
        fields.insert("turn".into(), json!(urls.split_whitespace().collect::<Vec<_>>()));
        fields.insert("turn_secret_file".into(), json!(secret));
    }
    commit(console, io, &patch, true, restart)?;
    let report = console.check().map_err(refuse)?;
    if let Some(url) = report["report"]["url"].as_str() {
        phone_test(io, url, false)?;
    }
    Ok(())
}

fn home(
    console: &dyn Console,
    io: &mut Io<'_>,
    current_public: bool,
    restart: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    if current_public && io.yes_no("Internet mode is on. Turn it off?", false)? {
        return commit(console, io, &json!({"public": false}), false, restart);
    }
    io.say("Nothing was changed.")
}

/// The whole guided set-up.
pub fn wizard(
    console: &dyn Console,
    io: &mut Io<'_>,
    tls_dir: &Path,
    restart: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    let state = console.check().map_err(refuse)?;
    io.say("Reaching this laptop from outside your home network")?;
    print_report(io, &state)?;
    io.say("")?;
    match io.choose(
        "How do you want to reach it?",
        &[
            "Through a VPN such as Tailscale (recommended: no router changes, works with any provider)",
            "Directly over the internet (you can forward ports on your router)",
            "Home network only",
        ],
        3,
    )? {
        1 => vpn(console, io, tls_dir, restart),
        2 => direct(console, io, tls_dir, restart),
        _ => home(console, io, state["report"]["public"] == true, restart),
    }
}

// ---- the verb ----

pub(crate) fn default_tls_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".config/blackroom/tls"))
}

pub fn restart_unit() -> bool {
    crate::setup::restart_console()
}

/// `blackroom internet ...`; `None` when the first argument is not `internet`.
pub fn dispatch(args: &[OsString]) -> Option<Result<(), Failure>> {
    let (verb, rest) = args.split_first()?;
    if verb.to_str() != Some("internet") {
        return None;
    }
    Some(run(rest))
}

fn run(args: &[OsString]) -> Result<(), Failure> {
    let (mut check, mut runtime, mut tls_dir, mut console_bin) = (false, None, None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || {
            iter.next()
                .map(PathBuf::from)
                .ok_or_else(|| (2, USAGE.to_string()))
        };
        match arg.to_str() {
            Some("--check") => check = true,
            Some("--runtime-dir") => runtime = Some(value()?),
            Some("--tls-dir") => tls_dir = Some(value()?),
            Some("--console-bin") => console_bin = Some(value()?),
            _ => return Err((2, USAGE.into())),
        }
    }
    let runtime = match runtime {
        Some(path) => path,
        None => crate::security::default_runtime().map_err(refuse)?,
    };
    let tls_dir = tls_dir
        .or_else(default_tls_dir)
        .ok_or_else(|| refuse("no absolute $HOME: pass --tls-dir"))?;
    if [&runtime, &tls_dir].iter().any(|path| !path.is_absolute()) {
        return Err((2, "paths must be absolute".into()));
    }
    let path = console_bin.or_else(Binary::find).ok_or_else(|| {
        refuse("blackroom-console is not installed (install the package, or pass --console-bin)")
    })?;
    let console = Binary { path, runtime };
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout();
    let mut io = Io::new(&mut input, &mut stdout);
    if check {
        let answer = console.check().map_err(refuse)?;
        return if print_report(&mut io, &answer)? {
            Ok(())
        } else {
            Err((1, "internet access has problems (see PROBLEM lines)".into()))
        };
    }
    if !rustix::termios::isatty(io::stdin()) {
        return Err((
            2,
            "the guided set-up needs a terminal; use --check for a report".into(),
        ));
    }
    wizard(&console, &mut io, &tls_dir, &restart_unit)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Fake {
        calls: RefCell<Vec<(Value, bool)>>,
        report: Value,
        dry_problems: Vec<String>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                report: json!({"access": "home", "public": false, "problems": [], "warnings": [], "notes": [], "forwards": [], "https_port": 8443, "turn_set": false}),
                dry_problems: Vec::new(),
            }
        }

        fn saved(&self) -> Vec<Value> {
            self.calls
                .borrow()
                .iter()
                .filter(|(_, save)| *save)
                .map(|(patch, _)| patch.clone())
                .collect()
        }
    }

    impl Console for Fake {
        fn check(&self) -> Result<Value, String> {
            Ok(json!({"ok": true, "report": self.report}))
        }

        fn apply(&self, patch: &Value, save: bool) -> Result<Value, String> {
            self.calls.borrow_mut().push((patch.clone(), save));
            let mut report = self.report.clone();
            if !save {
                report["problems"] = json!(self.dry_problems);
            }
            Ok(json!({"ok": true, "report": report}))
        }
    }

    fn drive(fake: &Fake, answers: &str, tls_dir: &Path) -> (Result<(), Failure>, String) {
        let mut input = io::Cursor::new(answers.as_bytes().to_vec());
        let mut out = Vec::new();
        let result = wizard(fake, &mut Io::new(&mut input, &mut out), tls_dir, &|| true);
        (result, String::from_utf8(out).unwrap())
    }

    fn with_cert(dir: &Path) {
        std::fs::write(dir.join("cert.pem"), "x").unwrap();
        std::fs::write(dir.join("key.pem"), "x").unwrap();
    }

    #[test]
    fn home_only_changes_nothing_and_turning_internet_mode_off_asks() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let (result, text) = drive(&fake, "\n", dir.path());
        assert!(result.is_ok() && fake.calls.borrow().is_empty());
        assert!(text.contains("Nothing was changed"));
        let mut on = Fake::new();
        on.report["public"] = json!(true);
        let (result, _) = drive(&on, "3\ny\ny\nn\n", dir.path());
        assert!(result.is_ok());
        assert_eq!(on.saved(), vec![json!({"public": false})]);
    }

    #[test]
    fn the_vpn_way_without_files_prints_the_commands_and_saves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let (result, text) = drive(&fake, "1\nlaptop.tailnet.ts.net\n", dir.path());
        assert!(result.is_ok());
        assert!(text.contains("sudo tailscale cert"));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn the_vpn_way_with_files_saves_a_private_network_setup_without_internet_mode() {
        let dir = tempfile::tempdir().unwrap();
        with_cert(dir.path());
        let fake = Fake::new();
        let (result, text) = drive(&fake, "1\nlaptop.tailnet.ts.net\ny\nn\n", dir.path());
        assert!(result.is_ok(), "{result:?}\n{text}");
        let saved = fake.saved();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["public"], false);
        assert_eq!(saved[0]["public_name"], "laptop.tailnet.ts.net");
        assert!(text.contains("Turn Wi-Fi off"));
    }

    #[test]
    fn a_provider_that_shares_one_address_is_steered_to_the_vpn() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let (_, text) = drive(&fake, "2\n3\nn\n", dir.path());
        assert!(text.contains("cannot work"));
        assert!(text.contains("Tailscale"));
        assert!(fake.calls.borrow().is_empty());
        let (_, text) = drive(&fake, "2\n2\n3\nn\n", dir.path());
        assert!(
            text.contains("100.64.x.x"),
            "an unsure owner gets the test to run"
        );
    }

    #[test]
    fn a_name_without_certificate_files_gets_the_certbot_steps_and_nothing_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let (result, text) = drive(&fake, "2\n1\n1\nhome.example.org\n", dir.path());
        assert!(result.is_ok());
        assert!(text.contains("sudo certbot certonly"));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn a_bare_ip_needs_an_explicit_yes_to_a_self_signed_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let (_, text) = drive(&fake, "2\n1\n2\n203.0.113.7\nn\n", dir.path());
        assert!(text.contains("A name with a real certificate is safer"));
        assert!(fake.calls.borrow().is_empty());
        // yes to self-signed, default range, default STUN, no TURN, then typed consent, then no restart.
        let (result, text) = drive(&fake, "2\n1\n2\n203.0.113.7\ny\n\n\n\nyes\nn\n", dir.path());
        assert!(result.is_ok(), "{result:?}\n{text}");
        let saved = fake.saved();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["public_cert"], "self_signed");
        assert_eq!(saved[0]["public_name"], "203.0.113.7");
        assert_eq!(saved[0]["tls_cert"], Value::Null);
        assert_eq!(saved[0]["ice_ports"], DEFAULT_MEDIA_PORTS);
        assert!(text.contains("trust-on-first-use"));
        assert!(text.contains("Direct internet access makes this login discoverable"));
    }

    #[test]
    fn internet_mode_is_saved_only_after_the_word_yes_and_never_when_the_dry_run_has_problems() {
        let dir = tempfile::tempdir().unwrap();
        with_cert(dir.path());
        let fake = Fake::new();
        let (result, text) = drive(
            &fake,
            "2\n1\n1\nhome.example.org\n\n\n\nmaybe\n",
            dir.path(),
        );
        assert!(result.is_ok(), "{result:?}\n{text}");
        assert!(fake.saved().is_empty(), "anything but yes saves nothing");
        assert!(text.contains("Nothing was saved."));
        let mut broken = Fake::new();
        broken.dry_problems = vec!["certificate: the certificate has expired".into()];
        let (_, text) = drive(
            &broken,
            "2\n1\n1\nhome.example.org\n\n\n\nyes\n",
            dir.path(),
        );
        assert!(text.contains("PROBLEM certificate: the certificate has expired"));
        assert!(broken.saved().is_empty());
    }

    #[test]
    fn closing_the_input_cancels_without_saving() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let (result, _) = drive(&fake, "2\n1\n", dir.path());
        assert!(
            result.is_err(),
            "the input ended at the first question about the name"
        );
        assert!(fake.calls.borrow().is_empty());
    }
}
