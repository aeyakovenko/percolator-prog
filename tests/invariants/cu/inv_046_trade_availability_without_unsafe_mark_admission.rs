//! INV-046 - Trade availability without unsafe mark admission.
//!
//! Normative obligation: Unsafe raw prices cannot poison state or remove every bounded user exit route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_nocpi_high_notional_ewma_exit_not_dosed_by_extreme_reported_price`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! The invalid-Hybrid-report history adds a stale losing owner's unilateral exit, with
//! independently priced loss, current certificate lanes, exact rollback, and funded withdrawal.
//! Its two-asset sibling covers a losing non-base long, late invalid observation rollback,
//! and combined certificate requirements while a separate short leg remains open.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_invalid_non_base_hybrid_report_preserves_mirrored_sibling_exit() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::support::fuzz_model::assert_current_certificate_matches_independent;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const CAPITAL: u128 = 2_000_000;
    const ENTRY: [u64; 2] = [2_000_000, 1_000_000];
    const ADMITTED: [u64; 2] = [2_000_000, 950_000];
    const CAP_BPS: u64 = 500;
    const LOSS: u128 = 2 * (ENTRY[1] - ADMITTED[1]) as u128;

    let mut env = inv018_public_spl_market_with_params(
        6,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: ENTRY[0],
            initial_margin_bps: 1_000,
            maintenance_margin_bps: 1_000,
            max_price_move_bps_per_slot: CAP_BPS,
            ..V16CuMarketParams::default()
        },
    );
    set_test_clock(&mut env, 1, 100);
    let feeds = [[0xea; 32], [0xeb; 32]];
    // Provider-owned input fixtures only; all market, portfolio and custody writes are public.
    let initial = [0, 1].map(|i| {
        let report = env.set_pyth_price_with_conf(&feeds[i], ENTRY[i] as i64, -6, 0, 100);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            i as u16,
            1,
            0,
            [feeds[i], [0; 32], [0; 32]],
            &[report],
            1,
            100,
            0,
            0,
            100,
            100,
        )
        .expect("configure independent Hybrid feeds publicly");
        report
    });

    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let mut portfolios = [Pubkey::default(); 3];
    let mut tokens = [Pubkey::default(); 3];
    for i in 0..3 {
        env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            env.program_id,
        );
        portfolios[i] = portfolio.pubkey();
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owners[i].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[i], false),
            ],
            &[&owners[i]],
        )
        .unwrap();
        env.portfolios.push(portfolios[i]);
        tokens[i] = create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
        if i < 2 {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[i],
                    &env.admin.pubkey(),
                    &[],
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap();
        }
    }
    let [owner, counterparty, observer] = portfolios;
    for (asset, size) in [(0, -(POS_SCALE as i128)), (1, (2 * POS_SCALE) as i128)] {
        let cu = env.trade_asset_with_cu(
            asset,
            &owners[0],
            owner,
            &owners[1],
            counterparty,
            size,
            ENTRY[asset as usize],
            0,
        );
        assert_cu_within("two-asset Hybrid opening trade", cu, TRADE_CU_LIMIT);
    }
    let old_owner = env.svm.get_account(&owner).unwrap();
    let old_cert = health_cert(&env.portfolio_state(owner));
    assert!(old_cert.valid);
    assert_eq!(old_cert.certified_equity, CAPITAL as i128);
    assert_eq!(old_cert.certified_initial_req, 400_000);
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(owner), 1).basis_pos_q,
        (2 * POS_SCALE) as i128
    );

    set_test_clock(&mut env, 2, 101);
    let admitted =
        [0, 1].map(|i| env.set_pyth_price_with_conf(&feeds[i], ADMITTED[i] as i64, -6, 0, 101));
    let observations = || {
        [0, 1]
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: 1,
            })
            .to_vec()
    };
    let observe_cu = env.crank_with_oracle_tail(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: observations(),
        },
        &admitted,
    );
    assert_cu_within(
        "two-asset admitted Hybrid observation",
        observe_cu,
        CRANK_CU_LIMIT,
    );
    let moved = env.market_state().1;
    assert!(old_cert.cert_oracle_epoch < moved.oracle_epoch);
    assert_eq!(env.svm.get_account(&owner).unwrap(), old_owner);
    let profiles = |env: &V16CuEnv| {
        let market = env.svm.get_account(&env.market).unwrap();
        [0, 1].map(|i| state::read_asset_oracle_profile(&market.data, i).unwrap())
    };
    let admitted_profiles = profiles(&env);
    for i in 0..2 {
        assert_eq!(moved.assets[i].effective_price, ADMITTED[i]);
        assert_eq!(moved.assets[i].raw_oracle_target_price, ADMITTED[i]);
        assert_eq!(moved.assets[i].slot_last, 2);
        let envelope = ENTRY[i] * CAP_BPS / 10_000;
        assert!(ENTRY[i].abs_diff(moved.assets[i].effective_price) <= envelope);
        assert_eq!(admitted_profiles[i].oracle_target_price_e6, ADMITTED[i]);
        assert!(admitted_profiles[i].mark_ewma_e6 > 0);
        assert!(ENTRY[i].abs_diff(admitted_profiles[i].mark_ewma_e6) <= envelope);
        assert_eq!(
            admitted_profiles[i].oracle_leg_prices_e6,
            [ADMITTED[i], 0, 0]
        );
        assert_eq!(admitted_profiles[i].oracle_leg_publish_times, [101, 0, 0]);
        assert_eq!(admitted_profiles[i].last_good_oracle_slot, 2);
    }

    // A valid sibling update precedes the invalid non-base report in the same crank.
    // No successful observation, admin action or counterparty signature follows this point.
    set_test_clock(&mut env, 3, 102);
    let sibling_report = env.set_pyth_price_with_conf(&feeds[0], 2_100_000, -6, 0, 102);
    let invalid = env.set_pyth_price_with_conf(&feeds[1], 0, -6, 0, 102);
    let tracked = [
        env.market,
        owner,
        counterparty,
        observer,
        env.vault,
        env.mint,
        tokens[0],
        tokens[1],
        tokens[2],
        initial[0],
        initial[1],
        admitted[0],
        admitted[1],
        sibling_report,
        invalid,
        owners[0].pubkey(),
        owners[1].pubkey(),
        owners[2].pubkey(),
        env.admin.pubkey(),
    ];
    let before = tracked.map(|key| env.svm.get_account(&key).unwrap());
    let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    let rejected = env
        .svm
        .send_transaction(Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(owner, false),
                        AccountMeta::new_readonly(sibling_report, false),
                        AccountMeta::new_readonly(invalid, false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: 3,
                        observations: observations(),
                    }
                    .encode(),
                },
            ],
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        ))
        .expect_err("a zero-price non-base report rolls back the complete observation set");
    assert_eq!(
        rejected.err,
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(PercolatorError::OracleInvalid as u32)
        )
    );
    assert_eq!(
        tracked.map(|key| env.svm.get_account(&key).unwrap()),
        before
    );
    payer_before.lamports -= FeeStructure::default().lamports_per_signature;
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer_before
    );
    let reject_cu = rejected.meta.compute_units_consumed;
    assert_cu_within(
        "invalid non-base Hybrid observation",
        reject_cu,
        CRANK_CU_LIMIT,
    );

    let mut reduction_cus = Vec::new();
    for (asset, remaining) in [
        (1, [POS_SCALE, POS_SCALE]),
        (1, [POS_SCALE, 0]),
        (0, [0, 0]),
    ] {
        env.svm.expire_blockhash();
        let cu = env.rebalance_reduce_with_cu(&owners[0], owner, asset, POS_SCALE);
        reduction_cus.push(cu);
        assert_cu_within(
            "mirrored sibling owner-only reduction",
            cu,
            CUSTODY_CU_LIMIT,
        );
        let group = env.market_state().1;
        let account = env.portfolio_state(owner);
        let cert = health_cert(&account);
        assert_eq!(profiles(&env), admitted_profiles);
        for i in 0..2 {
            assert_eq!(group.assets[i].effective_price, ADMITTED[i]);
            assert_eq!(group.assets[i].raw_oracle_target_price, ADMITTED[i]);
            assert_eq!(group.assets[i].slot_last, 2);
            assert_eq!(group.assets[i].oi_eff_long_q, remaining[i]);
            assert_eq!(group.assets[i].oi_eff_short_q, remaining[i]);
            if remaining[i] == 0 {
                assert!(!has_active_leg_for_asset(&account, i));
            } else {
                let sign = if i == 0 { -1 } else { 1 };
                assert_eq!(
                    active_leg_for_asset(&account, i).basis_pos_q,
                    sign * remaining[i] as i128
                );
            }
        }
        if asset == 1 {
            assert_eq!(
                group.assets[0], moved.assets[0],
                "reducing the long leaves the entire sibling asset unchanged"
            );
        }
        let notional = remaining[0] * ADMITTED[0] as u128 / POS_SCALE
            + remaining[1] * ADMITTED[1] as u128 / POS_SCALE;
        assert_eq!(account.capital.get(), CAPITAL - LOSS);
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(cert.certified_equity, (CAPITAL - LOSS) as i128);
        assert_eq!(cert.certified_initial_req, notional / 10);
        assert_eq!(cert.certified_maintenance_req, notional / 10);
        assert_eq!(cert.certified_worst_case_loss, notional);
        assert_eq!(cert.certified_liq_deficit, 0);
        assert!(assert_current_certificate_matches_independent(
            "mirrored sibling owner-only exit",
            &group,
            &account
        )
        .unwrap());
        assert_eq!(group.c_tot, 2 * CAPITAL - LOSS);
        assert_eq!(group.vault, 2 * CAPITAL);
        assert_eq!(group.insurance, 0);
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        for (key, original) in tracked.iter().zip(&before) {
            if ![env.market, owner].contains(key) {
                assert_eq!(&env.svm.get_account(key).unwrap(), original);
            }
        }
    }

    let flat_cert = health_cert(&env.portfolio_state(owner));
    let withdraw_cu = env
        .send(
            env.withdraw_ix(owner, CAPITAL - LOSS),
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(owner, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[0]],
        )
        .expect("the owner exits both assets without repairing the invalid report");
    assert_cu_within(
        "mirrored sibling principal withdrawal",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    let group = env.market_state().1;
    let account = env.portfolio_state(owner);
    assert_eq!(account.capital.get(), 0);
    assert_eq!(account.pnl.get(), 0);
    let mut withdrawn_cert = flat_cert;
    withdrawn_cert.valid = false;
    assert_eq!(health_cert(&account), withdrawn_cert);
    assert!(!assert_current_certificate_matches_independent(
        "mirrored sibling withdrawal",
        &group,
        &account
    )
    .unwrap());
    assert_eq!(group.c_tot, CAPITAL);
    assert_eq!(group.vault, CAPITAL + LOSS);
    assert_eq!(group.insurance, 0);
    assert_eq!(env.token_amount(tokens[0]) as u128, CAPITAL - LOSS);
    assert_eq!(env.token_amount(env.vault) as u128, CAPITAL + LOSS);
    assert_eq!(profiles(&env), admitted_profiles);
    assert_eq!(
        [0, 1].map(|i| (
            group.assets[i].oi_eff_long_q,
            group.assets[i].oi_eff_short_q
        )),
        [(0, 0); 2]
    );
    for (key, original) in tracked.iter().zip(&before) {
        if ![env.market, owner, env.vault, tokens[0]].contains(key) {
            assert_eq!(&env.svm.get_account(key).unwrap(), original);
        }
    }
    let supply = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
        .unwrap()
        .supply;
    assert_eq!(supply as u128, 2 * CAPITAL);
    assert_eq!(
        env.token_amount(env.vault) + tokens.map(|key| env.token_amount(key)).iter().sum::<u64>(),
        supply
    );
    eprintln!("invalid non-base Hybrid report: observe CU={observe_cu}, reject CU={reject_cu}, reductions CU={reduction_cus:?}, withdrawal CU={withdraw_cu}");
}

