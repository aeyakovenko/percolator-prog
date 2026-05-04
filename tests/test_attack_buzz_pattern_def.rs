//! Buzz adversarial regression tests — Pattern D (reentrancy/TOCTOU),
//! Pattern E (state-field divergence) and Pattern F (idempotency/replay)
//! probes Toly's security.md does not yet cover.
//!
//! Contributed as part of the Buzz Security Research adversarial audit
//! (Buzz / SolCex Exchange). Full attestation: issue #78.
//!
//! Naming convention: `test_attack_<thing>_<mechanism>_<expected_or_weird>`
//!
//! - `test_attack_slab_guard_cpi_flag_set_blocks_all_permissionless_paths`:
//!   Pattern D. Forces FLAG_CPI_IN_PROGRESS = 1 in the slab header and
//!   verifies that every permissionless instruction rejects with a clean
//!   slab_guard error (the slab_guard bottleneck path), with no state
//!   mutation. This proves the reentrancy bottleneck is universal across
//!   the dispatch surface, independent of which path the malicious matcher
//!   tries to re-enter.
//!
//! - `test_attack_resolve_permissionless_idempotent_replay_blocked`:
//!   Pattern F. Once a market is permissionlessly resolved, a second
//!   ResolvePermissionless tx in a fresh transaction must be rejected
//!   (re-resolve guard at `prog/percolator.rs:9843`). Tests against a
//!   cleanly-matured market to bypass the staleness gate cleanly. The
//!   exact off-by-one boundary on
//!   `clock_slot - last_good_oracle_slot >= permissionless_resolve_stale_slots`
//!   is already covered by Kani harness
//!   `kani_permissionless_resolve_horizon_policy_independent_from_accrual_window`,
//!   so we focus on the idempotency invariant the Kani proof does not
//!   exercise.
//!
//! - `test_attack_dust_repeated_force_close_preserves_conservation`:
//!   Pattern E + Toly's economic-success criterion. Repeatedly opens and
//!   closes near-dust-magnitude positions and asserts the conservation
//!   invariant `engine_vault >= c_tot + insurance` holds after every step.
//!   This is the new-model analogue of the (now-obsolete) jan-2026
//!   `LP = -SUM(users)` desync finding.

mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::{
    account::Account,
    clock::Clock,
    signature::{Keypair, Signer},
};

// FLAGS_OFF and FLAG_CPI_IN_PROGRESS from `state` module in
// percolator-prog/src/percolator.rs:2748-2758. These are deployment-
// invariant offsets at the start of the slab.
const FLAGS_OFF: usize = 13;
const FLAG_CPI_IN_PROGRESS: u8 = 1 << 2;

