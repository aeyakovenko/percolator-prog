# LoF / DoS / CU Bug-Hunting Requirements

## Core Classification Rule

A finding is a real LoF / DoS / CU issue only if it is reachable through the public wrapper interface and demonstrates one of these outcomes:

- Loss of funds, value leakage, theft, fee undercharge that lets an attacker move value or marks for free, or broken conservation.
- A normal user with an existing position cannot make progress through any bounded public path.
- A keeper/cranker cannot eventually drive an actionable account or market toward health, settlement, liquidation, close, or resolution.
- A successful instruction has unbounded or max-shape CU that can prevent required progress for ordinary accounts.
- A malformed public input reaches a side effect before the wrapper/engine rejects it, where the side effect itself creates value movement, state corruption, replay, auth bypass, or persistent liveness damage.

CU exhaustion by itself is not enough. On SVM, an error aborts and rolls back account changes. A CU probe is substantive only when it shows a user-facing progress failure, not merely that a badly chosen transaction burns compute and reverts.

## Security Labels

Every finding gets exactly one severity and one impact, written `[<SEVERITY> <IMPACT>]`.

- SEVERITY is one of: `BLOCKER`, `PRIVILEGED`, `HARDENING`.
- IMPACT is one of: `LoF` (loss of funds) or `DoS` (denial of service / fund-stranding).

Six labels total: `[BLOCKER LoF]`, `[BLOCKER DoS]`, `[PRIVILEGED LoF]`, `[PRIVILEGED DoS]`, `[HARDENING LoF]`, `[HARDENING DoS]`. Do **not** use `REAL`, `PARTIAL`, or `DEFENDED`.

### Severity

