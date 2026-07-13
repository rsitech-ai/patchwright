use patchwright_core::{
    Approval, Capability, InstructionKind, InstructionResolver, InstructionSource, Policy,
    PolicyDecision, Task, TaskState,
};

#[test]
fn task_state_machine_rejects_skipped_delivery() {
    let mut task = Task::new("Review issue 184", "/tmp/repository").unwrap();
    assert_eq!(task.state(), TaskState::Discovered);

    let error = task.transition(TaskState::Delivering).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid transition: discovered -> delivering"
    );

    task.transition(TaskState::Assessing).unwrap();
    task.transition(TaskState::Planned).unwrap();
    task.transition(TaskState::AwaitingPreparationApproval)
        .unwrap();
    assert_eq!(task.state(), TaskState::AwaitingPreparationApproval);
}

#[test]
fn task_state_machine_supports_the_approved_lifecycle() {
    let mut task = Task::new("Review issue 184", "/tmp/repository").unwrap();
    for state in [
        TaskState::Assessing,
        TaskState::Planned,
        TaskState::AwaitingPreparationApproval,
        TaskState::Preparing,
        TaskState::Implementing,
        TaskState::Verifying,
        TaskState::Reviewing,
        TaskState::AwaitingDeliveryApproval,
        TaskState::Delivering,
        TaskState::Monitoring,
        TaskState::AwaitingMergeApproval,
        TaskState::Merging,
        TaskState::Completed,
    ] {
        task.transition(state).unwrap();
    }
    assert_eq!(task.state(), TaskState::Completed);
    assert!(task.transition(TaskState::Assessing).is_err());
}

#[test]
fn task_requires_a_reason_to_interrupt_and_resumes_only_recoverable_states() {
    for interruption_state in [
        TaskState::Paused,
        TaskState::Blocked,
        TaskState::Failed,
        TaskState::Cancelled,
    ] {
        let mut task = Task::new("Review issue 184", "/tmp/repository").unwrap();
        task.transition(TaskState::Assessing).unwrap();
        assert!(task.interrupt(interruption_state, "  ").is_err());

        task.interrupt(interruption_state, "operator requested")
            .unwrap();
        assert_eq!(task.state(), interruption_state);
        let interruption = task.interruption().unwrap();
        assert_eq!(interruption.state, interruption_state);
        assert_eq!(interruption.resume_state, TaskState::Assessing);
        assert_eq!(interruption.reason, "operator requested");

        if matches!(interruption_state, TaskState::Paused | TaskState::Blocked) {
            task.resume().unwrap();
            assert_eq!(task.state(), TaskState::Assessing);
            assert!(task.interruption().is_none());
        } else {
            assert!(task.resume().is_err());
            assert!(task.transition(TaskState::Assessing).is_err());
        }
    }
}

#[test]
fn task_rejects_skipping_each_approval_gate() {
    let mut task = Task::new("Review issue 184", "/tmp/repository").unwrap();
    task.transition(TaskState::Assessing).unwrap();
    task.transition(TaskState::Planned).unwrap();
    assert!(task.transition(TaskState::Preparing).is_err());

    task.transition(TaskState::AwaitingPreparationApproval)
        .unwrap();
    task.transition(TaskState::Preparing).unwrap();
    task.transition(TaskState::Implementing).unwrap();
    task.transition(TaskState::Verifying).unwrap();
    task.transition(TaskState::Reviewing).unwrap();
    assert!(task.transition(TaskState::Delivering).is_err());

    task.transition(TaskState::AwaitingDeliveryApproval)
        .unwrap();
    task.transition(TaskState::Delivering).unwrap();
    task.transition(TaskState::Monitoring).unwrap();
    assert!(task.transition(TaskState::Merging).is_err());
}

#[test]
fn legacy_preparation_approval_state_decodes_to_the_durable_state() {
    let state: TaskState = serde_json::from_str("\"awaitingApproval\"").unwrap();
    assert_eq!(state, TaskState::AwaitingPreparationApproval);
    assert_eq!(
        serde_json::to_string(&state).unwrap(),
        "\"awaitingPreparationApproval\""
    );
}

#[test]
fn every_active_lifecycle_state_can_pause_and_resume_exactly() {
    let lifecycle = [
        TaskState::Discovered,
        TaskState::Assessing,
        TaskState::Planned,
        TaskState::AwaitingPreparationApproval,
        TaskState::Preparing,
        TaskState::Implementing,
        TaskState::Verifying,
        TaskState::Reviewing,
        TaskState::AwaitingDeliveryApproval,
        TaskState::Delivering,
        TaskState::Monitoring,
        TaskState::AwaitingMergeApproval,
        TaskState::Merging,
    ];

    for (index, expected_resume_state) in lifecycle.iter().copied().enumerate() {
        let mut task = Task::new("Review issue 184", "/tmp/repository").unwrap();
        for state in lifecycle.iter().copied().skip(1).take(index) {
            task.transition(state).unwrap();
        }
        task.interrupt(TaskState::Paused, "operator paused")
            .unwrap();
        assert_eq!(
            task.interruption().unwrap().resume_state,
            expected_resume_state
        );
        task.resume().unwrap();
        assert_eq!(task.state(), expected_resume_state);
    }
}

#[test]
fn merge_is_disabled_even_with_an_approval() {
    let policy = Policy::default();
    let approval = Approval::for_capability(Capability::MergePullRequest, "owner");

    assert_eq!(
        policy.authorize(Capability::MergePullRequest, Some(&approval)),
        PolicyDecision::Denied("merge is disabled".into())
    );
}

#[test]
fn remote_mutations_require_a_matching_approval() {
    let policy = Policy::default();
    assert!(matches!(
        policy.authorize(Capability::PushBranch, None),
        PolicyDecision::ApprovalRequired(_)
    ));

    let wrong = Approval::for_capability(Capability::CreatePullRequest, "owner");
    assert!(matches!(
        policy.authorize(Capability::PushBranch, Some(&wrong)),
        PolicyDecision::ApprovalRequired(_)
    ));

    let matching = Approval::for_capability(Capability::PushBranch, "owner");
    assert_eq!(
        policy.authorize(Capability::PushBranch, Some(&matching)),
        PolicyDecision::Allowed
    );
}

#[test]
fn instruction_resolver_orders_sources_and_reports_conflicts() {
    let sources = vec![
        InstructionSource::new(InstructionKind::Task, "task", "network: allow"),
        InstructionSource::new(InstructionKind::Organization, "org", "network: deny"),
        InstructionSource::new(InstructionKind::RootAgents, "AGENTS.md", "run tests"),
    ];

    let effective = InstructionResolver::resolve(sources);
    assert_eq!(
        effective
            .sources
            .iter()
            .map(|source| source.kind)
            .collect::<Vec<_>>(),
        vec![
            InstructionKind::Organization,
            InstructionKind::RootAgents,
            InstructionKind::Task,
        ]
    );
    assert_eq!(effective.conflicts.len(), 1);
    assert_eq!(effective.conflicts[0].key, "network");
    assert_eq!(effective.conflicts[0].effective_value, "allow");
}
