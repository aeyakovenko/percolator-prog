//! INV-078 - Permissionless recovery coverage.
//!
//! Normative obligation: unavailable backing or insurance cannot remove every
//! permissionless senior-preserving terminal path.
//!
//! Evidence in this file (I/bounded R): a public four-state resource lattice
//! crosses absent/expired backing with absent/tiny insurance. Every cell opens
//! a real favorable exposure against a bankrupt counterparty, settles an
//! authenticated adverse move, enters Recovery through `UpdateAssetLifecycle`,
//! and uses the public owner dead-leg forfeit route. The close ledger proves
//! expired backing contributes no support, tiny insurance is spent exactly,
//! and the remaining residual is booked to B before both legs clear. The test
//! also runs the independent stock and encumbrance census after every public
//! setup, mark, crank, lifecycle, and forfeit transition, reconciles engine
//! custody to SPL balances, and proves the route does not mint supply.
//! After the owner-forfeit prerequisite, the same four cells require a bounded
//! keeper-only stale-resolution/payout suffix for every funded portfolio, with
//! exact owner-window and terminal-retry rollback. Economic completion does not
//! claim permissionless portfolio deletion or asset/market retirement.
//! Supplementary public-route evidence is intentionally not duplicated here:
//! `cu/inv_028_source_domain_realizability_cap.rs` takes a live counterparty lien through exact
//! expiry/impairment and terminal disposition for all funded portfolios, while
//! `stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs` builds genuine
//! underfunded partial receipts and crosses claimant order with close/top-up route priority.
//! B-headroom exhaustion is covered at the engine boundary by deployed-U256 saturation execution,
//! a generic residual-partition contract, and a fully-declared-Recovery proof. Its direct universal
//! U256-division composition and the remaining lifecycle failure compositions remain outside this
//! publicly reachable topology.

use crate::support::fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census};
use crate::support::v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT};
use percolator::{active_bitmap_is_empty, MarketModeV16, BOUND_SCALE, POS_SCALE};
use percolator_prog::error::PercolatorError;
use percolator_prog::ix::CrankObservationHint;
use solana_sdk::signature::Signer;

const ASSET: u16 = 0;
const SOURCE_DOMAIN: usize = 0;
const OPEN_PRICE: u64 = 100;
const SIZE_Q: u128 = 3 * POS_SCALE / 100;

fn has_active_leg(env: &V16Svm, actor: usize) -> bool {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .any(|leg| leg.active && leg.asset_index == u32::from(ASSET))
}

fn assert_resource_census(label: &str, env: &V16Svm) {
    assert_public_stock_census(label, env)
        .unwrap_or_else(|error| panic!("{label} stock census failed: {error}"));
    assert_public_encumbrance_census(label, env)
        .unwrap_or_else(|error| panic!("{label} encumbrance census failed: {error}"));
}

