# Review Thread and Remote Reconciliation Reflection

## Task

- **ID/Title:** Patchwright final review-thread and historical-task closure
- **Date:** 2026-07-14
- **Scope:** multi-file GitHub integration and durable lifecycle

## Plan and Risks

- **Planned approach:** Add GraphQL review-thread ingestion and a separately approved typed resolution action, then add a read-only remote reconciliation RPC that validates immutable task identity before advancing local state.
- **Top failure hypotheses:** A thread node ID could belong to another repository or PR; a stale task head could resolve a thread after new commits; local reconciliation could mark a closed-but-unmerged PR or unrelated issue complete.
- **Success criteria:** Resolution is rejected for mismatched repository, PR, head, resolved state, or viewer authority; exact resolution succeeds once and survives re-ingestion; current pre-fix tasks become Completed only after remote identity/state verification; full and live sandbox gates pass.

## Candidate Attempts

| Candidate | Summary | Outcome | Signals | Why selected / rejected |
|---|---|---|---|---|
| A | GraphQL thread ingestion plus typed resolution and read-only reconciliation | Selected | GitHub exposes thread IDs and resolution state only through the GraphQL thread connection | Preserves exact identity and the existing approval architecture |
| B | Infer threads from REST review comment IDs | Rejected | REST comments have database IDs but not their owning review-thread node IDs | Could resolve the wrong object or require unsafe guessing |
| C | Directly patch the two historical SQLite task states | Rejected | Would make the current UI look correct without proving remote identity | Bypasses the production recovery path and leaves the real defect unfixed |

## Reflection

- **Failure modes observed:** The live PR and issue were remotely complete while their pre-fix local tasks remained at Awaiting Delivery Approval; review comments were ingested but thread node IDs were absent; live GitHub installation tokens reported `viewerCanResolve: false` even for an App-authored thread.
- **Root cause:** The first live delivery path persisted remote results separately from task state, the read model used REST comments/reviews without GraphQL thread identity, and GitHub reserves thread resolution for user-context authority rather than installation identity.
- **Fix that resolved it:** Paginated GraphQL ingestion, exact typed resolution, App-first then signed-in-user credential brokering for only the rejected resolution action, and read-only exact-identity task reconciliation.
- **What improved score/quality:** Server-side ownership/head validation, explicit viewer authority, non-persisted user-token fallback, no direct database repair, and using the same recovery interface for historical and future ambiguous outcomes.
- **Useful command-level evidence:** `gh pr view`, `gh issue view`, GraphQL thread query, focused action/relay/RPC tests, SQLite task timeline readback, `./script/verify.sh`, and `./script/smoke_codex.sh`.
- **Branch comparison insight:** Commit `7f92449` completed the normal merge transaction; this slice closes recovery and review-thread gaps without broadening repository authorization.

## Reusable Lesson

- **Pattern that worked:** Treat opaque provider node identity as first-class data and revalidate ownership immediately before mutation.
- **Pattern to avoid:** Conflating a review comment database ID with a review-thread node ID or repairing durable state outside the orchestrator.
- **Where to apply next:** Any future GraphQL-only GitHub actions and ambiguous external-write reconciliation.

## Decision

- **Final chosen approach:** GraphQL-native review threads, exact approval with a narrow user-context fallback required by GitHub, installation-token readback, and local atomic reconciliation.
- **Commit/rollback decision:** Keep the work in one independently revertible closure commit; retain the existing sandbox-only live mutation guard and do not merge PR #1.
- **Next step / follow-up:** Preserve the live sandbox evidence and replace the local `gh` user broker with an in-app GitHub App user-authorization flow before clean-machine distribution if review-thread resolution must work without `gh`.
