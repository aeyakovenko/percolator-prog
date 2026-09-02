# Percolator Whole-Route Security Invariants and Verification Plan

**Target:** Percolator risk engine v16.9.1 and the deployed `percolator-prog` wrapper
**Purpose:** Normative test and proof charter for public instructions, retained signed intents,
stateful fuzzing, model checking, Kani/Lean proofs, and SVM integration tests
**Status:** Verification plan. It complements rather than replaces the v16.9.1 source-of-truth
specification.

This document is not a certification claim. The executable coverage index is
[`tests/invariants/README.md`](tests/invariants/README.md). A direct regression adapted from a
finding proves public reachability, but does not count as independent discovery. Verification is
complete only after the generic invariant oracle and public-sequence generator independently
rediscover every open public-route LoF/DoS finding in the pinned benchmark, and all other completion
criteria in section 10 are satisfied.

---

## 0. Scope and interpretation

This plan tests the complete public transition system: instruction decoding, account validation,
authorization, wrapper policy, CPI/no-CPI matcher paths, engine transitions, SPL-token and lamport
effects, and terminal account closure. A leaf-kernel proof is not sufficient evidence for a public
route unless the wrapper and every composing transition are covered by an equivalent whole-route
proof or a differential equivalence argument.

The v16.8.0 specification does not currently define the names `market_id`, `asset_generation`,
`portfolio_id`, `position_episode`, or `authority_epoch`. Invariants 1-5 are therefore additional
implementation requirements for retained signed requests. If the implementation already has
equivalent fields or makes an account address permanently nonreusable with a typed program-owned
tombstone, the test harness must map that policy explicitly. If it does neither, the corresponding
tests should fail until identity or strict non-reuse is enforced.

### Important correction to asset binding

Binding an asset-specific request only to `(market_id, asset_index)` is insufficient if an asset
slot can be retired and reused inside the same market generation. The safe binding is:

```text
(program_domain, market_pubkey, market_id, asset_index, asset_generation)
```

An equivalent globally unique `asset_id` is also sufficient. The phrase "binds the current
market_id, not only asset_index" is retained below, but the invariant is strengthened to include
the slot generation because otherwise slot-reuse replay remains possible.

### Required retained-intent identity fields

A canonical retained signed message should contain, directly or through an unambiguous typed
submessage:

```text
IntentHeader {
    chain_or_genesis_domain
    program_id
    market_pubkey
    market_id                  // persistent market generation when the market pubkey is reusable
    instruction_kind
    message_version
    intent_id                  // monotonic nonce or unique replay key
    not_before_slot            // optional
    expiry_slot                // required for bounded replay windows
}

AssetBinding {
    asset_index
    asset_generation           // or globally unique asset_id
}

PortfolioBinding {
    portfolio_pubkey
    portfolio_id
}

EpisodeBinding {
    position_episode
    recovery_episode           // where applicable
    claim_or_receipt_episode    // where applicable
}

AuthorityBinding {
    authority_scope
    authority_epoch
}
```

`market_id`, `asset_generation`, `portfolio_id`, and authority epochs must be program assigned and
must not reset merely because an account is closed and recreated at the same pubkey. A market-level
or factory-level monotonic registry is required wherever account-local storage would disappear on
close. As a stricter equivalent for an account class, close may retain an immutable typed,
rent-exempt, program-owned tombstone that makes recreation at that pubkey impossible. That policy
must reject public reinitialization even after arbitrary lamport funding and must leave fresh
addresses usable; it does not remove chain/program/message-type domain requirements.

### Verification tags

- **P** - formal proof over the actual transition or a proven-equivalent pure transition.
- **F** - stateful/property-based fuzzing over public instruction sequences.
- **I** - SVM integration test with real account ownership, signer/writable flags, SPL transfers,
  CPI, and rollback.
- **M** - metamorphic or differential test between economically equivalent routes.
- **R** - exhaustive bounded reachability/model-checking test.
- **C** - compute-unit and maximum-shape benchmark.

Every invariant should have at least one executable test. Value conservation, authorization, replay
safety, state validity, and terminal liveness should have more than one independent verification
method.

### Known-finding rediscovery levels

- **Missing** - no executable whole-route witness.
- **Direct regression** - a finding-specific adapter reproduces or rejects the known trace.
- **Independent discovery** - a generic public-action generator, unaware of the PR-specific trace,
  reaches a minimized counterexample because a normative invariant oracle failed.
- **Certified** - the fixed implementation passes independent discovery attempts and every proof,
  integration, metamorphic, reachability, and CU method required by the invariant.

Only **Independent discovery** or **Certified** satisfies the known-finding benchmark. Renaming or
copying a PR regression into the fuzzer does not.

---

## 1. Identity, incarnation, replay, and consent

### INV-001 - Market incarnation binding

**Statement.** Every retained signed request binds the persistent market identity. The binding
includes the program domain and market-group pubkey. If that pubkey is reusable, it also includes a
program-assigned monotonic `market_id`. A stricter implementation may permanently tombstone the
retired pubkey so no later market incarnation can exist there. Under either policy, an earlier
market request can never become valid in a later market.

**Required tests.** For a reusable market, sign under generation `g`, close/recreate at the same
pubkey with generation `g+1`, and prove every old request rejects before mutation. For strict
non-reuse, close the market, publicly fund its exact tombstone, prove reinitialization and every old
request reject before mutation, and prove a fresh pubkey still initializes. Also test cross-market
and cross-program replay.
**Verification:** P, F, I, M

### INV-002 - Asset generation binding

**Statement.** Every asset-specific request binds the current `market_id`, not only `asset_index`,
and also binds the current slot generation or unique `asset_id`. Retiring and reusing an asset slot
cannot revive an order, oracle authorization, backing action, claim, matcher capability, or
destructive consent for the prior asset occupying that index.

**Required tests.** Sign for `(market_id=m, asset_index=i, asset_generation=a)`, retire the asset,
reactivate slot `i` with generation `a+1`, and prove the old request rejects. A test that binds only
`(m, i)` must demonstrate the replay vulnerability and remain a negative regression.
**Verification:** P, F, I, M

### INV-003 - Portfolio incarnation binding

**Statement.** Every portfolio-specific request binds the monotonic program-assigned `portfolio_id`
in addition to the portfolio pubkey. Closing and recreating a portfolio at the same pubkey cannot
revive old orders, withdrawal authority, matcher delegation, fee consent, claims, or close consent.

**Required tests.** Replay every retained portfolio request after close/recreate at the same pubkey
and after owner change. The request must reject even when the same owner returns.
**Verification:** P, F, I

### INV-004 - Position episode binding

**Statement.** Reductions, conversions, closes, claims, and forfeits bind the exact position or
recovery episode. A new episode begins at least on zero-to-nonzero open, cross-zero flip, side reset,
recovery conversion, terminal receipt creation, or any transition that replaces the economic
identity of the leg or claim.

**Required tests.** Sign a reduction or forfeit for episode `e`, close or cross through zero, reopen
episode `e+1`, and prove the old consent cannot touch the new leg. Repeat for recovery and
resolved-claim episodes.
**Verification:** P, F, I, M

### INV-005 - Authority incarnation binding

**Statement.** Retained authority requests cannot regain power after authority rotates away and
later returns. Every retained admin, oracle, insurance, backing, matcher, operator, or delegate
request binds a monotonic epoch for its exact authority scope.

**Required tests.** Exercise `A -> B -> A` and `A -> zero/disabled -> A`. A request signed by the
first incarnation of A must remain invalid after A returns. Epochs must change atomically with
authority updates.
**Verification:** P, F, I

### INV-006 - Program, chain, message-type, and version binding

**Statement.** Every retained request binds the chain/genesis domain, deployed program ID, market
pubkey, instruction kind, and message version. A signature for one program deployment, cluster,
instruction variant, or encoding cannot be interpreted as another valid operation.

