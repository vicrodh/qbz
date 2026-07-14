// crates/qbzd/src/cli/copy.rs — normative CLI copy for the auth verbs.
//
// Strings are reproduced verbatim from 02-cli-and-api.md §2.2, "modulo
// interpolated values" (§1.4): the ephemeral listener port is substituted into
// the `ssh -L` forward hint so it is actionable on a headless box, and the
// success line interpolates the validated session's email / plan / user id.
use qbz_models::UserSession;

/// The 300 s browser-login timeout (02 §2.2). `port` is the ephemeral port the
/// one-shot listener bound; it is interpolated into both halves of the
/// `ssh -L <port>:localhost:<port>` forward so a headless operator can tunnel the
/// exact port the redirect will target.
pub fn login_timeout(port: u16) -> String {
    format!(
        "error: no OAuth redirect received within 300 s
  → headless box? forward the port:  ssh -L {port}:localhost:{port} pi@kitchen-pi
    then open the login URL in this machine's browser
  → or paste the redirect URL:       qbzd login --paste
  → or inject a token directly:      qbzd login --token <user_auth_token>"
    )
}

/// Human success line for `qbzd login` (02 §2.2):
/// `logged in as user@example.com (studio) — user id 1234567`.
pub fn login_success(session: &UserSession) -> String {
    format!(
        "logged in as {} ({}) — user id {}",
        session.email, session.subscription_label, session.user_id
    )
}

/// Human success line for `qbzd logout` (02 §2.2). The daemon-up form names the
/// resulting NeedsAuth state so the operator knows playback stopped; the
/// daemon-down form is terse because there is nothing running to transition.
pub fn logout_success(daemon_nudged: bool) -> String {
    if daemon_nudged {
        "logged out — daemon is now in needs-auth state".to_string()
    } else {
        "logged out".to_string()
    }
}
