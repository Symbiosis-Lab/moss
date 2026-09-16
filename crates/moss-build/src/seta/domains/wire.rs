//! The shapes moss-seta speaks on the domain endpoints.
//!
//! Split out of `domains.rs` so the endpoint methods and the vocabulary they
//! parse are each readable on their own. The second half crossed from the
//! app's `domain/types.rs` with the client itself (ADR-078); the app
//! re-exports it from `domain::types`, so those paths and the specta bindings
//! that name it still resolve.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Domain search result from moss-seta.
#[derive(Debug, Deserialize, Serialize)]
pub struct DomainSearchResult {
    pub domain: String,
    pub available: bool,
    pub price: Option<f64>,
    pub currency: Option<String>,
}

/// Wrapper for domain search API response.
/// Supports backward compatibility with old API format that only returns results.
#[derive(Debug, Deserialize)]
pub struct DomainSearchResponse {
    pub results: Vec<DomainSearchResult>,
    /// Phase of search (optional for backward compatibility with old API)
    #[serde(default)]
    pub phase: String,
    /// Whether more TLDs are available to search (optional, defaults to false)
    #[serde(rename = "hasMore", default)]
    pub has_more: bool,
    /// Count of available domains (optional, will be computed if missing)
    #[serde(rename = "availableCount")]
    pub available_count: Option<u32>,
}

/// Payment intent response for domain purchase.
#[derive(Debug, Deserialize, Serialize)]
pub struct PaymentIntentResponse {
    pub client_secret: String,
    pub payment_intent_id: String,
    pub domain: String,
    pub years: i32,
    pub price_usd: i64,
    pub currency: String,
    #[serde(default)]
    pub registrar_cost_cents: Option<i32>,
    #[serde(default)]
    pub card_processing_cents: Option<i32>,
    /// ISO 3166-1 alpha-2 country hint from CF-IPCountry, used by the
    /// widget to default the country dropdown. None when the header is
    /// missing, XX, or T1 (Tor).
    #[serde(default)]
    pub country_hint: Option<String>,
}

/// Domain registration response.
#[derive(Debug, Deserialize, Serialize)]
pub struct DomainRegistrationResponse {
    pub domain: String,
    pub registered: bool,
    pub expires_at: i64,
}

/// User domains response from moss-seta.
#[derive(Debug, Deserialize, Serialize)]
pub struct UserDomainsResponse {
    pub domains: Vec<UserDomain>,
    /// Email the domains are associated with (if available)
    pub email: Option<String>,
}

/// Individual domain in the user domains response.
#[derive(Debug, Deserialize, Serialize)]
pub struct UserDomain {
    pub domain: String,
    pub registered_at: i64,
    pub expires_at: i64,
    #[serde(default)]
    pub dns_configured: bool,
    #[serde(default = "default_true")]
    pub auto_renew: bool,
    #[serde(default = "default_active")]
    pub status: String,
}

fn default_true() -> bool {
    true
}

fn default_active() -> String {
    "active".to_string()
}

/// Reference to the site a domain is assigned to. Object-shaped for
/// forward-compatibility with a future `name` field (human-readable display
/// label, see F5 follow-up). Today `id` is the site_id slug, which is already
/// human-readable (e.g. "her-blog", "landing").
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AssignedSiteRef {
    /// site_id (slug form, e.g. "her-blog").
    pub id: String,
}

/// A domain owned by the authenticated user (via moss purchase or external link).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AssignableDomain {
    /// Fully-qualified domain name, lowercase.
    pub host: String,
    /// "moss" if purchased via moss, "external" if brought from another registrar.
    pub source: String,
    /// The site this domain is currently assigned to, or null if unassigned.
    pub assigned_to: Option<AssignedSiteRef>,
}

/// Response from GET /api/domains/owned.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AssignableDomainsResponse {
    pub domains: Vec<AssignableDomain>,
}

/// DNS and TLS certificate status for an owned domain.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DomainStatus {
    /// Whether the domain's A record resolves to the VPS IP.
    pub dns_ok: bool,
    /// Whether a TLS certificate appears to have been issued (heuristic).
    pub cert_ok: bool,
    /// Unix timestamp (seconds) of when this status was last checked.
    pub last_checked: f64,
    /// Unix timestamp (seconds) of first configuration, or null if not yet configured.
    pub first_configured_at: Option<f64>,
}

/// A saved payment method as returned by `GET /api/payment/methods`.
///
/// Wire shape — not re-exported as a Tauri type. Commands convert this into
/// `StoredPaymentMethod` (which adapts field widths for specta bindings).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StoredPaymentMethodRemote {
    pub id: String,
    pub brand: String,
    pub last4: String,
    pub exp_month: u32,
    pub exp_year: u32,
}

/// Search phase for progressive TLD loading
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(rename_all = "lowercase")]
pub enum SearchPhase {
    /// Popular TLDs (com, net, org, io, etc.)
    Popular,
    /// Extended TLDs (online, site, store, etc.)
    Extended,
    /// All TLDs
    All,
}

/// Response from domain suggest endpoint
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct DomainSuggestResponse {
    /// Exact matches across TLDs
    pub lookup: Vec<DomainSuggestion>,
    /// AI-generated domain alternatives
    pub suggestions: Vec<DomainSuggestion>,
    /// Count of available domains (lookup + suggestions)
    #[serde(default, rename = "availableCount")]
    pub available_count: u32,
    /// Exact-TLD match pinned at top when the user typed a full domain.
    /// Populated from moss-seta when the query contained a dot. See
    /// docs/archive/2026-04-21-domain-search-ux-fix-design.md.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "exact_match")]
    pub exact_match: Option<DomainSuggestion>,
}