#[test]
fn v16_program_invalid_hybrid_report_preserves_stale_owner_only_exit() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use crate::support::fuzz_model::assert_current_certificate_matches_independent;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const CAPITAL: u128 = 1_000_000;
    const OPEN_PRICE: u64 = 1_000_000;
    const ADMITTED_PRICE: u64 = 1_050_000;
    const LOSS: u128 = 2 * (ADMITTED_PRICE - OPEN_PRICE) as u128;

    let mut env = inv018_public_spl_market_with_params(
        6,
        V16CuMarketParams {
            initial_price: OPEN_PRICE,
            initial_margin_bps: 1_000,
            maintenance_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    set_test_clock(&mut env, 1, 100);
    let feed = [0xe9; 32];
    let initial = env.set_pyth_price_with_conf(&feed, OPEN_PRICE as i64, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0; 32], [0; 32]],
        &[initial],
        1,
        100,
        0,
        0,
        100,
        100,
    )
    .expect("configure one authenticated Hybrid feed");

    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let mut portfolios = [Pubkey::default(); 3];
    let mut tokens = [Pubkey::default(); 3];
    for i in 0..3 {
        env.svm.airdrop(&owners[i].pubkey(), 1_000_000_000).unwrap();
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            env.program_id,
        );
        portfolios[i] = portfolio.pubkey();
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owners[i].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[i], false),
            ],
            &[&owners[i]],
        )
        .unwrap();
        env.portfolios.push(portfolios[i]);
        tokens[i] = create_ata_for_test(&mut env.svm, &env.payer, owners[i].pubkey(), env.mint);
        if i < 2 {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[i],
                    &env.admin.pubkey(),
                    &[],
                    CAPITAL as u64,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[i], CAPITAL),
                vec![
                    AccountMeta::new(owners[i].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(tokens[i], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[i]],
            )
            .unwrap();
        }
    }
    let [short, long, observer] = portfolios;
    env.trade_asset_with_cu(
        0,
        &owners[0],
        short,
        &owners[1],
        long,
        -((2 * POS_SCALE) as i128),
        OPEN_PRICE,
        0,
    );
    let old_account = env.svm.get_account(&short).unwrap();
    let old_cert = health_cert(&env.portfolio_state(short));
    assert!(old_cert.valid);
    assert_eq!(old_cert.certified_equity, CAPITAL as i128);
    assert_eq!(old_cert.certified_initial_req, 200_000);

    set_test_clock(&mut env, 2, 101);
    let admitted = env.set_pyth_price_with_conf(&feed, ADMITTED_PRICE as i64, -6, 0, 101);
    let observe_cu = env.crank_with_oracle_tail(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, 1),
        },
        &[admitted],
    );
    assert_cu_within("admitted Hybrid observation", observe_cu, CRANK_CU_LIMIT);
    let moved = env.market_state().1;
    assert_eq!(moved.assets[0].effective_price, ADMITTED_PRICE);
    assert_eq!(moved.assets[0].raw_oracle_target_price, ADMITTED_PRICE);
    assert_eq!(moved.assets[0].slot_last, 2);
    assert!(old_cert.cert_oracle_epoch < moved.oracle_epoch);
    assert_eq!(env.svm.get_account(&short).unwrap(), old_account);
    let admitted_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(
        admitted_profile.oracle_leg_prices_e6,
        [ADMITTED_PRICE, 0, 0]
    );
    assert_eq!(admitted_profile.oracle_leg_publish_times, [101, 0, 0]);
    assert_eq!(admitted_profile.last_good_oracle_slot, 2);

    // The next report has correct provenance and current time but is outside the price domain.
    // No successful observation, admin action, or counterparty signature follows this point.
    set_test_clock(&mut env, 3, 102);
    let invalid = env.set_pyth_price_with_conf(
        &feed,
        i64::try_from(percolator::MAX_ORACLE_PRICE + 1).unwrap(),
        -6,
        0,
        102,
    );
    let tracked = [
        env.market,
        short,
        long,
        observer,
        env.vault,
        env.mint,
        tokens[0],
        tokens[1],
        tokens[2],
        initial,
        admitted,
        invalid,
        owners[0].pubkey(),
        owners[1].pubkey(),
        owners[2].pubkey(),
        env.admin.pubkey(),
    ];
    let before = tracked.map(|key| env.svm.get_account(&key).unwrap());
    let mut payer_before = env.svm.get_account(&env.payer.pubkey()).unwrap();
    let rejected = env
        .svm
        .send_transaction(Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(short, false),
                        AccountMeta::new_readonly(invalid, false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: 3,
                        observations: crank_observations_with_accounts(0, 1),
                    }
                    .encode(),
                },
            ],
            Some(&env.payer.pubkey()),
            &[&env.payer],
            env.svm.latest_blockhash(),
        ))
        .expect_err("an out-of-range report cannot refresh a stale owner certificate");
    assert_eq!(
        rejected.err,
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(PercolatorError::OracleInvalid as u32)
        ),
    );
    assert_eq!(
        tracked.map(|key| env.svm.get_account(&key).unwrap()),
        before
    );
    payer_before.lamports -= FeeStructure::default().lamports_per_signature;
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer_before
    );
    assert_cu_within(
        "invalid Hybrid owner refresh",
        rejected.meta.compute_units_consumed,
        CRANK_CU_LIMIT,
    );

    let mut max_exit_cu = 0;
    for remaining in [POS_SCALE, 0] {
        env.svm.expire_blockhash();
        let cu = env.rebalance_reduce_with_cu(&owners[0], short, 0, POS_SCALE);
        max_exit_cu = max_exit_cu.max(cu);
        assert_cu_within("Hybrid owner-only reduction", cu, CUSTODY_CU_LIMIT);
        let group = env.market_state().1;
        let account = env.portfolio_state(short);
        let cert = health_cert(&account);
        assert_eq!(group.assets[0].effective_price, ADMITTED_PRICE);
        assert_eq!(group.assets[0].raw_oracle_target_price, ADMITTED_PRICE);
        assert_eq!(
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
                .unwrap(),
            admitted_profile
        );
        assert_eq!(group.assets[0].oi_eff_long_q, remaining);
        assert_eq!(group.assets[0].oi_eff_short_q, remaining);
        assert_eq!(account.capital.get(), CAPITAL - LOSS);
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(cert.certified_equity, (CAPITAL - LOSS) as i128);
        assert_eq!(
            cert.certified_initial_req,
            remaining * ADMITTED_PRICE as u128 / POS_SCALE / 10
        );
        assert_eq!(cert.certified_maintenance_req, cert.certified_initial_req);
        assert_eq!(
            cert.certified_worst_case_loss,
            remaining * ADMITTED_PRICE as u128 / POS_SCALE
        );
        assert_eq!(cert.certified_liq_deficit, 0);
        assert!(assert_current_certificate_matches_independent(
            "Hybrid owner-only exit",
            &group,
            &account
        )
        .unwrap());
        assert_eq!(group.c_tot, 2 * CAPITAL - LOSS);
        assert_eq!(group.vault, 2 * CAPITAL);
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        assert_eq!(group.insurance, 0);
        assert_eq!(env.svm.get_account(&long).unwrap(), before[2]);
        if remaining != 0 {
            assert_eq!(
                active_leg_for_asset(&account, 0).basis_pos_q,
                -(remaining as i128)
            );
        } else {
            assert!(!has_active_leg_for_asset(&account, 0));
        }
    }

    let withdraw = env.withdraw_ix(short, CAPITAL - LOSS);
    let cu = env
        .send(
            withdraw,
            vec![
                AccountMeta::new(owners[0].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
                AccountMeta::new(tokens[0], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[0]],
        )
        .expect("the flat owner withdraws at the admitted loss without repairing the report");
    assert_cu_within("Hybrid owner principal exit", cu, CUSTODY_CU_LIMIT);
    max_exit_cu = max_exit_cu.max(cu);
    assert_eq!(env.token_amount(tokens[0]) as u128, CAPITAL - LOSS);
    assert_eq!(env.portfolio_state(short).capital.get(), 0);
    let group = env.market_state().1;
    assert_eq!(group.c_tot, CAPITAL);
    assert_eq!(group.vault, CAPITAL + LOSS);
    assert_eq!(env.token_amount(env.vault) as u128, CAPITAL + LOSS);
    for (key, original) in tracked.iter().zip(&before) {
        if ![env.market, short, env.vault, tokens[0]].contains(key) {
            assert_eq!(&env.svm.get_account(key).unwrap(), original);
        }
    }
    assert_eq!(
        Mint::unpack(&before[5].data).unwrap().supply as u128,
        2 * CAPITAL
    );
    eprintln!("invalid Hybrid report: exact rollback, two owner reductions and withdrawal; max exit CU={max_exit_cu}");
}

#[test]
fn v16_attack_nocpi_high_notional_ewma_exit_not_dosed_by_extreme_reported_price() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const DEPOSIT: u128 = 4_900_000_000_000;
    const OPEN_Q: i128 = 4_800_000_000_000;
    const CLOSE_Q: i128 = -(POS_SCALE as i128);

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: 37,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(1);
        env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
        env.svm.warp_to_slot(5);
        let (owner_a, account_a, owner_b, account_b) =
            funded_no_cpi_reported_price_pair(&mut env, DEPOSIT);

        try_no_cpi_reported_price_trade_with_cu(
            &mut env, path, &owner_a, account_a, &owner_b, account_b, OPEN_Q, MARK, 0,
        )
        .unwrap_or_else(|err| panic!("{path:?}: high-notional setup open failed: {err}"));
        let (_, opened_group) = env.market_state();
        assert_eq!(
            opened_group.assets[0].oi_eff_long_q,
            OPEN_Q.unsigned_abs(),
            "{path:?}: setup creates high-notional long OI"
        );
        assert_eq!(
            opened_group.assets[0].oi_eff_short_q,
            OPEN_Q.unsigned_abs(),
            "{path:?}: setup creates high-notional short OI"
        );
        assert!(
            opened_group.vault < percolator::MAX_VAULT_TVL,
            "{path:?}: setup stays public-reachable under the vault cap"
        );

        env.svm.expire_blockhash();
        let exit = try_no_cpi_reported_price_trade_with_cu(
            &mut env,
            path,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            CLOSE_Q,
            percolator::MAX_ORACLE_PRICE,
            0,
        );
        assert!(
            exit.is_ok(),
            "{path:?}: high-notional EWMA exit must not be DoSed by valid extreme reported price: {exit:?}"
        );
        let (_, after) = env.market_state();
        assert_eq!(
            after.assets[0].oi_eff_long_q,
            opened_group.assets[0].oi_eff_long_q - POS_SCALE,
            "{path:?}: high-notional exit reduces long OI"
        );
        assert_eq!(
            after.assets[0].oi_eff_short_q,
            opened_group.assets[0].oi_eff_short_q - POS_SCALE,
            "{path:?}: high-notional exit reduces short OI"
        );
        assert_eq!(
            after.vault as u64,
            env.token_amount(env.vault),
            "{path:?}: high-notional EWMA exit keeps vault accounting tied to SPL custody"
        );
        assert!(
            after.vault >= after.c_tot + after.insurance,
            "{path:?}: high-notional EWMA exit preserves senior conservation"
        );
    }
}

