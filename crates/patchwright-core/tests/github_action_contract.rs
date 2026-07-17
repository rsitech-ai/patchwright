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
        GitHubAction::review(12, SHA_A, ReviewEvent::Approve, "Verified.", vec![]).unwrap(),
        GitHubAction::resolve_review_thread(12, "PRRT_kwDOExample", SHA_B).unwrap(),
        GitHubAction::check_run("Patchwright", SHA_B, "completed", Some("success")).unwrap(),
        GitHubAction::draft_pull_request("Fix", "feat/fix", "main", &"a".repeat(40), "Body")
            .unwrap(),
        GitHubAction::update_pull_request_branch(12, SHA_A).unwrap(),
        GitHubAction::ready_pull_request(12, SHA_B).unwrap(),
        GitHubAction::close_pull_request(12, SHA_A).unwrap(),
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

#[test]
fn action_json_rejects_values_that_bypass_constructor_validation() {
    let invalid_actions = [
        json!({"kind":"comment","issueNumber":0,"body":"body"}),
        json!({"kind":"createBranch","branch":"-unsafe","fromSha":SHA_A}),
        json!({"kind":"pushIntent","branch":"feat/fix","headSha":"short"}),
        json!({"kind":"comment","issueNumber":1,"body":"ghp_leakedcredential"}),
        json!({"kind":"comment","issueNumber":1,"body":"x".repeat(65_537)}),
        json!({
            "kind":"review",
            "pullRequestNumber":1,
            "expectedHeadSha":SHA_A,
            "event":"comment",
            "body":"review",
            "inlineComments":[{"path":"../secret","line":0,"body":"inline"}]
        }),
        json!({
            "kind":"review",
            "pullRequestNumber":1,
            "expectedHeadSha":SHA_A,
            "event":"comment",
            "body":"review",
            "inlineComments":[
                {"path":"src/lib.rs","line":9,"body":"first"},
                {"path":"src/lib.rs","line":9,"body":"second"}
            ]
        }),
    ];

    for value in invalid_actions {
        assert!(
            serde_json::from_value::<GitHubAction>(value.clone()).is_err(),
            "invalid action was accepted: {value}"
        );
    }
}

#[test]
fn nested_boundary_json_rejects_invalid_identity_preconditions_and_inline_comments() {
    assert!(
        serde_json::from_value::<RemoteIdentity>(json!({
            "repositoryId":0,
            "installationId":7,
            "repositoryFullName":"octo/fixture"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RemoteIdentity>(json!({
            "repositoryId":1,
            "installationId":7,
            "repositoryFullName":"octo/fixture/extra"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RemotePrecondition>(json!({
            "expectedHeadSha":"short",
            "expectedBaseSha":SHA_B,
            "snapshotGeneration":1
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RemotePrecondition>(json!({
            "expectedHeadSha":SHA_A,
            "expectedBaseSha":SHA_B,
            "snapshotGeneration":0
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<patchwright_core::InlineReviewComment>(json!({
            "path":"/absolute/path.rs",
            "line":1,
            "body":"inline"
        }))
        .is_err()
    );
}

#[test]
fn preview_json_recomputes_and_verifies_derived_security_fields() {
    let valid = serde_json::to_value(preview(
        GitHubAction::comment(12, "Applied the verified fix.").unwrap(),
    ))
    .unwrap();

    for (field, forged) in [
        ("payloadSha256", json!("0".repeat(64))),
        ("idempotencySha256", json!("1".repeat(64))),
        ("requiredPermissions", json!(["administration:write"])),
    ] {
        let mut value = valid.clone();
        value[field] = forged;
        assert!(
            serde_json::from_value::<GitHubActionPreview>(value).is_err(),
            "forged {field} was accepted"
        );
    }
}

#[test]
fn nested_boundaries_accept_camel_case_and_legacy_snake_case() {
    let camel_identity: RemoteIdentity = serde_json::from_value(json!({
        "repositoryId":123,
        "installationId":7,
        "repositoryFullName":"octo/fixture"
    }))
    .unwrap();
    let legacy_identity: RemoteIdentity = serde_json::from_value(json!({
        "repository_id":123,
        "installation_id":7,
        "repository_full_name":"octo/fixture"
    }))
    .unwrap();
    assert_eq!(camel_identity, legacy_identity);

    let camel_precondition: RemotePrecondition = serde_json::from_value(json!({
        "expectedHeadSha":SHA_A,
        "expectedBaseSha":SHA_B,
        "snapshotGeneration":9
    }))
    .unwrap();
    let legacy_precondition: RemotePrecondition = serde_json::from_value(json!({
        "expected_head_sha":SHA_A,
        "expected_base_sha":SHA_B,
        "snapshot_generation":9
    }))
    .unwrap();
    assert_eq!(camel_precondition, legacy_precondition);

    let valid = preview(GitHubAction::comment(12, "Body").unwrap());
    let mut legacy_preview = serde_json::to_value(&valid).unwrap();
    for (camel, snake) in [
        ("payloadSha256", "payload_sha256"),
        ("idempotencySha256", "idempotency_sha256"),
        ("requiredPermissions", "required_permissions"),
    ] {
        let value = legacy_preview
            .as_object_mut()
            .unwrap()
            .remove(camel)
            .unwrap();
        legacy_preview
            .as_object_mut()
            .unwrap()
            .insert(snake.into(), value);
    }
    assert_eq!(
        serde_json::from_value::<GitHubActionPreview>(legacy_preview).unwrap(),
        valid
    );
}
