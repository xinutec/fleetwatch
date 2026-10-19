//! Where a login lands afterwards: same-site paths only, judged the way the
//! browser resolves them.

use fleetwatch::routes::auth::validate_return_to;

/// A browser's URL parser drops tabs and newlines and reads `\` as `/`, so
/// `/\t/host` lands on `//host`, another site. Each attempt must land here.
#[test]
fn every_accepted_path_resolves_to_this_site() {
    let site = url::Url::parse("https://fleetwatch.example/").unwrap();
    for attempt in [
        "/\t/evil.example",
        "/\n/evil.example",
        "/\r/evil.example",
        "/\t\\evil.example",
        "/ /evil.example",
        "/problems",
    ] {
        let dest = validate_return_to(Some(attempt));
        let landed = site.join(&dest).unwrap();
        assert_eq!(
            landed.host_str(),
            Some("fleetwatch.example"),
            "{attempt:?} was let through as {dest:?}"
        );
    }
}