#[test]
fn v16_bpf_tradenocpi_allows_off_mark_strict_reduction_without_value_extraction() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let (_, before_group) = state::read_market(&before_market.data).unwrap();
    assert_eq!(before_group.insurance, 1_000_000);
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    let vault_tokens_before = env.token_amount(env.vault);
    assert!(
        health_cert(&before_probe_state).certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted recycling trade"
    );

    let close_cu = env
        .try_trade_asset_with_cu(
            0,
            &extractor_owner,
            extractor,
            &probe_owner,
            probe,
            -((10 * POS_SCALE) as i128),
            500,
            0,
        )
        .expect("an extreme reported price cannot block a bilateral strict reduction");
    assert_cu_within(
        "off-mark TradeNoCpi strict reduction",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let (_, after_group) = env.market_state();
    let after_extractor = env.portfolio_state(extractor);
    let after_probe = env.portfolio_state(probe);
    let pair_equity_after = after_extractor.capital.get() as i128
        + after_extractor.pnl.get()
        + after_probe.capital.get() as i128
        + after_probe.pnl.get();
    assert!(!has_active_leg_for_asset(&after_extractor, 0));
    assert!(!has_active_leg_for_asset(&after_probe, 0));
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        before_group.assets[0].raw_oracle_target_price
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    let fair_gain = 10 * (before_group.assets[0].effective_price as i128 - 100);
    assert_eq!(after_extractor.pnl.get(), fair_gain);
    assert_eq!(after_probe.pnl.get(), -(fair_gain - 1_000));
    assert_eq!(pair_equity_after, 11_000);
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}