For a wrapper that exposes no detached-signature interpreter, the signed Solana transaction message
is the retained envelope: it binds the transaction-message version, deployed program ID, every
account key, exact instruction bytes, and recent blockhash. In that architecture, cluster admission
is delegated to validator recent-blockhash semantics and collision resistance rather than duplicated
in each instruction schema. Adding Ed25519/secp instruction introspection, a relayer signature,
durable detached consent, or another signature-bearing payload immediately requires the explicit
typed application-domain header above.

**Required tests.** Cross-cluster, cross-program, cross-instruction, and version-downgrade replay
must reject. Ambiguous byte encodings and prefix-compatible messages must not verify as another
type. A source-complete audit must prove whether any detached-signature surface exists; when none
exists, mutate every Solana-message domain after signing, prove signature rejection and exact
rollback, and compose with strict decoder/version-downgrade coverage. Validator blockhash admission
is a named runtime assumption, not a property of the wrapper instruction body.
**Verification:** P, F, I

### INV-007 - No ABA reuse

**Statement.** Closing and recreating any market, asset slot, portfolio, receipt, delegate,
capability, or auxiliary account at the same pubkey never revives old consent, capability, counters,
reservations, liens, or claims. Each reusable class has program-assigned incarnation identity that
survives deletion of account-local state, or close leaves a permanent typed program-owned tombstone
that makes same-pubkey recreation impossible.

**Required tests.** For each reusable account class, create A, authorize an action, close A,
recreate A with identical visible keys, and replay. For each strict non-reuse class, publicly fund
the tombstone and attempt recreation. No old state or authority may reappear, and fresh addresses
must remain usable.
**Verification:** P, F, I, R

### INV-008 - Intent uniqueness and bounded replay

**Statement.** Every retryable retained request has a unique monotonic `intent_id` or a bounded
replay key and expiry. Across direct, batch, CPI, no-CPI, and retry routes, the same economic intent
executes at most once.

**Required tests.** Duplicate the same signed request in one transaction, in later transactions,
through another entrypoint, and after partial failure. Exactly one economic execution is permitted.
**Verification:** P, F, I, M

### INV-009 - Partial-fill and retry accounting

**Statement.** Partial execution records the exact remaining authorized quantity, aggregate fee
budget, slippage budget, and expiry. A retry cannot reset consumed limits, execute already-filled
quantity, or collect a second per-intent minimum fee.

**Required tests.** Randomly partition a signed intent into fills, interleave failures and retries,
and assert cumulative quantity and fees never exceed the original signed bounds.
**Verification:** P, F, I, M

### INV-010 - Out-of-order safety

**Statement.** For every landing order of otherwise valid retained requests, each request either
rejects atomically or produces an outcome within every affected signer's original authorization.
Reordering cannot install worse price, fee, collateral use, position, claim treatment, or
destructive state.

**Required tests.** Permute trade, deposit, withdraw, reduce, authority rotation, policy update,
resolve, and claim instructions. Compare every successful outcome against each message's signed
postconditions.
**Verification:** F, I, M, R

### INV-011 - Signed aggregate economic bounds

**Statement.** Trades and conversions bind aggregate maximum fee, total quantity, slippage, price
limits, deadline, final position bounds, and permitted collateral or PnL-credit use across all legs
and partial fills. Per-leg checks cannot silently exceed an aggregate signed limit.

**Required tests.** Split one intent into many individually acceptable legs whose aggregate exceeds
one signed bound; the sequence must reject or stop exactly at the remaining allowance.
**Verification:** P, F, I, M

### INV-012 - Capability and delegate scope

**Statement.** Matcher and delegate authorization binds the program domain, market generation,
portfolio incarnation, matcher program/context/slab, delegate key, authority epoch, allowed assets
and operations, limits, enabled state, and expiry. A capability valid for one scope has no authority
in another.

**Required tests.** Substitute one field at a time, including matcher context and asset generation.
Disable and re-enable the same delegate and prove old capabilities remain invalid.
**Verification:** P, F, I

### INV-013 - Destructive-consent scope

**Statement.** Shutdown, resolve, forfeit, close, liquidation delegation, recovery, and reduction
consent cannot affect a later market, asset, portfolio, position, close, claim, or recovery episode.

**Required tests.** Retain each destructive consent across every relevant lifecycle transition and
assert it rejects before any state change in the later episode.
**Verification:** P, F, I

### INV-014 - Delayed-policy and policy-epoch safety

**Statement.** A delayed request executes only under the policy version it explicitly bound or
under current policy whose result remains inside all signed economic bounds. A later fee, oracle,
insurance, matcher, or risk-policy update cannot redirect value or worsen terms beyond the signer's
authorization.

**Required tests.** Sign before a policy change, execute after the change, and test both stricter and
looser policies. The outcome must be bounded by the signed request, not merely by current
configuration.
**Verification:** P, F, I, M

---

## 2. Solana account and wrapper boundary

### INV-015 - Account ownership, layout, discriminator, and length validity

**Statement.** Every program-owned account is checked for owner, exact account class, supported
layout version, minimum and maximum length, alignment, and valid enum/discriminator values before
any zero-copy view or mutation. No malformed account can be reinterpreted as another type.

**Required tests.** Fuzz owner, data length, discriminator, version, padding, enum values, and
alignment. Every malformed account rejects without mutation or panic.
**Verification:** P, F, I

### INV-016 - Canonical PDA and seed binding

**Statement.** Every PDA is derived from canonical seeds that include all required incarnation
identifiers, and the program accepts only the canonical bump. A PDA for an earlier generation or
another role cannot be supplied as the current account.

**Required tests.** Substitute noncanonical bumps, reordered seeds, omitted generation fields, and a
valid PDA from another role or market.
**Verification:** P, F, I

### INV-017 - Signer, writable-role, and account-alias safety

**Statement.** Every role has explicit signer and writable requirements. Passing the same account
in two semantic roles either is rejected or is proven safe under that alias. No duplicate
mutable-account arrangement can bypass pair-local, fee, ownership, or conservation checks.

**Required tests.** Systematically alias every pair of instruction accounts, including long/short
portfolios, vault/destination, authority/operator, and market/oracle accounts.
**Verification:** F, I, M

### INV-018 - Quote mint, vault, token-program, and authority integrity

**Statement.** All quote-token movements use the configured quote mint, canonical vault, correct
token program, expected decimals, and canonical vault authority. Token-2022 extensions or alternate
token programs cannot alter accounting unless explicitly supported and proven.

**Required tests.** Substitute mint, token program, decimals, vault owner, authority, destination,
fee-on-transfer mint, frozen account, and delegated account. The internal quote delta must equal the
actual received or sent amount.
**Verification:** P, F, I

### INV-019 - CPI invocation and return-data binding

**Statement.** Matcher and oracle CPI results bind the exact invoked program, current invocation,
market generation, portfolio incarnations, matcher context, asset generation, quantities, and
price. Stale return data or return data from another CPI cannot authorize a trade or mark update.

**Required tests.** Leave old return data, invoke a benign CPI before/after the matcher, spoof the
return format, and mismatch every bound field.
**Verification:** P, F, I

### INV-020 - Authenticated clock, slot, and oracle provenance

**Statement.** Security-relevant time and oracle observations come from authenticated
sysvars/accounts and are monotonic relative to stored state. Caller-supplied fallback values are
untrusted and can never make a stale state fresher, accelerate expiry, or trigger favorable
resolution.

**Required tests.** Omit or corrupt sysvars where possible, vary fallback slots, rewind stored
slots, and test expiry-1, expiry, and expiry+1.
**Verification:** P, F, I

### INV-021 - Account creation, reallocation, close, rent, and lamport safety

**Statement.** Creation and reallocation preserve type, bounds, and zero-initialization rules.
Closing an account sends refundable lamports only to the authorized destination after all claims
and encumbrances are cleared. A strict non-reuse close may retain exactly the rent required for its
typed tombstone; no other value may remain. Close/recreate cannot recover old bytes or bypass
generation or tombstone checks.

