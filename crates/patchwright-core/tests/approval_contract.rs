use chrono::{Duration, TimeZone, Utc};
use patchwright_core::{
    ActionFingerprint, ActionFingerprintDraft, Approval, ApprovalClass, Capability, Policy,
    PolicyDecision, TaskId,
};

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0)
        .single()
        .unwrap()
}

fn fingerprint_draft() -> ActionFingerprintDraft {
    ActionFingerprintDraft {
        task_id: TaskId::new(),
        github_repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        action_kind: "pushBranch".into(),
        pull_request_number: Some(18),
        branch: Some("patchwright/task-18".into()),
        head_sha: Some("a".repeat(40)),
        base_sha: Some("b".repeat(40)),
        payload_sha256: "c".repeat(64),
        policy_sha256: "d".repeat(64),
        instruction_sha256: "e".repeat(64),
        invalidation_generation: 7,
    }
}

fn fingerprint() -> ActionFingerprint {
    ActionFingerprint::try_from(fingerprint_draft()).unwrap()
}

fn fingerprint_for(capability: Capability) -> ActionFingerprint {
    ActionFingerprint::try_from(ActionFingerprintDraft {
        action_kind: capability.action_kind().into(),
        ..fingerprint_draft()
    })
    .unwrap()
}

#[test]
fn approvals_retain_separate_authority_classes() {
    let current = now();
    for (class, capability) in [
        (ApprovalClass::Preparation, Capability::PrepareWorktree),
        (ApprovalClass::CodexRuntime, Capability::RunKnownCommand),
        (ApprovalClass::LocalCapability, Capability::AccessNetwork),
        (ApprovalClass::GitHubDelivery, Capability::PushBranch),
        (ApprovalClass::Merge, Capability::MergePullRequest),
    ] {
        let approval = Approval::new(
            class,
            capability,
            fingerprint_for(capability),
            "owner",
            current,
            current + Duration::minutes(10),
        )
        .unwrap();
        assert_eq!(approval.class(), class);
        assert_eq!(approval.capability(), capability);
    }
}

#[test]
fn preparation_authority_is_distinct_from_runtime_and_delivery() {
    let fingerprint = fingerprint_for(Capability::PrepareWorktree);
    let now = Utc::now();
    let runtime = Approval::new(
        ApprovalClass::CodexRuntime,
        Capability::PrepareWorktree,
        fingerprint.clone(),
        "operator",
        now,
        now + Duration::minutes(5),
    );
    let delivery = Approval::new(
        ApprovalClass::GitHubDelivery,
        Capability::PrepareWorktree,
        fingerprint.clone(),
        "operator",
        now,
        now + Duration::minutes(5),
    );

    assert!(runtime.is_err());
    assert!(delivery.is_err());
    assert!(matches!(
        Policy::default().authorize(Capability::PrepareWorktree, &fingerprint, None, now),
        PolicyDecision::ApprovalRequired(_)
    ));
}

#[test]
fn delivery_approval_matches_every_action_fingerprint_field() {
    let current = now();
    let expected = fingerprint();
    let approval = Approval::new(
        ApprovalClass::GitHubDelivery,
        Capability::PushBranch,
        expected.clone(),
        "owner",
        current,
        current + Duration::minutes(10),
    )
    .unwrap();
    let policy = Policy::with_automation_disabled(false);
    assert_eq!(
        policy.authorize(Capability::PushBranch, &expected, Some(&approval), current),
        PolicyDecision::Allowed
    );

    let mut mismatches = Vec::new();
    let mut draft = fingerprint_draft();
    draft.task_id = TaskId::new();
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.github_repository_id = 43;
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.repository_full_name = "octocat/other".into();
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.action_kind = "createPullRequest".into();
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.pull_request_number = Some(19);
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.branch = Some("patchwright/other".into());
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.head_sha = Some("f".repeat(40));
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.base_sha = Some("0".repeat(40));
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.payload_sha256 = "1".repeat(64);
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.policy_sha256 = "2".repeat(64);
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.instruction_sha256 = "3".repeat(64);
    mismatches.push(draft);
    let mut draft = fingerprint_draft();
    draft.invalidation_generation += 1;
    mismatches.push(draft);

    for mismatch in mismatches {
        assert_ne!(
            policy.authorize(
                Capability::PushBranch,
                &ActionFingerprint::try_from(mismatch).unwrap(),
                Some(&approval),
                current
            ),
            PolicyDecision::Allowed
        );
    }
}

