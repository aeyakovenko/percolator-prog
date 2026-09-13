//! INV-082 - State-indexed liveness theorem.
//!
//! Normative obligation: every publicly reachable nonterminal state in each
//! lifecycle mode either is terminal already or has a constructible bounded
//! public action that decreases the mode-specific rank.
//!
//! Evidence in this file (I/F/CU): this deterministic LiteSVM scenario first
//! lands ordinary public routes, then injects non-progressing discovery noise:
//! empty/duplicate crank hints, retained no-CPI execution, and account
//! substitution attempts. Rejected substitutions must roll back through the
//! shared scenario oracle. The same state must still admit complete-hint
//! permissionless crank progress, a normal user exit, and an independent
//! liquidation-progress probe under the compute ceiling.
//!
//! Guarantee boundary: this is a concrete whole-route witness for adversarial
//! landing order around the liveness theorem. The exhaustive quantification over
//! every reachable state remains the model/proof frontier.
//!
//! The mixed-lifecycle witness retains a priced Recovery leg beside an Active
//! terminal-accrual backlog, in both asset-index placements. A fixed-Clock public
//! suffix lowers a lifecycle-indexed rank without accruing the frozen asset, then
//! resolves and pays every funded owner without owner or administrator signatures.
//! This is not completed-hint replay, a resource-failure lattice, or an engine proof.

use super::{crank_observations, crank_observations_for_assets};
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, run_scenario, Action, HintMode,
    Scenario, SmallMarketConfig, SubstitutionKind, TradeRoute,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT};
use percolator::{active_bitmap_is_empty, AssetLifecycleV16, MarketModeV16, POS_SCALE};
use percolator_prog::error::PercolatorError;
use solana_sdk::{account::Account, pubkey::Pubkey, signature::Signer};

#[path = "inv_082_terminal_destination_recovery.rs"]
pub(crate) mod terminal_destination_recovery;

#[path = "inv_082_terminal_reassigned_custody.rs"]
mod terminal_reassigned_custody;