**Required tests.** Reallocate smaller/larger, close with residual obligations, close to a
substituted recipient, and recreate with an identical pubkey.
**Verification:** F, I

### INV-022 - Instruction decoding and schema/upgrade safety

**Statement.** Instruction decoding is total, rejects unknown versions and trailing ambiguity, and
cannot reinterpret old signed bytes after a program or schema upgrade. Any upgrade that changes
retained-message semantics changes the message version or domain separator.

**Required tests.** Fuzz arbitrary bytes, truncated messages, appended bytes, duplicate fields, old
versions, and upgrade migration boundaries.
**Verification:** P, F, I

### INV-023 - Caller-input confinement for derived safety state

**Statement.** Public wrappers treat caller-supplied values as signed intent, authenticated external
observations, or discovery hints only. Callers cannot directly choose admission/funding thresholds,
future slots, lifecycle transitions, B chunk sizes, claim-bound membership or formulas, backing
freshness, source-credit rates, lien interpretation, support/insurance allocation, residual
attribution, close ownership/serialization, recovery prices, recovery transfer bounds, or cross-instance
netting. Every such value is derived from authenticated state and canonical configuration.

**Required tests.** Fuzz every scalar and account supplied by the caller, attempt to substitute each
internal derived value through alternate entrypoints, and assert hints can affect work discovery but
not economic truth or safety decisions.
**Verification:** P, F, I

---

## 3. Quote value, source credit, backing, insurance, and attribution

### INV-024 - Attributed quote-value conservation

**Statement.** For every successful instruction, every quote atom has one debit and one credit in
the `TokenValueFlowProof`. Value attributable to account or domain A cannot become withdrawable by
B without explicit signed transfer or a deterministic protocol rule already binding A.

```text
sum(quote_debits) = sum(quote_credits)
```

**Required tests.** Assert the equality after every successful public transition and exact zero
external/internal quote delta on failure. Track ownership attribution, not only aggregate vault
balance.
**Verification:** P, F, I

### INV-025 - Exact stock reconciliation

**Statement.** The vault and every accounting stock class reconcile at genesis, activation, every
successful instruction, mode transition, recovery, resolution, and terminal close. Settlement
rounding residue and protocol surplus are explicit stock classes.

**Required tests.** Recompute stock independently from raw state after every fuzz step; do not trust
cached totals.
**Verification:** P, F, I

### INV-026 - Reservation and encumbrance conservation is separate from token value

**Statement.** Backing buckets, source-credit reservations, liens, insurance reservations, pending
obligations, and close reserves are labels over value, not independent value. A separate
`ReservationEncumbranceProof` shows every encumbered atom has exactly one owner, source, state, and
release/consume path.

**Required tests.** Verify label creation moves no quote, label consumption cannot spend the same
atom twice, and label release restores only the original available class.
**Verification:** P, F

### INV-027 - Protected principal seniority

**Statement.** Senior capital, booked loss, pending obligations, and required insurance/recovery
reserves are satisfied before junior positive PnL becomes withdrawable or usable for new risk.
Junior value cannot outrank protected principal through route choice, ordering, or stale
certificates.

**Required tests.** Generate underbacked and loss-stale states, then attempt every favorable
operation through every route.
**Verification:** P, F, I, M

### INV-028 - Source-domain realizability cap

**Statement.** For each source domain D, usable positive credit never exceeds independently
recomputed available backing:

```text
usable_positive_credit_num[D] <= available_backing_num[D]

available_backing_num[D] =
    fresh_reserved_backing_num[D]
  - valid_liened_backing_num[D]
  + insurance_credit_reserved_num[D]
  - valid_liened_insurance_num[D]
  - impaired_liened_insurance_num[D]
```

All subtractions are nonnegative and checked. Any backing dependency chain must strictly consume or
reserve independently available backing and cannot form a circular credit cycle without external
senior capital.

**Required tests.** Oracle manipulation, stale backing, partial lien consume, expiry, insurance
impairment, cross-asset use, and cyclic A-backs-B-backs-A plans must never exceed the recomputed cap.
**Verification:** P, F, I

### INV-029 - Positive claim bounds never understate

**Statement.** `positive_claim_bound_num[D]` is an upper bound on all exact, bucketed, pending,
unresolved, and recovery positive claims owed by D. Replacing a bound with an exact receipt is
permitted only with a proof that the exact claim does not exceed the prior contribution.

The deployed v16 profile currently has no approximate claim-bound bucket or rebucketing route:
every production source claim is exact and atom-scaled. For that profile, public-transition tests
must require `exact_positive_claim_num[D] == positive_claim_bound_num[D]`, and a source-complete
absence guard must reject any non-exact bound ingress. Adding an approximate bucket reopens this
invariant and requires the bucket-specific evidence below before deployment.

**Required tests.** Exhaustively test favorable price/funding bounds, stale uncertainty, and receipt
replacement. When approximate buckets are enabled, also exhaust bucket range edges and prove
out-of-range inputs fail closed or rebucket without understatement.
**Verification:** P, F, R

### INV-030 - Credit-rate determinism and fail-closed behavior

**Statement.** The source credit rate is a deterministic function of current claim bounds and
available backing, lies in `[0, CREDIT_RATE_SCALE]`, and cannot become more favorable from stale,
expired, impaired, or omitted state. Any unrepresentable or unverifiable input yields zero credit or
recovery.

**Required tests.** Independently recompute every rate and compare. Across every public transition,
unchanged formula inputs preserve the rate, any formula-input mutation advances the source-credit
epoch, and a live claim's rate can increase only when independently available backing increases or
its valid claim bound decreases. Pure expiry or impairment cannot improve either quantity. Omitted
embedded state and every invalid persisted formula/ledger relation reject before mutation.
**Verification:** P, F

### INV-031 - No double use of claim, backing, or insurance atoms

**Statement.** The same claim, backing, or insurance atom cannot support two accounts, domains,
instances, risk increases, payouts, or residual cures at once. Each atom has exactly one canonical
lifecycle state.

**Required tests.** Attempt duplicate lien creation, cross-domain reservation, retry after partial
consumption, and concurrent route use.
**Verification:** P, F, I

### INV-032 - Exact counterparty-lien lifecycle

**Statement.** Create, consume, release, impair, and recover transitions update bucket-local and
source-domain aggregates exactly once. A consumed lien is not valid, a released lien is not
reserved, and an impaired lien is not fresh or available.

**Required tests.** Differentially recompute all bucket sums after every lifecycle action and every
error injection point.
**Verification:** P, F

### INV-033 - Insurance-backed lien single classification

**Statement.** An insurance-backed lien consumed for cure or payout is classified exactly once as
insurance spend or an explicitly equivalent insurance term. It is never simultaneously counted as
counterparty support, generic support, live reservation, or fresh backing.

**Required tests.** Exercise every consume/release/impair/recovery branch and assert disjoint
category membership.
**Verification:** P, F

### INV-034 - Domain and instance isolation

**Statement.** Health, collateral, PnL, backing, lien, insurance, B, payout, receipt, recovery, and
claim state never cross market-group instances. Within an instance, a source domain cannot pay
another domain's debt or receive its fees unless an explicit configured transition names both
source and destination.

**Required tests.** Substitute valid accounts and domains from another instance and attempt
cross-domain use through every public route.
**Verification:** P, F, I

### INV-035 - No global B pool; residuals remain local

**Statement.** Bankruptcy residual and B accounting are charged only to the exact
`(asset, opposing_side)` domain that generated the exposure. No global B index or unrelated-side
socialization can absorb the loss.

**Required tests.** Multi-asset bankruptcies with ordering permutations must produce identical
domain-local residuals.
**Verification:** P, F, M

### INV-036 - Fee destination and policy-version integrity

**Statement.** Every charged fee reaches only its signed or protocol-defined destination and domain
under the bound policy epoch. Delayed policy changes, zero-activity assets, or route choice cannot
redirect or siphon fees.

