//! INV-041 - Deterministic allocation and caller-order independence.
//!
//! Normative obligation: caller-selected pair and continuation ordering cannot
//! change user value, source-domain claims, backing classification, or loss
//! attribution.
//!
//! Evidence in this file (I/bounded R/M): two equal-sized public positions
//! create claims against one deliberately scarce backing domain. The original
//! small-state topology exhausts both pair orders crossed with one-shot and
//! dust-chunked force-close schedules and requires exact account/domain state.
//! A second, deliberately underfunded topology reaches Recovery through eight
//! individually bounded authenticated mark moves, uses a complete round-robin
//! public scheduler, and compares one-shot/dust schedules through resolution.
//! Chunking changes nonwithdrawable intermediate claim rounding, but pair order
//! is exact within each schedule and both schedules produce identical terminal
//! SPL payouts, residual custody, and token supply. A separate public LiteSVM
//! regression reuses INV-052's two-asset
//! source-lien world and assigns a 50% utilization fee to one source domain and
//! zero to the other. Reversing otherwise identical signed trade history must
//! preserve the complete allocation and terminal economics through direct and
//! matcher-CPI routes. On engine `422893fa`, insertion order moved 2,378 quote
//! atoms between target payout and provider earnings; engine `c0dec8ce`
//! canonicalizes the bounded persisted source-domain set, with native and Kani
//! field-preservation coverage. Both pre-fix outcomes remained inside signed fee
//! bounds, so this is deterministic-allocation correctness rather than a public
//! LoF or persistent DoS finding. INV-052 separately exhausts public
//! liquidation split/order and three-/four-claimant payout orders, while
//! INV-075 settles all six affected portfolios after both same-domain
//! close-start landing orders and requires exact terminal economic equality.
//! INV-033 source-locks insurance-lien reservation as engine-only. Together
//! these products close the current wrapper order surface; a new allocation
//! route, wrapper insurance-reservation ingress, or implemented close-preemption
//! policy reopens it.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{BOUND_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

const ASSET: u16 = 0;
const SOURCE_DOMAIN: usize = 1;
const OPEN_PRICE: u64 = 101;
const CLOSE_PRICE: u64 = 137;
const SMALL_SIZE_Q: u128 = POS_SCALE + 17;
const UNDERFUNDED_SIZE_Q: u128 = 3_000_000 * POS_SCALE + 17;
const MARK_PATH: [u64; 8] = [105, 110, 115, 120, 125, 130, 135, CLOSE_PRICE];