fn assert_resource_terminal(env: &V16Svm, actor: usize) {
    let account = env.primary_portfolio(actor);
    let market = env.primary_market_state().1;
    assert_eq!(market.mode, MarketModeV16::Resolved);
    assert_eq!(account.capital.get(), 0);
    assert_eq!(account.pnl.get(), 0);
    assert_eq!(account.reserved_pnl.get(), 0);
    assert_eq!(account.fee_credits.get(), 0);
    assert_eq!(account.cancel_deposit_escrow.get(), 0);
    assert_eq!(account.stale_state, 0);
    assert_eq!(account.b_stale_state, 0);
    assert_eq!(account.rebalance_lock, 0);
    assert_eq!(account.liquidation_lock, 0);
    assert_eq!(account.last_fee_slot.get(), market.resolved_slot);
    assert_eq!(account.health_cert.valid, 0);
    assert!(active_bitmap_is_empty(
        account.active_bitmap.map(|word| word.get())
    ));
    assert!(account
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    let close = account.close_progress.try_to_runtime().unwrap();
    assert!(!close.active || (close.finalized && close.residual_remaining == 0));
    let receipt = account.resolved_payout_receipt.try_to_runtime().unwrap();
    assert!(!receipt.present || receipt.finalized);
}

fn crank_to_fixed_point(
    env: &mut V16Svm,
    actor: usize,
    slot: u64,
    observations: &[CrankObservationHint],
    label: &str,
) {
    let mut progressed = false;
    for step in 0..16 {
        if progressed && !has_active_leg(env, actor) {
            break;
        }
        match env.crank(actor, slot, observations.to_vec()) {
            Ok(_) => {
                progressed = true;
                assert_resource_census(&format!("{label} crank step {step}"), env);
            }
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => panic!("actor {actor} crank failed before fixed point: {error}"),
        }
    }
    assert!(progressed, "actor {actor} must make bounded crank progress");
}

#[test]
fn v16_program_recovery_resource_failure_lattice_preserves_public_exit() {
    for resource_mask in 0u8..4 {
        let has_expired_backing = resource_mask & 1 != 0;
        let has_tiny_insurance = resource_mask & 2 != 0;
        let mut config = MarketConfig {
            initial_price: OPEN_PRICE,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            ..MarketConfig::default()
        };
        config.actor_deposits[0] = 10;
        config.actor_deposits[1] = 3;
        let deposits = config.actor_deposits;
        let mut env = V16Svm::new([0x78; 32], config);
        env.configure_permissionless_resolve(100, 1)
            .expect("configure public Recovery timing");
        assert_resource_census(
            &format!("INV-078 resource cell {resource_mask} configured"),
            &env,
        );
        if has_expired_backing {
            env.top_up_backing_bucket(SOURCE_DOMAIN as u16, 1, 26)
                .expect("fund backing that will expire before recovery");
            assert_resource_census(
                &format!("INV-078 resource cell {resource_mask} backing funded"),
                &env,
            );
        }
        if has_tiny_insurance {
            env.top_up_insurance_domain(SOURCE_DOMAIN as u16, 1)
                .expect("fund deliberately insufficient insurance");
            assert_resource_census(
                &format!("INV-078 resource cell {resource_mask} insurance funded"),
                &env,
            );
        }
        let (_, funded) = env.primary_market_state();
        assert_eq!(
            funded.insurance_domain_budget[SOURCE_DOMAIN],
            u128::from(has_tiny_insurance),
            "resource cell {resource_mask} insurance setup is vacuous"
        );
        assert_eq!(
            funded.source_backing_buckets[SOURCE_DOMAIN].fresh_unliened_backing_num,
            u128::from(has_expired_backing) * BOUND_SCALE,
            "resource cell {resource_mask} backing setup is vacuous"
        );

        env.trade_no_cpi(0, 1, ASSET, SIZE_Q as i128, OPEN_PRICE, 0)
            .expect("open public claim-producing position pair");
        assert_resource_census(
            &format!("INV-078 resource cell {resource_mask} position opened"),
            &env,
        );
        let supply_before = env.token_supply_observed();
        let observations = vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
        }];
        for slot in 2u64..=41 {
            env.warp_to_slot(slot);
            env.push_auth_mark(ASSET, slot, 300)
                .expect("publish authenticated adverse mark");
            assert_resource_census(
                &format!("INV-078 resource cell {resource_mask} mark slot {slot}"),
                &env,
            );
            crank_to_fixed_point(
                &mut env,
                0,
                slot,
                &observations,
                &format!("INV-078 resource cell {resource_mask} actor 0 slot {slot}"),
            );
        }
        env.crank(1, 41, observations)
            .expect("settle bankrupt counterparty once before Recovery");
        assert_resource_census(
            &format!("INV-078 resource cell {resource_mask} loser settled"),
            &env,
        );
        let terminal_observations = vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
        }];
        crank_to_fixed_point(
            &mut env,
            0,
            41,
            &terminal_observations,
            &format!("INV-078 resource cell {resource_mask} winner settled"),
        );
        let loser_before_recovery = env.primary_portfolio(1);
        let effective_price = env.primary_market_state().1.assets[ASSET as usize].effective_price;
        assert_eq!(effective_price, 300);
        assert!(has_active_leg(&env, 0));
        assert!(has_active_leg(&env, 1));
        assert_eq!(
            (u128::from(effective_price - OPEN_PRICE) * SIZE_Q) / POS_SCALE,
            6,
            "resource cell {resource_mask} must carry a six-atom favorable exposure"
        );
        assert!(
            loser_before_recovery.pnl.get() < 0 && loser_before_recovery.capital.get() == 0,
            "resource cell {resource_mask} must create a publicly bankrupt counterparty"
        );

        env.warp_to_slot(42);
        env.shutdown_asset(ASSET, 42)
            .expect("enter public Recovery lifecycle");
        assert_resource_census(
            &format!("INV-078 resource cell {resource_mask} Recovery entered"),
            &env,
        );
        env.warp_to_slot(44);
        env.forfeit_recovery_leg(1, ASSET, u128::MAX)
            .expect("bankrupt owner forfeits its Recovery leg");
        assert_resource_census(
            &format!("INV-078 resource cell {resource_mask} loser forfeited"),
            &env,
        );
        env.forfeit_recovery_leg(0, ASSET, u128::MAX)
            .expect("counterparty owner forfeits its Recovery leg");
        assert_resource_census(
            &format!("INV-078 resource cell {resource_mask} winner forfeited"),
            &env,
        );
        for actor in [1usize, 0usize] {
            if has_active_leg(&env, actor) {
                crank_to_fixed_point(
                    &mut env,
                    actor,
                    44,
                    &terminal_observations,
                    &format!(
                        "INV-078 resource cell {resource_mask} actor {actor} retained obligation"
                    ),
                );
            }
        }

        let (_, after) = env.primary_market_state();
        let loser_close = env
            .primary_portfolio(1)
            .close_progress
            .try_to_runtime()
            .expect("decode terminal close ledger");
        assert!(!has_active_leg(&env, 0));
        assert!(!has_active_leg(&env, 1));
        assert!(loser_close.finalized && loser_close.residual_remaining == 0);
        assert_eq!(
            loser_close.support_consumed, 0,
            "resource cell {resource_mask} must not consume absent or expired backing"
        );
        assert_eq!(
            loser_close.insurance_spent,
            u128::from(has_tiny_insurance),
            "resource cell {resource_mask} must spend exactly the available tiny insurance"
        );
        assert_eq!(
            loser_close.b_loss_booked,
            3 - u128::from(has_tiny_insurance),
            "resource cell {resource_mask} must book the exact uncovered residual to B"
        );
        assert_eq!(after.assets[ASSET as usize].oi_eff_long_q, 0);
        assert_eq!(after.assets[ASSET as usize].oi_eff_short_q, 0);
        assert!(
            after.vault >= after.c_tot + after.insurance,
            "resource cell {resource_mask} violates senior stock ordering"
        );
        assert_eq!(after.vault as u64, env.token_amount(env.vault));
        assert_eq!(env.token_supply_observed(), supply_before);

        // Owner forfeit ends here. No owner/admin participates in the economic completion.
        // Freeze this public checkpoint's senior claims, not a full setup-history entitlement.
        let principal: [u128; PRIMARY_ACTOR_COUNT] = std::array::from_fn(|actor| {
            let account = env.primary_portfolio(actor);
            assert_eq!(account.pnl.get(), 0);
            assert_eq!(account.reserved_pnl.get(), 0);
            account.capital.get()
        });
        assert!(principal[0] > 0 && principal[0] <= deposits[0]);
        assert_eq!(principal[1], 0);
        assert_eq!(principal[2..], deposits[2..]);
        let funded = deposits.iter().sum::<u128>()
            + u128::from(has_expired_backing)
            + u128::from(has_tiny_insurance);
        assert_eq!(after.vault, funded);
        let destinations_before: [u64; PRIMARY_ACTOR_COUNT] =
            std::array::from_fn(|actor| env.token_amount(env.actors[actor].destination_token));
        let cfg = env.primary_market_state().0;
        let maturity = cfg.last_good_oracle_slot + cfg.permissionless_resolve_stale_slots;
        env.begin_public_trace();
        env.resolve_stale_permissionless(maturity)
            .expect("keeper resolves the stale resource-failure world at exact maturity");
        assert_eq!(env.primary_market_state().1.mode, MarketModeV16::Resolved);
        assert_resource_census("INV-078 resource world resolved", &env);
        let payout_slot = maturity + cfg.force_close_delay_slots;
        for use_crank in [false, true] {
            let result = if use_crank {
                env.crank(0, maturity, vec![])
            } else {
                env.close_resolved_primary(0)
            };
            let error = result.expect_err("owner window is not yet permissionless");
            assert!(error.contains(&format!(
                "Custom({})",
                PercolatorError::ExpectedSigner as u32
            )));
        }
        env.warp_to_slot(payout_slot);
        let mut paid = 0u128;
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let result = if actor % 2 == 0 {
                env.crank(actor, payout_slot, vec![])
            } else {
                env.close_resolved_primary(actor)
            };
            result.expect("one bounded keeper-only terminal call per resource-world account");
            assert_resource_terminal(&env, actor);
            let payout =
                env.token_amount(env.actors[actor].destination_token) - destinations_before[actor];
            let expected = principal[actor];
            assert_eq!(
                u128::from(payout),
                expected,
                "resource cell {resource_mask} actor {actor}"
            );
            paid += expected;
            assert_resource_census("INV-078 resource world payout", &env);
            for other in 0..PRIMARY_ACTOR_COUNT {
                let delivered = if other <= actor { principal[other] } else { 0 };
                assert_eq!(
                    env.primary_portfolio(other).capital.get(),
                    principal[other] - delivered
                );
                assert_eq!(
                    u128::from(
                        env.token_amount(env.actors[other].destination_token)
                            - destinations_before[other]
                    ),
                    delivered,
                    "resource cell {resource_mask} payout prefix {actor} owner {other}"
                );
            }
            assert_eq!(
                env.primary_market_state().1.c_tot,
                principal.iter().sum::<u128>() - paid
            );
            assert_eq!(env.primary_market_state().1.vault, funded - paid);
            assert_eq!(u128::from(env.token_amount(env.vault)), funded - paid);
            for use_crank in [false, true] {
                let retry = if use_crank {
                    env.crank(actor, payout_slot, vec![])
                } else {
                    env.close_resolved_primary(actor)
                };
                let error =
                    retry.expect_err("completed resource-world account cannot progress twice");
                assert!(error.contains(&format!(
                    "Custom({})",
                    PercolatorError::EngineNonProgress as u32
                )));
            }
        }
        let terminal = env.primary_market_state().1;
        assert_eq!(terminal.c_tot, 0);
        assert_eq!(terminal.vault, funded - principal.iter().sum::<u128>());
        assert_eq!(
            terminal.materialized_portfolio_count,
            PRIMARY_ACTOR_COUNT as u64
        );
        assert_eq!(env.token_supply_observed(), supply_before);
        let trace = env.finish_public_trace();
        trace
            .validate_public_execution()
            .expect("public keeper-only completion with exact rollback");
        assert_eq!(trace.steps.len(), 3 + 3 * PRIMARY_ACTOR_COUNT);
        assert_eq!(
            trace.steps.iter().filter(|step| step.succeeded).count(),
            1 + PRIMARY_ACTOR_COUNT
        );
        for step in &trace.steps {
            assert_eq!(step.transaction_signers, vec![step.fee_payer]);
            assert_ne!(step.fee_payer.to_bytes(), cfg.marketauth);
            assert!(env
                .actors
                .iter()
                .all(|actor| actor.signer.pubkey() != step.fee_payer));
            assert!(step
                .accounts
                .iter()
                .all(|meta| !meta.is_signer || meta.key == step.fee_payer));
        }
        eprintln!(
            "INV-078 resource cell {resource_mask}: keeper_calls={} paid={paid} remaining_vault={} max_cu={}",
            trace.steps.len(),
            terminal.vault,
            trace.steps.iter().filter_map(|step| step.compute_units).max().unwrap()
        );
    }
}
