//! Integration-test target (also anchors `rustc-link-arg-tests`: test binaries
//! need the Common-Controls v6 manifest or they fail to load — see build.rs).

#[test]
fn error_types_are_displayable() {
    use cracktivity_lib::store::StoreError;
    let e = StoreError::DekOrphaned;
    assert!(!e.to_string().is_empty());
}