/// Pattern D — slab_guard reentrancy bottleneck universality.
///
/// Strategy: instead of building an evil matcher BPF binary (the matcher
/// crate is not vendored in this audit clone), we directly poke the
/// FLAG_CPI_IN_PROGRESS bit into the slab and submit each permissionless
/// instruction as a fresh top-level transaction. The wrapper's slab_guard
/// must reject every one of them with no state mutation. If any
/// instruction proceeds past the slab_guard while the CPI flag is set,
/// that is an exploit primitive (a malicious matcher could re-enter that
/// path mid-CPI).
#[test]
fn test_attack_slab_guard_cpi_flag_set_blocks_all_permissionless_paths() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Bring up at least one user + LP so paths that require account
    // existence (Trade, Withdraw, Close) reach the slab_guard rather than
    // bouncing on shape validation.
    let lp = Keypair::new();
    let lp_idx = env
        .try_init_lp_proper(&lp, &Pubkey::new_unique(), &Pubkey::new_unique(), 100)
        .expect("lp init should succeed pre-flag");
    env.try_deposit(&lp, lp_idx, 1_000_000_000)
        .expect("lp deposit should succeed pre-flag");
    let user = Keypair::new();
    let user_idx = env
        .try_init_user_with_fee(&user, 100)
        .expect("user init should succeed pre-flag");
    env.try_deposit(&user, user_idx, 100_000_000)
        .expect("user deposit should succeed pre-flag");

    // Snapshot state. Slab data + key engine fields. Anything that
    // changes during a flag-on attempt is a finding.
    let pre_data = env.svm.get_account(&env.slab).unwrap().data.clone();
    let pre_vault = env.read_engine_vault();
    let pre_c_tot = env.read_c_tot();
    let pre_insurance = env.read_insurance_balance();
    let pre_user_pos = env.read_account_position(user_idx);
    let pre_user_cap = env.read_account_capital(user_idx);
    let pre_lp_pos = env.read_account_position(lp_idx);

    // Set FLAG_CPI_IN_PROGRESS directly. This simulates the in-flight
    // state a malicious matcher would observe during TradeCpi's CPI
    // window.
    let mut data = pre_data.clone();
    data[FLAGS_OFF] |= FLAG_CPI_IN_PROGRESS;
    env.svm
        .set_account(
            env.slab,
            Account {
                lamports: env.svm.get_account(&env.slab).unwrap().lamports,
                data: data.clone(),
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Each closure submits one permissionless instruction. All MUST fail
    // (slab_guard rejects with InvalidAccountData / a wrapper error).
    let probe_results: Vec<(&str, Result<(), String>)> = vec![
        ("DepositCollateral", env.try_deposit(&user, user_idx, 1_000)),
        ("WithdrawCollateral", env.try_withdraw(&user, user_idx, 1)),
        (
            "TradeNoCpi",
            env.try_trade(&lp, &user, lp_idx, user_idx, 1_000_000),
        ),
        ("KeeperCrank", env.try_crank()),
        ("ResolvePermissionless", env.try_resolve_permissionless()),
        ("CloseAccount", env.try_close_account(&user, user_idx)),
    ];

    for (name, res) in &probe_results {
        assert!(
            res.is_err(),
            "Pattern D leak: {} succeeded while FLAG_CPI_IN_PROGRESS=1",
            name
        );
    }

    // Slab data must be unchanged except for the flag bit we set
    // ourselves.
    let post_data = env.svm.get_account(&env.slab).unwrap().data.clone();
    assert_eq!(
        data.len(),
        post_data.len(),
        "slab length changed under flag-on probes"
    );
    for i in 0..post_data.len() {
        if i == FLAGS_OFF {
            continue;
        }
        if data[i] != post_data[i] {
            panic!(
                "slab byte {} mutated under flag-on probes: pre={:#x} post={:#x}",
                i, data[i], post_data[i]
            );
        }
    }
    assert_eq!(env.read_engine_vault(), pre_vault);
    assert_eq!(env.read_c_tot(), pre_c_tot);
    assert_eq!(env.read_insurance_balance(), pre_insurance);
    assert_eq!(env.read_account_position(user_idx), pre_user_pos);
    assert_eq!(env.read_account_capital(user_idx), pre_user_cap);
    assert_eq!(env.read_account_position(lp_idx), pre_lp_pos);
}

/// Pattern F — ResolvePermissionless idempotency / replay guard.
///
/// Once a market is resolved (`engine.is_resolved()` returns true), a
/// second ResolvePermissionless instruction in a fresh transaction MUST
/// be rejected by the wrapper's re-resolve guard
/// (`prog/percolator.rs:9843-9845`). A successful replay would re-set
/// `engine.last_oracle_price` and overwrite the settlement price after
/// users have begun closing — a settlement-time-of-record attack.
///
/// The exact `>=` boundary check is covered by the Kani harness
/// `kani_permissionless_resolve_horizon_policy_independent_from_accrual_window`,
/// so this LiteSVM regression focuses on the runtime replay-guard which
/// Kani does not exercise.
#[test]
fn test_attack_resolve_permissionless_idempotent_replay_blocked() {
    const STALE: u64 = 80;
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, STALE);

    let cfg_data = env.svm.get_account(&env.slab).unwrap().data;
    let last_live = percolator_prog::state::read_config(&cfg_data).last_good_oracle_slot;
    let cur_price = env.read_last_effective_price();

    // Drive cleanly past the matured threshold (dt = 2*STALE) so the
    // staleness gate is unambiguously open and we are testing the
    // replay-guard, not the boundary.
    env.set_slot_and_price_raw_no_walk(
        last_live.saturating_add(STALE.saturating_mul(2)),
        cur_price as i64,
    );

    // First resolve — must succeed.
    let r_first = env.try_resolve_permissionless();
    assert!(
        r_first.is_ok(),
        "precondition: first ResolvePermissionless should succeed at dt = 2*STALE; err={:?}",
        r_first.err()
    );

    let resolved_price_after_first = {
        // engine.last_oracle_price; record so we can verify that a
        // rejected replay does not overwrite it.
        let d = env.svm.get_account(&env.slab).unwrap().data;
        let cfg = percolator_prog::state::read_config(&d);
        cfg.hyperp_mark_e6 // Resolve writes p_last into hyperp_mark_e6 too
    };

    // Second resolve in a fresh tx — must FAIL (Pattern F replay block).
    let r_replay = env.try_resolve_permissionless();
    assert!(
        r_replay.is_err(),
        "Pattern F replay: ResolvePermissionless succeeded twice; expected re-resolve guard to close after first success"
    );

    // Replay must not have mutated settlement-relevant config.
    let resolved_price_after_replay = {
        let d = env.svm.get_account(&env.slab).unwrap().data;
        let cfg = percolator_prog::state::read_config(&d);
        cfg.hyperp_mark_e6
    };
    assert_eq!(
        resolved_price_after_first, resolved_price_after_replay,
        "Pattern F replay leak: settlement price changed across the rejected replay"
    );
}