#[test]
fn v16_program_public_source_lien_allocation_is_domain_order_canonical() {
    super::inv_052_split_merge_invariance::
        verify_public_source_lien_allocation_is_domain_order_canonical();
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct AccountOutcome {
    capital: u128,
    pnl: i128,
    fee_credits: i128,
    source_claims: Vec<(u32, u64, u128)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SmallAllocationOutcome {
    accounts: Vec<AccountOutcome>,
    insurance: u128,
    c_tot: u128,
    vault: u128,
    source_positive_claim_bound_num: u128,
    source_fresh_reserved_backing_num: u128,
    source_provider_receivable_num: u128,
    bucket_fresh_unliened_backing_num: u128,
    bucket_valid_liened_backing_num: u128,
    bucket_consumed_liened_backing_num: u128,
    vault_tokens: u64,
    token_supply: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UnderfundedAllocationOutcome {
    accounts: Vec<AccountOutcome>,
    insurance: u128,
    c_tot: u128,
    vault: u128,
    source_positive_claim_bound_num: u128,
    source_fresh_reserved_backing_num: u128,
    source_provider_receivable_num: u128,
    bucket_fresh_unliened_backing_num: u128,
    bucket_valid_liened_backing_num: u128,
    bucket_consumed_liened_backing_num: u128,
    vault_tokens: u64,
    token_supply: u128,
    terminal_payouts: [u128; 5],
    terminal_vault: u128,
    terminal_vault_tokens: u64,
    terminal_token_supply: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalEconomics {
    payouts: [u128; 5],
    vault: u128,
    vault_tokens: u64,
    token_supply: u128,
}

impl UnderfundedAllocationOutcome {
    fn terminal_economics(&self) -> TerminalEconomics {
        TerminalEconomics {
            payouts: self.terminal_payouts,
            vault: self.terminal_vault,
            vault_tokens: self.terminal_vault_tokens,
            token_supply: self.terminal_token_supply,
        }
    }
}

fn has_active_leg(env: &V16Svm, actor: usize) -> bool {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .any(|leg| leg.active && leg.asset_index == u32::from(ASSET))
}

fn crank_to_fixed_point(env: &mut V16Svm, actor: usize, slot: u64) {
    let observations = vec![CrankObservationHint {
        asset_index: ASSET,
        oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
    }];
    let mut progressed = false;
    for _ in 0..16 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => panic!("actor {actor} crank failed before fixed point: {error}"),
        }
    }
    assert!(progressed, "actor {actor} must make bounded crank progress");
}

fn position_for_asset(env: &V16Svm, actor: usize) -> i128 {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active && leg.asset_index == u32::from(ASSET))
        .map(|leg| leg.basis_pos_q)
        .sum()
}

fn crank_once_if_actionable(env: &mut V16Svm, actor: usize, slot: u64) -> bool {
    let observations = vec![CrankObservationHint {
        asset_index: ASSET,
        oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
    }];
    match env.crank_if_actionable(actor, slot, observations) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(error) if error.contains("Custom(21)") => false,
        Err(error) => panic!("actor {actor} crank failed: {error}"),
    }
}

fn crank_all_once(env: &mut V16Svm, slot: u64) -> bool {
    let mut progressed = false;
    for actor in 0..5 {
        progressed |= crank_once_if_actionable(env, actor, slot);
    }
    progressed
}

fn crank_all_to_quiescence(env: &mut V16Svm, slot: u64) -> bool {
    let mut any_progress = false;
    for _ in 0..16 {
        let round_progress = crank_all_once(env, slot);
        if !round_progress {
            return any_progress;
        }
        any_progress = true;
    }
    panic!("complete public crank scan did not quiesce in 16 rounds");
}

fn force_close_with_progress(
    env: &mut V16Svm,
    winner: usize,
    loser: usize,
    slot: u64,
    close_q: u128,
) -> bool {
    for attempt in 0..16 {
        match env.force_close_abandoned_asset(4, winner, loser, ASSET, slot, close_q) {
            Ok(_) => {
                crank_all_to_quiescence(env, slot);
                return true;
            }
            Err(error) if error.contains("Custom(20)") || error.contains("Custom(22)") => {
                let progressed = crank_all_to_quiescence(env, slot);
                if !progressed {
                    return false;
                }
            }
            Err(error) => panic!(
                "force-close continuation failed for winner={winner}, loser={loser}, attempt={attempt}: {error}"
            ),
        }
    }
    panic!("force close did not become actionable in 16 bounded continuations");
}

fn pair_force_close_is_actionable(env: &V16Svm, winner: usize, loser: usize) -> bool {
    let winner_position = position_for_asset(env, winner);
    let loser_position = position_for_asset(env, loser);
    if winner_position != 0 && loser_position != 0 {
        return winner_position.signum() != loser_position.signum();
    }
    let residue = if winner_position != 0 {
        winner_position
    } else if loser_position != 0 {
        loser_position
    } else {
        return false;
    };
    let asset = env.primary_market_state().1.assets[ASSET as usize];
    if residue > 0 {
        asset.oi_eff_long_q == 0
    } else {
        asset.oi_eff_short_q == 0
    }
}