#[test]
fn v16_program_public_liveness_survives_bad_hints_retained_route_and_substitutions() {
    let scenario = Scenario {
        seed: [0x82; 32],
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
        actions: vec![
            Action::Deposit {
                actor: 0,
                amount: 300,
            },
            Action::Deposit {
                actor: 1,
                amount: 300,
            },
            Action::Trade {
                route: TradeRoute::NoCpi,
                taker: 0,
                maker: 1,
                asset: 0,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::Trade {
                route: TradeRoute::Cpi,
                taker: 2,
                maker: 3,
                asset: 1,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::RetainTrade {
                taker: 0,
                maker: 1,
                asset: 0,
                units: 1,
            },
            Action::LandRetained,
            Action::Trade {
                route: TradeRoute::BatchNoCpi,
                taker: 0,
                maker: 2,
                asset: 0,
                units: -1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::Trade {
                route: TradeRoute::BatchCpi,
                taker: 1,
                maker: 3,
                asset: 1,
                units: -1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::PushMark {
                asset: 0,
                dt: 2,
                move_bps: 500,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Empty,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Duplicate,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Complete,
            },
            Action::SyncMaintenanceFee { actor: 0, dt: 2 },
            Action::AccountSubstitution {
                actor: 0,
                kind: SubstitutionKind::ForeignTradePortfolio,
            },
            Action::AccountSubstitution {
                actor: 1,
                kind: SubstitutionKind::ForeignDepositVault,
            },
            Action::AccountSubstitution {
                actor: 2,
                kind: SubstitutionKind::ForeignWithdrawVault,
            },
            Action::AccountSubstitution {
                actor: 3,
                kind: SubstitutionKind::ForeignCrankPortfolio,
            },
            Action::AccountSubstitution {
                actor: 0,
                kind: SubstitutionKind::MismatchedMatcherBinding,
            },
        ],
    };

    let coverage = run_scenario(&scenario).expect("INV-082 bad-hint liveness scenario");
    assert!(
        coverage.retained_landed > 0,
        "retained no-CPI route must execute under the same state oracle: {coverage:?}"
    );
    assert!(
        coverage
            .substitution_rejections
            .iter()
            .all(|rejections| *rejections > 0),
        "every account-substitution boundary must reject with exact rollback: {coverage:?}"
    );
    assert!(
        coverage.crank_progress > 0,
        "complete-hint public crank must still decrease rank: {coverage:?}"
    );
    assert!(
        coverage.user_positions_closed > 0,
        "normal-user exit campaign must still close a public position: {coverage:?}"
    );
    assert!(
        coverage.liquidation_steps > 0 && coverage.liquidated_abs_q > 0,
        "independent liquidation liveness probe must still progress: {coverage:?}"
    );
}

fn inv082_mixed_terminal(env: &V16Svm, actor: usize) -> bool {
    let account = env.primary_portfolio(actor);
    let market = env.primary_market_state().1;
    let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
    let close = account.close_progress.try_to_runtime().unwrap();
    market.mode == MarketModeV16::Resolved
        && account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == market.resolved_slot
        && account.health_cert.valid == 0
        && active_bitmap_is_empty(account.active_bitmap.map(|word| word.get()))
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Inv082MixedRank {
    mode: u8,
    accrual_slots: u64,
    wait_slots: u64,
    active_legs: u64,
    occupied_sources: usize,
    pnl_accounts: usize,
    unfinished_receipts: usize,
    nonterminal_accounts: usize,
    unpaid: u128,
}

#[test]
fn v16_program_mixed_recovery_active_terminal_accrual_has_bounded_public_exit() {
    const PRICE: u64 = 100;
    const FROZEN_PRICE: u64 = 105;
    const LIVE_TARGET: u64 = 150;
    const FREEZE_SLOT: u64 = 2;
    const MARK_SLOT: u64 = 3;
    const STALE_SLOTS: u64 = 6;
    const RESOLVE_SLOT: u64 = MARK_SLOT + STALE_SLOTS;
    const EXIT_DELAY: u64 = 2;
    const DEPOSITS: [u128; PRIMARY_ACTOR_COUNT] = [1_000, 1_100, 1, 17, 123];
    const TOTAL: u128 = 2_241;
    const PROFIT: u128 = (FROZEN_PRICE - PRICE + LIVE_TARGET - PRICE) as u128;

    let mut max_cu = 0;
    let mut total_steps = 0;
    for frozen in [0usize, 1] {
        let live = 1 - frozen;
        let mut env = V16Svm::new(
            [0x82 + frozen as u8; 32],
            MarketConfig {
                initial_price: PRICE,
                max_price_move_bps_per_slot: 1_000,
                max_accrual_dt_slots: 1,
                actor_deposits: DEPOSITS,
                ..MarketConfig::default()
            },
        );
        env.configure_permissionless_resolve(STALE_SLOTS, EXIT_DELAY)
            .expect("configure finite public terminal deadlines");
        for asset in [0, 1] {
            env.trade_no_cpi(0, 1, asset, POS_SCALE as i128, PRICE, 0)
                .expect("publicly open both funded legs");
        }
        env.warp_to_slot(FREEZE_SLOT);
        env.push_auth_mark(frozen as u16, FREEZE_SLOT, FROZEN_PRICE)
            .expect("authenticate the Recovery leg's nonzero gain");
        env.crank(0, FREEZE_SLOT, crank_observations_for_assets(&[0, 1]))
            .expect("commit the pre-shutdown mark");
        env.shutdown_asset(frozen as u16, FREEZE_SLOT)
            .expect("publicly freeze only one asset");
        env.warp_to_slot(MARK_SLOT);
        env.push_auth_mark(live as u16, MARK_SLOT, LIVE_TARGET)
            .expect("retain an independent Active terminal-accrual target");
        env.warp_to_slot(RESOLVE_SLOT);

        let initial = env.primary_market_state().1;
        let frozen_asset = initial.assets[frozen];
        let frozen_profile = env.primary_profile(frozen);
        assert_eq!(initial.mode, MarketModeV16::Live);
        assert_eq!(frozen_asset.lifecycle, AssetLifecycleV16::Recovery);
        assert_eq!(frozen_asset.effective_price, FROZEN_PRICE);
        assert_eq!(frozen_asset.slot_last, FREEZE_SLOT);
        assert_eq!(initial.assets[live].lifecycle, AssetLifecycleV16::Active);
        assert_eq!(initial.assets[live].effective_price, PRICE);
        assert_eq!(initial.config.max_abs_funding_e9_per_slot, 0);
        assert_eq!(
            env.primary_portfolio(0).pnl.get(),
            (FROZEN_PRICE - PRICE) as i128
        );
        for asset in &initial.assets[..2] {
            assert_eq!(
                (asset.oi_eff_long_q, asset.oi_eff_short_q),
                (POS_SCALE, POS_SCALE)
            );
        }
        let supply = env.token_supply_observed();
        let expected = [DEPOSITS[0] + PROFIT, DEPOSITS[1] - PROFIT, 1, 17, 123];
        let destinations_before: [u64; PRIMARY_ACTOR_COUNT] =
            std::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));
        let paid = |env: &V16Svm, actor: usize| -> u128 {
            u128::from(
                env.token_amount(env.actors[actor].destination_token)
                    .checked_sub(destinations_before[actor])
                    .expect("payout cannot decrease"),
            )
        };
        let census = |env: &V16Svm| {
            assert_public_stock_census("INV-082 mixed lifecycle", env).unwrap();
            assert_public_encumbrance_census("INV-082 mixed lifecycle", env).unwrap();
            let market = env.primary_market_state().1;
            let total_paid: u128 = (0..PRIMARY_ACTOR_COUNT)
                .map(|actor| {
                    let amount = paid(env, actor);
                    assert!(amount <= expected[actor], "owner-local entitlement ceiling");
                    amount
                })
                .sum();
            assert_eq!(market.vault + total_paid, TOTAL);
            assert_eq!(market.vault, u128::from(env.token_amount(env.vault)));
            assert_eq!(market.insurance, 0);
            assert_eq!(env.token_supply_observed(), supply);
        };
        // Fixed zero-funding endpoints need bounded accrual only while the Active price is
        // unfinished. Recovery's old slot is deliberately excluded, but its retained leg and
        // unpaid economic claim are not. No engine selector or hypothetical transition is used.
        let rank = |env: &V16Svm| {
            let market = env.primary_market_state().1;
            assert_eq!(market.assets[frozen].lifecycle, AssetLifecycleV16::Recovery);
            assert_eq!(market.assets[live].lifecycle, AssetLifecycleV16::Active);
            let (mode, accrual, wait) = match market.mode {
                MarketModeV16::Live => (
                    1u8,
                    if market.assets[live].effective_price != LIVE_TARGET {
                        RESOLVE_SLOT
                            .checked_sub(market.assets[live].slot_last)
                            .unwrap()
                    } else {
                        0
                    },
                    0,
                ),
                MarketModeV16::Resolved => (
                    0,
                    0,
                    (market.resolved_slot + EXIT_DELAY).saturating_sub(env.current_slot()),
                ),
                mode => panic!("unconstructed market mode: {mode:?}"),
            };
            let mut legs = 0u64;
            let mut sources = 0usize;
            let mut pnl = 0usize;
            let mut receipts = 0usize;
            let mut nonterminal = 0usize;
            let mut unpaid = TOTAL;
            for actor in 0..PRIMARY_ACTOR_COUNT {
                let account = env.primary_portfolio(actor);
                legs += account
                    .active_bitmap
                    .iter()
                    .map(|word| u64::from(word.get().count_ones()))
                    .sum::<u64>();
                sources += account
                    .source_domains
                    .iter()
                    .filter(|source| source.is_occupied())
                    .count();
                pnl += usize::from(account.pnl.get() != 0);
                let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
                receipts += usize::from(receipt.present && !receipt.finalized);
                nonterminal += usize::from(!inv082_mixed_terminal(env, actor));
                unpaid = unpaid.checked_sub(paid(env, actor)).unwrap();
            }
            Inv082MixedRank {
                mode,
                accrual_slots: accrual,
                wait_slots: wait,
                active_legs: legs,
                occupied_sources: sources,
                pnl_accounts: pnl,
                unfinished_receipts: receipts,
                nonterminal_accounts: nonterminal,
                unpaid,
            }
        };
        let frame = |env: &V16Svm| -> Vec<(Pubkey, Option<Account>)> {
            env.all_economic_account_lamports()
                .into_iter()
                .map(|(key, _)| key)
                .chain(env.actors.iter().map(|actor| actor.signer.pubkey()))
                .map(|key| (key, env.svm.get_account(&key)))
                .collect()
        };
        let assert_frame =
            |env: &V16Svm, before: &[(Pubkey, Option<Account>)], allowed: &[Pubkey]| {
                for (key, account) in before {
                    if !allowed.contains(key) {
                        assert_eq!(&env.svm.get_account(key), account, "economic frame {key}");
                    }
                }
            };
        census(&env);
        let initial_rank = rank(&env);
        assert!(initial_rank.accrual_slots > 1);
        assert_eq!(initial_rank.active_legs, 4);
        assert_eq!(initial_rank.nonterminal_accounts, PRIMARY_ACTOR_COUNT);
        assert_eq!(initial_rank.unpaid, TOTAL);
        env.begin_public_trace();
        let before = frame(&env);
        let error = env
            .resolve_stale_permissionless(RESOLVE_SLOT)
            .expect_err("Active accrual is a real prerequisite, despite frozen Recovery exposure");
        assert!(
            error.contains(&format!("Custom({})", PercolatorError::EngineStale as u32)),
            "{error}"
        );
        assert_frame(&env, &before, &[]);

        let mut accrual_calls = 0;
        while rank(&env).accrual_slots != 0 {
            assert!(
                accrual_calls < initial_rank.accrual_slots,
                "finite authenticated-slot bound"
            );
            let before_rank = rank(&env);
            env.crank(0, 0, crank_observations(live as u16))
                .expect("Active-only public hint must advance beside a frozen Recovery leg");
            assert!(
                rank(&env) < before_rank,
                "strict mixed-lifecycle accrual edge: {before_rank:?} -> {:?}",
                rank(&env)
            );
            assert_eq!(env.current_slot(), RESOLVE_SLOT);
            assert_eq!(env.primary_market_state().1.assets[frozen], frozen_asset);
            assert_eq!(env.primary_profile(frozen), frozen_profile);
            assert_frame(&env, &before, &[env.market]);
            census(&env);
            accrual_calls += 1;
        }
        assert!(
            accrual_calls > 1,
            "require multiple concrete bounded continuations"
        );
        assert_eq!(
            env.primary_market_state().1.assets[live].effective_price,
            LIVE_TARGET
        );
        let before_rank = rank(&env);
        env.resolve_stale_permissionless(RESOLVE_SLOT)
            .expect("mixed lifecycle must resolve");
        assert!(
            rank(&env) < before_rank,
            "resolution must lower the mode lane"
        );
        assert_frame(&env, &before, &[env.market]);
        assert_eq!(env.primary_market_state().1.assets[frozen], frozen_asset);
        assert_eq!(env.primary_profile(frozen), frozen_profile);
        census(&env);

        let resolved_rank = rank(&env);
        assert_eq!(resolved_rank.wait_slots, EXIT_DELAY);
        let resolved_frame = frame(&env);
        env.warp_to_slot(RESOLVE_SLOT + EXIT_DELAY - 1);
        assert!(rank(&env) < resolved_rank);
        assert_frame(&env, &resolved_frame, &[]);
        let before = frame(&env);
        let error = env
            .close_resolved_primary(0)
            .expect_err("owner window remains a finite barrier");
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::ExpectedSigner as u32
            )),
            "{error}"
        );
        assert_frame(&env, &before, &[]);
        let waiting_rank = rank(&env);
        assert_eq!(waiting_rank.wait_slots, 1);
        env.warp_to_slot(RESOLVE_SLOT + EXIT_DELAY);
        assert!(
            rank(&env) < waiting_rank,
            "one authenticated slot discharges the wait lane"
        );
        assert_frame(&env, &before, &[]);

        let mut terminal_calls = 0;
        for _ in 0..8 {
            if (0..PRIMARY_ACTOR_COUNT).all(|actor| inv082_mixed_terminal(&env, actor)) {
                break;
            }
            let sweep_rank = rank(&env);
            for actor in 0..PRIMARY_ACTOR_COUNT {
                if inv082_mixed_terminal(&env, actor) {
                    continue;
                }
                let before = frame(&env);
                let before_rank = rank(&env);
                match env.close_resolved_primary(actor) {
                    Ok(_) => {
                        assert!(
                            rank(&env) < before_rank,
                            "strict resolved continuation for actor {actor}: {before_rank:?} -> {:?}", rank(&env)
                        );
                        assert_frame(
                            &env,
                            &before,
                            &[
                                env.market,
                                env.actors[actor].portfolio,
                                env.actors[actor].destination_token,
                                env.vault,
                            ],
                        );
                        terminal_calls += 1;
                    }
                    Err(error) => {
                        assert!(
                            error.contains(&format!(
                                "Custom({})",
                                PercolatorError::EngineNonProgress as u32
                            )),
                            "{error}"
                        );
                        assert_frame(&env, &before, &[]);
                        assert_eq!(rank(&env), before_rank);
                    }
                }
                census(&env);
            }
            assert!(
                rank(&env) < sweep_rank,
                "every nonterminal sweep must construct progress"
            );
        }
        assert_eq!(rank(&env), Inv082MixedRank::default());
        for actor in 0..PRIMARY_ACTOR_COUNT {
            assert!(inv082_mixed_terminal(&env, actor));
            assert_eq!(paid(&env, actor), expected[actor]);
            let before = frame(&env);
            let error = env
                .close_resolved_primary(actor)
                .expect_err("terminal retry is not progress");
            assert!(
                error.contains(&format!(
                    "Custom({})",
                    PercolatorError::EngineNonProgress as u32
                )),
                "{error}"
            );
            assert_frame(&env, &before, &[]);
        }
        let final_market = env.primary_market_state().1;
        assert_eq!(
            final_market.materialized_portfolio_count,
            PRIMARY_ACTOR_COUNT as u64
        );
        assert_eq!(
            final_market.assets[frozen].lifecycle,
            AssetLifecycleV16::Recovery
        );
        assert_eq!(final_market.assets[frozen].slot_last, FREEZE_SLOT);
        assert_eq!(final_market.assets[frozen].effective_price, FROZEN_PRICE);
        for asset in &final_market.assets[..2] {
            assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (0, 0));
        }
        census(&env);
        let trace = env.finish_public_trace();
        trace
            .validate_public_execution()
            .expect("compiled public-route and exact rollback evidence");
        let keeper = trace.steps[0].fee_payer;
        assert!(env
            .actors
            .iter()
            .all(|actor| actor.signer.pubkey() != keeper));
        assert_ne!(keeper, env.foreign_actor.signer.pubkey());
        assert_ne!(
            keeper,
            Pubkey::new_from_array(env.primary_market_state().0.marketauth)
        );
        for step in &trace.steps {
            assert_eq!(step.fee_payer, keeper);
            assert_eq!(step.transaction_signers, vec![keeper]);
            assert!(step
                .accounts
                .iter()
                .all(|meta| !meta.is_signer || meta.key == keeper));
            if let Some(cu) = step.compute_units {
                assert!(cu < TX_CU_LIMIT);
                max_cu = max_cu.max(cu);
            }
        }
        total_steps += trace.steps.len();
        eprintln!("INV-082 mixed Recovery/Active frozen={frozen} accrual_calls={accrual_calls} terminal_calls={terminal_calls} suffix_calls={} payouts={expected:?} max_cu={max_cu}", trace.steps.len());
    }
    eprintln!("INV-082 mixed lifecycle: worlds=2 suffix_calls={total_steps} max_cu={max_cu}");
}
