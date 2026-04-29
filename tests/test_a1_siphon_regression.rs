//! A1 / PR39-F7 self-dealing insurance-siphon regression tests.
//!
//! # Attack
//!
//! The attacker controls two distinct keypairs on the same market:
//!   • A: a user account, opens a LONG
//!   • B: an LP account, takes the matched SHORT via the default matcher
//!
//! Both sides are filled at the same entry price from the same wallet
//! cluster. The attacker then drops the oracle price sharply. Under a
//! waterfall that over-pays the LP from insurance (§4.17 pre-v12.19),
//! the LP-side profit is funded by insurance while the user-side loss
//! never propagates cleanly, so the aggregate `(A_cap + A_pnl) +
//! (B_cap + B_pnl)` exceeds the pair's combined deposits — and the
//! surplus came out of the insurance fund.
//!
//! This is the regression surface described by PR39/F7 ("insurance
//! teleport via residual inflation in self-trade liquidation"). The
//! engine intentionally keeps insurance absorption in junior residual
//! for honest two-party loss sharing, so the wrapper-level property is
//! not "residual never grows." It is: a self-dealing matched pair must
//! not convert that residual into attacker profit or a meaningful
//! insurance-fund drain.
//!
//! # Defense (v12.19)
//!
//! Three layers neuter the attack:
//!
//! 1. `max_price_move_bps_per_slot` — the per-slot oracle-move cap is an
//!    immutable init-time RiskParam. A single-instruction 25% gap is
//!    rejected by `accrue_market_to`, so the attacker can't realize an
//!    adverse move in one shot.
//! 2. §1.4 solvency envelope — `max_price_move * max_accrual_dt +
//!    funding_contribution + liquidation_fee <= maintenance_margin`.
//!    Even across many cranks the worst-case pre-liquidation drift on
//!    LP collateral is bounded by maintenance margin, so insurance never
//!    has to backstop a zero-collateral unwind.
//! 3. Admission-threshold gate — `admit_h_max_consumption_threshold_bps`
//!    (spec §12.21, plumbed through every `_v2`-now-canonical call) stops
//!    a generation with a depleted price-move budget from admitting new
//!    positions or ADL enqueues.
//!
//! # Acceptance criterion
//!
//! For each market type exercised below, the test asserts:
//!
//! ```text
//! attacker_delta  =  (A_cap + A_pnl) + (B_cap + B_pnl)
//!                 -  (A_deposit + B_deposit)
//!                 <=  ROUNDING_TOLERANCE
//!
//! insurance_after  >=  insurance_before - INSURANCE_DROP_TOLERANCE
//! ```
//!
//! A violation (attacker extracts value, or insurance shrinks by more
//! than rounding) would indicate a regression of the v12.19 defense —
//! DO NOT paper over it.

mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::signature::{Keypair, Signer};

/// Shared summary of the attacker's end-state for ≤-0 assertions.
struct AttackOutcome {
    attacker_a_deposit: u128,
    attacker_b_deposit: u128,
    attacker_a_equity: i128,
    attacker_b_equity: i128,
    insurance_before: u128,
    insurance_after: u128,
}

impl AttackOutcome {
    fn attacker_delta(&self) -> i128 {
        let combined_deposit = self.attacker_a_deposit as i128 + self.attacker_b_deposit as i128;
        self.attacker_a_equity + self.attacker_b_equity - combined_deposit
    }

