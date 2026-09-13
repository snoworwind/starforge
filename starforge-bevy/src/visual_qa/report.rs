//! K01 acceptance-report contract.
//!
//! A successful visual-QA process only proves that its requested scenes ran.
//! It does not imply that GPU timing, human review, integration, or packaging
//! passed. This schema keeps those meanings separate and gives every omitted
//! check an explicit reason.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const ACCEPTANCE_REPORT_SCHEMA_VERSION: u32 = 1;
pub const ACCEPTANCE_REPORT_KIND: &str = "starforge.visual_acceptance";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Skipped,
    NotRun,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct EvidenceRef {
    /// Portable path relative to the run root. Forward slashes are required.
    pub path: String,
    pub kind: String,
}

impl EvidenceRef {
    pub fn new(path: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            path: path.into().replace('\\', "/"),
            kind: kind.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AcceptanceCheck {
    pub id: String,
    pub label: String,
    pub required_for_release: bool,
    pub status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub evidence: Vec<EvidenceRef>,
}

impl AcceptanceCheck {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        required_for_release: bool,
        status: CheckStatus,
        reason: Option<String>,
        evidence: Vec<EvidenceRef>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            required_for_release,
            status,
            reason,
            evidence,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct StatusCounts {
    pub pass: usize,
    pub fail: usize,
    pub skipped: usize,
    pub not_run: usize,
}

impl StatusCounts {
    fn from_checks(checks: &[AcceptanceCheck]) -> Self {
        let mut counts = Self::default();
        for check in checks {
            match check.status {
                CheckStatus::Pass => counts.pass += 1,
                CheckStatus::Fail => counts.fail += 1,
                CheckStatus::Skipped => counts.skipped += 1,
                CheckStatus::NotRun => counts.not_run += 1,
            }
        }
        counts
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AcceptanceReport {
    pub schema_version: u32,
    pub report_kind: String,
    pub run_label: String,
    pub commit: Option<String>,
    /// Whether this invocation completed its requested scene work.
    pub execution_status: CheckStatus,
    /// Aggregate of all `required_for_release` checks. This remains
    /// `not_run`/`skipped` until later K work packages supply their evidence.
    pub release_status: CheckStatus,
    pub counts: StatusCounts,
    pub checks: Vec<AcceptanceCheck>,
    pub notes: Vec<String>,
}

impl AcceptanceReport {
    pub fn new(
        run_label: impl Into<String>,
        commit: Option<String>,
        execution_status: CheckStatus,
        checks: Vec<AcceptanceCheck>,
    ) -> Self {
        let release_status = aggregate_release_status(&checks);
        let counts = StatusCounts::from_checks(&checks);
        Self {
            schema_version: ACCEPTANCE_REPORT_SCHEMA_VERSION,
            report_kind: ACCEPTANCE_REPORT_KIND.to_string(),
            run_label: run_label.into(),
            commit,
            execution_status,
            release_status,
            counts,
            checks,
            notes: vec![
                "execution_status only describes this visual-QA invocation; it is not release approval".to_string(),
                "a skipped or not_run check is never counted as pass".to_string(),
                "paths are relative to the visual-QA run root".to_string(),
            ],
        }
    }

    /// Structural validation used by the writer and the checked-in example.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.schema_version != ACCEPTANCE_REPORT_SCHEMA_VERSION {
            issues.push("unsupported schema_version".to_string());
        }
        if self.report_kind != ACCEPTANCE_REPORT_KIND {
            issues.push("unexpected report_kind".to_string());
        }
        if self.run_label.trim().is_empty() {
            issues.push("run_label is empty".to_string());
        }
        if self.release_status != aggregate_release_status(&self.checks) {
            issues.push("release_status does not match required checks".to_string());
        }
        if self.counts != StatusCounts::from_checks(&self.checks) {
            issues.push("status counts do not match checks".to_string());
        }
        let mut ids = BTreeSet::new();
        for check in &self.checks {
            if check.id.trim().is_empty() || !ids.insert(check.id.as_str()) {
                issues.push(format!("empty or duplicate check id: {}", check.id));
            }
            if check.status != CheckStatus::Pass && check.reason.as_deref().unwrap_or("").is_empty()
            {
                issues.push(format!("non-pass check lacks a reason: {}", check.id));
            }
            for evidence in &check.evidence {
                if evidence.path.is_empty()
                    || evidence.kind.is_empty()
                    || evidence.path.contains('\\')
                    || evidence.path.starts_with('/')
                    || evidence.path.as_bytes().get(1) == Some(&b':')
                    || evidence.path.split('/').any(|part| part == "..")
                {
                    issues.push(format!("invalid evidence reference in {}", check.id));
                }
            }
        }
        issues
    }
}

fn aggregate_release_status(checks: &[AcceptanceCheck]) -> CheckStatus {
    let required: Vec<CheckStatus> = checks
        .iter()
        .filter(|check| check.required_for_release)
        .map(|check| check.status)
        .collect();
    if required.contains(&CheckStatus::Fail) {
        CheckStatus::Fail
    } else if required.is_empty() || required.contains(&CheckStatus::NotRun) {
        CheckStatus::NotRun
    } else if required.contains(&CheckStatus::Skipped) {
        CheckStatus::Skipped
    } else {
        CheckStatus::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str, status: CheckStatus) -> AcceptanceCheck {
        AcceptanceCheck::new(
            id,
            id,
            true,
            status,
            (status != CheckStatus::Pass).then(|| "not complete".to_string()),
            vec![],
        )
    }

    #[test]
    fn statuses_have_unambiguous_wire_values() {
        assert_eq!(
            serde_json::to_string(&CheckStatus::Pass).unwrap(),
            "\"pass\""
        );
        assert_eq!(
            serde_json::to_string(&CheckStatus::Fail).unwrap(),
            "\"fail\""
        );
        assert_eq!(
            serde_json::to_string(&CheckStatus::Skipped).unwrap(),
            "\"skipped\""
        );
        assert_eq!(
            serde_json::to_string(&CheckStatus::NotRun).unwrap(),
            "\"not_run\""
        );
    }

    #[test]
    fn release_status_never_promotes_missing_work_to_pass() {
        assert_eq!(
            aggregate_release_status(&[check("gpu", CheckStatus::Skipped)]),
            CheckStatus::Skipped
        );
        assert_eq!(
            aggregate_release_status(&[
                check("gpu", CheckStatus::Skipped),
                check("review", CheckStatus::NotRun),
            ]),
            CheckStatus::NotRun
        );
        assert_eq!(
            aggregate_release_status(&[
                check("gpu", CheckStatus::NotRun),
                check("scene", CheckStatus::Fail),
            ]),
            CheckStatus::Fail
        );
    }

    #[test]
    fn checked_in_example_matches_the_runtime_contract() {
        let example: AcceptanceReport = serde_json::from_str(include_str!(
            "../../docs/art-overhaul/examples/K01-acceptance-report-v1.json"
        ))
        .expect("example parses");
        assert!(example.validate().is_empty(), "{:?}", example.validate());

        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../docs/art-overhaul/schemas/acceptance-report-v1.schema.json"
        ))
        .expect("schema parses");
        assert_eq!(schema["properties"]["schema_version"]["const"], 1);
        assert_eq!(
            schema["$id"],
            "https://starforge.local/schemas/acceptance-report-v1.schema.json"
        );
    }

    #[test]
    fn validation_rejects_ambiguous_checks_and_nonportable_paths() {
        let mut report = AcceptanceReport::new(
            "bad-example",
            None,
            CheckStatus::Pass,
            vec![
                AcceptanceCheck::new(
                    "duplicate",
                    "first",
                    true,
                    CheckStatus::Fail,
                    None,
                    vec![EvidenceRef::new("C:/private/result.json", "result")],
                ),
                AcceptanceCheck::new(
                    "duplicate",
                    "second",
                    true,
                    CheckStatus::Pass,
                    None,
                    vec![EvidenceRef::new("../other-run.json", "result")],
                ),
            ],
        );
        report.counts.pass += 1;
        let issues = report.validate().join("\n");
        assert!(issues.contains("duplicate check id"));
        assert!(issues.contains("non-pass check lacks a reason"));
        assert!(issues.contains("invalid evidence reference"));
        assert!(issues.contains("status counts do not match"));
    }
}