**Required tests.** Add parasitic assets, rotate fee policies, execute delayed intents, and compare
single/batch and CPI/no-CPI paths.
**Verification:** P, F, I, M

### INV-037 - Exact residual partition

**Statement.** Every close or bankruptcy residual satisfies one exact disjoint partition:

```text
gross_loss_at_close_start
+ adverse_close_drift
=
  support_consumed
+ insurance_spent
+ b_loss_booked
+ explicit_loss_assigned
+ pending_obligation_credits
+ consumed_counterparty_credit_lien_backing
+ remaining_residual
```

No atom appears in two categories and no category is silently dropped.

For the deployed v16.9.1 ledger, `drift_consumed` is the reserved adverse-drift term and
`support_consumed` is the realizable value payment. `junior_face_burned` is claim-face metadata,
not an additional payment, and must not be added to the value partition. Any abstract category
folded into a deployed field must have one documented, disjoint mapping; an absent category is not
implicitly proven nonzero or independently attributable.

**Required tests.** Independently recompute the equality after every continuation, competing-close
rejection, cancel attempt, recovery, and finalization.
**Verification:** P, F

### INV-038 - Rounding and ratio conservation

**Statement.** For every exact amount X divided into rounded allocations:

```text
X = sum(allocations) + residue
residue >= 0
```

The residue is credited only to `SettlementRoundingResidue` or `UnallocatedProtocolSurplus`. It
cannot create health, backing, insurance capacity, payout entitlement, or senior capital.

**Required tests.** Symbolic and boundary operands, sequential ratio changes, split allocations,
resolved claims, B booking, and social-loss remainder clearing.
**Verification:** P, F, M

### INV-039 - Pending-loss obligation durability

**Statement.** A participant cannot erase or externalize its share of pending residual or close
drift by reducing weight, clearing a leg, transferring a position, resolving, retiring, or closing.
It must escrow, settle, or pull forward the obligation exactly once before weight removal.

**Required tests.** Interleave pending loss with trade, rebalance, partial liquidation, transfer,
reset, resolution, and account close.
**Verification:** P, F, I

### INV-040 - No fee seniority

**Statement.** Uncollectible protocol, matcher, maintenance, or liquidation fees are dropped or
forgiven. They are never paid from protected principal belonging to others, insurance, source
backing, or B socialization.

**Required tests.** Generate accounts with insufficient fee-paying value but remaining senior
obligations and attempt all fee-charging routes.
**Verification:** P, F

### INV-041 - Deterministic allocation and caller-order independence

**Statement.** Liquidation order, support allocation, insurance allocation, lien consumption,
residual attribution, and payout calculation are deterministic from state and signed inputs.
Caller-supplied ordering cannot improve the caller's economic result or change which domain bears
loss.

**Required tests.** Permute equal-priority legs, hints, claim order, and continuation order; compare
normalized outcomes.
**Verification:** F, M, R

### INV-042 - Recovery fallback envelope

**Statement.** Fallback recovery price is deterministic, caller-independent, activation-validated,
and within the configured reference-price deviation envelope. Per-account and per-domain value
transfer is computed and bounded; if the bound is unavailable or exceeded, no positive junior
payout occurs.

**Current-surface applicability.** Engine v16.9 reserves synthetic fallback pricing and the deployed
wrapper exposes no fallback price, reference, deviation, envelope, or value-transfer-bound input.
Recovery and abandoned-asset force close use the last stored authenticated effective price; junior
forfeit paths do not synthesize a replacement price. Therefore the numeric fallback envelope is
`N/A` for the current public transition system. The executable obligation is that no public wire or
handler consumes the reserved fallback controls. Adding any such consumer immediately reopens the
statement and all tests below before activation can rely on it.

**Required tests.** Boundary prices, stale/unavailable reference, maximum positions, multiple
accounts, and fallback just inside/outside the cap once the mechanism is implemented. While it is
reserved, source-lock the public absence and prove existing Recovery routes use authenticated stored
state without caller-selected pricing.
**Verification:** P, F, I

### INV-043 - Hedge and correlation credit envelope

**Statement.** Any hedge/correlation credit is granted only by a deterministic bucket and a
conservative combined-loss envelope. After credit, worst-case portfolio loss remains fully covered
under the configured scenario set; caller-selected correlations or omitted legs cannot improve
health.

**Current-surface applicability.** Numeric hedge/correlation credit is disabled in the pinned v16
public profile. Health certification sums gross per-leg initial margin, maintenance margin, and
worst-case loss without a cross-leg offset. The envelope is therefore `N/A` while the credit remains
exactly zero. Any public configuration, persisted field, or health-path consumer that can make it
nonzero reopens the full statement before activation.

**Required tests.** Exhaustive small portfolios, sign flips, bucket boundaries, missing legs, and
scenario extremes when numeric credit is implemented. While disabled, require a nonvacuous
cross-asset opposite-exposure test and source-lock the absence of every public credit control.
**Verification:** P, F, R

### INV-044 - No phantom value from indices, certificates, or labels

**Statement.** A/K/F/B indices, health certificates, claim bounds, reservations, liens, backing
tags, lifecycle flags, and soft maintenance credit cannot by themselves increase token stock or
withdrawable value. Soft maintenance credit must be revalidated on every favorable action and
cannot fund withdrawal, conversion, fee payment, residual cure, or new risk without the required
durable lien. Every value increase must have an attributed quote source.

**Required tests.** Mutate each derived index only through public transitions, attempt every durable
use of soft credit without a lien, and assert token and encumbrance proofs remain balanced.
**Verification:** P, F

---

## 4. Trading, oracle, health, liquidation, and ADL

### INV-045 - No free mark movement

**Statement.** Every accepted mark update, including raw target 0, 1, and MAX, stays inside the
elapsed-time movement envelope from the prior nonzero mark. No CPI/no-CPI, single/batch,
self-controlled, zero-fee, zero-size, or zero-price route bypasses the cap.

```text
mark_after in movement_envelope(mark_before, elapsed_slots, configured_cap)
```

**Required tests.** Differentially compare every mark-updating route over all zero/boundary cases
and repeated same-slot calls.
**Verification:** P, F, I, M

### INV-046 - Trade availability without unsafe mark admission

**Statement.** A raw execution price cannot poison mark/oracle state or eliminate every exit path.
Unsafe raw prices may be ignored for discovery, safely clamped for risk/fees, or rejected only when
necessary, while at least one bounded risk-reducing route remains available.

**Required tests.** Zero, extreme, stale, and out-of-band prices during normal, drain, reset,
recovery, and close states.
**Verification:** F, I, R

### INV-047 - Equivalent-route semantics

**Statement.** After normalizing explicitly documented route-specific fees, the same authorized
economic intent through CPI/no-CPI, single/batch, direct/composite, or wrapper/engine variants
produces the same economic state delta or rejects. No route omits a safety check enforced by
another.

**Required tests.** Paired metamorphic executions from identical snapshots across every route
combination.
**Verification:** F, I, M

### INV-048 - Matched trade and open-interest coherence

**Statement.** Every matched trade preserves signed quantity and canonical aggregate equations:

```text
sum(account_position_deltas) = 0

OI_eff_long  = sum(current_epoch_effective_long_legs_that_count_toward_OI)

OI_eff_short = sum(current_epoch_effective_short_legs_that_count_toward_OI)
```

Pending-loss obligations are reconciled by their own ledgers and must not be silently used to
repair an OI mismatch unless the canonical specification explicitly defines an OI-carrying
obligation class. In normal live state, effective long and short OI match.

**Required tests.** Recompute the logical aggregate from all portfolios in the bounded test universe
after trade, liquidation, rebalance, reset, resolved close, and recovery, and compare it with the
maintained O(1) counters.
**Verification:** P, F, M

### INV-049 - Canonical single net leg per asset generation