    fn assert_defended(&self, label: &str) {
        println!("--- {} ---", label);
        println!(
            "  attacker_a: deposit={} equity={}",
            self.attacker_a_deposit, self.attacker_a_equity
        );
        println!(
            "  attacker_b: deposit={} equity={}",
            self.attacker_b_deposit, self.attacker_b_equity
        );
        println!(
            "  insurance: before={} after={}",
            self.insurance_before, self.insurance_after
        );
        let delta = self.attacker_delta();
        println!("  attacker_delta = {}", delta);

        // Pre-v12.19 the A1 attack extracted the entire insurance balance —
        // ~5_000_000_000 units in this fixture. Post-v12.19 the attacker's
        // combined equity may drift by a few hundred integer units from
        // floor-rounding inside the accrual walk (10+ crank round-trips ×
        // sub-unit residuals). Tolerate up to 10_000 units (~2.5e-7 of the
        // pair's combined 40B deposit) — still seven orders of magnitude
        // below any meaningful siphon.
        const ROUNDING_TOLERANCE: i128 = 10_000;
        assert!(
            delta <= ROUNDING_TOLERANCE,
            "{}: attacker extracted {} (> rounding tolerance {}) — v12.19 A1 defense regressed",
            label,
            delta,
            ROUNDING_TOLERANCE,
        );
        // Insurance is allowed to ABSORB LP-side losses (shrink) if the
        // attacker's own capital is consumed. What it must NOT do is pay
        // the attacker: if it shrinks, the shrinkage must be covered by
        // the attacker's combined loss (delta <= 0), which the previous
        // assertion already enforces.
        //
        // Stronger invariant: on a well-defended market the LP's own
        // capital absorbs the matched-pair loss before insurance is
        // touched. The attacker_delta <= 1 check alone is sufficient to
        // rule out an insurance siphon.
        let ins_drop = self.insurance_before.saturating_sub(self.insurance_after);
        println!("  insurance drop = {}", ins_drop);
        // Strongest check: if insurance dropped at all, the drop must be
        // within rounding tolerance. Any meaningful drop (≫ tolerance)
        // means insurance was paying the attacker.
        const INSURANCE_DROP_TOLERANCE: u128 = 10_000;
        assert!(
            ins_drop <= INSURANCE_DROP_TOLERANCE,
            "{}: insurance dropped by {} (> tolerance {}) — v12.19 A1 defense regressed",
            label,
            ins_drop,
            INSURANCE_DROP_TOLERANCE,
        );
    }
}

/// A1a: external Pyth oracle, non-Hyperp market.
///
/// Attack vector: attacker_A opens a long; attacker_B (an LP controlled
/// by a second keypair) takes the matched short via the default
/// `try_trade` helper (which calls TradeNoCpi under the hood — the LP
/// owner signs since we use non-TradeCpi here). Attacker then drops the
/// external Pyth price by ~25%. Cranks run multiple times. Under
/// v12.19, the per-slot cap + envelope ensure the engine never lets the
/// adverse price land fully in one accrual step, and the §1.4 envelope
/// + admission threshold block insurance from subsidizing the LP side.
#[test]
fn test_a1_external_pyth_siphon_defended() {
    let mut env = TestEnv::new();
    // Non-Hyperp with external Pyth oracle. init_market_with_invert uses
    // TEST_MAX_PRICE_MOVE_BPS_PER_SLOT = 4 and perm_resolve = 10_000.
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 5_000_000_000);

    let attacker_a = Keypair::new();
    let a_idx = env.init_user(&attacker_a);
    let deposit_a: u64 = 20_000_000_000;
    env.deposit(&attacker_a, a_idx, deposit_a);

    let attacker_b = Keypair::new();
    let b_idx = env.init_lp(&attacker_b);
    let deposit_b: u64 = 20_000_000_000;
    env.deposit(&attacker_b, b_idx, deposit_b);

    env.crank();
    let insurance_before = env.read_insurance_balance();

    // Open matched pair at entry price. Size = 1_000_000 (POS_SCALE units).
    let pair_size: i128 = 1_000_000;
    env.trade(&attacker_a, &attacker_b, b_idx, a_idx, pair_size);

    // Drive the oracle adversarially ~25% down relative to the 138M
    // baseline over a horizon the set_slot_and_price helper can walk
    // inside the §1.4 envelope. Target: 138M * 0.75 = 103.5M.
    let adverse_px: i64 = 103_500_000;
    let target_slot: u64 = 2_500; // well past MAX_ACCRUAL_DT_SLOTS; helper chunks.
    env.set_slot_and_price(target_slot, adverse_px);

    // Run several cranks to let the engine flush accrual + any
    // lifecycle work. Each crank is bounded by the envelope, and the
    // admission-threshold gate prevents ADL enqueues from admitting
    // fresh reserve against an exhausted generation.
    for _ in 0..5 {
        let _ = env.try_crank();
    }

    let cap_a = env.read_account_capital(a_idx);
    let cap_b = env.read_account_capital(b_idx);
    let pnl_a = env.read_account_pnl(a_idx);
    let pnl_b = env.read_account_pnl(b_idx);
    let insurance_after = env.read_insurance_balance();

    let outcome = AttackOutcome {
        attacker_a_deposit: deposit_a as u128,
        attacker_b_deposit: deposit_b as u128,
        attacker_a_equity: cap_a as i128 + pnl_a,
        attacker_b_equity: cap_b as i128 + pnl_b,
        insurance_before,
        insurance_after,
    };
    outcome.assert_defended("PR39/F7 A1a external-Pyth");
}

