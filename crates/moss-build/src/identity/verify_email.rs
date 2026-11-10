//! Verifying a folder's key for an account from the command line.
//!
//! Registering a site is refused until the folder's key is linked to an email
//! the person has confirmed. The desktop app does that with a magic link; this
//! is the same exchange for a terminal, run only when `moss deploy` was given
//! `--email`, because sending a message to an address is something the caller
//! must ask for.

use crate::deploy::PublishError;
use crate::seta::client::NOT_OWNER;
use std::path::Path;
use std::time::Duration;

/// How often the sign-in session is asked whether the link was opened.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How long a sign-in link is waited for before giving up.
pub const WAIT_LIMIT: Duration = Duration::from_secs(15 * 60);

/// The terminal's words for a folder the hosting service would not accept a
/// publish from: one never signed in, or one whose key is gone. `host` is the
/// site's address once it has one, else the hosting service.
pub fn not_signed_in(folder: &Path, host: &str) -> String {
    let root = crate::vault::paths::VaultRoot::resolve(folder);
    let name = root.name();
    format!("{name} isn't signed in to {host}. Sign it in: moss deploy {} --email <address>", folder.display())
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
        Err(format!("error: '{address}' is not an email address: --email=<address>"))
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

/// What a sign-in link names: the folder's site, if it has been deployed, and
/// the folder's own name. Never a path, which would show the person's file
/// system to whoever the link is forwarded to.
pub fn sign_in_folder(folder: &Path) -> crate::seta::client::SignInFolder {
    let name = crate::vault::paths::VaultRoot::resolve(folder).name().to_string();
    crate::seta::client::SignInFolder {
        site_id: crate::build::site_config::site_id_for_folder(folder).ok(),
        name: Some(name).filter(|n| !n.trim().is_empty()),
    }
}

/// The longest wait the server may name before a sign-in request is sent
/// again; a longer one is reported instead.
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// Ask for a sign-in link for `folder`'s key, naming the folder. A key the
/// server says is signed in to another site is a copy of another folder's
/// key: the folder gets one of its own and the link is asked for again, after
/// the wait the server names, if it names one (`on_wait` is told first).
/// Returns the identity the link signs in.
pub async fn request_sign_in(
    folder: &Path,
    client: &crate::seta::client::MossSetaClient,
    email: &str,
    session_id: &str,
    lang: Option<&str>,
    on_wait: impl Fn(Duration),
) -> Result<super::keypair::Identity, crate::seta::client::SetaError> {
    use crate::seta::client::SetaError;
    let identity_error = |e: super::keypair::IdentityError| SetaError::Identity(e.to_string());
    let named = sign_in_folder(folder);
    let mut service = super::service::IdentityService::new(folder);
    let identity = service.key_for_sign_in().map_err(identity_error)?.clone();
    match client.request_auth(email, &identity.pubkey, session_id, lang, &named).await {
        Err(SetaError::KeyHasOtherSite { site }) => {
            log::info!(target: "identity", "this folder's key is signed in to {site}; the folder signs in with a key of its own");
            let own = service.own_key_for_sign_in().map_err(identity_error)?.clone();
            let send = || client.request_auth(email, &own.pubkey, session_id, lang, &named);
            match send().await {
                Err(SetaError::TooSoon { retry_after, .. }) if retry_after <= LONGEST_WAIT => {
                    on_wait(retry_after);
                    tokio::time::sleep(retry_after).await;
                    send().await?;
                }
                other => other?,
            }
            Ok(own)
        }
        other => other.map(|()| identity),
    }
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
    let env = crate::build::site_config::resolve_environment(&folder.to_string_lossy());
    let client = crate::seta::client::MossSetaClient::new_for_environment(&env);
    let session_id = new_session_id();
    let waiting = |wait: Duration| eprintln!("Waiting {} s to send the sign-in link to {email}.", wait.as_secs());
    let mut identity = request_sign_in(folder, &client, email, &session_id, None, waiting)
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
                        "The mosspub.com service refused the sign-in status check {PERMANENT_REFUSALS} times in a row: {e}. \
                         Nothing was changed."
                    ));
                }
            }
        }
        if tokio::time::Instant::now() + interval > deadline {
            return Err(match (any_poll_succeeded, last_error) {
                (false, Some(last)) => format!(
                    "The mosspub.com service could not be reached to check whether the sign-in link sent to {email} \
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
    // The service already links the key to the account, and this file is only a
    // local copy of that fact: a folder that cannot be written (read-only, a
    // cloud placeholder) must not fail a publish the service would accept.
    if let Err(e) = identity.save(folder) {
        eprintln!("Could not record {email} in this folder ({e}); the service has it, so the publish continues.");
    }
    eprintln!("Signed {} in as {email}.", crate::vault::paths::VaultRoot::resolve(folder).name());
    Ok(())
}

/// Record in the folder the account its key belongs to now. `public.json`
/// holds only a local copy of that, and a key can change accounts: a site
/// transfer moves it on the receiver's grant. Any failure leaves the copy as
/// it was; a key the service links to no account changes nothing.
pub async fn refresh_recorded_email(
    folder: &Path,
    identity: &super::keypair::Identity,
    client: &crate::seta::client::MossSetaClient,
) {
    let Ok(Some(email)) = client.whoami().await.map(|who| who.email) else { return };
    match identity.email.as_deref() {
        Some(old) if old.eq_ignore_ascii_case(&email) => return,
        Some(old) => eprintln!("This folder's key now belongs to {email}, not {old}."),
        None => {}
    }
    let mut updated = identity.clone();
    updated.link_email(email);
    let _ = updated.save_public(folder);
}

/// Whether the hosting service says this folder's key belongs to no account,
/// as a fresh key does until it is verified. False when it cannot be asked.
async fn key_is_unlinked(folder: &Path) -> bool {
    let Ok(identity) = super::keypair::Identity::load_for_signing(folder) else { return false };
    let env = crate::build::site_config::resolve_environment(&folder.to_string_lossy());
    let client = crate::seta::client::MossSetaClient::for_environment(&identity, &env);
    client.whoami().await.is_ok_and(|who| who.email.is_none())
}

/// Run `attempt`; if it is refused for want of a verified account and `email`
/// was given, verify that address and run it once more. A not-the-owner refusal
/// counts only when the service says the key belongs to no account (asked
/// again here because only a refused publish needs the answer).
pub async fn retrying_after_verification<T, F, Fut>(
    folder: &Path,
    email: Option<&str>,
    interval: Duration,
    limit: Duration,
    attempt: F,
) -> Result<T, PublishError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, PublishError>>,
{
    match (attempt().await, email) {
        (Err(refusal), Some(email))
            if matches!(refusal, PublishError::NotSignedIn { .. })
                || (matches!(&refusal, PublishError::Other(text) if text.contains(NOT_OWNER)) && key_is_unlinked(folder).await) =>
        {
            verify_folder_email(folder, email, interval, limit).await?;
            attempt().await
        }
        (result, _) => result,
    }
}

#[cfg(test)]
#[path = "verify_email_tests.rs"]
pub(crate) mod verify_email_tests;
