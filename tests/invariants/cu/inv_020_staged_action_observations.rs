//! Row 426: complete current evidence across market progress, certification, and account action.
//! All economic state is constructed with System/SPL/ATA/wrapper instructions. Snapshot replay
//! copies captured whole Accounts verbatim; it never fabricates or edits program-owned fields.
//! Clock and external Pyth fixtures are the only harness-supplied authenticated inputs.
//! The active-account selector covers empty/Hybrid-only omissions; see the row-426
//! staged-observation entry in ../README.md for its omitted-Hybrid residual gap.
//! The separate flat-account withdrawal word below covers omitted Hybrid discovery and mature
//! stale-report fallback without claiming active-leg certificate or liquidation equivalence.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

#[path = "inv_020_mixed_provider_liquidation.rs"]
mod mixed_provider_liquidation;

#[path = "inv_020_interrupted_refresh_fees.rs"]
mod interrupted_refresh_fees;

#[path = "inv_020_partial_observation_routes.rs"]
mod partial_observation_routes;

#[path = "inv_020_active_keeper_observations.rs"]
mod active_keeper_observations;

#[path = "inv_020_renewed_liquidation.rs"]
mod renewed_liquidation;

const PRICE: u64 = 1_000_000;
const CURRENT: [u64; 2] = [1_040_000, 1_050_000];
const DEPOSITS: [u128; 3] = [10_000_000, 220_000, 1_000];

fn funded_owner(env: &mut V16CuEnv, owner: &Keypair, amount: u128) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[owner],
    )
    .expect("public portfolio initialization");
    env.portfolios.push(portfolio.pubkey());
    let tokens = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &env.mint,
            &tokens,
            &env.admin.pubkey(),
            &[],
            amount as u64,
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("public collateral mint");
    env.send(
        env.deposit_ix(portfolio.pubkey(), amount),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(tokens, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("public collateral deposit");
    (portfolio.pubkey(), tokens)
}

#[derive(Clone, Copy, Debug)]
enum Evidence {
    Full,
    Reverse,
    Empty,
    HybridOnly,
}

impl Evidence {
    fn complete(self) -> bool {
        matches!(self, Self::Full | Self::Reverse)
    }
}

fn observe(
    env: &mut V16CuEnv,
    target: Pubkey,
    keeper: &Keypair,
    reward: Pubkey,
    report: Pubkey,
    evidence: Evidence,
) -> Result<u64, String> {
    let mut observations = match evidence {
        Evidence::Full | Evidence::Reverse => vec![
            CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 1,
            },
            CrankObservationHint {
                asset_index: 1,
                oracle_accounts: 0,
            },
        ],
        Evidence::Empty => vec![],
        Evidence::HybridOnly => crank_observations_with_accounts(0, 1),
    };
    if matches!(evidence, Evidence::Reverse) {
        observations.reverse();
    }
    let mut accounts = vec![
        AccountMeta::new(keeper.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(target, false),
    ];
    if !matches!(evidence, Evidence::Empty) {
        accounts.push(AccountMeta::new_readonly(report, false));
    }
    accounts.push(AccountMeta::new(reward, false));
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations,
        },
        accounts,
        &[keeper],
    )
}