/// A1a gap variant: the oracle jumps between keeper turns.
///
/// This models the speed requirement directly: the attacker opens a
/// near-margin matched pair, then publishes a raw 25% adverse oracle move
/// at a later slot without walking the intermediate slots or giving the
/// keeper a crank in between. The market is configured with the maximum
/// currently-allowed non-Hyperp permissionless-stale window (100 slots),
/// so a 99-slot gap is live but still bounded by the init-time
/// `max_price_move_bps_per_slot * dt` cap.
#[test]
fn test_a1_external_pyth_raw_gap_move_defended() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 5_000_000_000);

    // Put the market at a known fresh baseline before creating exposure.
    // This helper may walk/crank, but there are no open positions yet.
    env.set_slot_and_price(100, 138_000_000);
    env.crank();

    let attacker_a = Keypair::new();
    let a_idx = env.init_user(&attacker_a);
    let deposit_a: u64 = 16_000_000_000;
    env.deposit(&attacker_a, a_idx, deposit_a);

    let attacker_b = Keypair::new();
    let b_idx = env.init_lp(&attacker_b);
    let deposit_b: u64 = 20_000_000_000;
    env.deposit(&attacker_b, b_idx, deposit_b);

    env.crank();
    let insurance_before = env.read_insurance_balance();

    // At 138.0 e6, this is near initial margin for attacker_a but
    // still admitted. A raw 25% target move would be far worse than
    // maintenance if it could land in one step.
    let pair_size: i128 = 1_000_000_000;
    env.trade(&attacker_a, &attacker_b, b_idx, a_idx, pair_size);

    let base_slot = env.svm.get_sysvar::<Clock>().slot;
    let gap_slot = base_slot + 99;
    let adverse_px: i64 = 103_500_000; // 138M * 0.75

    // Critical adversarial step: no price walking and no crank between
    // base_slot and gap_slot.
    env.set_slot_and_price_raw_no_walk(gap_slot, adverse_px);

    let mut ok_cranks = 0u64;
    let mut rejected_cranks = 0u64;
    for step in 0..10u64 {
        // After the initial gap, keep publishing the adverse target as
        // cranks resume. A vulnerable wrapper would have let the first
        // post-gap crank realize the full target and overpay the LP from
        // insurance. The defended path only feeds capped effective prices.
        env.set_slot_and_price_raw_no_walk(gap_slot + step, adverse_px);
        match env.try_crank() {
            Ok(()) => ok_cranks += 1,
            Err(err) => {
                rejected_cranks += 1;
                println!("post-gap crank step {} rejected: {}", step, err);
            }
        }
    }
    assert!(
        ok_cranks > 0,
        "expected at least one live post-gap crank; rejected={}",
        rejected_cranks
    );

    let cap_a = env.read_account_capital(a_idx);
    let cap_b = env.read_account_capital(b_idx);
    let pnl_a = env.read_account_pnl(a_idx);
    let pnl_b = env.read_account_pnl(b_idx);
    let insurance_after = env.read_insurance_balance();

    let outcome = AttackOutcome {
        attacker_a_deposit: deposit_a as u128,
        attacker_b_deposit: deposit_b as u128,
        attacker_a_equity: cap_a as i128 + pnl_a,
        attacker_b_equity: cap_b as i128 + pnl_b,
        insurance_before,
        insurance_after,
    };
    outcome.assert_defended("PR39/F7 A1a external-Pyth raw no-walk gap");
}

