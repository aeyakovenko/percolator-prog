# Blind Worker Protocol

This file is the only briefing to give invariant-coverage subagents.

## Goal

Extend public-route invariant coverage from the normative charter in `INVARIANTS.md`.
Work from source code, existing generic tests, and public API behavior. Add only
net-useful invariant tests, oracles, and minimal supporting fixtures.

## Withheld Data Boundary

Do not inspect GitHub issues, GitHub PRs, open PR branches, finding-specific audit
notes, `tests/invariants/traceability_gaps.tsv`, or README sections that name
holdout rows. Do not search for issue numbers, PR numbers, titles, or branch names.

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
