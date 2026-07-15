const RPC_SOURCE: &str = include_str!("../src/rpc.rs");

fn function_source(signature: &str) -> &str {
    let start = RPC_SOURCE
        .find(signature)
        .unwrap_or_else(|| panic!("missing function signature: {signature}"));
    let body_start = RPC_SOURCE[start..]
        .find('{')
        .map(|offset| start + offset)
        .expect("function body must start");
    let mut depth = 0_u32;
    for (offset, character) in RPC_SOURCE[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &RPC_SOURCE[start..=body_start + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function body: {signature}");
}

#[test]
fn github_mutations_use_only_delivery_scoped_app_installation_tokens() {
    let mutation = function_source("async fn execute_github_action(");

    assert!(mutation.contains("InstallationPermissions::delivery()"));
    assert!(mutation.contains("token.expose_for_authorization_header()"));
    assert!(
        !mutation.contains("GhCliCredentialBroker"),
        "mutation scope must never consult GitHub CLI credentials"
    );
    assert!(
        !mutation.contains("github_cli_path()"),
        "mutation scope must never discover a user gh executable"
    );
    assert!(
        !mutation.contains("user_token"),
        "mutation scope must never fall back to a user token"
    );
}

#[test]
fn github_cli_credentials_are_confined_to_read_only_sync_sources() {
    let cancellable_sync = function_source("fn github_source_from_environment(");
    let direct_sync = function_source("async fn sync_github(");
    let allowed_broker_uses = cancellable_sync
        .matches("GhCliCredentialBroker::new")
        .count()
        + direct_sync.matches("GhCliCredentialBroker::new").count();
    let all_broker_uses = RPC_SOURCE.matches("GhCliCredentialBroker::new").count();

    assert_eq!(allowed_broker_uses, 2, "both read-only sync paths need gh");
    assert_eq!(
        all_broker_uses, allowed_broker_uses,
        "GhCliCredentialBroker escaped the read-only sync boundary"
    );
}