fn account_outcome(env: &V16Svm, actor: usize) -> AccountOutcome {
    let account = env.primary_portfolio(actor);
    let mut source_claims = account
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .map(|source| {
            (
                source.domain.get(),
                source.source_claim_market_id.get(),
                source.source_claim_bound_num.get(),
            )
        })
        .collect::<Vec<_>>();
    source_claims.sort_unstable();
    AccountOutcome {
        capital: account.capital.get(),
        pnl: account.pnl.get(),
        fee_credits: account.fee_credits.get(),
        source_claims,
    }
}

fn run_small_schedule(pair_order: [usize; 2], chunks: &[u128]) -> SmallAllocationOutcome {
    let config = MarketConfig {
        initial_price: OPEN_PRICE,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new([0x41; 32], config);
    env.configure_permissionless_resolve(100, 1)
        .expect("configure public force-close timing");
    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, 50, 20)
        .expect("fund deliberately scarce source backing");

    for (winner, loser) in [(0usize, 2usize), (1, 3)] {
        env.trade_no_cpi(winner, loser, ASSET, SMALL_SIZE_Q as i128, OPEN_PRICE, 0)
            .expect("open equal public position pair");
    }
    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, CLOSE_PRICE)
        .expect("publish authenticated favorable mark");
    crank_to_fixed_point(&mut env, 4, 2);
    for actor in 0..4 {
        crank_to_fixed_point(&mut env, actor, 2);
    }

    env.warp_to_slot(3);
    env.shutdown_asset(ASSET, 3)
        .expect("enter public Recovery lifecycle");
    env.warp_to_slot(5);
    let pairs = [(0usize, 2usize), (1usize, 3usize)];
    for pair_index in pair_order {
        let (winner, loser) = pairs[pair_index];
        for &chunk in chunks {
            if !has_active_leg(&env, winner) {
                break;
            }
            env.force_close_abandoned_asset(4, winner, loser, ASSET, 5, chunk)
                .expect("force-close continuation");
        }
        if has_active_leg(&env, winner) {
            env.force_close_abandoned_asset(4, winner, loser, ASSET, 5, u128::MAX)
                .expect("terminal force-close remainder");
        }
        assert!(!has_active_leg(&env, winner));
        assert!(!has_active_leg(&env, loser));
    }

    let (_, market) = env.primary_market_state();
    assert_eq!(market.assets[ASSET as usize].oi_eff_long_q, 0);
    assert_eq!(market.assets[ASSET as usize].oi_eff_short_q, 0);
    let source = market.source_credit[SOURCE_DOMAIN];
    let bucket = market.source_backing_buckets[SOURCE_DOMAIN];
    SmallAllocationOutcome {
        accounts: (0..4).map(|actor| account_outcome(&env, actor)).collect(),
        insurance: market.insurance,
        c_tot: market.c_tot,
        vault: market.vault,
        source_positive_claim_bound_num: source.positive_claim_bound_num,
        source_fresh_reserved_backing_num: source.fresh_reserved_backing_num,
        source_provider_receivable_num: source.provider_receivable_num,
        bucket_fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
        bucket_valid_liened_backing_num: bucket.valid_liened_backing_num,
        bucket_consumed_liened_backing_num: bucket.consumed_liened_backing_num,
        vault_tokens: env.token_amount(env.vault),
        token_supply: env.token_supply_observed(),
    }
}

fn drain_resolved_accounts(env: &mut V16Svm) -> [u128; 5] {
    let payout_before: [u64; 5] =
        core::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));
    for round in 0..64 {
        let market_before = env.market_data(false);
        let portfolios_before = env.all_primary_portfolio_data();
        let tokens_before = env.all_token_account_data();
        for actor in 0..5 {
            let _ = env.close_resolved_primary_signed(actor);
            let _ = env.claim_resolved_payout_topup_primary(actor);
        }
        if env.market_data(false) == market_before
            && env.all_primary_portfolio_data() == portfolios_before
            && env.all_token_account_data() == tokens_before
        {
            return core::array::from_fn(|actor| {
                u128::from(
                    env.token_amount(env.actors[actor].destination_token)
                        .checked_sub(payout_before[actor])
                        .expect("resolved payout destination cannot decrease"),
                )
            });
        }
        assert!(round + 1 < 64, "resolved account set did not quiesce");
    }
    unreachable!()
}

