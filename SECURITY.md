# Security policy

## Reporting a problem

Report privately, not in a public issue: on the repository page open the **Security** tab and choose **Report a vulnerability**
(GitHub private reporting). No e-mail address is published. Say which version, what you did, what you saw, and what you think
it allows. Do not include real credentials, recovery codes, Remote Access Keys or one-time URLs; if a report needs one, say so
and it will be arranged without sending the secret.

This is one person's project: replies are best effort, with no promised timeline. A fix goes into the next release and the
changelog; credit is given if you want it.

## What is supported

Only the newest release. The project is a technical preview for the configuration in
[docs/ops/compatibility-matrix.md](docs/ops/compatibility-matrix.md); a problem that needs an untested or unsupported setup is
still worth reporting but may be answered with a documented limit instead of a fix.

## What is in scope

The console's login (password, authenticator, Remote Access Key, trusted browsers), its sessions and revocation, the https and
http listeners, the host settings page, the input-grab daemon, the emergency chord and the restore and lock steps after a session,
the `blackroom` command, and the package scripts.

Known limits are listed, not hidden: [docs/security/red-team-report.md](docs/security/red-team-report.md) and
[docs/security/threat-model.md](docs/security/threat-model.md). In particular the key and authenticator are not phishing-proof, a
self-signed certificate relies on trust on first use, the console runs as the logged-in user, and Direct internet access has not
been tried on a real router.

## If you run it

Keep the Remote Access Key and recovery codes in a password manager, prefer a private VPN over opening a port, and keep a second
device with SSH access to the laptop. If a secret leaks: `blackroom reset security`; if a device is lost: forget it on the host
settings page or `blackroom ... revoke-device`. Details: [docs/ops/runbook.md](docs/ops/runbook.md).
