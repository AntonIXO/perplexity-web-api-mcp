//! Rate-limit / usage-quota inspection via Perplexity's internal REST endpoint.
//!
//! Perplexity's web UI reads `/rest/rate-limit/all` to render its usage
//! counters and to enforce per-plan limits client-side. This module exposes the
//! same data so the MCP server can (a) surface a clear "out of limit" error
//! instead of the silent empty answer the SSE endpoint returns when a quota is
//! exhausted, and (b) let callers query their remaining quotas directly.
//!
//! The endpoint requires authenticated session cookies. The response shape was
//! verified against the live API in 2026-07; it exposes only *remaining*
//! counts (no totals or reset timestamps) for the top-level features, plus an
//! optional per-source monthly-limit map.

use crate::types::SearchMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-source monthly limit entry from `sources.source_to_limit`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceLimit {
    /// Monthly cap for this source, if the source is metered.
    #[serde(default)]
    pub monthly_limit: Option<i64>,
    /// Remaining queries for this source this month, if metered.
    #[serde(default)]
    pub remaining: Option<i64>,
}

impl SourceLimit {
    /// Returns `true` when the source has no monthly cap (unlimited).
    pub fn is_unlimited(&self) -> bool {
        self.monthly_limit.is_none()
    }

    /// Returns `true` when a metered source has run out of remaining queries.
    pub fn is_exhausted(&self) -> bool {
        matches!(self.remaining, Some(r) if r <= 0)
    }
}

/// Wrapper matching the nested `sources` object in the API response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sources {
    /// Map of source id (e.g. `"web"`, `"google_drive"`) to its limit entry.
    #[serde(default)]
    pub source_to_limit: BTreeMap<String, SourceLimit>,
}

/// Current rate-limit status parsed from `GET /rest/rate-limit/all`.
///
/// All top-level fields are *remaining* counts. A value of `0` means the
/// corresponding feature's quota is exhausted for the current window
/// (Pro Search is a weekly rolling window; Research / Labs / Browser-Agent are
/// monthly).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimits {
    /// Remaining Pro Search queries (weekly rolling window).
    #[serde(default)]
    pub remaining_pro: i64,
    /// Remaining Deep Research queries (monthly).
    #[serde(default)]
    pub remaining_research: i64,
    /// Remaining Create Files &amp; Apps ("Labs") queries (monthly).
    #[serde(default)]
    pub remaining_labs: i64,
    /// Remaining Browser Agent / Computer queries (monthly).
    #[serde(default)]
    pub remaining_agentic_research: i64,
    /// Per-model limits, when the account exposes them (often empty).
    #[serde(default)]
    pub model_specific_limits: serde_json::Value,
    /// Per-source monthly limits.
    #[serde(default)]
    pub sources: Sources,
}

impl RateLimits {
    /// Returns `true` when at least one Pro Search query remains.
    pub fn has_pro_queries(&self) -> bool {
        self.remaining_pro > 0
    }

    /// Returns `true` when at least one Deep Research query remains.
    pub fn has_research_queries(&self) -> bool {
        self.remaining_research > 0
    }

    /// Returns `true` when at least one Browser Agent / Computer query remains.
    pub fn has_agentic_queries(&self) -> bool {
        self.remaining_agentic_research > 0
    }

    /// Maps a [`SearchMode`] to the feature quota it consumes, returning the
    /// feature's human-readable name and its remaining count.
    ///
    /// Returns `None` for modes that draw on the free/unmetered tier
    /// ([`SearchMode::Auto`]).
    pub fn quota_for_mode(&self, mode: SearchMode) -> Option<QuotaStatus> {
        let (feature, remaining) = match mode {
            // Free/unmetered — the turbo path does not draw a Pro quota.
            SearchMode::Auto => return None,
            // Copilot search backends consume Pro Search.
            SearchMode::Pro | SearchMode::Reasoning | SearchMode::Study => {
                ("Pro Search", self.remaining_pro)
            }
            SearchMode::DeepResearch => ("Deep Research", self.remaining_research),
            SearchMode::Computer => {
                ("Browser Agent / Computer", self.remaining_agentic_research)
            }
            // Document review runs on the Labs (Files &amp; Apps) allotment.
            SearchMode::DocumentReview => ("Create Files & Apps", self.remaining_labs),
        };
        Some(QuotaStatus { feature, remaining })
    }

    /// One-line human-readable summary of the primary feature quotas.
    pub fn summary(&self) -> String {
        format!(
            "Pro Search: {} | Deep Research: {} | Files & Apps: {} | Browser Agent: {}",
            self.remaining_pro,
            self.remaining_research,
            self.remaining_labs,
            self.remaining_agentic_research,
        )
    }
}

/// The remaining quota for the feature a given search mode consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaStatus {
    /// Human-readable feature name (e.g. `"Pro Search"`).
    pub feature: &'static str,
    /// Remaining queries for that feature in the current window.
    pub remaining: i64,
}

impl QuotaStatus {
    /// Returns `true` when the feature's quota is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_response() {
        let json = r#"{
            "remaining_pro": 12,
            "remaining_research": 0,
            "remaining_labs": 5,
            "remaining_agentic_research": 3,
            "sources": { "source_to_limit": {
                "web": { "monthly_limit": null, "remaining": null },
                "google_drive": { "monthly_limit": 500, "remaining": 0 }
            }}
        }"#;
        let rl: RateLimits = serde_json::from_str(json).unwrap();
        assert_eq!(rl.remaining_pro, 12);
        assert!(rl.has_pro_queries());
        assert!(!rl.has_research_queries());
        assert!(rl.sources.source_to_limit["web"].is_unlimited());
        assert!(rl.sources.source_to_limit["google_drive"].is_exhausted());
    }

    #[test]
    fn tolerates_missing_fields() {
        let rl: RateLimits = serde_json::from_str("{}").unwrap();
        assert_eq!(rl.remaining_pro, 0);
        assert!(!rl.has_pro_queries());
        assert!(rl.sources.source_to_limit.is_empty());
    }

    #[test]
    fn maps_modes_to_quotas() {
        let rl = RateLimits {
            remaining_pro: 0,
            remaining_research: 4,
            remaining_agentic_research: 0,
            ..Default::default()
        };
        assert!(rl.quota_for_mode(SearchMode::Auto).is_none());
        assert!(rl.quota_for_mode(SearchMode::Reasoning).unwrap().is_exhausted());
        assert!(!rl.quota_for_mode(SearchMode::DeepResearch).unwrap().is_exhausted());
        assert!(rl.quota_for_mode(SearchMode::Computer).unwrap().is_exhausted());
    }
}