**Statement.** A portfolio has at most one active canonical signed net leg for each
`(asset_index, asset_generation)`. Same-asset opposite exposure nets into that leg; duplicate slots,
hidden opposite legs, stale-generation legs, and simultaneous current/recovery legs cannot coexist
unless the state machine defines a disjoint explicitly proven representation.

**Required tests.** Attempt duplicate attachment through every trade, transfer, reset, recovery,
reactivation, and deserialization path. Independently scan the bounded portfolio after every
transition and reject duplicate current-generation legs.
**Verification:** P, F, I

### INV-050 - Cross-zero decomposition

**Statement.** A cross-zero trade is decomposed into a reduction no larger than that portfolio
episode's real ADL-effective exposure and a new-open component. The new-open component passes all
normal margin, oracle, lifecycle, certificate, lien, and currentness gates. Unrelated aggregate OI
cannot authorize the reduction.

**Required tests.** Partial liquidation followed by cross-zero, with and without unrelated auxiliary
OI, through all trade routes.
**Verification:** P, F, I, M

### INV-051 - Canonical ADL-effective quantity

**Statement.** Transfer, resize, rebalance, liquidation, clear, resolved close, recovery, and
retirement all use one canonical effective-quantity function derived from raw basis, `a_basis`, side
epoch, and conservative rounding. No route subtracts raw quantity from an effective-OI counter.

**Required tests.** Compare every route against the same pure reference function after partial ADL
and side reset.
**Verification:** P, F, M

### INV-052 - Split/merge invariance

**Statement.** Splitting or merging a trade, liquidation, reduction, withdrawal, lien consumption,
insurance withdrawal, or claim cannot bypass cumulative position, OI, health, backing, fee,
cooldown, rate, or policy limits. Apart from explicitly bounded conservative rounding, partitioned
execution is not more favorable than aggregate execution.

**Required tests.** Generate arbitrary partitions and permutations; compare cumulative results with
one aggregate operation.
**Verification:** P, F, I, M

### INV-053 - Full-health recertification equivalence

**Statement.** Any incremental or fast certificate after a candidate transition equals a full
recomputation over every active leg, or is strictly more conservative in every equity and
requirement lane. It includes oracle target/effective lag, pending obligations, impaired liens, ADL
factors, and all active-asset penalties.

**Required tests.** Differentially compare fast and full certification after every single and batch
fill, especially when a nontraded asset is lagging.
**Verification:** P, F, M

### INV-054 - Certificate epoch completeness

**Statement.** A health certificate binds every state version capable of affecting health. In the
deployed representation this means the account active bitmap plus the market `oracle_epoch`,
`funding_epoch`, `risk_epoch`, and `asset_set_epoch`. Every health-relevant writer, including raw
target/effective-price, A/K/F/B, source-credit, lien, pending-obligation, generation, lifecycle, and
close-state changes, must either advance the appropriate bound epoch, atomically issue a certificate
for the post-state, or conservatively invalidate the certificate. A policy field that cannot affect
health need not create a phantom certificate epoch; any policy that can affect health must map to one
of these deployed keys or extend the certificate schema.

**Required tests.** Change one bound input at a time and prove stale certificates cannot authorize a
favorable action.
**Verification:** P, F, I

### INV-055 - State-indexed admission

**Statement.** Each market, asset, side, portfolio, close, and recovery mode has an explicit
allowed-operation set. Risk increase requires the asset lifecycle to be Active and both sides to be
Normal. ResetPending, DrainOnly, Recovery, Resolved, and Retired cannot admit inconsistent
operations.

**Required tests.** Cross every public instruction with every lifecycle mode and assert the
admission matrix.
**Verification:** P, F, R

### INV-056 - Hints are discovery only; favorable actions fully refresh

**Statement.** Stale, missing, duplicated, or adversarial hints cannot improve health or omit a
liability. Every user-favorable action fully refreshes the bounded active portfolio or uses a
proven-equivalent exact certificate covering the candidate transition.

**Required tests.** Omit the worst leg, reorder hints, duplicate benign legs, and use stale
positions. Outcome must equal full canonical discovery or reject.
**Verification:** P, F, I, M

### INV-057 - Risk-reduction availability

**Statement.** From every publicly reachable live, drain, reset, recovery, or resolved state with
exposure, the owner has at least one bounded public action that reduces exposure, creates a terminal
receipt, or explicitly forfeits only junior value while preserving senior claims.

**Required tests.** Reach each state by public instructions and search for an owner-callable reducing
action; failures become liveness counterexamples.
**Verification:** F, I, R

### INV-058 - Cumulative position, OI, notional, and rate-limit integrity

**Statement.** Per-instruction and cumulative limits use the post-transition effective state and
cannot be bypassed by batching, splitting, cross-zero, transfer, account recreation, or route
choice. All arithmetic is checked at zero, one, maximum, and near-maximum values.

**Required tests.** Stateful sequences around every hard bound and every partition strategy.
**Verification:** P, F, I, M

### INV-059 - Fee-fragmentation bound

**Statement.** Across one liquidation or execution episode, cumulative fees do not exceed the
signed or configured episode cap. A minimum fee is charged once per episode, or sub-minimum
operations are rejected except for a final residual close.

**Required tests.** Split a close into one-atom operations, retries, route changes, and partial
failures; compare with one aggregate close.
**Verification:** P, F, I, M

### INV-060 - Single-sided margin and penalty accounting

**Statement.** Each pending obligation, impaired lien, oracle-lag loss, reserve, and penalty appears
either as an equity deduction or as a requirement add-on in a given health test, never both and
never neither.

**Required tests.** Independent decomposition of every certificate lane under combinations of
penalties.
**Verification:** P, F

### INV-061 - Deterministic, bounded liquidation

**Statement.** Liquidation selects and sizes legs deterministically from the refreshed account,
never increases risk, respects ADL-effective exposure, preserves OI coherence, and terminates in
bounded work. Caller-chosen ordering or tiny close sizes cannot extract additional fees or alter
loss attribution.

**Required tests.** Permute requests, split closes, use equal-risk legs, and test max-asset
portfolios.
**Verification:** P, F, I, M, C

### INV-062 - No identity assumptions; self-trade containment is economic

**Statement.** Solvency and manipulation resistance do not depend on detecting common ownership or
self-trading. Two attacker-controlled portfolios receive no privilege, and self-controlled trades
cannot create unbacked value, bypass mark costs, or alter attribution.

**Required tests.** Treat all counterparties as potentially common-controlled and repeat
manipulation sequences without any identity oracle.
**Verification:** P, F, I

---

## 5. Lifecycle, resolution, payout, crankability, and terminal liveness

### INV-063 - Backing-expiry normalization

**Statement.** Any backing bucket with `expiry_slot <= current_slot` behaves as expired before add,
consume, release, claim, close, payout, or retirement processing, regardless of its stored tag.
Normalization is bounded and cannot increase available backing or credit rate without new
independent backing.

**Required tests.** Exercise every consumer at expiry-1, expiry, and expiry+1, including a bucket
still tagged Fresh after expiry.
**Verification:** P, F, I

### INV-064 - Insurance-withdrawal policy equivalence

**Statement.** Every route reaching live insurance funds shares one enable flag, one aggregate
proportional cap, one cooldown, one policy epoch, and one last-withdrawal update. Splitting across
routes, assets, or domains cannot bypass the policy.

**Required tests.** Interleave all insurance withdrawal variants and compare cumulative allowance
against one canonical reference ledger.
**Verification:** P, F, I, M

### INV-065 - Reset, recovery, and retired-state isolation

**Statement.** New risk cannot be opened into a side or asset whose reset, recovery, or retirement
transition is in progress. Recovery and reset affect exactly the bound side/asset generation and
cannot orphan newly created legs outside the episode.

**Required tests.** Attempt trades between begin/finalize reset steps, during recovery, and after
retirement/reactivation.
**Verification:** P, F, I, R

### INV-066 - Resolved-payout fairness and order independence