fn run_underfunded_schedule(
    pair_order: [usize; 2],
    chunks: &[u128],
) -> UnderfundedAllocationOutcome {
    let config = MarketConfig {
        initial_price: OPEN_PRICE,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new([0x41; 32], config);
    env.configure_permissionless_resolve(100, 1)
        .expect("configure public force-close timing");
    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, 50, 20)
        .expect("fund deliberately scarce source backing");

    for (winner, loser) in [(0usize, 2usize), (1, 3)] {
        env.trade_no_cpi(
            winner,
            loser,
            ASSET,
            UNDERFUNDED_SIZE_Q as i128,
            OPEN_PRICE,
            0,
        )
        .expect("open equal public position pair");
    }
    for (offset, mark) in MARK_PATH.into_iter().enumerate() {
        let slot = 2 + offset as u64;
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, mark)
            .expect("publish authenticated favorable mark");
        for winner in [0usize, 1] {
            env.crank(
                winner,
                slot,
                vec![CrankObservationHint {
                    asset_index: ASSET,
                    oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
                }],
            )
            .expect("settle winner while preserving the losing historical cohort");
        }
    }
    assert_eq!(
        env.primary_market_state().1.assets[ASSET as usize].effective_price,
        CLOSE_PRICE,
        "bounded authenticated marks must reach the force-close price",
    );

    let shutdown_slot = 2 + MARK_PATH.len() as u64;
    let force_close_slot = shutdown_slot + 2;
    env.warp_to_slot(shutdown_slot);
    env.shutdown_asset(ASSET, shutdown_slot)
        .expect("enter public Recovery lifecycle");
    env.warp_to_slot(force_close_slot);
    let pairs = [(0usize, 2usize), (1usize, 3usize)];
    for &chunk in chunks {
        for pair_index in pair_order {
            let (winner, loser) = pairs[pair_index];
            if pair_force_close_is_actionable(&env, winner, loser) {
                assert!(force_close_with_progress(
                    &mut env,
                    winner,
                    loser,
                    force_close_slot,
                    chunk,
                ));
            }
        }
    }
    for round in 0..32 {
        if pairs
            .iter()
            .all(|(winner, loser)| !has_active_leg(&env, *winner) && !has_active_leg(&env, *loser))
        {
            break;
        }
        let mut progressed = crank_all_to_quiescence(&mut env, force_close_slot);
        for pair_index in pair_order {
            let (winner, loser) = pairs[pair_index];
            if pair_force_close_is_actionable(&env, winner, loser) {
                progressed |=
                    force_close_with_progress(&mut env, winner, loser, force_close_slot, u128::MAX);
            }
        }
        assert!(
            progressed,
            "complete public scheduler stalled with live Recovery legs at round {round}: market={:?}",
            env.primary_market_state().1,
        );
    }
    for (winner, loser) in pairs {
        assert!(!has_active_leg(&env, winner));
        assert!(!has_active_leg(&env, loser));
    }

    let (_, market) = env.primary_market_state();
    assert_eq!(market.assets[ASSET as usize].oi_eff_long_q, 0);
    assert_eq!(market.assets[ASSET as usize].oi_eff_short_q, 0);
    let source = market.source_credit[SOURCE_DOMAIN];
    let bucket = market.source_backing_buckets[SOURCE_DOMAIN];
    let vault_tokens = env.token_amount(env.vault);
    let token_supply = env.token_supply_observed();
    let mut accounts = (0..4)
        .map(|actor| account_outcome(&env, actor))
        .collect::<Vec<_>>();
    accounts[..2].sort_unstable();
    accounts[2..].sort_unstable();
    env.resolve_market()
        .expect("resolve completed public Recovery allocation");
    let terminal_payouts = drain_resolved_accounts(&mut env);
    let terminal = env.primary_market_state().1;
    assert_eq!(terminal.negative_pnl_account_count, 0);
    let terminal_vault_tokens = env.token_amount(env.vault);
    let terminal_token_supply = env.token_supply_observed();
    UnderfundedAllocationOutcome {
        accounts,
        insurance: market.insurance,
        c_tot: market.c_tot,
        vault: market.vault,
        source_positive_claim_bound_num: source.positive_claim_bound_num,
        source_fresh_reserved_backing_num: source.fresh_reserved_backing_num,
        source_provider_receivable_num: source.provider_receivable_num,
        bucket_fresh_unliened_backing_num: bucket.fresh_unliened_backing_num,
        bucket_valid_liened_backing_num: bucket.valid_liened_backing_num,
        bucket_consumed_liened_backing_num: bucket.consumed_liened_backing_num,
        vault_tokens,
        token_supply,
        terminal_payouts,
        terminal_vault: terminal.vault,
        terminal_vault_tokens,
        terminal_token_supply,
    }
}