/// Result of authoritative domain availability check
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct DomainAvailabilityResult {
    /// The domain that was checked
    pub domain: String,
    /// Whether the domain is available for registration
    pub available: bool,
    /// Price in USD (if available from registrar)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// Currency code (e.g., "USD")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Error message if the check failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// ICANN-required registrant contact information.
///
/// New (2026-04-22) minimal shape: the server-side registrant resolver
/// derives city/state from postal code (US/CA/AU) and pulls postal_code +
/// country from Stripe billing_details. We only forward fields the server
/// genuinely needs from the user. Email is server-side (user's verified
/// account email), so it's not in this struct.
///
/// City is optional in the wire format because it's only required when
/// country ∉ {US, CA, AU} — for US/CA/AU buyers the server derives it
/// from the postal code via bundled GeoNames data.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct RegistrantContact {
    /// Single Name field; server splits into first/last on first whitespace
    /// (mononyms duplicate to satisfy OpenSRS's required-field check).
    pub name: String,
    /// Phone number; server normalizes to OpenSRS format +CCC.NNNNNNNNNN.
    pub phone: String,
    /// Street address; max 64 chars (OpenSRS limit).
    pub address1: String,
    /// ISO 3166-1 alpha-2 country code.
    pub country: String,
    /// Required iff country ∉ {US, CA, AU}; otherwise the server derives
    /// city from the bundled GeoNames postal-code map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
}

/// Billing address returned by seta from Stripe PaymentMethod.billing_details.
///
/// All fields may be absent — Stripe only populates what the card issuer
/// supplies. The widget maps these snake_case fields to the camelCase
/// `StripeBillingAddress` shape it already uses for new-card flows.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BillingAddress {
    pub line1: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

/// Result of charging a saved payment method for a domain purchase.
///
/// Returned by `charge_saved_for_domain` (POST /api/payment/domains/charge-saved).
/// Under the unified manual-capture model the happy-path status is
/// `requires_capture`, not `succeeded`:
/// - `status: "requires_capture"` — card authorized; client should call
///   POST /api/domains/register immediately.
/// - `status: "requires_action"` — SCA required; client runs Stripe
///   `handleNextAction(client_secret)`, then calls POST /api/domains/register.
/// - `status: "failed"` — backend returns HTTP 400 in this case, surfaced as
///   `Err` from the client method; callers will not see this variant on success.
///
/// `billing_address` is no longer returned: the server retrieves billing
/// details from the PaymentIntent itself on `/register`. The field is kept
/// in the struct (with `default`) for forward-compat with any in-flight
/// stale clients; new code should not read it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct ChargeSavedResult {
    pub status: String,
    pub payment_intent_id: String,
    pub client_secret: String,
    pub price_cents: u32,
    /// Country hint from CF-IPCountry, mirrors PaymentIntent.country_hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country_hint: Option<String>,
    /// DEPRECATED: server no longer returns this; widget no longer reads it.
    /// Kept on the struct so older server responses still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_address: Option<BillingAddress>,
}

/// Subscription status returned by the seta backend.
///
/// Used to proactively check whether the user has an active subscription
/// before deploying, providing better UX than a deploy-time 403 error.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SubscriptionStatus {
    /// Subscription plan name (e.g., "free", "pro", "lifetime")
    pub plan: String,
    /// Whether the subscription is currently active as reported by the server.
    /// `None` means the server didn't include the field — use [`Self::is_active`]
    /// to get a defensive answer derived from `expires_at`.
    #[serde(default)]
    pub active: Option<bool>,
    /// Days remaining until expiration; `None` for unlimited (monthly/lifetime)
    #[serde(rename = "daysRemaining", default)]
    pub days_remaining: Option<u32>,
    /// Storage quota in megabytes
    #[serde(rename = "storageMb")]
    pub storage_mb: u32,
    /// Monthly bandwidth quota in megabytes
    #[serde(rename = "bandwidthMbMonth")]
    pub bandwidth_mb_month: u32,
    /// Unix timestamp (seconds) when subscription expires; `None` if no expiry
    #[serde(rename = "expiresAt", default)]
    pub expires_at: Option<u64>,
}

impl Default for SearchPhase {
    fn default() -> Self {
        SearchPhase::Popular
    }
}

impl std::fmt::Display for SearchPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchPhase::Popular => write!(f, "popular"),
            SearchPhase::Extended => write!(f, "extended"),
            SearchPhase::All => write!(f, "all"),
        }
    }
}

/// Domain suggestion from NAME_SUGGEST API
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct DomainSuggestion {
    /// The full domain name (e.g., "example.com")
    pub domain: String,
    /// Availability status: "available", "taken", or "undetermined"
    pub status: String,
    /// Price in USD (if available)
    #[serde(default)]
    pub price: Option<f64>,
    /// Currency code (e.g., "USD")
    #[serde(default)]
    pub currency: Option<String>,
}

impl SubscriptionStatus {
    /// Defensive "is this subscription usable right now?" check.
    ///
    /// Prefers the server's `active` flag when present (it knows server time
    /// and any business-rule overrides). Falls back to comparing `expires_at`
    /// against local time when the field is missing — covers older/variant
    /// server shapes that omit `active`.
    ///
    /// Caveat: the fallback uses the local wall clock. If the user's system
    /// clock is skewed, this can disagree with the server. Accepted because
    /// this path only runs when the server didn't include the authoritative
    /// flag, and moss-seta's whoami route now derives and returns it
    /// (feat/whoami-active-field) — the fallback is belt-and-suspenders, not
    /// the primary contract.
    pub fn is_active(&self) -> bool {
        if let Some(active) = self.active {
            return active;
        }
        match self.expires_at {
            None => true, // lifetime / unlimited
            Some(exp) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                exp > now
            }
        }
    }
}