**Statement.** Payout entitlement derives from an immutable snapshot and exact proven claim face.
Authority actions cannot reduce the unreceipted bound except atomically with protocol-proven removal
of the same claim. Valid claimant orders differ only by designated conservative rounding residue.

**Required tests.** Permute claim order, attempted authority refinements, top-ups, exact receipt
replacement, and recovery transitions.
**Verification:** P, F, M, R

### INV-067 - Terminal payout completeness and exact-once settlement

**Statement.** Every valid claim episode is paid exactly once at its protocol-defined entitlement,
explicitly forfeited for that same episode, or converted into one terminal recovery receipt. No
claim is duplicated, silently dropped, or left in a nonprogressing intermediate state.

**Required tests.** Retry, replay, partial top-up, close/recreate, claim order, and recovery
conversion.
**Verification:** P, F, I, R

### INV-068 - Receipt uniqueness and monotonic top-ups

**Statement.** A receipt binds market, portfolio incarnation, exact face, snapshot, and its claim
episode. Asset/source-domain and explicit receipt-ID fields are required for transferable,
domain-local, or concurrently addressable receipts. The deployed v16 market-wide embedded receipt
is equivalent only while all of the following hold: exactly one receipt can inhabit a portfolio;
the receipt is neither caller-selected nor transferable; its face and snapshot are derived from the
current resolved market; a nonfinal receipt prevents portfolio dematerialization; and Resolved mode
prevents portfolio, asset-generation, and Recovery episode reuse until the receipt is finalized or
terminally cleared. Cumulative payout is monotonic and never exceeds final entitlement; replacing a
bound with an exact claim preserves the prior upper-bound constraint.

**Required tests.** Duplicate receipts, altered face, stale snapshot, split top-ups, cross-portfolio
substitution, premature close, terminal close, and either asset/episode reuse rejection or explicit
generation binding.
**Verification:** P, F, I

### INV-069 - Terminal normalization and retirement

**Statement.** An economically empty account or asset can normalize inert indices and historical
audit counters and enter its terminal state. Historical insurance-spend counters, price-only K
movement, expired labels, default-only fields, or old epochs cannot permanently block retirement,
while real obligations cannot be erased by normalization.

**Required tests.** Empty-but-cranked asset, previously consumed insurance, reset history, and
nonempty-obligation controls.
**Verification:** P, F, I, R

### INV-070 - Zero unattributed terminal residue and CloseSlab

**Statement.** Once all portfolios and claims are terminal:

```text
user_claimable_value   = 0
live_encumbrances      = 0
unresolved_loss        = 0
pending_receipts       = 0
unexplained_accounting = 0
```

Any remaining vault value is exactly classified as protocol surplus or rounding residue and can be
swept through an explicit balanced transition. After that sweep, `CloseSlab` succeeds and leaves
only the exact typed market tombstone and its rent when strict address non-reuse is configured.

**Required tests.** Full-market lifecycle sequences with adversarial order, rounding, recovery, and
prior insurance consumption.
**Verification:** P, F, I, R

### INV-071 - Crank progress

**Statement.** Every actionable live or wind-down state has a permissionless successful crank or
continuation that strictly decreases a finite lexicographic liveness rank, net of explicitly bounded
close drift. A successful no-op crank is forbidden.

**Required tests.** Record rank before and after every successful crank; fuzzers fail on nondecrease
unless the transition enters a strictly lower-priority terminal mode.
**Verification:** P, F, R

### INV-072 - Order-robust crankability

**Statement.** Hints are discovery only. Stale, duplicated, omitted, adversarially ordered, or
partially valid hints may be ignored or reclassified from current state; they cannot prevent an
honest caller from discovering and executing a progressing action.

**Required tests.** Mutate hint sets and order while holding state fixed; canonical progress must
remain discoverable.
**Verification:** F, I, M, R

### INV-073 - No permanent user lock

**Statement.** From every publicly reachable funded state, under explicit assumptions about
authenticated time and oracle/recovery availability, a finite public sequence returns senior
capital, settles the account into a terminal receipt, or applies an explicitly authorized
junior-value forfeit.

**Required tests.** Exhaustive small-state reachability plus long stateful fuzz sequences; every
nonterminal funded state must have a path to terminal disposition.
**Verification:** F, I, R

### INV-074 - Scope locality

**Statement.** An asset-, side-, portfolio-, domain-, close-, or receipt-scoped field may affect
only operations in that scope. A field used as a global lock must be a complete and correctly
maintained global summary, not the state of the last touched asset.

**Required tests.** Make one asset stale, locked, resetting, or recovering and assert unrelated
domains remain usable except where a documented complete global invariant requires a lock.
**Verification:** P, F, I

### INV-075 - Exclusive close ownership and episode integrity

**Statement.** Close ownership is deterministic and exclusive: at most one active close may hold a
domain and at most one active close may belong to an account. A contender for an occupied domain
rejects before mutation; each close holds only one domain, so hold-and-wait cycles are impossible.
`close_id` is strictly monotonic per account, while market/portfolio generation, drift anchor,
gross loss, and maximum close slot remain immutable throughout the episode. An expired close routes
to Recovery instead of retaining its domain. This is the v16.9.1 exclusive-serialization model; it
supersedes the earlier unimplemented priority/preemption proposal.

**Required tests.** Competing closers in both landing orders, same-domain exclusion, different-domain
coexistence, exact rejected-contender rollback, stale continuations, expiry-to-Recovery,
cure-and-cancel, canceled/finalized replay, and owner deposit interleavings.
**Verification:** P, F, I, R

### INV-076 - Close drift, residual durability, and finalization atomicity

**Statement.** Adverse drift is measured from the immutable close anchor and bounded by a funded
reserve or recovery. Basis, OI, PnL, and side weight are not freed until residuals are durably
booked/backed/assigned. Quantity ADL, exposure clear, and ledger advancement are atomic or protected
by a nonpreemptible finalization barrier.

**Required tests.** Inject failure at every close phase, advance price/funding, submit competing
close attempts, restart, and verify no double booking or orphan exposure.
**Verification:** P, F, I

### INV-077 - Bounded work and maximum-shape compute

**Statement.** Every public instruction has statically shape-bounded loops and measured compute
below the transaction limit at maximum supported N, bucket count, domain count, proof-account count,
and hint count. No attacker-controlled append or collection makes a required exit, close,
maintenance, or recovery path unexecutable.

**Required tests.** Initialize maximum supported shapes, measure CU for every public route, and
reject unsupported shapes before activation.
**Verification:** I, C

### INV-078 - Permissionless recovery coverage

**Statement.** Every state where ordinary bounded progress cannot continue has a permissionless
terminal recovery, dead-leg forfeit, exact receipt, or other senior-preserving terminal path. No
privileged actor is required to release user capital.

**Required tests.** Trigger each documented failure class: stale/unavailable oracle, B exhaustion,
backing failure, lien impairment, close expiry, domain lock, insurance exhaustion, payout conflict,
and lifecycle failure.
**Verification:** F, I, R

### INV-079 - Public reachability evidence

**Statement.** A loss-of-funds or persistent-DoS result is accepted only if it reproduces from
public instructions and valid account construction without mutating program-owned bytes out of
band, and terminates in exact SPL/lamport loss, unauthorized withdrawable value, or a persistent
exit lock.

**Required tests.** Every regression PoC records the full public instruction trace, signers,
accounts, pre/post token balances, and terminal liveness state.
**Verification:** I, R

---

## 6. Atomic execution, proof fidelity, and test validity

### INV-080 - Error propagation and exact rollback

**Statement.** Every nonzero engine result becomes an instruction error. On every error path, no
program-owned account bytes, SPL balances, lamports, or other persistent effects commit. Logs and
return data are non-authoritative; no later economic action may treat an errored invocation as
success. SVM rollback is relied upon only when an actual error is returned.

**Verification boundary.** Exact rollback after a returned instruction error is an SVM semantic
assumption, not a property reimplemented by this wrapper. The wrapper proof obligation is complete
error propagation: every engine error maps to a nonzero instruction error, every exceptional
disposition has a named safe-success postcondition, every dispatcher arm returns its handler result,
and each deployed entrypoint preserves that result.