fn reduce(
    env: &mut V16CuEnv,
    owners: &[Keypair; 3],
    portfolios: [Pubkey; 3],
) -> Result<u64, String> {
    let ix = env.trade_no_cpi_ix(
        portfolios[0],
        portfolios[1],
        0,
        -(POS_SCALE as i128),
        CURRENT[0],
        0,
    );
    env.svm.expire_blockhash();
    env.send(
        ix,
        vec![
            AccountMeta::new(owners[0].pubkey(), true),
            AccountMeta::new(owners[1].pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolios[0], false),
            AccountMeta::new(portfolios[1], false),
        ],
        &[&owners[0], &owners[1]],
    )
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<(Pubkey, Account)> {
    keys.iter()
        .map(|key| (*key, env.svm.get_account(key).unwrap()))
        .collect()
}

fn replay(env: &mut V16CuEnv, snapshot: &[(Pubkey, Account)]) {
    // Restore only exact public snapshots, including Clock. No field-level state mutation.
    for (key, account) in snapshot {
        env.svm.set_account(*key, account.clone()).unwrap();
    }
}

fn assert_current_short(env: &V16CuEnv, short: Pubkey, equity: u128, margin: u128) {
    let group = env.market_state().1;
    let account = env.portfolio_state(short);
    let cert = health_cert(&account);
    assert!(cert.valid);
    assert_eq!(cert.cert_oracle_epoch, group.oracle_epoch);
    assert_eq!(cert.cert_funding_epoch, group.funding_epoch);
    assert_eq!(cert.cert_risk_epoch, group.risk_epoch);
    assert_eq!(cert.cert_asset_set_epoch, group.asset_set_epoch);
    assert_eq!(cert.active_bitmap_at_cert, active_bitmap(&account));
    assert_eq!(account.capital.get(), equity);
    assert_eq!(account.pnl.get(), 0);
    assert_eq!(cert.certified_equity, equity as i128);
    assert_eq!(cert.certified_initial_req, margin);
    assert_eq!(cert.certified_maintenance_req, margin);
    assert_eq!(cert.certified_liq_deficit, margin.saturating_sub(equity));
    for (asset, price) in group.assets[..2].iter().zip(CURRENT) {
        assert_eq!(asset.effective_price, price);
        assert_eq!(asset.raw_oracle_target_price, price);
        assert_eq!(asset.slot_last, 64);
        assert_eq!(asset.f_long_num, 0);
        assert_eq!(asset.f_short_num, 0);
    }
}

#[test]
fn v16_program_staged_observations_match_current_liquidation_and_reduction() {
    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: PRICE,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 10,
            max_accrual_dt_slots: 64,
            min_funding_lifetime_slots: 64,
            liquidation_fee_bps: 100,
            liquidation_fee_cap: 10_000,
            ..V16CuMarketParams::default()
        },
    );
    env.update_liquidation_fee_policy_with_cu(5_000);
    set_test_clock(&mut env, 0, 100);
    let feed = [0xb6; 32];
    let initial = env.set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0; 32], [0; 32]],
        &[initial],
        0,
        100,
        0,
        0,
        100,
        0,
    )
    .expect("public Hybrid configuration");
    env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let funded = [0, 1, 2].map(|i| funded_owner(&mut env, &owners[i], DEPOSITS[i]));
    let portfolios = funded.map(|(portfolio, _)| portfolio);
    let sources = funded.map(|(_, source)| source);
    let [long, short, keeper] = portfolios;
    for asset in 0..2 {
        env.trade_asset_with_cu(
            asset,
            &owners[0],
            long,
            &owners[1],
            short,
            POS_SCALE as i128,
            PRICE,
            0,
        );
    }
    let initial_cert = health_cert(&env.portfolio_state(short));
    assert_eq!(initial_cert.certified_equity, 220_000);
    assert_eq!(initial_cert.certified_maintenance_req, 200_000);
    assert_eq!(initial_cert.certified_liq_deficit, 0);

    set_test_clock(&mut env, 0, 101);
    env.push_auth_mark_for_asset_as_admin(1, u64::MAX, CURRENT[1]);
    let report = env.set_pyth_price_with_conf(&feed, CURRENT[0] as i64, -6, 0, 101);
    observe(&mut env, short, &owners[2], keeper, report, Evidence::Full)
        .expect("same-slot target staging");
    let account_prefix = portfolios.map(|key| env.svm.get_account(&key).unwrap());
    set_test_clock(&mut env, 64, 102);
    let prefix_cu = observe(&mut env, short, &owners[2], keeper, report, Evidence::Full)
        .expect("first bounded market-only prefix");
    assert_cu_within("staged market-only prefix", prefix_cu, CRANK_CU_LIMIT);
    assert_eq!(percolator::V16_MAX_ACCRUAL_PATH_STEPS, 32);
    for asset in &env.market_state().1.assets[..2] {
        assert_eq!(asset.slot_last, 32);
        assert_eq!(asset.effective_price, 1_032_000);
    }
    assert_eq!(
        portfolios.map(|key| env.svm.get_account(&key).unwrap()),
        account_prefix
    );
    assert!(
        health_cert(&env.portfolio_state(short)).cert_oracle_epoch
            < env.market_state().1.oracle_epoch
    );

    // All initialized protocol/custody/provider accounts and non-fee signers, plus Clock.
    let keys = [
        env.market,
        long,
        short,
        keeper,
        env.mint,
        env.vault,
        sources[0],
        sources[1],
        sources[2],
        initial,
        report,
        owners[0].pubkey(),
        owners[1].pubkey(),
        owners[2].pubkey(),
        env.admin.pubkey(),
        solana_sdk::sysvar::clock::ID,
    ];
    let prefix = frame(&env, &keys);
    let custody = frame(&env, &keys[4..]);
    let mut max_refresh = 0;
    let mut max_action = 0;
    let mut rejections = 0;
    let mut outcomes = 0;
    let mut refresh_reference = None;
    for liquidation in [true, false] {
        let mut action_reference = None;
        for evidence in [
            Evidence::Full,
            Evidence::Reverse,
            Evidence::Empty,
            Evidence::HybridOnly,
        ] {
            let label = format!("liquidation={liquidation}, evidence={evidence:?}");
            replay(&mut env, &prefix);
            assert_eq!(frame(&env, &keys), prefix);
            if !evidence.complete() {
                for _ in 0..2 {
                    let error = observe(&mut env, short, &owners[2], keeper, report, evidence)
                        .expect_err(
                            "omitted pending AuthMark cannot authorize recertification/liquidation",
                        );
                    assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                    assert_eq!(frame(&env, &keys), prefix, "{label}: exact rejection frame");
                    rejections += 1;
                }
            }
            let cu = observe(
                &mut env,
                short,
                &owners[2],
                keeper,
                report,
                if evidence.complete() {
                    evidence
                } else {
                    Evidence::Full
                },
            )
            .expect("complete current retry finishes refresh");
            max_refresh = max_refresh.max(cu);
            assert_cu_within(&label, cu, CRANK_CU_LIMIT);
            // Two unit shorts lose 40,000 + 50,000. Gross 10% margin is 209,000.
            assert_current_short(&env, short, 130_000, 209_000);
            assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
            for asset in &env.market_state().1.assets[..2] {
                assert_eq!(asset.oi_eff_long_q, POS_SCALE);
                assert_eq!(asset.oi_eff_short_q, POS_SCALE);
            }
            let refreshed = frame(&env, &keys);
            if let Some(reference) = &refresh_reference {
                assert_eq!(
                    &refreshed, reference,
                    "{label}: full-current recertification"
                );
            } else {
                refresh_reference = Some(refreshed);
            }
            // The reduction control explicitly refreshes the peer. Other schedules leave
            // its original certificate stale and require trade-time full recertification.
            assert!(
                health_cert(&env.portfolio_state(long)).cert_oracle_epoch
                    < env.market_state().1.oracle_epoch
            );
            if !liquidation && matches!(evidence, Evidence::Full) {
                let cu = observe(&mut env, long, &owners[2], keeper, report, Evidence::Full)
                    .expect("explicit full-current peer recomputation control");
                max_refresh = max_refresh.max(cu);
                assert_cu_within(&label, cu, CRANK_CU_LIMIT);
            }
            let before = env.market_state().1;
            let position_epoch = env.portfolio_position_epoch(short);
            let cu = if liquidation {
                observe(&mut env, short, &owners[2], keeper, report, evidence)
                    .expect("already-current evidence permits deterministic liquidation")
            } else {
                reduce(&mut env, &owners, portfolios)
                    .expect("complete-current owner reduction is health-restoring")
            };
            max_action = max_action.max(cu);
            assert_cu_within(&label, cu, 500_000);
            let after = env.market_state().1;
            assert_eq!(env.portfolio_position_epoch(short), position_epoch + 1);
            if liquidation {
                let remaining = active_leg_for_asset(&env.portfolio_state(short), 0)
                    .basis_pos_q
                    .unsigned_abs();
                assert!(remaining > 0 && remaining < POS_SCALE);
                assert_eq!(after.assets[0].oi_eff_short_q, remaining);
                assert_eq!(after.assets[0].oi_eff_long_q, remaining);
                let capital = env.portfolio_state(short).capital.get();
                let penalty = 130_000 - capital;
                let reward = env.portfolio_state(keeper).capital.get() - DEPOSITS[2];
                assert!(penalty > 0 && penalty <= 10_000 && reward > 0);
                assert_eq!(reward, penalty / 2);
                assert_eq!(after.insurance - before.insurance, penalty - reward);
                assert_eq!(
                    after.c_tot + after.insurance,
                    before.c_tot + before.insurance
                );
                let margin = (remaining * u128::from(CURRENT[0])).div_ceil(POS_SCALE * 10)
                    + u128::from(CURRENT[1]) / 10;
                assert_current_short(&env, short, capital, margin);
                assert_eq!(
                    health_cert(&env.portfolio_state(short)).certified_liq_deficit,
                    0
                );
            } else {
                assert_eq!(after.assets[0].oi_eff_long_q, 0);
                assert_eq!(after.assets[0].oi_eff_short_q, 0);
                for portfolio in [long, short] {
                    assert!(!has_active_leg_for_asset(
                        &env.portfolio_state(portfolio),
                        0
                    ));
                }
                assert_current_short(&env, short, 130_000, 105_000);
                assert_eq!(
                    env.portfolio_state(long).capital.get() as i128
                        + env.portfolio_state(long).pnl.get(),
                    10_090_000
                );
                assert_eq!(env.portfolio_state(keeper).capital.get(), DEPOSITS[2]);
                assert_eq!(after.insurance, 0);
            }
            assert_eq!(after.assets[1].oi_eff_long_q, POS_SCALE);
            assert_eq!(after.assets[1].oi_eff_short_q, POS_SCALE);
            assert_eq!(after.vault, DEPOSITS.iter().sum::<u128>());
            assert_eq!(after.vault, u128::from(env.token_amount(env.vault)));
            assert_eq!(
                after.c_tot,
                portfolios
                    .iter()
                    .map(|key| env.portfolio_state(*key).capital.get())
                    .sum::<u128>()
            );
            assert_eq!(
                frame(&env, &keys[4..]),
                custody,
                "{label}: custody and external frame"
            );
            let outcome = frame(&env, &keys);
            if let Some(reference) = &action_reference {
                assert_eq!(
                    &outcome, reference,
                    "{label}: full-current action equivalence"
                );
            } else {
                action_reference = Some(outcome);
            }
            outcomes += 1;
        }
    }
    assert_eq!(outcomes, 8);
    assert_eq!(rejections, 8);
    println!("row426 staged observations: {outcomes} equivalent outcomes, {rejections} atomic rejections; prefix={prefix_cu}, refresh={max_refresh}, action={max_action} CU");
}