/// A1b: Hyperp (internal mark) market.
///
/// On a Hyperp market, `TradeNoCpi` is explicitly disabled
/// (`HyperpTradeNoCpiDisabled`, error 0x1b) — positions can only be
/// opened through `TradeCpi` with a registered matcher program. That
/// makes the classic dual-keypair matched-pair setup impossible on
/// Hyperp without the external `matcher_program.so` binary (see A1c).
///
/// What we *can* verify here is the closest independent surface: the
/// hyperp mark-push authority cannot siphon insurance even when it
/// adversely pushes the mark by ~25%, because every push is rate-
/// limited by the engine's mark-smoothing cap
/// (`clamp_toward_with_dt`, §hyperp index smoothing) and every crank
/// runs through the same §1.4 solvency envelope as the non-Hyperp
/// path. Without any open positions, the insurance balance is
/// trivially unaffected — the real invariant being verified is that a
/// succession of rate-limited mark pushes does not *create* an
/// insurance-draining path via any lazy-settle side-effect.
#[test]
fn test_a1_hyperp_mark_siphon_defended() {
    let mut env = TestEnv::new();
    env.init_market_hyperp(1_000_000);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set hyperp mark authority");
    env.try_push_oracle_price(&admin, 1_000_000, 100)
        .expect("seed hyperp mark");

    env.top_up_insurance(&admin, 5_000_000_000);

    let attacker_a = Keypair::new();
    let a_idx = env.init_user(&attacker_a);
    let deposit_a: u64 = 20_000_000_000;
    env.deposit(&attacker_a, a_idx, deposit_a);

    let attacker_b = Keypair::new();
    let b_idx = env.init_lp(&attacker_b);
    let deposit_b: u64 = 20_000_000_000;
    env.deposit(&attacker_b, b_idx, deposit_b);

    env.crank();
    let insurance_before = env.read_insurance_balance();

    // NOTE: no trade — TradeNoCpi is blocked on Hyperp, and TradeCpi needs
    // an external matcher program (A1c). Exercise the authority-only flavor.

    // Push the hyperp mark ~25% adverse over many small steps; each push
    // is rate-limited by the engine's mark-smoothing cap. Some steps will
    // be rejected by the rate limiter — tolerate with let _ = ....
    let steps = 50;
    let start_px: i64 = 1_000_000;
    let end_px: i64 = 750_000; // -25%
    for i in 1..=steps {
        let slot = 200 + (i * 20) as u64;
        let px = start_px + (end_px - start_px) * i / steps;
        env.set_slot(slot);
        let _ = env.try_push_oracle_price(&admin, px as u64, slot as i64);
        let _ = env.try_crank();
    }

    for _ in 0..5 {
        let _ = env.try_crank();
    }

    let cap_a = env.read_account_capital(a_idx);
    let cap_b = env.read_account_capital(b_idx);
    let pnl_a = env.read_account_pnl(a_idx);
    let pnl_b = env.read_account_pnl(b_idx);
    let insurance_after = env.read_insurance_balance();

    let outcome = AttackOutcome {
        attacker_a_deposit: deposit_a as u128,
        attacker_b_deposit: deposit_b as u128,
        attacker_a_equity: cap_a as i128 + pnl_a,
        attacker_b_equity: cap_b as i128 + pnl_b,
        insurance_before,
        insurance_after,
    };
    outcome.assert_defended("PR39/F7 A1b Hyperp mark-push (authority-only)");
}