#[test]
fn approval_rejects_wrong_class_capability_and_time_window() {
    let current = now();
    let expected = fingerprint();
    let wrong_class = Approval::new(
        ApprovalClass::LocalCapability,
        Capability::AccessNetwork,
        fingerprint_for(Capability::AccessNetwork),
        "owner",
        current,
        current + Duration::minutes(10),
    )
    .unwrap();
    let expired = Approval::new(
        ApprovalClass::GitHubDelivery,
        Capability::PushBranch,
        expected.clone(),
        "owner",
        current,
        current + Duration::minutes(1),
    )
    .unwrap();
    let policy = Policy::with_automation_disabled(false);
    for (approval, at) in [
        (&wrong_class, current),
        (&expired, current + Duration::minutes(2)),
        (&expired, current - Duration::seconds(1)),
    ] {
        assert!(matches!(
            policy.authorize(Capability::PushBranch, &expected, Some(approval), at),
            PolicyDecision::ApprovalRequired(_)
        ));
    }
    assert!(
        Approval::new(
            ApprovalClass::GitHubDelivery,
            Capability::PushBranch,
            expected.clone(),
            " ",
            current,
            current + Duration::minutes(10),
        )
        .is_err()
    );
    assert!(
        Approval::new(
            ApprovalClass::GitHubDelivery,
            Capability::PushBranch,
            expected,
            "owner",
            current,
            current + Duration::minutes(31),
        )
        .is_err()
    );
}

#[test]
fn approval_capability_must_match_the_typed_action_kind() {
    let current = now();
    let mut draft = fingerprint_draft();
    draft.action_kind = "createPullRequest".into();
    assert!(
        Approval::new(
            ApprovalClass::GitHubDelivery,
            Capability::PushBranch,
            ActionFingerprint::try_from(draft).unwrap(),
            "owner",
            current,
            current + Duration::minutes(5),
        )
        .is_err()
    );
}

#[test]
fn merge_requires_a_separate_exact_merge_approval() {
    let current = now();
    let mut draft = fingerprint_draft();
    draft.action_kind = "mergePullRequest".into();
    let expected = ActionFingerprint::try_from(draft).unwrap();
    let policy = Policy::with_automation_disabled(false);
    assert!(matches!(
        policy.authorize(Capability::MergePullRequest, &expected, None, current),
        PolicyDecision::ApprovalRequired(_)
    ));
    let approval = Approval::new(
        ApprovalClass::Merge,
        Capability::MergePullRequest,
        expected.clone(),
        "owner",
        current,
        current + Duration::minutes(5),
    )
    .unwrap();
    assert_eq!(
        policy.authorize(
            Capability::MergePullRequest,
            &expected,
            Some(&approval),
            current
        ),
        PolicyDecision::Allowed
    );
}

#[test]
fn administrator_bypass_is_never_approvable() {
    let current = now();
    let expected = fingerprint_for(Capability::MergePullRequest);
    let approval = Approval::new(
        ApprovalClass::Merge,
        Capability::MergePullRequest,
        expected.clone(),
        "owner",
        current,
        current + Duration::minutes(5),
    )
    .unwrap();
    assert!(matches!(
        Policy::with_automation_disabled(false).authorize(
            Capability::AdministratorBypass,
            &expected,
            Some(&approval),
            current
        ),
        PolicyDecision::Denied(_)
    ));
}

#[test]
fn automation_kill_switch_denies_mutation_but_retains_read_access() {
    let current = now();
    let expected = fingerprint();
    let approval = Approval::new(
        ApprovalClass::GitHubDelivery,
        Capability::PushBranch,
        expected.clone(),
        "owner",
        current,
        current + Duration::minutes(5),
    )
    .unwrap();
    let policy = Policy::with_automation_disabled(true);
    assert!(matches!(
        policy.authorize(Capability::PushBranch, &expected, Some(&approval), current),
        PolicyDecision::Denied(_)
    ));
    assert_eq!(
        policy.authorize(
            Capability::ReadRepository,
            &ActionFingerprint::try_from(ActionFingerprintDraft {
                action_kind: "readRepository".into(),
                ..fingerprint_draft()
            })
            .unwrap(),
            None,
            current
        ),
        PolicyDecision::Allowed
    );
}

#[test]
fn fingerprint_rejects_unvalidated_remote_identity_and_digests() {
    let mut draft = fingerprint_draft();
    draft.github_repository_id = 0;
    assert!(ActionFingerprint::try_from(draft).is_err());
    let mut draft = fingerprint_draft();
    draft.repository_full_name.clear();
    assert!(ActionFingerprint::try_from(draft).is_err());
    let mut draft = fingerprint_draft();
    draft.payload_sha256 = "not-a-sha".into();
    assert!(ActionFingerprint::try_from(draft).is_err());
}
