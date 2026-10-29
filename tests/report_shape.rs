//! The upload schema refuses what it does not know: a producer's typo is a
//! dropped upload and a stale producer, never a thinner check.

use fleetwatch::report::types::ReportUpload;

const REPORT: &str = r#"{"schema":1,"id":"01J","collector":"c","collected_at":"2026-10-29T00:00:00Z",
    "checks":[{"section":"s","label":"l","verdict":"pass","observed":"o"}]}"#;

#[test]
fn a_known_shape_parses() {
    assert!(serde_json::from_str::<ReportUpload>(REPORT).is_ok());
}

#[test]
fn an_unknown_check_key_is_refused() {
    let typo = REPORT.replace(r#""observed""#, r#""observe""#);
    assert!(serde_json::from_str::<ReportUpload>(&typo).is_err());
}

#[test]
fn an_unknown_report_key_is_refused() {
    let extra = REPORT.replace(r#""schema":1,"#, r#""schema":1,"host":"x","#);
    assert!(serde_json::from_str::<ReportUpload>(&extra).is_err());
}