- **BLOCKER** — an independent, non-signing party loses value or is denied progress, and it is reachable **even when the operator controls all admin keys and holds them safely**. Either an unprivileged public caller triggers it, or an unprivileged relayer replays an *honest* party's own signed transaction (the signer is honest; the attacker just re-submits public/pre-signed bytes — bounded by Solana's ~150-slot recent-blockhash TTL and legitimate address/slot reuse, but not preventable by key custody). Must fix before launch.
- **PRIVILEGED** — the loss or DoS **only fires when a privileged admin key is malicious or compromised**, acting beyond its reserved trusts (see Bounded Admin-Authority Requirement): e.g. raising a mutable fee past a counterparty's signed consent, seizing an authority role without the holder's consent, or draining backing/insurance to a non-owner. Does not occur under honest, uncompromised keys; defense-in-depth against key compromise, not a launch blocker under an honest-key threat model.
- **HARDENING** — no independent-victim value is at risk. The apparent exploit is self-inflicted (victim == signer, or a self-reclaim withdrawal that can only pay the signer's own account), reachable only via a host/test artifact (e.g. recreating a program-owned account at a reaped keyless address through `set_account`), dependent on a dishonest oracle mark, or non-extractable rounding drift. Robustness / liveness-UX only; never merged as a live loss.

### Impact

- **LoF** requires a normally initialized live market and net extractable loss borne by an independent user or canonical backing/provider — a party's withdrawable balance falls and someone else's rises, or funds leave canonical custody. Withdrawal of the caller's own isolated, domain-segregated reserve, the caller reclaiming its own fees, fair-price limit mismatches, harm that requires a dishonest oracle mark, and non-extractable rounding drift are `HARDENING`, not LoF.
- **DoS** requires a publicly reachable state in which every bounded owner/keeper continuation fails despite one honest cranker supplying correct inputs (terminal Resolved, LockActive forever, unbounded-CU / unbounded-transaction wind-down). Fund-stranding is DoS even when no attacker profits. Batch-only rejection, blocked new admission, empty retired-slot reuse, a working public forfeit/escape path, next-slot recovery, and attacks that require the adversary to win ordering every slot are not persistent DoS.

### Evidence gate

Before applying a `BLOCKER` or `PRIVILEGED` label, run the exploit against the **exact pre-fix parent** and the **fixed head**. State injection, prose-only reachability, or a green-only regression is insufficient. If the red test relies on self-re-auth, self-reclaim, a host-artifact reinit, a dishonest oracle, or state injection, the finding is `HARDENING`. If it needs a compromised admin key, it is `PRIVILEGED`. Otherwise, with an independent victim and net-extractable loss or a no-escape DoS, it is `BLOCKER`.

## Bounded Admin-Authority Requirement

`marketauth` / admin keys are trusted only within their intended point of control. Model a compromised admin key as an adversary. After a market is initialized and running, a compromised admin key — absent an oracle compromise — must not be able to cause LoF or DoS to users. Having the admin key sign an admin instruction is a legitimate public path, not state injection.

Reserved admin trusts (out of scope):

- **Pre-init configuration.** Users choose to enter a market after observing its config, so init-time parameters are trusted.
- **Oracle authority.** Users accept the oracle as a separate, explicit trust. Honest oracle marks (within the circuit breaker) are ground truth. A finding whose harm *requires* a dishonest oracle mark is an oracle-trust issue tested separately, not counted here.
- **Shutdown-with-exit.** Admin may shut down an asset or market, but every shutdown path must leave each affected user a bounded public path to close its position and withdraw its capital. Shutdown is the only admin power that may touch user liveness, and only because the user can still get out.

In scope as `PRIVILEGED` findings (a compromised admin key is a real, tracked threat — not merely "hardening" — but under the honest-key threat model these are defense-in-depth, not `BLOCKER`; a finding that additionally works with *no* admin-key compromise is `BLOCKER`):

- Any admin action that moves a user's, or canonical backing/provider's, value to the admin or a third party — e.g. shutdown-cleanup misrouting, or draining backing/insurance to a payee other than its owner. Abandoned residuals must escheat to the canonical (asset-0) insurance fund like fees, never to the admin signer.
- Any admin action, using a **non-oracle** power, that strands a user with no bounded public exit — *even if it requires an admin shutdown to set up*. "Requires a shutdown" is not a reachability downgrade under this model; shutdown-preserves-exit is exactly the property under test. A shutdown-created state whose only escape path is itself bricked is an in-scope DoS.
- Timing abuses that pair a legitimate oracle mark with a non-oracle admin power to extract value from a user — e.g. withdrawing backing after a committed adverse mark but before the crank publishes it, leaving a winner underbacked. The mark push is trusted; the non-oracle withdraw race is the abuse.
- Admin config or lock manipulation on a running market that freezes an unrelated healthy user's withdrawal, conversion, or exit.

Out of scope (still hardening, not LoF):

- The admin reclaiming its own isolated, domain-segregated reserve that no user can ever draw on.
- Harm reachable only by pushing a dishonest oracle mark — that is the accepted oracle trust, tested separately.

A probe under this section uses public instructions with the admin key as the adversary's signer, against a normally initialized live market, and shows the user-facing loss or the stranded exit against the exact pre-fix parent and the fixed head.

## Public-Reachability Requirement

Keep a probe only when it uses public instructions and normal account construction:

- No direct state injection for the exploit condition, except for fixtures that emulate legitimate external programs or oracle accounts. A privileged signer using its own key on an admin instruction is a public path, not injection.
- No private engine calls as the exploit path.
- No hand-mutating portfolio or market bytes to create an impossible state unless the test is explicitly classed as defense-in-depth and is not merged as a real LoF / DoS / CU bug.
- The failing transaction must be reproducible with LiteSVM against the wrapper API.

## Blind Invariant-Worker Requirement

Use `scripts/worker_blind.md` as the briefing for Astra/invariant workers. The open
issue/PR set is a withheld validation dataset, not worker context. Do not include
issue numbers, PR numbers, titles, branch names, repro summaries, or bug-shaped
hints in worker prompts. The coordinator may evaluate the frozen invariant suite
against the holdout set in a separate verification worktree after worker output is
reviewed.

## Trading Liveness Requirement

Valid trading should not be blocked solely to avoid a CU cliff. Users must be able to find a market and get out of positions when the engine has a valid progress path.

Acceptable guards:

- Reject structurally invalid requests: bad provenance, wrong owner, bad signer, malformed matcher return, duplicate batch asset, invalid fee limit, unsupported lifecycle risk increase, invalid oracle account, hidden/corrupt leg, or malformed token account.
- Reject calls that would perform the wrong action or commit stale/incorrect state while a correct public continuation remains available.
- Bound optional fanout, such as matcher tail accounts, when a smaller public call preserves the same user operation.

Misclassified guards:

- Rejecting an otherwise valid trade only because the submitted transaction could run out of CU.
- Requiring a pre-crank when the consequence of skipping it is only a reverted transaction, not persistent state damage or inability to progress.
- Calling a reverted CU burn a DoS without showing that every bounded public continuation for the affected user also fails.

## Crank / Progress Requirement

Crank safety is an existence-and-progress property:

- For every actionable account or market state, there must be at least one permissionless public instruction that succeeds under bounded CU.
- A successful crank must reduce a rank, apply a needed observation, settle a stale leg, liquidate, close, resolve, or otherwise move the state toward a terminal/healthy condition.
- The wrapper must propagate nonzero engine errors as instruction errors; SVM rollback handles preservation on error.
- Error paths do not need to preserve state in proofs beyond relying on SVM rollback, but tests should assert rollback when exercising wrapper-side CPI or token side effects.

## What To Keep In PR 135

Keep only net-new substantive probes:

- Positive max-shape progress tests.
- Public-interface TDD reproductions for confirmed LoF / DoS / CU bugs.
- Regression tests that show the fixed path now succeeds or rejects for a correctness reason, not merely because it avoids compute.

Delete marginal probes:

- Tests that only confirm an existing rejection.
- Tests that assert a CU abort rolls back without connecting it to user progress.
- State-injection probes that cannot be reached from public APIs.
- Duplicates of engine-proven behavior unless the wrapper adds auth, routing, account, CPI, token, or oracle-specific risk.

## Current Misclassification Audit

- Reverted: PR 171, stale fresh-asset `BatchTradeCpi` pre-crank. It blocked a valid trade solely to avoid CU exhaustion.
- Reverted: PR 161, stale fresh-asset `TradeCpi` pre-crank. It was the same policy on the single-fill route.
- Not currently classed as the same bug: matcher-tail bounding, invalid fee-bps rejection, lifecycle risk-increase rejection, inactive-market rejection, stale selected-mark auto-crank rejection, stale-window liquidation rejection, sub-atom fee accounting, and backing-fee batch gate. These either preserve a valid smaller public operation, prevent stale/incorrect committed state, or enforce value/lifecycle invariants.
- Re-scope under Bounded Admin-Authority: findings previously discounted as "privileged self-rug" or "needs admin shutdown" are in scope when a compromised admin key moves user/provider value or strands a user's exit without an oracle compromise. Shutdown-cleanup fund misrouting must escheat to asset-0 insurance, not the admin.
