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

    task.transition(TaskState::Planned).unwrap();
    task.transition(TaskState::AwaitingApproval).unwrap();
    assert_eq!(task.state(), TaskState::AwaitingApproval);
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
