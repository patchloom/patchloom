//! Over-wide fuzzy span policy for library hosts (#1981).

use super::AGENT_MIN_FUZZY_SCORE;

/// Thresholds for [`fuzzy_span_suspicious`] / [`fuzzy_span_suspicious_with_policy`] (#1981).
///
/// Defaults match the Bline-informed starting policy (host refuse, not library
/// auto-revert): Unicode char counts, wide-span cap, and a tighter ratio near
/// the agent fuzzy floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuzzySpanPolicy {
    /// Refuse when `matched_chars > max_ratio * old_chars` (ceiling).
    pub max_ratio: f64,
    /// Also allow at least `old_chars + abs_extra_chars` before the ratio alone
    /// can fire (so tiny `old` strings do not explode on modest expansions).
    pub abs_extra_chars: usize,
    /// Lower bound of the "near floor" score band (inclusive), typically
    /// [`AGENT_MIN_FUZZY_SCORE`].
    pub near_floor_score_lo: f64,
    /// Upper bound of the near-floor score band (exclusive).
    pub near_floor_score_hi: f64,
    /// Within the near-floor score band, refuse when matched/old char ratio
    /// exceeds this value.
    pub near_floor_ratio: f64,
}

impl Default for FuzzySpanPolicy {
    fn default() -> Self {
        Self {
            max_ratio: 4.0,
            abs_extra_chars: 40,
            near_floor_score_lo: AGENT_MIN_FUZZY_SCORE,
            near_floor_score_hi: 0.95,
            near_floor_ratio: 2.0,
        }
    }
}

/// Whether a **fuzzy** matched span is suspiciously wider than the requested
/// `old` string (#1981).
///
/// Hosts should call this **before treating Apply as trusted** when
/// `match_mode` is fuzzy (or when `matched_text` is present from fuzzy
/// recovery). Uses Unicode scalar counts (`chars().count()`), not bytes.
///
/// Default thresholds ([`FuzzySpanPolicy::default`]):
/// - `matched_chars > max(4 * old_chars, old_chars + 40)`, or
/// - score in `[0.90, 0.95)` and `matched_chars / old_chars > 2`
///
/// Returns `false` when `matched_text` is `None` or empty (nothing to judge).
/// Empty `old` with a non-empty match is treated as suspicious.
///
/// Prefer this helper over a redundant length field on [`super::EditResult`]: when
/// `matched_text` is populated, hosts already have the span.
///
/// ```rust
/// use patchloom::api::{fuzzy_span_suspicious, AGENT_MIN_FUZZY_SCORE};
///
/// let old = "process_data";
/// let matched = "fn process_data() {\n    // body\n}\n";
/// assert!(fuzzy_span_suspicious(
///     old,
///     Some(matched),
///     Some(AGENT_MIN_FUZZY_SCORE),
/// ));
/// assert!(!fuzzy_span_suspicious(
///     old,
///     Some("process_data"),
///     Some(0.99),
/// ));
/// ```
#[must_use]
pub fn fuzzy_span_suspicious(
    old: &str,
    matched_text: Option<&str>,
    match_score: Option<f64>,
) -> bool {
    fuzzy_span_suspicious_with_policy(old, matched_text, match_score, &FuzzySpanPolicy::default())
}

/// Like [`fuzzy_span_suspicious`] with a custom [`FuzzySpanPolicy`].
#[must_use]
pub fn fuzzy_span_suspicious_with_policy(
    old: &str,
    matched_text: Option<&str>,
    match_score: Option<f64>,
    policy: &FuzzySpanPolicy,
) -> bool {
    let Some(matched) = matched_text.filter(|s| !s.is_empty()) else {
        return false;
    };
    let old_n = old.chars().count();
    let matched_n = matched.chars().count();
    if old_n == 0 {
        return matched_n > 0;
    }
    let ratio_cap = (old_n as f64 * policy.max_ratio).ceil() as usize;
    let abs_cap = old_n.saturating_add(policy.abs_extra_chars);
    let max_allowed = ratio_cap.max(abs_cap);
    if matched_n > max_allowed {
        return true;
    }
    if let Some(score) = match_score
        && score >= policy.near_floor_score_lo
        && score < policy.near_floor_score_hi
    {
        let ratio = matched_n as f64 / old_n as f64;
        if ratio > policy.near_floor_ratio {
            return true;
        }
    }
    false
}