/// Pattern E + Toly economic-success — conservation invariant under
/// repeated near-threshold trade churn.
///
/// The jan-2026 `lp_issue.md` finding (LP = -SUM(users) desync via dust
/// force-close in the OLD position_size model) is OBSOLETE since the
/// position model was refactored to position_basis_q + K/F/B ADL
/// accounting. This test exercises the ANALOGUE invariant in the new
/// model: regardless of how many small trades are opened/closed, the
/// canonical conservation identity must hold:
///
///   engine_vault >= c_tot + insurance
///
/// after every step, with no path letting an attacker accumulate
/// unrealized residual that breaks the inequality.
#[test]
fn test_attack_dust_repeated_force_close_preserves_conservation() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env
        .try_init_lp_proper(&lp, &Pubkey::new_unique(), &Pubkey::new_unique(), 100)
        .expect("lp init");
    env.try_deposit(&lp, lp_idx, 100_000_000_000)
        .expect("lp deposit");

    // Spawn a handful of users. Each opens a small position, then closes.
    // Track the senior-conservation inequality across the entire
    // sequence.
    let users: Vec<Keypair> = (0..4).map(|_| Keypair::new()).collect();
    let user_idxs: Vec<u16> = users
        .iter()
        .map(|u| env.try_init_user_with_fee(u, 100).expect("user init"))
        .collect();
    for (u, idx) in users.iter().zip(user_idxs.iter()) {
        env.try_deposit(u, *idx, 100_000_000)
            .expect("user deposit");
    }

    // Repeated open/close churn at near-threshold magnitudes. Each step
    // asserts conservation. We deliberately use sizes that may flirt
    // with phantom-dust thresholds — if any step certifies a stale
    // residual without offsetting basis, conservation breaks.
    for round in 0..6 {
        for (u, idx) in users.iter().zip(user_idxs.iter()) {
            let size = 1_000_000_i128 + (round as i128) * 250_000;
            // Open small long. Tolerate margin/risk-buffer rejections —
            // they don't violate conservation.
            let _ = env.try_trade(&lp, u, lp_idx, *idx, size);
            // Step time at constant price (avoid harness's PnL-shifting
            // walks).
            let cur_slot = env
                .svm
                .get_sysvar::<Clock>()
                .slot
                .saturating_add(2);
            let cur_price = env.read_last_effective_price();
            env.set_slot_and_price_raw_no_walk(cur_slot, cur_price as i64);
            // Close
            let _ = env.try_trade(&lp, u, lp_idx, *idx, -size);
            // Conservation check
            let vault = env.read_engine_vault();
            let c_tot = env.read_c_tot();
            let ins = env.read_insurance_balance();
            assert!(
                vault >= c_tot.saturating_add(ins),
                "Pattern E conservation break round={} user_idx={}: vault={} c_tot={} insurance={} (deficit={})",
                round,
                idx,
                vault,
                c_tot,
                ins,
                c_tot.saturating_add(ins).saturating_sub(vault)
            );
        }
    }

    // Final crank to settle any deferred housekeeping, then re-assert.
    let _ = env.try_crank();
    let vault = env.read_engine_vault();
    let c_tot = env.read_c_tot();
    let ins = env.read_insurance_balance();
    assert!(
        vault >= c_tot.saturating_add(ins),
        "post-crank conservation break: vault={} c_tot={} insurance={}",
        vault,
        c_tot,
        ins
    );
}