**Required tests.** Prove the complete engine-error mapping and source-lock every exceptional
disposition and public dispatch/entrypoint edge. Use representative late-failure integration tests
to confirm the SVM assumption across program bytes, SPL/lamport effects, CPI return consumers, and
multi-instruction transactions; do not duplicate platform rollback semantics at every `?` site.
**Verification:** P, F, I

### INV-081 - Success-state validity over complete public routes

**Statement.** For every public wrapper instruction over the deployed state representation:

```text
Ok(S, instruction) = S'
    implies GlobalInvariant(S')
         and AuthorizedDelta(S, instruction, S')
```

The proof covers the wrapper plus engine composition, not only leaf kernels.

**Required tests.** Assert the full invariant suite after every successful fuzz step and prove
high-risk composite routes directly or through a proven-equivalent pure transition.
**Verification:** P, F, I

### INV-082 - State-indexed liveness theorem

**Statement.** For every publicly reachable nonterminal state in each lifecycle mode, either a
terminal condition already holds or there exists a constructible bounded public action that
decreases the mode-specific rank. The theorem quantifies over reachable states rather than assuming
a hand-built advancing state.

**Required tests.** Exhaustive bounded state graph plus a proof of rank decrease for each abstract
transition and mode.
**Verification:** P, R

### INV-083 - Boundary completeness

**Statement.** Proofs and fuzz generators explicitly cover zero, one, maximum, maximum-minus-one,
equal-slot expiry, one-slot-before/after expiry, full-width magnitudes, cross-zero positions,
empty/full buckets, and near-overflow products. Any assumption excluding such values has a separate
proof that the excluded state is publicly unreachable.

**Required tests.** Maintain a machine-checked boundary corpus and coverage report for every
arithmetic and lifecycle field.
**Verification:** P, F, I

### INV-084 - Proof assumptions are reachable and nonvacuous

**Statement.** Every proof precondition is either established by the preceding public route or
separately proven for every publicly reachable state entering the function. No `assume` silently
excludes the exploit class or makes the postcondition vacuous.

**Required tests.** Produce assumption-coverage witnesses, mutation-test assumptions, and prove at
least one reachable model satisfies each nontrivial harness.
**Verification:** P, R

### INV-085 - Proven arithmetic equals deployed arithmetic

**Statement.** Formal proofs execute the same wide-integer implementation that ships, or a formal
equivalence theorem connects the proof and deployed representations. Carry, borrow,
multiplication, division, and scale conversions are covered over adversarial boundary partitions.

**Required tests.** Differential full-boundary corpus between Kani representation, host
representation, BPF representation, and a big-integer oracle.
**Verification:** P, F, M

### INV-086 - Reference-model and deployed-transition equivalence

**Statement.** A small, clear reference model for identity, balances, OI, source credit, liens,
payout, and lifecycle produces the same normalized state delta as the deployed implementation for
all generated bounded sequences. Divergence is a failure even when both states pass local shape
checks.

**Required tests.** Stateful differential fuzzing with shrinking to a minimal public trace.
**Verification:** F, I, M, R

### INV-087 - No phantom controls or dead security fields

**Statement.** Every persisted security field and configured control has a writer, an enforcement
read, and a test witnessing its effect, or is removed. Default-only locks, cooldowns, escrow fields,
or counters cannot be mistaken for active protection.

**Required tests.** Static read/write inventory plus mutation tests that flip each control and
observe the intended admission or accounting change.
**Verification:** P, F, I

### INV-088 - Global summaries are not account-local proofs

**Statement.** A market/global accumulator or "last touched" summary cannot substitute for an
account-, asset-, or domain-local proof unless it is independently proven complete for that scope
and updated on every relevant transition.

**Required tests.** Touch unrelated assets in adversarial order and compare local decisions against
an independent full recomputation.
**Verification:** P, F, M

### INV-089 - Activation, reactivation, and initialization equivalence

**Statement.** Fresh activation and retired-slot reactivation apply the same nonzero-authority
checks, generation increments, full recovery/price/rate envelope validation, zero-state
requirements, configured hard bounds, `support_weight == FULL_SUPPORT_WEIGHT` for Active assets,
fresh source-credit ledgers, fresh per-generation replay watermarks, and certificate invalidation.
A reactivation route cannot create a state forbidden by initial activation. A prior generation's
maximum sequence value cannot poison any retained-operation lane in the replacement generation.

**Required tests.** Differentially compare fresh activation and reuse with identical intended
configuration; inject zero authorities, residual state, stale epochs, exhausted replay watermarks,
and unsupported N.
**Verification:** P, F, I, M

---

## 7. Required verification architecture

### 7.1 Layer 1 - Leaf arithmetic and ledger proofs

Use Kani or equivalent for checked arithmetic, exact delta equations, disjoint classifications, and
local state transitions. These proofs should use full-width boundary partitions and the deployed
wide-math implementation or a proven-equivalent representation.

Leaf proofs are necessary but do not establish whole-route safety.

### 7.2 Layer 2 - Composite public-transition proofs

Prioritize proofs over the actual public state structs for:

1. single trade and batch trade;
2. CPI and no-CPI trade routes;
3. partial liquidation followed by trade or rebalance;
4. cross-zero trade;
5. unilateral reduce and resolved clear;
6. full account recertification and fast-path equivalence;
7. backing expiry followed by close or claim;
8. insurance withdrawal through every route;
9. resolve, receipt creation, top-up, and claim close;
10. bankrupt close continuation, exclusive contention, recovery, and finalization;
11. asset retirement and slot reuse;
12. portfolio close and same-pubkey recreation.

Where direct wrapper proof is too expensive, extract one pure typed transition and prove both:

```text
wrapper_validation_and_unpacking(input_accounts, bytes)
    == typed_transition_inputs

deployed_post_state
    == pure_transition(pre_state, typed_transition_inputs)
```

### 7.3 Layer 3 - Stateful model-based fuzzing

The fuzzer should generate long sequences containing:

- market, asset, portfolio, authority, position, recovery, claim, and receipt generations;
- account close/recreate ABA;
- authority `A -> B -> A` rotations;
- retained intent replay, partial fills, expiry, and route switching;
- deposits, withdrawals, trades, batch trades, CPI/no-CPI variants;
- self-controlled counterparties;
- oracle target and effective-price lag;
- zero and extreme execution prices;
- partial liquidation, ADL, cross-zero, rebalance, and unilateral reduction;
- backing add, consume, release, impairment, and exact expiry boundaries;
- insurance reservation, spend, impairment, and every withdrawal route;
- close start, exclusive contention, continuation, cure-and-cancel, and fault injection;
- reset, recovery, resolve, exact receipts, top-ups, claims, forfeits, retirement, and `CloseSlab`;
- maximum N, bucket count, domain count, and hint count.

After every successful instruction, recompute all invariants from raw state. After every failed
instruction, compare the complete state snapshot and external balances for exact equality.

The generator must be finding-agnostic. PR-specific `reproduce_*` adapters remain useful direct
regressions but do not satisfy the independent-discovery completion gate.

### 7.4 Layer 4 - Metamorphic test pairs

Every applicable operation should be tested as paired executions from the same snapshot:

```text
single fill             vs split fills
CPI                     vs no-CPI
single instruction      vs batch
original order          vs relevant permutations
first execution         vs replay
A authority             vs A -> B -> A
old account             vs close/recreate same pubkey
old asset               vs retire/reuse same asset_index
position episode e      vs close/reopen episode e+1
expiry - 1              vs expiry vs expiry + 1
raw price 0             vs 1 vs MAX
normal close            vs contended/restarted close
one withdrawal route    vs all alternate routes
fast certificate        vs full recomputation
proof U256              vs deployed U256 vs bigint oracle
```

### 7.5 Layer 5 - Exhaustive bounded reachability