#[test]
fn v16_program_scarce_backing_force_close_exhausts_pair_and_chunk_orders() {
    let full = [u128::MAX];
    let dust = [1, POS_SCALE / 3, 7, POS_SCALE / 2, u128::MAX];
    let baseline = run_small_schedule([0, 1], &full);
    assert!(
        baseline.source_positive_claim_bound_num > 50 * BOUND_SCALE,
        "topology must create claims exceeding deliberately scarce backing"
    );
    for (pair_order, chunks) in [
        ([1, 0], full.as_slice()),
        ([0, 1], dust.as_slice()),
        ([1, 0], dust.as_slice()),
    ] {
        assert_eq!(
            run_small_schedule(pair_order, chunks),
            baseline,
            "caller-selected pair/chunk order changed allocation: pairs={pair_order:?}, chunks={chunks:?}"
        );
    }
}

#[test]
fn v16_program_underfunded_recovery_terminal_economics_are_order_independent() {
    let full = [u128::MAX];
    let dust = [1, POS_SCALE / 3, 7, POS_SCALE / 2, u128::MAX];
    let baseline = run_underfunded_schedule([0, 1], &full);
    assert!(
        baseline.source_positive_claim_bound_num > 50 * BOUND_SCALE,
        "topology must create claims exceeding deliberately scarce backing"
    );
    assert_eq!(baseline.insurance, 0);
    assert_eq!(
        run_underfunded_schedule([1, 0], &full),
        baseline,
        "pair order changed one-shot underfunded allocation"
    );

    let dust_baseline = run_underfunded_schedule([0, 1], &dust);
    assert_eq!(
        run_underfunded_schedule([1, 0], &dust),
        dust_baseline,
        "pair order changed dust-chunked underfunded allocation"
    );
    assert_ne!(
        dust_baseline.accounts, baseline.accounts,
        "chunk topology must exercise distinct intermediate rounding"
    );
    assert_eq!(
        dust_baseline.terminal_economics(),
        baseline.terminal_economics(),
        "chunking changed terminal public payouts or custody"
    );

    for outcome in [&baseline, &dust_baseline] {
        let paid = outcome
            .terminal_payouts
            .iter()
            .try_fold(0u128, |sum, payout| sum.checked_add(*payout))
            .expect("bounded terminal payout sum");
        assert_eq!(
            outcome.terminal_vault,
            u128::from(outcome.terminal_vault_tokens)
        );
        assert_eq!(
            paid + outcome.terminal_vault,
            u128::from(outcome.vault_tokens),
            "terminal public payouts must reconcile to pre-resolution custody"
        );
        assert_eq!(outcome.terminal_token_supply, outcome.token_supply);
    }
}