#[test]
fn v16_bpf_tradecpi_allows_off_mark_strict_reduction_without_value_extraction() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let (matcher_ctx, matcher_delegate, _) = env
        .init_matcher_context_with_passive_spread_authorized(
            matcher_program,
            &extractor_owner,
            extractor,
            9_000,
            9_000,
        );
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let before_matcher = env.svm.get_account(&matcher_ctx).unwrap();
    let (before_cfg, before_group) = state::read_market(&before_market.data).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    let vault_tokens_before = env.token_amount(env.vault);
    assert!(
        health_cert(&before_probe_state).certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted matcher recycling trade"
    );

    let close_cu = env
        .try_trade_cpi_with_cu_on_asset(
            &probe_owner,
            probe,
            &extractor_owner,
            extractor,
            matcher_program,
            matcher_ctx,
            matcher_delegate,
            0,
            (10 * POS_SCALE) as i128,
            0,
        )
        .expect("an extreme matcher quote cannot block a bilateral strict reduction");
    assert_cu_within(
        "off-mark TradeCpi strict reduction",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let (after_cfg, after_group) = env.market_state();
    let after_extractor = env.portfolio_state(extractor);
    let after_probe = env.portfolio_state(probe);
    let pair_equity_after = after_extractor.capital.get() as i128
        + after_extractor.pnl.get()
        + after_probe.capital.get() as i128
        + after_probe.pnl.get();
    assert!(!has_active_leg_for_asset(&after_extractor, 0));
    assert!(!has_active_leg_for_asset(&after_probe, 0));
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        before_group.assets[0].raw_oracle_target_price
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    let fair_gain = 10 * (before_group.assets[0].effective_price as i128 - 100);
    assert_eq!(after_extractor.pnl.get(), fair_gain);
    assert_eq!(after_probe.pnl.get(), -(fair_gain - 1_000));
    assert_eq!(pair_equity_after, 11_000);
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
    assert_eq!(after_cfg.matcher_req_seq, before_cfg.matcher_req_seq + 1);
    assert_ne!(
        env.svm.get_account(&matcher_ctx).unwrap().data,
        before_matcher.data,
        "the successful CPI fill commits its matcher response"
    );
}