Build a small abstract model with at least:

- two assets;
- both sides per asset;
- three or four portfolios;
- two authority keys plus disabled state;
- two generations per market/asset/portfolio/position;
- small exact integer ranges including zero and maxima;
- all public lifecycle modes and public actions.

Enumerate all reachable states with BFS or equivalent. For every reachable funded nonterminal state,
check that a bounded public action decreases the liveness rank or creates a terminal receipt/forfeit
outcome. This is the primary counterexample engine for INV-057, INV-071 through INV-078, and
INV-082.

### 7.6 Layer 6 - SVM integration and compute tests

Run LiteSVM, Mollusk, or equivalent tests using actual account metas, signer/writable flags, PDA
derivation, CPI return data, SPL transfers, lamports, account close/recreate, transaction rollback,
and compute metering. Engine-only tests cannot establish wrapper authorization or external-value
correctness.

---

## 8. Highest-priority regression harnesses

The following should exist as named whole-route regressions before broader fuzz coverage is
considered mature:

1. `market_generation_replay_rejected_after_same_pubkey_recreate`
2. `asset_slot_reuse_rejects_old_market_and_asset_generation_intent`
3. `portfolio_recreate_rejects_old_portfolio_id_intent`
4. `position_close_reopen_rejects_old_episode_reduction_or_forfeit`
5. `authority_a_b_a_rotation_never_restores_old_request`
6. `same_intent_executes_economically_at_most_once_across_all_routes`
7. `all_trade_routes_preserve_full_health_oi_and_signed_bounds`
8. `cross_zero_after_partial_liquidation_cannot_use_unrelated_oi`
9. `split_trade_cannot_exceed_adl_effective_transferable_quantity`
10. `incremental_certificate_never_healthier_than_full_recompute`
11. `rebalance_reduce_preserves_portfolio_and_market_oi_equations`
12. `resolved_clear_uses_canonical_adl_effective_quantity`
13. `zero_and_extreme_exec_prices_cannot_escape_mark_envelope`
14. `unsafe_raw_price_cannot_block_all_owner_exit_paths`
15. `backing_expiry_is_normalized_before_add_close_claim_and_payout`
16. `resolved_claim_permutations_preserve_snapshot_fairness_and_stock`
17. `every_insurance_withdraw_route_shares_one_cap_and_cooldown`
18. `parasitic_zero_activity_asset_receives_no_account_level_fee`
19. `split_liquidations_do_not_multiply_minimum_fee`
20. `reset_pending_side_rejects_new_risk`
21. `empty_asset_with_price_only_k_state_can_retire_safely`
22. `historical_insurance_spend_does_not_permanently_block_retirement`
23. `reactivated_asset_rejects_zero_authorities_and_increments_generation`
24. `every_reachable_resolved_account_has_close_or_terminal_receipt_path`
25. `every_reachable_funded_state_has_public_exit_or_rank_decreasing_action`
26. `all_error_paths_restore_program_bytes_tokens_and_lamports_exactly`
27. `proof_and_deployed_u256_match_bigint_on_boundary_partition`
28. `maximum_supported_state_keeps_every_required_exit_below_cu_limit`
29. `stale_or_adversarial_hints_cannot_prevent_canonical_progress`
30. `close_contention_restart_cannot_double_book_residual_or_free_exposure_early`

---

## 9. Audit-finding traceability

| Audit item | Primary invariants |
| --- | --- |
| ADV-00 Zero-price EWMA mark manipulation | INV-019, INV-045, INV-046, INV-047, INV-083 |
| ADV-01 Same-side legs after partial liquidation | INV-048, INV-050, INV-051, INV-081 |
| ADV-02 Resolved payout fairness manipulation | INV-029, INV-038, INV-066, INV-067, INV-068 |
| ADV-03 Oracle lag omitted in recertification | INV-053, INV-054, INV-056, INV-081 |
| ADV-04 Split trades bypass ADL-effective limits | INV-051, INV-052, INV-058 |
| ADV-05 Rebalance OI mismatch | INV-048, INV-051, INV-076 |
| ADV-06 Parasitic assets siphon fees / scale work | INV-036, INV-077, INV-089 |
| ADV-07 Trading during ResetPending | INV-055, INV-065 |
| ADV-08 Expired Fresh bucket blocks close | INV-063, INV-073, INV-078 |
| ADV-09 Global stale flag blocks unrelated domain | INV-074, INV-088 |
| ADV-10 Resolved clear ignores ADL size | INV-051, INV-069, INV-073 |
| ADV-11 Domain insurance withdrawal bypass | INV-014, INV-064 |
| ADV-12 Historical insurance spend blocks retirement | INV-069, INV-070 |
| ADV-13 Split liquidation minimum fees | INV-052, INV-059, INV-061 |
| ADV-14 Price-only K state blocks retirement | INV-044, INV-069 |
| ADV-15 Slot reuse accepts zero authorities | INV-002, INV-005, INV-089 |
| SUG-00 Inert insurance cooldown | INV-064, INV-087 |
| SUG-01 Raw mode/lifecycle literals | INV-015, INV-055, INV-087 |
| SUG-02 Caller-supplied slot fallback | INV-020 |
| SUG-03 Dropped social-loss remainder | INV-025, INV-038 |
| SUG-04 Unused exported constants | INV-087 |
| SUG-05 Fields read but never written | INV-087 |
| FV-00 Leaf-only proofs | INV-081 and section 7.2 |
| FV-01 Bounded abstract values hide edges | INV-083, INV-084 |
| FV-02 Concrete-only rate/rounding composition | INV-029, INV-038, INV-066 |
| FV-03 Proven arithmetic differs from deployed | INV-085, INV-086 |
| FV-04 No state-indexed reachability invariant | INV-071 through INV-078, INV-082 |

Every open public-route LoF/DoS PR must also appear in the pinned known-finding benchmark described
in [`tests/invariants/README.md`](tests/invariants/README.md). That benchmark is a completeness
oracle for the test system, not evidence that each PR's proposed fix is correct.

---

## 10. Completion criteria

The program is not considered verification-complete merely because all unit tests or leaf proofs
pass. Minimum completion requires:

1. Every invariant above has at least one executable test or proof with an owner and CI target.
2. Every public instruction is covered by success-postcondition and exact-error-rollback checks.
3. The identity/generation invariants are enforced in the signed-message format and public handlers,
   not only in clients.
4. All high-risk route pairs have metamorphic equivalence tests.
5. Stateful fuzzing has stable invariant checking after every successful step and shrinking to
   public traces.
6. A bounded reachability model finds no funded nonterminal state without a public terminal or
   rank-decreasing action.
7. Maximum supported shapes are measured below the compute ceiling for every required exit and
   recovery route.
8. The proof arithmetic is the deployed arithmetic or is formally equivalent to it.
9. All OtterSec advisories have a public-route regression tied to the invariant matrix above.
10. CI stores the exact program commit, wrapper commit, feature flags, account-layout version,
    proof-tool version, fuzz seed corpus, and compute-budget assumptions used for the result.
11. Every open public-route LoF/DoS finding in the pinned benchmark is independently rediscovered by
    a finding-agnostic public-sequence generator and normative invariant oracle on its vulnerable
    baseline. A copied or PR-specific regression is not sufficient for this criterion.
12. Each independently rediscovered finding has a fixed-pin test proving the same public trace no
    longer violates the invariant, while preserving required user exit and crank progress.

---

## 11. Source alignment

This plan is aligned with the v16.8.0 source-of-truth requirements for protected principal,
source-domain realizability, exact lien and insurance lifecycle, quote-value and encumbrance proofs,
stock reconciliation, explicit rounding residue, local B domains, close priority, pending
obligations, recovery envelopes, instance isolation, bounded account-local work, and permissionless
forward progress.

The identity/generation rules in INV-001 through INV-005 and the retained-intent message schema are
additional requirements requested for replay and ABA safety; they should be incorporated into the
program specification if adopted.
