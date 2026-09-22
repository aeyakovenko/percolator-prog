# Blind Worker Protocol

This file is the only briefing to give invariant-coverage subagents.

## Goal

Extend public-route invariant coverage from the normative charter in `INVARIANTS.md`.
Work from source code, existing generic tests, and public API behavior. Add only
net-useful invariant tests, oracles, and minimal supporting fixtures.
Use `scripts/invariant_coverage_checklist.md` for the evidence gates. Do not mark
an invariant complete from a leaf test alone; record the public route, independent
oracle, boundary classes, rollback behavior, exact selector, and remaining scope.

## Withheld Data Boundary

Do not inspect GitHub issues, GitHub PRs, open PR branches, finding-specific audit
notes, `tests/invariants/traceability_gaps.tsv`,
`tests/invariants/invariant_status.tsv`,
`tests/invariants/coverage_reopenings.tsv`, holdout/traceability ledgers, or README
sections that name holdout rows. Do not search for issue numbers, PR numbers,
titles, or branch names. If a task requires a prohibited artifact, stop and ask
the coordinator for a generic invariant-only input instead.

The coordinator owns holdout evaluation after the invariant suite is frozen. Worker
results must stand on their own as generic invariant evidence.

## Output Contract

Report only:

- invariant IDs and public route families covered;
- files changed;
- exact commands and results;
- remaining generic gaps and assumptions.

If a test exposes a bug, describe it as an invariant violation discovered from
source/test exploration. Do not map it to a GitHub issue or PR.