#[test]
fn v16_bpf_batch_tradecpi_allows_off_mark_strict_reduction_without_value_extraction() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );

    let (matcher_ctx, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &extractor_owner, extractor);
    let mut configure_spread = vec![4u8];
    configure_spread.extend_from_slice(&9_000u64.to_le_bytes());
    configure_spread.extend_from_slice(&9_000u64.to_le_bytes());
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: matcher_program,
            accounts: vec![
                AccountMeta::new_readonly(extractor_owner.pubkey(), true),
                AccountMeta::new(matcher_ctx, false),
            ],
            data: configure_spread,
        },
        &[&extractor_owner],
    )
    .expect("configure auth matcher spread");

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let (_, before_group) = state::read_market(&before_market.data).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    let vault_tokens_before = env.token_amount(env.vault);
    assert!(
        health_cert(&before_probe_state).certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted batch matcher recycling trade"
    );

    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            env.batch_trade_cpi_ix(
                probe,
                extractor,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    size_q: (10 * POS_SCALE) as i128,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(probe_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(probe, false),
                AccountMeta::new(extractor, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_ctx, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[&probe_owner],
        )
        .expect(
            "a one-leg batch CPI extreme matcher quote cannot block a bilateral strict reduction",
        );
    assert_cu_within(
        "off-mark BatchTradeCpi strict reduction",
        close_cu,
        TRADE_CU_LIMIT,
    );

    let (_, after_group) = env.market_state();
    let after_extractor = env.portfolio_state(extractor);
    let after_probe = env.portfolio_state(probe);
    let pair_equity_after = after_extractor.capital.get() as i128
        + after_extractor.pnl.get()
        + after_probe.capital.get() as i128
        + after_probe.pnl.get();
    assert!(!has_active_leg_for_asset(&after_extractor, 0));
    assert!(!has_active_leg_for_asset(&after_probe, 0));
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        before_group.assets[0].raw_oracle_target_price
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    let fair_gain = 10 * (before_group.assets[0].effective_price as i128 - 100);
    assert_eq!(after_extractor.pnl.get(), fair_gain);
    assert_eq!(after_probe.pnl.get(), -(fair_gain - 1_000));
    assert_eq!(pair_equity_after, 11_000);
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}
