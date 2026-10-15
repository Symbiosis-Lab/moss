//! Verifying a folder's key for an account from the command line.
//!
//! Registering a site is refused until the folder's key is linked to an email
//! the person has confirmed. The desktop app does that with a magic link; this
//! is the same exchange for a terminal, run only when `moss deploy` was given
//! `--email`, because sending a message to an address is something the caller
//! must ask for.

use std::path::Path;
use std::time::Duration;

/// How often the sign-in session is asked whether the link was opened.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How long a sign-in link is waited for before giving up.
pub const WAIT_LIMIT: Duration = Duration::from_secs(15 * 60);

/// The sentence a refused registration starts with. The CLI recognises a
/// refusal by it, so the producer and the recogniser share one constant.
pub const UNVERIFIED_REFUSAL: &str = "Identity not yet verified for an account.";

/// Whether `message` is the refusal that `--email` can lift.
pub fn is_unverified_refusal(message: &str) -> bool {
    message.starts_with(UNVERIFIED_REFUSAL)
}

/// A shape check, not a deliverability check: one `@`, something either side,
/// a dot in the domain, no whitespace.
pub fn validate_email(address: &str) -> Result<(), String> {
    let ok = address.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty()
            && !domain.contains('@')
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
    }) && !address.chars().any(char::is_whitespace);
    if ok {
        Ok(())
    } else {
        Err(format!("error: '{address}' is not an email address. Pass the address of the account this site should belong to: --email=<address>"))
    }
}

/// 32 random bytes as 64 hex characters, as the app names its sign-in sessions.
fn new_session_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// The line printed when a confirmed address replaces one already stored.
pub fn replaced_email_notice(old: &str, new: &str) -> String {
    format!("The address stored for this folder, {old}, is being replaced by {new}.")
}

/// Polls in a row that the service answers with a 4xx before the wait is
/// abandoned: a refusal that repeats is a decision, not a blip.
const PERMANENT_REFUSALS: usize = 5;

fn is_client_error(e: &crate::seta::client::SetaError) -> bool {
    use crate::seta::client::SetaError;
    matches!(e, SetaError::Api { status, .. } | SetaError::Challenge { status } if (400..500).contains(status))
}

/// Send a sign-in link to `email`, wait for it to be opened, and record the
/// address in the folder's identity.
pub async fn verify_folder_email(
    folder: &Path,
    email: &str,
    interval: Duration,
    limit: Duration,
) -> Result<(), String> {
    validate_email(email)?;
    let mut identity = super::keypair::Identity::load_for_signing(folder).map_err(|e| e.to_string())?;
    let env = crate::build::site_config::resolve_environment(&folder.to_string_lossy());
    let client = crate::seta::client::MossSetaClient::for_environment(&identity, &env);

    let session_id = new_session_id();
    client
        .request_auth(email, &identity.pubkey, &session_id, None)
        .await
        .map_err(|e| format!("Could not send a sign-in link to {email}: {e}"))?;
    eprintln!("Sent a sign-in link to {email}. Open it to continue \u{2014} waiting (Ctrl-C to stop).");

    let deadline = tokio::time::Instant::now() + limit;
    let mut last_error: Option<String> = None;
    let mut any_poll_succeeded = false;
    let mut refusals_in_a_row = 0usize;
    loop {
        // A failed poll is a network blip, not a verdict: keep waiting, unless
        // the service keeps refusing the check outright.
        match client.check_auth_status(&session_id).await {
            Ok(status) => {
                any_poll_succeeded = true;
                refusals_in_a_row = 0;
                match status.status.as_str() {
                    "pending" => {}
                    "verified" => {
                        if let Some(opened_by) = status.email.as_deref().map(str::trim) {
                            if !opened_by.eq_ignore_ascii_case(email.trim()) {
                                return Err(format!(
                                    "The sign-in link was opened for {opened_by}, not {email}. \
                                     Nothing was changed. Run the command again with --email set to \
                                     the address of the account that should own this site."
                                ));
                            }
                        }
                        break;
                    }
                    other => {
                        return Err(format!(
                            "The sign-in link to {email} was not accepted (status: {other}). \
                             Run the command again with --email to get a new link."
                        ))
                    }
                }
            }
            Err(e) => {
                refusals_in_a_row = if is_client_error(&e) { refusals_in_a_row + 1 } else { 0 };
                last_error = Some(e.to_string());
                if refusals_in_a_row >= PERMANENT_REFUSALS {
                    return Err(format!(
                        "The hosting service refused the sign-in status check {PERMANENT_REFUSALS} times in a row: {e}. \
                         Nothing was changed."
                    ));
                }
            }
        }
        if tokio::time::Instant::now() + interval > deadline {
            return Err(match (any_poll_succeeded, last_error) {
                (false, Some(last)) => format!(
                    "The hosting service could not be reached to check whether the sign-in link sent to {email} \
                     was opened (last error: {last}). Check the connection and run the command again with --email."
                ),
                _ => format!(
                    "No one opened the sign-in link sent to {email} within {} minutes. \
                     Run the command again with --email to get a new link.",
                    limit.as_secs() / 60
                ),
            });
        }
        tokio::time::sleep(interval).await;
    }

    if let Some(old) = identity.email.as_deref().filter(|old| !old.eq_ignore_ascii_case(email)) {
        eprintln!("{}", replaced_email_notice(old, email));
    }
    identity.link_email(email.to_string());
    identity.save(folder).map_err(|e| e.to_string())?;
    eprintln!("Verified {email}.");
    Ok(())
}

/// Run `attempt`; if it is refused for want of a verified account and `email`
/// was given, verify that address and run it once more.
pub async fn retrying_after_verification<T, F, Fut>(
    folder: &Path,
    email: Option<&str>,
    interval: Duration,
    limit: Duration,
    attempt: F,
) -> Result<T, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    match (attempt().await, email) {
        (Err(refusal), Some(email)) if is_unverified_refusal(&refusal) => {
            verify_folder_email(folder, email, interval, limit).await?;
            attempt().await
        }
        (result, _) => result,
    }
}

#[cfg(test)]
#[path = "verify_email_tests.rs"]
pub(crate) mod verify_email_tests;