/// A1c: TradeCpi (matcher-routed) — dual-keypair self-dealing attack.
///
/// Exercises the same self-dealing matched-pair setup as A1a but routes
/// the trade through `TradeCpi` with the registered default matcher.
/// This is the path where the pre-v12.19 attack had its fullest
/// expression (the attacker could, in principle, pair any LP they own
/// with any user they own against a tame matcher). v12.19's defenses
/// (max_price_move_bps_per_slot, §1.4 envelope, admission threshold)
/// apply identically across TradeCpi and TradeNoCpi — the crank pipeline
/// is the same once a trade is admitted.
///
/// The TradeCpi-specific defenses (ABI echo check, matcher identity
/// binding, LP PDA shape, nonce discipline, oracle-price echo) are
/// exhaustively exercised by `tests/test_tradecpi.rs`. What remains
/// specific to A1 is the `attacker_delta <= ROUNDING_TOLERANCE` +
/// `insurance_drop <= ROUNDING_TOLERANCE` pair, which we assert here
/// after an adverse oracle drop against a matched-pair position opened
/// via TradeCpi.
#[test]
fn test_a1_tradecpi_siphon_defended() {
    let mut env = TradeCpiTestEnv::new();
    // Use Hyperp mode so the test can drive the oracle adversarially
    // via PushHyperpMark — TradeCpi is legal on Hyperp, so this covers
    // the same attack surface as A1c (attacker controlling both sides
    // of a matched pair through the registered matcher).
    env.init_market_hyperp(1_000_000);
    let matcher_prog = env.matcher_program_id;
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set hyperp mark authority");
    env.try_push_oracle_price(&admin, 1_000_000, 100)
        .expect("seed mark");

    // Attacker controls both the LP and the taker.
    let attacker_lp = Keypair::new();
    let (lp_idx, matcher_ctx) = env.init_lp_with_matcher(&attacker_lp, &matcher_prog);
    let deposit_lp: u64 = 20_000_000_000;
    env.deposit(&attacker_lp, lp_idx, deposit_lp);

    let attacker_user = Keypair::new();
    let user_idx = env.init_user(&attacker_user);
    let deposit_user: u64 = 20_000_000_000;
    env.deposit(&attacker_user, user_idx, deposit_user);

    env.top_up_insurance(&admin, 5_000_000_000);
    let insurance_before = env.read_insurance_balance();

    env.set_slot(100);
    env.crank();
    // Open the matched pair via TradeCpi (user long, LP short).
    let size: i128 = 1_000_000;
    env.try_trade_cpi(
        &attacker_user,
        &attacker_lp.pubkey(),
        lp_idx,
        user_idx,
        size,
        &matcher_prog,
        &matcher_ctx,
    )
    .expect("TradeCpi matched pair must admit under v12.19 envelope");

    // Adverse mark push ~25% down over many small steps. Each push is
    // clamped by the engine's mark-smoothing cap; cranks between pushes
    // walk funding/accrual without tripping §1.4.
    let steps = 50;
    let start_px: i64 = 1_000_000;
    let end_px: i64 = 750_000;
    for i in 1..=steps {
        let slot = 200 + (i * 20) as u64;
        let px = start_px + (end_px - start_px) * i / steps;
        env.set_slot(slot);
        let _ = env.try_push_oracle_price(&admin, px as u64, slot as i64);
        env.crank();
    }
    for _ in 0..5 {
        env.crank();
    }

    let cap_lp = env.read_account_capital(lp_idx);
    let cap_user = env.read_account_capital(user_idx);
    let pnl_lp = env.read_account_pnl(lp_idx);
    let pnl_user = env.read_account_pnl(user_idx);
    let insurance_after = env.read_insurance_balance();

    let outcome = AttackOutcome {
        attacker_a_deposit: deposit_user as u128,
        attacker_b_deposit: deposit_lp as u128,
        attacker_a_equity: cap_user as i128 + pnl_user,
        attacker_b_equity: cap_lp as i128 + pnl_lp,
        insurance_before,
        insurance_after,
    };
    outcome.assert_defended("PR39/F7 A1c TradeCpi matched-pair (Hyperp)");
}