fn observation_withdrawal_step(
    env: &mut V16CuEnv,
    owner: &Keypair,
    instruction: Instruction,
    tracked: &[Pubkey],
    rejection: Option<PercolatorError>,
) -> u64 {
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&env.payer.pubkey()),
        &[&env.payer, owner],
        env.svm.latest_blockhash(),
    );
    let mut keys = tracked.to_vec();
    keys.extend(tx.message.account_keys.iter().copied());
    keys.sort_unstable();
    keys.dedup();
    keys.retain(|key| *key != env.payer.pubkey());
    let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
    let result = env.svm.send_transaction(tx);
    let cu = if let Some(error) = rejection {
        let failure = result.expect_err("incomplete provenance cannot renew a funded withdrawal");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(2, InstructionError::Custom(error as u32)),
            "{failure:?}"
        );
        for (key, account) in keys.iter().zip(before) {
            assert_eq!(env.svm.get_account(key), account, "rollback {key}");
        }
        failure.meta.compute_units_consumed
    } else {
        result
            .expect("public observation or custody continuation")
            .compute_units_consumed
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    assert_cu_within("observation/withdrawal step", cu, CRANK_CU_LIMIT);
    cu
}

#[test]
fn v16_program_omitted_hybrid_and_stale_fallback_cannot_renew_funded_withdrawal() {
    const OPEN_SLOT: u64 = 1;
    const OPEN_TIME: i64 = 100;
    const STALE_SLOTS: u64 = 5;
    const NOW: i64 = 162;
    const MARK: u64 = 100;
    const AUTH_MARK: u64 = 101;
    const DEPOSIT: u128 = 1_000;
    const WITHDRAW: u128 = 137;

    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 8,
            min_funding_lifetime_slots: 8,
            ..V16CuMarketParams::default()
        },
    );
    env.configure_permissionless_resolve_with_cu(STALE_SLOTS, 1);
    set_test_clock(&mut env, OPEN_SLOT, OPEN_TIME);
    let feed = [0xb7; 32];
    let initial = env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, OPEN_TIME);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0; 32], [0; 32]],
        &[initial],
        OPEN_SLOT,
        OPEN_TIME,
        0,
        0,
        3,
        100,
    )
    .expect("public Hybrid configuration with a three-slot fallback threshold");
    env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, MARK);
    let owners = [Keypair::new(), Keypair::new()];
    let funded = owners
        .each_ref()
        .map(|owner| funded_owner(&mut env, owner, DEPOSIT));
    let portfolios = funded.map(|pair| pair.0);
    let tokens = funded.map(|pair| pair.1);
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &env.mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &env.admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&env.admin],
    )
    .expect("fix the public collateral supply");
    for asset in 0..2 {
        for size in [POS_SCALE as i128, -(POS_SCALE as i128)] {
            env.trade_asset_with_cu(
                asset,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                size,
                MARK,
                0,
            );
        }
    }
    for portfolio in portfolios {
        assert!(active_bitmap(&env.portfolio_state(portfolio))
            .iter()
            .all(|word| *word == 0));
        assert_eq!(env.portfolio_state(portfolio).capital.get(), DEPOSIT);
    }
    let old_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(old_profile.last_good_oracle_slot, OPEN_SLOT);
    let stale_report = env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, NOW - 61);
    let fresh_report = env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, NOW);
    let keys = [
        env.market,
        portfolios[0],
        portfolios[1],
        env.vault,
        env.mint,
        tokens[0],
        tokens[1],
        initial,
        stale_report,
        fresh_report,
        owners[0].pubkey(),
        owners[1].pubkey(),
        env.admin.pubkey(),
        solana_sdk::sysvar::clock::ID,
    ];
    let initial_frame = frame(&env, &keys);
    let mut peak = 0;
    let mut rejected = 0;
    let mut paid = 0;
    for real_slot in [OPEN_SLOT + STALE_SLOTS, OPEN_SLOT + STALE_SLOTS + 1] {
        for report in [None, Some(stale_report), Some(fresh_report)] {
            let mut reference = None;
            for caller_slot in [0, u64::MAX] {
                let fresh = report == Some(fresh_report);
                replay(&mut env, &initial_frame);
                let auth_slot = OPEN_SLOT + STALE_SLOTS - 1;
                set_test_clock(&mut env, auth_slot, NOW);
                env.push_auth_mark_for_asset_as_admin(1, auth_slot, AUTH_MARK);
                let auth = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    1,
                )
                .unwrap();
                assert_eq!(auth.last_good_oracle_slot, auth_slot);
                assert_eq!(auth.mark_ewma_last_slot, auth_slot);
                assert!(real_slot - auth.last_good_oracle_slot < STALE_SLOTS);
                let prefix = |env: &V16CuEnv, report: Option<Pubkey>| {
                    let mut observations = crank_observations(1);
                    let mut accounts = vec![
                        AccountMeta::new_readonly(owners[0].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                    ];
                    if let Some(report) = report {
                        observations.extend(crank_observations_with_accounts(0, 1));
                        accounts.push(AccountMeta::new_readonly(report, false));
                    }
                    Instruction {
                        program_id: env.program_id,
                        accounts,
                        data: ProgInstruction::PermissionlessCrank {
                            now_slot: caller_slot,
                            observations,
                        }
                        .encode(),
                    }
                };
                let withdraw = |env: &V16CuEnv| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[0].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(tokens[0], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(portfolios[0], WITHDRAW).encode(),
                };
                let before = env.market_state().1;
                let immutable = frame(&env, &keys[2..]);
                let observation = prefix(&env, report);
                peak = peak.max(observation_withdrawal_step(
                    &mut env,
                    &owners[0],
                    observation,
                    &keys,
                    None,
                ));
                let (cfg, group) = env.market_state();
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(group.current_slot, auth_slot);
                assert!(group.assets[1].slot_last > before.assets[1].slot_last);
                assert_eq!(group.assets[1].slot_last, auth_slot);
                if report.is_some() {
                    assert!(group.assets[0].slot_last > before.assets[0].slot_last);
                    assert_eq!(group.assets[0].slot_last, auth_slot);
                }
                let good_slot = if fresh { auth_slot } else { OPEN_SLOT };
                let publish_time = if fresh { NOW } else { OPEN_TIME };
                assert_eq!(cfg.last_good_oracle_slot, good_slot);
                let hybrid = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap();
                assert_eq!(hybrid.last_good_oracle_slot, good_slot);
                assert_eq!(hybrid.oracle_target_publish_time, publish_time);
                assert_eq!(hybrid.oracle_leg_publish_times, [publish_time, 0, 0]);
                assert_eq!(
                    hybrid.oracle_leg_prices_e6,
                    old_profile.oracle_leg_prices_e6
                );
                if !fresh {
                    assert_eq!(hybrid.mark_ewma_last_slot, old_profile.mark_ewma_last_slot);
                }
                assert_eq!(frame(&env, &keys[2..]), immutable);

                // Settlement and the unrelated AuthMark are newer than Hybrid provenance.
                // The fresh report at the prefix renews this window; settlement alone does not.
                set_test_clock(&mut env, real_slot, NOW);
                assert_eq!(real_slot < good_slot + STALE_SLOTS, fresh);
                if fresh {
                    let retained = prefix(&env, report);
                    peak = peak.max(observation_withdrawal_step(
                        &mut env, &owners[0], retained, &keys, None,
                    ));
                }
                let debit = withdraw(&env);
                peak = peak.max(observation_withdrawal_step(
                    &mut env,
                    &owners[0],
                    debit,
                    &keys,
                    (!fresh).then_some(PercolatorError::OracleStale),
                ));
                paid += usize::from(fresh);
                rejected += usize::from(!fresh);
                let withdrawn = if fresh { WITHDRAW } else { 0 };
                let (cfg, group) = env.market_state();
                assert_eq!(cfg.last_good_oracle_slot, good_slot);
                assert_eq!(cfg.oracle_target_publish_time, publish_time);
                assert_eq!(group.mode, MarketModeV16::Live);
                for asset in &group.assets[..2] {
                    assert_eq!(asset.oi_eff_long_q, 0);
                    assert_eq!(asset.oi_eff_short_q, 0);
                }
                assert_eq!(
                    env.portfolio_state(portfolios[0]).capital.get(),
                    DEPOSIT - withdrawn
                );
                assert_eq!(env.portfolio_state(portfolios[1]).capital.get(), DEPOSIT);
                assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 0);
                assert_eq!(env.token_amount(tokens[0]) as u128, withdrawn);
                assert_eq!(env.token_amount(tokens[1]), 0);
                assert_eq!(group.vault, 2 * DEPOSIT - withdrawn);
                assert_eq!(env.token_amount(env.vault) as u128, group.vault);
                assert_eq!(group.c_tot, group.vault);
                assert_eq!(group.insurance, 0);
                assert_eq!(frame(&env, &keys[6..13]), immutable[4..11]);
                assert_eq!(env.svm.get_account(&env.mint).unwrap(), initial_frame[4].1);
                let outcome = frame(&env, &keys);
                if let Some(reference) = &reference {
                    assert_eq!(
                        &outcome, reference,
                        "caller hints preserve complete withdrawal outcomes"
                    );
                } else {
                    reference = Some(outcome);
                }
            }
        }
    }
    assert_eq!(rejected, 8);
    assert_eq!(paid, 4);
    println!("row426 omitted Hybrid/fallback withdrawal: 12 histories, {rejected} atomic rejections, {paid} exact funded controls; peak {peak} CU");
}
