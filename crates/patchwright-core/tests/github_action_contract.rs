use patchwright_core::{
    GitHubAction, GitHubActionPreview, MergeMethod, RemoteIdentity, RemotePrecondition, ReviewEvent,
};
use serde_json::json;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn preview(action: GitHubAction) -> GitHubActionPreview {
    GitHubActionPreview::new(
        RemoteIdentity::new(123, 7, "octo/fixture").unwrap(),
        action,
        RemotePrecondition::new(Some(SHA_A), Some(SHA_B), 9).unwrap(),
    )
    .unwrap()
}

#[test]
fn every_delivery_and_merge_action_has_stable_exact_identity() {
    let actions = vec![
        GitHubAction::create_branch("feat/fix", SHA_A).unwrap(),
        GitHubAction::push_intent("feat/fix", SHA_B).unwrap(),
        GitHubAction::comment(12, "Applied the verified fix.").unwrap(),
        GitHubAction::review(12, ReviewEvent::Approve, "Verified.", vec![]).unwrap(),
        GitHubAction::resolve_review_thread(12, "PRRT_kwDOExample", SHA_B).unwrap(),
        GitHubAction::check_run("Patchwright", SHA_B, "completed", Some("success")).unwrap(),
        GitHubAction::draft_pull_request("Fix", "feat/fix", "main", "Body").unwrap(),
        GitHubAction::update_pull_request_branch(12, SHA_A).unwrap(),
        GitHubAction::ready_pull_request(12, SHA_B).unwrap(),
        GitHubAction::close_pull_request(12).unwrap(),
        GitHubAction::close_issue(13).unwrap(),
        GitHubAction::enqueue_pull_request(12, SHA_B).unwrap(),
        GitHubAction::merge_pull_request(12, SHA_B, MergeMethod::Squash).unwrap(),
    ];
    let mut digests = std::collections::HashSet::new();
    for action in actions {
        let first = preview(action.clone());
        let second = preview(action);
        assert_eq!(first.idempotency_sha256(), second.idempotency_sha256());
        assert!(digests.insert(first.idempotency_sha256().to_owned()));
        assert_eq!(first.remote().repository_id(), 123);
        assert_eq!(first.remote().installation_id(), 7);
        assert_eq!(first.precondition().snapshot_generation(), 9);
        assert!(!first.required_permissions().is_empty());
    }
}

#[test]
fn action_contract_rejects_ambiguous_or_unsafe_boundaries() {
    assert!(GitHubAction::create_branch("-unsafe", SHA_A).is_err());
    assert!(GitHubAction::create_branch("feat/fix", "short").is_err());
    assert!(GitHubAction::comment(0, "body").is_err());
    assert!(GitHubAction::comment(1, &"x".repeat(65_537)).is_err());
    assert!(GitHubAction::comment(1, "Authorization: Bearer ghs_secret").is_err());
    assert!(GitHubAction::close_issue(0).is_err());
    assert!(GitHubAction::resolve_review_thread(1, "", SHA_B).is_err());
    assert!(GitHubAction::resolve_review_thread(1, "PRRT_bad space", SHA_B).is_err());
    assert!(GitHubAction::merge_pull_request(1, SHA_B, MergeMethod::Merge).is_ok());
    assert!(RemoteIdentity::new(0, 7, "octo/fixture").is_err());
    assert!(RemoteIdentity::new(1, 0, "octo/fixture").is_err());
    assert!(RemoteIdentity::new(1, 7, "not-a-repository").is_err());
    assert!(RemotePrecondition::new(Some("short"), Some(SHA_B), 1).is_err());
}

#[test]
fn action_json_uses_swift_facing_camel_case_and_accepts_legacy_snake_case() {
    let action = GitHubAction::create_branch("feat/fix", SHA_A).unwrap();
    assert_eq!(
        serde_json::to_value(&action).unwrap(),
        json!({"kind":"createBranch","branch":"feat/fix","fromSha":SHA_A})
    );

    let legacy: GitHubAction = serde_json::from_value(json!({
        "kind": "mergePullRequest",
        "pull_request_number": 12,
        "expected_head_sha": SHA_B,
        "method": "squash"
    }))
    .unwrap();
    assert_eq!(
        legacy,
        GitHubAction::merge_pull_request(12, SHA_B, MergeMethod::Squash).unwrap()
    );
}
