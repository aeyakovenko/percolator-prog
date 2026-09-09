//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_source_fee_consent_route_matrix_discovers_unsigned_debits` constructs positive
//! source-backed PnL and varies the consuming trade across CPI/no-CPI, single/batch, and both
//! participant roles. Every request is retained before the backing-fee policy change. Its common
//! oracle requires the LP debit to stay within prior consent and traces any debit through the
//! backing provider's earnings into an exact public SPL withdrawal. Finding-specific impact
//! regressions remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! Secondary coverage: INV-014 because the same route matrix varies the policy state retained by
//! the signer and rejects economic terms introduced after consent.
//! `v16_program_retained_source_fees_survive_repricing_policy_and_settlement_orders` carries
//! earned source fees through later policy/repricing, paid-prefix rollback, and same/cross-route
//! retained exits. Provider SPL settlement and each trader's marked value converge in both orders.
//!
//! Guarantee boundary: PRs 223, 224, 259, 310, 313, and 314 are fixed-pin certifications here.
//! The source-fee matrix is the independent holdout oracle for delayed trader-fee consent, while
//! INV-014 certifies PR339's distinct backing-provider consent and policy-order matrix.

use super::*;

#[test]
fn v16_program_retained_source_fees_survive_repricing_policy_and_settlement_orders() {
    use crate::support::{
        fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
        v16_svm::{MarketConfig, V16Svm},
    };
    use percolator::{BOUND_SCALE, POS_SCALE};
    use percolator_prog::{ix::CrankObservationHint, processor::ASSET_AUTH_BACKING_BUCKET};
    use solana_sdk::{signature::Signer, transaction::Transaction};

    const ASSET: u16 = 1;
    const DOMAIN: u16 = 3;
    const PROVIDER: usize = 2;
    const RATE: u16 = 3_333;
    const DEPOSITS: [u128; 5] = [52_502, 2_000_000, 0, 0, 0];
    const OPEN: i128 = 1_000 * POS_SCALE as i128;
    const INCREASE: i128 = 50 * POS_SCALE as i128;

    fn settle(env: &mut V16Svm, slot: u64) {
        for actor in [1, 0] {
            let mut complete = false;
            for _ in 0..8 {
                if env
                    .crank_if_actionable(
                        actor,
                        slot,
                        vec![CrankObservationHint {
                            asset_index: ASSET,
                            oracle_accounts: 0,
                        }],
                    )
                    .unwrap()
                    .is_none()
                {
                    complete = true;
                    break;
                }
            }
            assert!(complete, "bounded public source settlement");
        }
    }

    fn frame(env: &V16Svm) -> Vec<Option<solana_sdk::account::Account>> {
        let mut keys = vec![
            env.market,
            env.foreign_market,
            env.vault,
            env.foreign_vault,
            env.mint,
            env.backing_domain_ledger,
            env.provider_source_token,
            env.provider_destination_token,
            solana_sdk::pubkey::Pubkey::new_from_array(env.primary_market_state().0.marketauth),
            env.program_id,
            env.matcher_program,
            spl_token::ID,
        ];
        for actor in &env.actors {
            keys.extend([
                actor.signer.pubkey(),
                actor.portfolio,
                actor.source_token,
                actor.destination_token,
                actor.matcher_context,
                actor.matcher_delegate,
            ]);
        }
        keys.into_iter()
            .map(|key| env.svm.get_account(&key))
            .collect()
    }

    fn reject(env: &mut V16Svm, tx: Transaction, label: &str, stale: bool) {
        use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

        let before = frame(env);
        let expected = TransactionError::InstructionError(
            u8::try_from(tx.message.instructions.len() - 1).unwrap(),
            InstructionError::Custom(if stale {
                percolator_prog::error::PercolatorError::EngineStale as u32
            } else {
                percolator_prog::error::PercolatorError::InvalidInstruction as u32
            }),
        );
        let error = env
            .land_retained(tx)
            .expect_err("retained bounds must reject");
        assert!(
            error.contains(&format!("{expected:?}")),
            "{label}: wrong rejection: {error}"
        );
        if !stale {
            assert!(error.contains(&format!("Program {} success", spl_token::ID)));
            assert!(error.contains(&format!("Program {} success", env.program_id)));
            if label == "price limit" {
                assert!(error.contains(&format!("Program {} success", env.matcher_program)));
            }
        }
        assert_eq!(frame(env), before, "exact program/SPL/matcher rollback");
    }

    let mut canonical = None;
    let mut worlds = 0;
    for source_cpi in [false, true] {
        for bilateral_exit in [false, true] {
            for policy_first in [false, true] {
                for pay_first in [false, true] {
                    let mut env = V16Svm::new(
                        [0x6e; 32],
                        MarketConfig {
                            initial_price: 100,
                            initial_margin_bps: 5_000,
                            maintenance_margin_bps: 1_000,
                            max_price_move_bps_per_slot: 500,
                            max_accrual_dt_slots: 1,
                            min_funding_lifetime_slots: 1,
                            actor_deposits: DEPOSITS,
                            ..MarketConfig::default()
                        },
                    );
                    let supply = env.token_supply_observed();
                    env.begin_public_trace();
                    let handoff = env.build_retained_asset_authority_handoff_from_admin(
                        ASSET,
                        ASSET_AUTH_BACKING_BUCKET,
                        PROVIDER,
                    );
                    env.land_retained(handoff).unwrap();
                    env.update_backing_fee_policy(DOMAIN, RATE, 0).unwrap();
                    env.top_up_backing_bucket_for_actor(PROVIDER, DOMAIN, 100_000, 100)
                        .unwrap();
                    env.trade_no_cpi(0, 1, ASSET, OPEN, 100, 0).unwrap();
                    env.warp_to_slot(2);
                    env.push_auth_mark(ASSET, 2, 105).unwrap();
                    settle(&mut env, 2);
                    assert_eq!(env.primary_portfolio(0).pnl.get(), 5_000);
                    if source_cpi {
                        env.set_matcher_config_with_trade_fee_cap(1, 1, 0).unwrap();
                        let tx = env.build_retained_cpi_trade_with_backing_fee_cap(
                            0, 1, ASSET, INCREASE, 105, RATE,
                        );
                        env.land_retained(tx).unwrap();
                    } else {
                        env.trade_no_cpi_with_backing_fee_cap(0, 1, ASSET, INCREASE, 105, 0, RATE)
                            .unwrap();
                    }
                    let group = env.primary_market_state().1;
                    let bucket = group.source_backing_buckets[DOMAIN as usize];
                    let fee = bucket.utilization_fee_earnings;
                    assert!(fee > 1);
                    assert_eq!(bucket.valid_liened_backing_num % BOUND_SCALE, 0);
                    let numerator =
                        bucket.valid_liened_backing_num / BOUND_SCALE * u128::from(RATE);
                    assert_eq!(
                        fee,
                        numerator / 10_000 + u128::from(numerator % 10_000 != 0)
                    );
                    assert_eq!(env.primary_portfolio(0).capital.get(), DEPOSITS[0] - fee);
                    assert_eq!(group.insurance, 0);
                    assert_public_stock_census("retained source fee created", &env).unwrap();
                    assert_public_encumbrance_census("retained source fee created", &env).unwrap();

                    env.set_matcher_config_with_trade_fee_cap(1, 1, 0).unwrap();
                    let retain_exit = |env: &mut V16Svm, limit| {
                        env.build_retained_cpi_trade(0, 1, ASSET, -OPEN - INCREASE, limit)
                    };
                    let price_bounded = retain_exit(&mut env, 105);
                    let fee_bounded = retain_exit(&mut env, 104);
                    let authorized = if bilateral_exit {
                        env.build_retained_no_cpi_trade(0, 1, ASSET, -OPEN - INCREASE, 104)
                    } else {
                        retain_exit(&mut env, 104)
                    };
                    let stale_policy = env.build_retained_trade_fee_policy(0);
                    let payout = env.build_retained_backing_bucket_earnings_withdrawal_for_actor(
                        PROVIDER, DOMAIN, fee,
                    );
                    let price_bundle =
                        env.bundle_retained_transactions(&[payout.clone(), price_bounded]);
                    let fee_bundle =
                        env.bundle_retained_transactions(&[payout.clone(), fee_bounded]);
                    let retained_bytes = bincode::serialize(&authorized).unwrap();
                    for tx in [
                        &price_bundle,
                        &fee_bundle,
                        &authorized,
                        &payout,
                        &stale_policy,
                    ] {
                        tx.verify().unwrap();
                        assert!(
                            bincode::serialize(tx).unwrap().len()
                                <= solana_sdk::packet::PACKET_DATA_SIZE
                        );
                        let before = frame(&env);
                        env.svm.simulate_transaction(tx.clone().into()).unwrap();
                        assert_eq!(frame(&env), before);
                    }

                    if policy_first {
                        env.update_trade_fee_policy(1).unwrap();
                    }
                    env.warp_to_slot(3);
                    env.push_auth_mark(ASSET, 3, 104).unwrap();
                    settle(&mut env, 3);
                    if !policy_first {
                        env.update_trade_fee_policy(1).unwrap();
                    }
                    assert_eq!(
                        env.primary_market_state().1.assets[ASSET as usize].effective_price,
                        104
                    );
                    reject(&mut env, fee_bundle, "fee cap", false);
                    reject(&mut env, stale_policy, "superseded policy", true);
                    env.update_trade_fee_policy(0).unwrap();
                    assert_eq!(env.primary_market_state().0.trade_fee_base_bps, 0);
                    reject(&mut env, price_bundle, "price limit", false);
                    assert_eq!(
                        env.primary_market_state().1.source_backing_buckets[DOMAIN as usize]
                            .utilization_fee_earnings,
                        fee
                    );
                    let destination = env.actors[PROVIDER].destination_token;
                    let destination_before = env.token_amount(destination);
                    let vault_before = env.token_amount(env.vault);
                    if pay_first {
                        env.land_retained(payout.clone()).unwrap();
                    }
                    assert_eq!(bincode::serialize(&authorized).unwrap(), retained_bytes);
                    env.land_retained(authorized).unwrap();
                    if !pay_first {
                        env.land_retained(payout).unwrap();
                    }
                    settle(&mut env, 3);
                    let final_group = env.primary_market_state().1;
                    assert_eq!(
                        env.token_amount(destination) - destination_before,
                        fee as u64
                    );
                    assert_eq!(vault_before - env.token_amount(env.vault), fee as u64);
                    assert_eq!(
                        final_group.source_backing_buckets[DOMAIN as usize]
                            .utilization_fee_earnings,
                        0
                    );
                    assert_eq!(final_group.insurance, 0);
                    assert_eq!(final_group.assets[ASSET as usize].oi_eff_long_q, 0);
                    assert_eq!(final_group.assets[ASSET as usize].oi_eff_short_q, 0);
                    let values = [0, 1].map(|actor| {
                        let account = env.primary_portfolio(actor);
                        account.capital.get() as i128 + account.pnl.get()
                    });
                    assert_eq!(
                        values,
                        [
                            DEPOSITS[0] as i128 + 3_950 - fee as i128,
                            DEPOSITS[1] as i128 - 3_950
                        ]
                    );
                    assert_eq!(env.token_supply_observed(), supply);
                    assert_public_stock_census("retained source fee settled", &env).unwrap();
                    assert_public_encumbrance_census("retained source fee settled", &env).unwrap();
                    env.finish_public_trace()
                        .validate_public_execution()
                        .unwrap();
                    let endpoint = (
                        fee,
                        values,
                        env.token_amount(destination),
                        env.token_amount(env.vault),
                    );
                    if let Some(expected) = &canonical {
                        assert_eq!(&endpoint, expected);
                    } else {
                        canonical = Some(endpoint);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    eprintln!("INV-036 retained source fees: {worlds} worlds, 48 exact rejections, endpoint={canonical:?}");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_036_source_fee_consent_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_fee_consent_route_matrix_discovers_unsigned_debits(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_source_fee_consent_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(
            discoveries.len(),
            SourceFeeConsentKind::ALL.len() * SourceFeeConsentRole::ALL.len()
        );
        for ((expected_kind, expected_role), discovery) in SourceFeeConsentKind::ALL
            .into_iter()
            .flat_map(|kind| SourceFeeConsentRole::ALL.into_iter().map(move |role| (kind, role)))
            .zip(&discoveries)
        {
            prop_assert_eq!(discovery.kind, expected_kind);
            prop_assert_eq!(discovery.victim_role, expected_role);
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| (discovery.kind, discovery.victim_role))
            .collect();
        eprintln!("independent source-fee consent discoveries: {violations:?}");
        for discovery in discoveries.iter().filter(|discovery| discovery.is_violation()) {
            prop_assert_eq!(discovery.lp_capital_debit, discovery.provider_earnings_credit);
            prop_assert_eq!(
                discovery.provider_earnings_credit,
                discovery.extracted_provider_tokens
            );
            let exact_terminal_loss = matches!(
                discovery.terminal_classification,
                crate::support::v16_svm::PublicTerminalClassification::LossOfFunds {
                    victim_loss_atoms,
                    unauthorized_gain_atoms,
                } if victim_loss_atoms == discovery.lp_capital_debit
                    && unauthorized_gain_atoms == discovery.extracted_provider_tokens
            );
            prop_assert!(exact_terminal_loss);
        }
        for discovery in &discoveries {
            let single_route = matches!(
                discovery.kind,
                SourceFeeConsentKind::NoCpi | SourceFeeConsentKind::Cpi
            );
            prop_assert_eq!(discovery.authorized_retry_landed, single_route);
            prop_assert_eq!(discovery.over_cap_rejected_exact_rollback, single_route);
            if single_route {
                prop_assert!(discovery.authorized_retry_lp_capital_debit > 0);
                prop_assert_eq!(
                    discovery.authorized_retry_lp_capital_debit,
                    discovery.authorized_retry_provider_earnings_credit
                );
                prop_assert_eq!(
                    discovery.authorized_retry_provider_earnings_credit,
                    discovery.authorized_retry_extracted_provider_tokens
                );
                prop_assert!(discovery.authorized_retry_compute_units.is_some());
            } else {
                prop_assert_eq!(discovery.authorized_retry_lp_capital_debit, 0);
                prop_assert_eq!(discovery.authorized_retry_provider_earnings_credit, 0);
                prop_assert_eq!(discovery.authorized_retry_extracted_provider_tokens, 0);
                prop_assert!(discovery.authorized_retry_compute_units.is_none());
            }
        }
        prop_assert!(
            violations.is_empty(),
            "retained trade accepted a backing fee that the debited trader never authorized: {violations:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr224_cpi_caller_fee_protection_fuzz(
        (seed, route) in cpi_caller_fee_strategy()
    ) {
        let protection = verify_cpi_caller_fee_protection(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.route, route);
        prop_assert_eq!(protection.requested_fee_bps, 10_000);
        prop_assert_eq!(protection.attacker_profit, 0);
        prop_assert_eq!(protection.lp_loss, 0);
        prop_assert_eq!(protection.withdrawable_insurance, 0);
        prop_assert!(protection.insurance_withdraw_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.total_payout, 2_000_000);
        prop_assert!(protection.token_supply_conserved);
        prop_assert!(protection.max_trade_cu < crate::support::v16_svm::TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_pr223_cpi_backing_fee_consent_fuzz(
        seed in cpi_backing_fee_seed_strategy()
    ) {
        let protection = verify_cpi_backing_fee_consent(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.matcher_cap_bps, 5_000);
        prop_assert!(protection.rejected_without_consent);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.unconsented_provider_earnings, 0);
        prop_assert_eq!(protection.lp_capital_loss, protection.provider_earnings);
        prop_assert!(protection.provider_earnings > 0);
        prop_assert_eq!(protection.provider_earnings, u128::from(protection.extracted_tokens));
        prop_assert_eq!(protection.attacker_capital_delta, -120);
        prop_assert!(protection.zero_cap_risk_reduction_landed);
        prop_assert!(protection.max_route_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }

    #[test]
    fn v16_program_pr314_activation_fee_consent_fuzz(
        seed in activation_fee_consent_seed_strategy()
    ) {
        let protection = verify_activation_fee_consent(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(protection.stale_policy_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.unconsented_creator_loss, 0);
        prop_assert_eq!(protection.unconsented_insurance_delta, 0);
        prop_assert_eq!(protection.charged_fee, protection.current_fee);
        prop_assert_eq!(protection.insured_fee, u128::from(protection.current_fee));
        prop_assert!(protection.current_fee <= protection.consented_max_fee);
        prop_assert!(protection.asset_active);
        prop_assert!(protection.activation_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }

    #[test]
    fn v16_program_pr313_cpi_base_fee_consent_protection_fuzz(
        (seed, route) in cpi_base_fee_consent_strategy()
    ) {
        let protection = verify_cpi_base_fee_consent(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.route, route);
        prop_assert!(protection.invalid_cap_rejected);
        prop_assert!(protection.invalid_cap_exact_rollback);
        prop_assert!(protection.stale_fill_rejected);
        prop_assert!(protection.stale_fill_exact_rollback);
        prop_assert!(protection.position_epoch_preserved);
        prop_assert_eq!(protection.unconsented_lp_loss, 0);
        prop_assert_eq!(protection.unconsented_insurance_delta, 0);
        prop_assert_eq!(protection.consented_lp_fee, 100_000);
        prop_assert_eq!(protection.consented_insurance_fee, 200_000);
        prop_assert_eq!(protection.total_payout, 200_000_000);
        prop_assert!(protection.max_route_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }

    #[test]
    fn v16_program_pr310_bilateral_base_fee_consent_protection_fuzz(
        (seed, route) in bilateral_base_fee_consent_strategy()
    ) {
        let protection = verify_bilateral_base_fee_consent(seed, route)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(protection.route, route);
        prop_assert!(protection.stale_open_rejected);
        prop_assert!(protection.stale_close_rejected);
        prop_assert!(protection.rejected_exact_rollback);
        prop_assert_eq!(protection.unconsented_victim_loss, 0);
        prop_assert_eq!(protection.unconsented_insurance_delta, 0);
        prop_assert_eq!(protection.consented_victim_fee, 100_000);
        prop_assert_eq!(protection.consented_insurance_fee, 200_000);
        prop_assert_eq!(protection.total_payout, 200_000_000);
        prop_assert!(protection.open_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.close_cu < crate::support::v16_svm::TX_CU_LIMIT);
        prop_assert!(protection.token_supply_conserved);
    }
}
