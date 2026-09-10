//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): wash-trade fee coverage,
//! no-CPI zero/epsilon/extreme reported-price normalization, CPI matcher quote pinning, same-slot
//! and per-slot EWMA circuit breakers, and underfunded exits that cannot move EWMA with an
//! uncollectible fee. A public 14-asset composition additionally pays the maximum elapsed-time
//! movement, refreshes both portfolios, enters DrainOnly, closes every leg at raw price one within
//! the SVM ceiling, converts the exact released PnL, and reconciles terminal custody. These tests
//! exercise the deployed public wrapper with real SBF/LiteSVM account construction and assert
//! economic state, token, rollback, liveness, or compute outcomes appropriate to the invariant.
//! Two further maximum-shape compositions cross stale HybridAfterHours pricing and delegated batch
//! CPI into either terminal resolution or Recovery. The Recovery branch refreshes both complete
//! certificates permissionlessly, closes all legs atomically at raw price one, and returns every
//! non-fee atom, so the maximum-shape guarantee is not specific to no-CPI/DrainOnly or Resolved.
//! `v16_program_mark_writer_and_trade_exit_composition_is_source_complete` closes the finite
//! current wrapper surface by inventorying every mark writer, proving all four trade variants use
//! one of two shared normalization chains, and binding arithmetic, fee, sequencing, lifecycle,
//! oracle-failure, and maximum-shape evidence to those chains.
//! The `accepted_price_reward` child adds fresh-feed lag/catchup composition with
//! independent per-actor fee/PnL attribution and exact keeper SPL withdrawal.
//! The `staggered_mark_envelope` child compares a two-asset batch with single reductions
//! when distinct mark ages straddle the accrual horizon, checking each paid movement independently.
//! The `interleaved_cap_carry` child preserves a fractional fresh-oracle cap across mixed trade
//! routes and repeated reports, attributing fees and keeper payouts at each committed-price prefix.
//! The `late_stale_crank_rollback` child composes paid CPI discovery with a late stale-feed
//! rejection, then fresh observation recovery and a mixed-mode no-CPI batch reversal.
//! The `custody_cap_carry` child frames two unequal pending carries across deposit,
//! withdrawal, maintenance, and insurance words before/after canonical public cranks.
//! The target-reversal test below composes nonzero old-target carry, a neutral reduction,
//! atomic publication rollback, and fractional accrual after the effective price has moved.
//! The terminal carry test orders a due single/batch reduction around canonical accrual,
//! then freezes the remaining fraction through resolution and exact owner SPL payouts.
//! The `trade_origin_catchup` child prices liquidation of a paid pending Hybrid mark
//! across stale-report substitutions and catchup order, preserving zero keeper entitlement.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[path = "inv_045_accepted_price_reward.rs"]
mod accepted_price_reward;

#[path = "inv_045_staggered_mark_envelope.rs"]
mod staggered_mark_envelope;

#[path = "inv_045_interleaved_cap_carry.rs"]
mod interleaved_cap_carry;

#[path = "inv_045_late_stale_crank_rollback.rs"]
mod late_stale_crank_rollback;

#[path = "inv_045_custody_cap_carry.rs"]
mod custody_cap_carry;

#[path = "inv_045_public_carry_order.rs"]
mod public_carry_order;

#[path = "inv_045_trade_origin_catchup.rs"]
mod trade_origin_catchup;

#[test]
fn v16_program_caught_up_hybrid_reward_uses_selected_asset_provenance_in_both_hint_orders() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

    const ENTRY: u64 = 1_000_000;
    const STEP: u64 = 2_400;
    const REPORT: u64 = ENTRY - 3 * STEP;
    const QUANTITY: u128 = 100 * POS_SCALE;
    const FUNDS: [u64; 3] = [5_100_000, 100_000_000, 1_000];
    const SHARE: u128 = 3_333;
    let mut outcomes = Vec::new();

    for hybrid_first in [false, true] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                max_abs_funding_e9_per_slot: 0,
                ..production_risk_params()
            },
        );
        set_test_clock(&mut env, 1, 100);
        env.configure_ewma_mark_with_cu(1, ENTRY, 1, 0);
        assert_eq!(
            env.market_state().1.assets[1].lifecycle,
            AssetLifecycleV16::Active
        );
        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
        let feed = [0x49; 32];
        let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            1,
            1,
            0,
            [feed, [0; 32], [0; 32]],
            &[initial],
            1,
            100,
            0,
            0,
            1_000,
            0,
        )
        .expect("configure the non-base Hybrid asset from authenticated evidence");
        assert_eq!(env.market_state().0.fee_redirect_to_market_0_bps, 0);
        assert_eq!(env.market_state().1.config.liquidation_fee_bps, 5);

        let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
        let mut portfolios = [Pubkey::default(); 3];
        let mut tokens = [Pubkey::default(); 3];
        for actor in 0..3 {
            let owner = &owners[actor];
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            let portfolio = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[actor] = portfolio.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap();
            env.portfolios.push(portfolio.pubkey());
            tokens[actor] = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    FUNDS[actor],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio.pubkey(), FUNDS[actor] as u128),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio.pubkey(), false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .unwrap();
        }
        let [target, peer, keeper] = portfolios;
        env.trade_asset_with_cu(
            1,
            &owners[0],
            target,
            &owners[1],
            peer,
            QUANTITY as i128,
            ENTRY,
            0,
        );
        let values = |env: &V16CuEnv| {
            portfolios.map(|key| {
                let account = env.portfolio_state(key);
                account.capital.get() as i128 + account.pnl.get()
            })
        };
        let mut keys = vec![env.market, env.mint, env.vault, env.admin.pubkey(), initial];
        keys.extend(portfolios);
        keys.extend(tokens);
        keys.extend(owners.each_ref().map(Signer::pubkey));
        let frame = |env: &V16CuEnv, keys: &[Pubkey]| -> Vec<Option<Account>> {
            keys.iter().map(|key| env.svm.get_account(key)).collect()
        };
        let hints: Vec<_> = if hybrid_first { [1, 0] } else { [0, 1] }
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: u8::from(asset_index == 1),
            })
            .into();
        let crank = |env: &mut V16CuEnv,
                     account: Pubkey,
                     observations: Vec<CrankObservationHint>,
                     oracles: &[Pubkey],
                     reward: bool| {
            let mut accounts = vec![
                AccountMeta::new(owners[2].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account, false),
            ];
            accounts.extend(
                oracles
                    .iter()
                    .map(|key| AccountMeta::new_readonly(*key, false)),
            );
            if reward {
                accounts.push(AccountMeta::new(keeper, false));
            }
            env.svm.expire_blockhash();
            env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: env.svm.get_sysvar::<Clock>().slot,
                    observations,
                },
                accounts,
                &[&owners[2]],
            )
        };
        let vault = FUNDS.iter().sum::<u64>();
        assert_eq!(env.token_amount(env.vault), vault);
        let mut max_cu = 0;
        let mut report = initial;
        for elapsed in 1..=3 {
            set_test_clock(&mut env, 1 + elapsed, 100 + elapsed as i64);
            if elapsed == 1 {
                report = env.set_pyth_price_with_conf(&feed, REPORT as i64, -6, 0, 101);
                keys.push(report);
            }
            if elapsed == 3 {
                // Both valid observations precede this invalid suffix, including the
                // final Hybrid catchup step. Rejection must undo its staged writes.
                let before = frame(&env, &keys);
                let mut invalid = hints.clone();
                invalid.push(CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 1,
                });
                let error = crank(&mut env, target, invalid, &[report, report], true)
                    .expect_err("duplicate final observation must reject");
                assert!(error.contains("Custom(9)"), "{error}");
                assert_eq!(frame(&env, &keys), before, "exact invalid-suffix rollback");
            }
            max_cu = max_cu.max(crank(&mut env, keeper, hints.clone(), &[report], false).unwrap());
            let group = env.market_state().1;
            let asset = group.assets[1];
            let profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                1,
            )
            .unwrap();
            assert_eq!(asset.effective_price, ENTRY - STEP * elapsed);
            assert_eq!(asset.raw_oracle_target_price, REPORT);
            assert_eq!(profile.mark_ewma_e6, asset.effective_price);
            assert_eq!(profile.oracle_target_price_e6, REPORT);
            assert_eq!(profile.oracle_target_publish_time, 101);
            assert_eq!(profile.last_good_oracle_slot, 2);
            assert_eq!(asset.k_long, -((STEP * elapsed) as i128) * ADL_ONE as i128);
            assert_eq!(asset.k_short, (STEP * elapsed) as i128 * ADL_ONE as i128);
            assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
            assert_eq!(
                (asset.oi_eff_long_q, asset.oi_eff_short_q),
                (QUANTITY, QUANTITY)
            );
            assert_eq!(
                values(&env),
                FUNDS.map(i128::from),
                "publication earns no reward"
            );
            assert_eq!(group.insurance, 0);
            assert!(group
                .insurance_domain_budget
                .iter()
                .all(|amount| *amount == 0));
            assert_eq!(group.vault, vault as u128);
            assert_eq!(env.token_amount(env.vault), vault);
        }

        let fee_at = |quantity: u128, price: u64| {
            ((quantity * price as u128).div_ceil(POS_SCALE) * 5).div_ceil(10_000)
        };
        let mut liquidation = None;
        for _ in 0..6 {
            let before = env.market_state().1;
            let before_values = values(&env);
            let before_target = env.portfolio_state(target);
            let before_peer = env.svm.get_account(&peer);
            max_cu = max_cu.max(crank(&mut env, target, hints.clone(), &[report], true).unwrap());
            let after = env.market_state().1;
            assert_eq!(after.assets[1].effective_price, REPORT);
            assert_eq!(after.assets[0].effective_price, ENTRY);
            assert_eq!(
                (
                    after.assets[0].oi_eff_long_q,
                    after.assets[0].oi_eff_short_q
                ),
                (0, 0)
            );
            assert_eq!(env.svm.get_account(&peer), before_peer);
            assert_eq!(after.vault, vault as u128);
            assert_eq!(env.token_amount(env.vault), vault);
            let closed = before.assets[1].oi_eff_long_q - after.assets[1].oi_eff_long_q;
            if closed == 0 {
                assert_eq!(values(&env)[2], before_values[2]);
                assert_eq!(after.insurance, before.insurance);
                assert_eq!(
                    after.insurance_domain_budget,
                    before.insurance_domain_budget
                );
                continue;
            }
            assert!(health_cert(&before_target).certified_liq_deficit > 0);
            assert!(closed < QUANTITY);
            let fee = fee_at(closed, REPORT);
            let reward = fee * SHARE / 10_000;
            assert!(reward > 0 && reward < fee);
            assert_ne!(
                fee,
                fee_at(closed, ENTRY),
                "the unrelated EWMA price must distinguish the fee oracle"
            );
            assert_eq!(before_values[0] - values(&env)[0], fee as i128);
            assert_eq!(values(&env)[1], before_values[1]);
            assert_eq!(values(&env)[2] - before_values[2], reward as i128);
            assert_eq!(after.insurance - before.insurance, fee - reward);
            assert_eq!(&after.insurance_domain_budget[..2], &[0, 0]);
            assert_eq!(
                &after.insurance_domain_budget[2..4],
                &[(fee - reward) / 2, (fee - reward).div_ceil(2)]
            );
            assert_eq!(
                after.assets[1].oi_eff_long_q,
                after.assets[1].oi_eff_short_q
            );
            assert_eq!(
                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                0
            );
            liquidation = Some((closed, fee, reward));
            break;
        }
        let (closed, fee, reward) =
            liquidation.expect("bounded refresh reaches a real partial liquidation");
        max_cu = max_cu.max(crank(&mut env, peer, hints.clone(), &[report], false).unwrap());
        let loss = ((ENTRY - REPORT) as u128 * QUANTITY / POS_SCALE) as i128;
        let entitled = [
            FUNDS[0] as i128 - loss - fee as i128,
            FUNDS[1] as i128 + loss,
            FUNDS[2] as i128 + reward as i128,
        ];
        assert_eq!(values(&env), entitled);
        assert_eq!(
            entitled.iter().sum::<i128>() + env.market_state().1.insurance as i128,
            vault as i128
        );

        let mut reached_fixed_point = false;
        for _ in 0..4 {
            let before_retry = frame(&env, &keys);
            let before = env.market_state().1;
            match crank(&mut env, target, hints.clone(), &[report], true) {
                Ok(cu) => max_cu = max_cu.max(cu),
                Err(error) => {
                    assert!(is_engine_non_progress_error(&error), "{error}");
                    assert_eq!(frame(&env, &keys), before_retry);
                    reached_fixed_point = true;
                    break;
                }
            }
            let after = env.market_state().1;
            assert_eq!(values(&env), entitled, "healthy refresh cannot pay again");
            assert_eq!(after.assets[1].oi_eff_long_q, QUANTITY - closed);
            assert_eq!(after.assets[1].oi_eff_short_q, QUANTITY - closed);
            assert_eq!(after.insurance, before.insurance);
            assert_eq!(
                after.insurance_domain_budget,
                before.insurance_domain_budget
            );
            assert_eq!(env.token_amount(env.vault), vault);
            assert_eq!(
                health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                0
            );
        }
        assert!(
            reached_fixed_point,
            "healthy refresh has a bounded fixed point"
        );
        env.svm.expire_blockhash();
        let exit_cu = env.trade_asset_with_cu(
            1,
            &owners[0],
            target,
            &owners[1],
            peer,
            -((QUANTITY - closed) as i128),
            REPORT,
            0,
        );
        assert_cu_within("caught-up Hybrid owner exit", exit_cu, TRADE_CU_LIMIT);
        assert_eq!(values(&env), entitled);
        assert_eq!(
            (
                env.market_state().1.assets[1].oi_eff_long_q,
                env.market_state().1.assets[1].oi_eff_short_q
            ),
            (0, 0)
        );
        for portfolio in [target, peer] {
            assert!(!has_active_leg_for_asset(
                &env.portfolio_state(portfolio),
                1
            ));
        }
        let payout = FUNDS[2] as u128 + reward;
        let withdrawal = env.withdraw_ix(keeper, payout);
        let withdraw_cu = env
            .send(
                withdrawal,
                vec![
                    AccountMeta::new(owners[2].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(keeper, false),
                    AccountMeta::new(tokens[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[2]],
            )
            .unwrap();
        assert_cu_within(
            "caught-up Hybrid keeper withdrawal",
            withdraw_cu,
            CUSTODY_CU_LIMIT,
        );
        assert_cu_within("caught-up Hybrid mixed observations", max_cu, 450_000);
        assert_eq!(
            tokens.map(|key| env.token_amount(key)),
            [0, 0, payout as u64]
        );
        assert_eq!(values(&env), [entitled[0], entitled[1], 0]);
        let final_group = env.market_state().1;
        assert_eq!(final_group.insurance, fee - reward);
        assert_eq!(final_group.vault, vault as u128 - payout);
        assert_eq!(env.token_amount(env.vault) as u128, final_group.vault);
        assert_eq!(
            values(&env).iter().sum::<i128>() + final_group.insurance as i128 + payout as i128,
            vault as i128
        );
        assert_eq!(
            Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                .unwrap()
                .supply,
            vault
        );
        outcomes.push((
            closed,
            fee,
            reward,
            values(&env),
            final_group.insurance_domain_budget,
            final_group.vault,
        ));
        println!("Hybrid-first={hybrid_first}: fee={fee}, reward={reward}, max_crank_cu={max_cu}, exit_cu={exit_cu}, withdraw_cu={withdraw_cu}");
    }
    assert_eq!(
        outcomes[0], outcomes[1],
        "observation order cannot change selected-asset entitlement"
    );
}

#[test]
fn v16_program_pending_fractional_carry_survives_due_trade_and_resolution() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    const ENTRY: u64 = 100;
    const CAP_BPS: u64 = 24;
    const PRINCIPAL: [u64; 2] = [100_003, 200_009];
    let mut max_cu = 0;
    let mut histories = 0;
    for direction in [-1i128, 1] {
        let mut baseline = None;
        for batch in [false, true] {
            for trade_first in [false, true] {
                let label =
                    format!("direction={direction}, batch={batch}, trade_first={trade_first}");
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        initial_price: ENTRY,
                        max_price_move_bps_per_slot: CAP_BPS,
                        max_accrual_dt_slots: 6,
                        min_funding_lifetime_slots: 6,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(0);
                env.configure_auth_mark_with_cu(0, ENTRY);
                let owners = [Keypair::new(), Keypair::new()];
                let mut portfolios = [Pubkey::default(); 2];
                let mut tokens = [Pubkey::default(); 2];
                for actor in 0..2 {
                    env.svm
                        .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                        .unwrap();
                    let portfolio = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &portfolio,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    portfolios[actor] = portfolio.pubkey();
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                    env.portfolios.push(portfolios[actor]);
                    tokens[actor] = create_ata_for_test(
                        &mut env.svm,
                        &env.payer,
                        owners[actor].pubkey(),
                        env.mint,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &tokens[actor],
                            &env.admin.pubkey(),
                            &[],
                            PRINCIPAL[actor],
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();
                    env.send(
                        env.deposit_ix(portfolios[actor], PRINCIPAL[actor] as u128),
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                }
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
                .unwrap();
                env.trade_with_cu(
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    7 * POS_SCALE as i128,
                    ENTRY,
                    0,
                );
                let target = (ENTRY as i128 + 20 * direction) as u64;
                env.push_auth_mark_with_cu(0, target);
                inv045_reversal_checkpoint(&env, portfolios, 0, ENTRY, target, 0, 7, 0);

                let crank = |env: &V16CuEnv, actor: usize, duplicate: bool| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: env.svm.get_sysvar::<Clock>().slot,
                        observations: if duplicate {
                            crank_observations_for_assets(&[0, 0])
                        } else {
                            crank_observations(0)
                        },
                    }
                    .encode(),
                };
                env.svm.warp_to_slot(1);
                let ix = crank(&env, 1, false);
                let cu = send_raw_tx(&mut env.svm, &env.payer, ix, &[]).unwrap();
                max_cu = max_cu.max(cu);
                inv045_reversal_checkpoint(
                    &env,
                    portfolios,
                    1,
                    ENTRY,
                    target,
                    ENTRY * CAP_BPS,
                    7,
                    0,
                );
                env.svm.warp_to_slot(4);
                if !trade_first {
                    let ix = crank(&env, 0, false);
                    let cu = send_raw_tx(&mut env.svm, &env.payer, ix, &[]).unwrap();
                    max_cu = max_cu.max(cu);
                    inv045_reversal_checkpoint(
                        &env,
                        portfolios,
                        4,
                        ENTRY,
                        target,
                        ENTRY * CAP_BPS * 4,
                        7,
                        0,
                    );
                }
                // Both schedules reduce before any price atom is representable.
                // A route may retain elapsed work, but cannot discard the stored fraction.
                let reduction = if batch {
                    env.batch_trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        vec![BatchTradeLeg {
                            asset_index: 0,
                            market_id: env.asset_market_id(0),
                            size_q: -(POS_SCALE as i128),
                            exec_price: ENTRY,
                            fee_bps: 0,
                        }],
                    )
                } else {
                    env.trade_no_cpi_ix(
                        portfolios[0],
                        portfolios[1],
                        0,
                        -(POS_SCALE as i128),
                        ENTRY,
                        0,
                    )
                };
                env.svm.expire_blockhash();
                let cu = env
                    .send(
                        reduction,
                        vec![
                            AccountMeta::new(owners[0].pubkey(), true),
                            AccountMeta::new(owners[1].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[0], false),
                            AccountMeta::new(portfolios[1], false),
                        ],
                        &[&owners[0], &owners[1]],
                    )
                    .unwrap_or_else(|error| panic!("{label}: reduction: {error}"));
                max_cu = max_cu.max(cu);
                eprintln!("INV-045 terminal carry prefix: {label}, slot=4");
                let accrued_slot = env.market_state().1.assets[0].slot_last;
                assert!((1..=4).contains(&accrued_slot), "{label}");
                inv045_reversal_checkpoint(
                    &env,
                    portfolios,
                    accrued_slot,
                    ENTRY,
                    target,
                    ENTRY * CAP_BPS * accrued_slot,
                    6,
                    0,
                );
                let retained = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap()
                .price_move_remainder_bps_num;
                assert_eq!(
                    u64::from(retained) + ENTRY * CAP_BPS * (4 - accrued_slot),
                    ENTRY * CAP_BPS * 4,
                    "stored fraction plus unprocessed time is route independent: {label}",
                );

                let keys = [
                    env.market,
                    env.mint,
                    env.vault,
                    portfolios[0],
                    portfolios[1],
                    tokens[0],
                    tokens[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    env.admin.pubkey(),
                    env.payer.pubkey(),
                ];
                let frame = |env: &V16CuEnv| keys.map(|key| env.svm.get_account(&key).unwrap());
                if trade_first {
                    let ix = crank(&env, 0, false);
                    env.svm.expire_blockhash();
                    let cu = send_raw_tx(&mut env.svm, &env.payer, ix, &[])
                        .expect("canonical crank after the due reduction");
                    max_cu = max_cu.max(cu);
                }
                inv045_reversal_checkpoint(
                    &env,
                    portfolios,
                    4,
                    ENTRY,
                    target,
                    ENTRY * CAP_BPS * 4,
                    6,
                    0,
                );
                env.svm.warp_to_slot(6);
                let valid = crank(&env, usize::from(trade_first), false);
                let mut before = frame(&env);
                env.svm.expire_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[heap_ix(), cu_ix(), valid.clone(), crank(&env, 0, true)],
                    Some(&env.payer.pubkey()),
                    &[&env.payer],
                    env.svm.latest_blockhash(),
                );
                before[10].lamports -= u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature;
                let failure = env
                    .svm
                    .send_transaction(tx)
                    .expect_err("duplicate observation suffix");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(3, InstructionError::Custom(9)),
                    "{label}"
                );
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {} success", env.program_id))
                        .count(),
                    1,
                    "the carry-consuming prefix must have executed: {label}"
                );
                max_cu = max_cu.max(failure.meta.compute_units_consumed);
                assert_eq!(
                    frame(&env),
                    before,
                    "exact carry/PnL/custody rollback: {label}"
                );
                inv045_reversal_checkpoint(
                    &env,
                    portfolios,
                    4,
                    ENTRY,
                    target,
                    ENTRY * CAP_BPS * 4,
                    6,
                    0,
                );
                env.svm.expire_blockhash();
                let cu = send_raw_tx(&mut env.svm, &env.payer, valid, &[]).unwrap();
                max_cu = max_cu.max(cu);
                let numerator = ENTRY * CAP_BPS * 6;
                let profit = 6 * direction * i128::from(numerator / 10_000);
                let live = inv045_reversal_checkpoint(
                    &env, portfolios, 6, ENTRY, target, numerator, 6, profit,
                );
                assert_eq!(live.1 as u64, 4_400);

                let cu = env
                    .send(
                        ProgInstruction::ResolveMarket {
                            asset_generation_frontier: 0,
                            authority_epoch: env.control_sequences(0).authority_epoch,
                        },
                        vec![
                            AccountMeta::new(env.admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                        ],
                        &[&env.admin.insecure_clone()],
                    )
                    .expect("resolve at the already committed fractional frontier");
                max_cu = max_cu.max(cu);
                let expected = [PRINCIPAL[0] as i128 + profit, PRINCIPAL[1] as i128 - profit];
                assert_eq!(live.2, expected);
                let terminal_frame = |env: &V16CuEnv| {
                    let group = env.market_state().1;
                    let asset = group.assets[0];
                    let profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0,
                    )
                    .unwrap();
                    assert_eq!(group.mode, MarketModeV16::Resolved);
                    assert_eq!(group.resolved_slot, 6);
                    assert_eq!(asset.effective_price, live.0);
                    assert_eq!(profile.price_move_remainder_bps_num, live.1);
                    assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
                    assert_eq!(group.insurance, 0);
                    let paid = tokens.map(|key| env.token_amount(key) as u128);
                    let vault = env.token_amount(env.vault) as u128;
                    assert_eq!(group.vault, vault);
                    assert_eq!(vault + paid.iter().sum::<u128>(), 300_012);
                    assert_eq!(
                        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                            .unwrap()
                            .supply,
                        300_012
                    );
                    assert_eq!(
                        group.c_tot,
                        portfolios
                            .iter()
                            .map(|key| env.portfolio_state(*key).capital.get())
                            .sum::<u128>()
                    );
                    for actor in 0..2 {
                        assert!(
                            paid[actor] <= expected[actor] as u128,
                            "wrong-owner payout: {label}"
                        );
                    }
                    paid
                };
                terminal_frame(&env);
                // Later Clock values cannot turn the unused cap fraction into terminal PnL.
                env.svm.warp_to_slot(100);
                for round in 0..16 {
                    if portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&env, *key))
                    {
                        break;
                    }
                    for actor in if trade_first { [1, 0] } else { [0, 1] } {
                        if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                            continue;
                        }
                        let before = frame(&env);
                        env.svm.expire_blockhash();
                        let result = env.send(
                            ProgInstruction::CloseResolved {
                                fee_rate_per_slot: 0,
                            },
                            vec![
                                AccountMeta::new_readonly(owners[actor].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[actor], false),
                                AccountMeta::new(tokens[actor], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&owners[actor]],
                        );
                        match result {
                            Ok(cu) => {
                                max_cu = max_cu.max(cu);
                                assert_ne!(
                                    frame(&env)[..10],
                                    before[..10],
                                    "bounded terminal progress: {label}, round={round}"
                                );
                            }
                            Err(error) => {
                                assert!(is_engine_non_progress_error(&error), "{label}: {error}");
                                assert_eq!(
                                    frame(&env)[..10],
                                    before[..10],
                                    "terminal retry rollback: {label}"
                                );
                            }
                        }
                        terminal_frame(&env);
                    }
                }
                assert!(
                    portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&env, *key)),
                    "{label}"
                );
                let paid = terminal_frame(&env);
                assert_eq!(
                    paid,
                    expected.map(|amount| amount as u128),
                    "exact owner attribution: {label}"
                );
                let group = env.market_state().1;
                assert_eq!((group.vault, group.c_tot, group.pnl_pos_tot), (0, 0, 0));
                assert_eq!(
                    (
                        group.assets[0].oi_eff_long_q,
                        group.assets[0].oi_eff_short_q
                    ),
                    (0, 0)
                );
                let outcome = (live, paid);
                if let Some(expected) = baseline {
                    assert_eq!(
                        outcome, expected,
                        "normalized route/terminal order: {label}"
                    );
                } else {
                    baseline = Some(outcome);
                }
                histories += 1;
            }
        }
    }
    assert_eq!(histories, 8);
    assert_cu_within(
        "pending carry through due trade and resolution",
        max_cu,
        1_400_000,
    );
    eprintln!("INV-045 terminal carry: {histories} histories and exact suffix rollbacks, peak {max_cu} CU");
}

fn inv045_reversal_checkpoint(
    env: &V16CuEnv,
    portfolios: [Pubkey; 2],
    slot: u64,
    start_price: u64,
    target: u64,
    numerator: u64,
    lots: i128,
    profit: i128,
) -> (u64, u16, [i128; 2]) {
    let group = env.market_state().1;
    let asset = group.assets[0];
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let direction = (target as i128 - start_price as i128).signum();
    let price = (start_price as i128 + direction * (numerator / 10_000) as i128) as u64;
    assert_eq!(asset.slot_last, slot);
    // Authenticated target staging retains the original cap anchor until arrival.
    assert_eq!(asset.fund_px_last, 100);
    assert_eq!(asset.raw_oracle_target_price, target);
    assert_eq!(asset.effective_price, price);
    assert_eq!(
        u64::from(profile.price_move_remainder_bps_num),
        numerator % 10_000
    );
    assert_eq!((asset.f_long_num, asset.f_short_num), (0, 0));
    assert_eq!((asset.b_long_num, asset.b_short_num), (0, 0));
    assert_eq!((asset.a_long, asset.a_short), (ADL_ONE, ADL_ONE));
    assert_eq!(
        (asset.k_long, asset.k_short),
        (
            (price as i128 - 100) * ADL_ONE as i128,
            (100 - price as i128) * ADL_ONE as i128
        )
    );
    assert_eq!(asset.oi_eff_long_q, lots as u128 * POS_SCALE);
    assert_eq!(asset.oi_eff_short_q, asset.oi_eff_long_q);
    assert_eq!(
        (
            asset.pending_obligation_count_long,
            asset.pending_obligation_count_short
        ),
        (0, 0)
    );
    let accounts = portfolios.map(|portfolio| env.portfolio_state(portfolio));
    let values = std::array::from_fn(|actor| {
        let account = &accounts[actor];
        let leg = active_leg_for_asset(account, 0);
        let sign = if actor == 0 { 1 } else { -1 };
        assert_eq!(leg.basis_pos_q, sign * lots * POS_SCALE as i128);
        assert_eq!(leg.a_basis, ADL_ONE);
        assert_eq!((leg.b_rem, leg.f_snap), (0, 0));
        let k = if actor == 0 {
            asset.k_long
        } else {
            asset.k_short
        };
        let unsettled = leg.basis_pos_q.abs() * (k - leg.k_snap);
        let denominator = POS_SCALE as i128 * ADL_ONE as i128;
        assert_eq!(unsettled % denominator, 0);
        account.capital.get() as i128 + account.pnl.get() + unsettled / denominator
    });
    assert_eq!(values, [100_003 + profit, 200_009 - profit]);
    assert_eq!(
        group.c_tot,
        accounts
            .iter()
            .map(|account| account.capital.get())
            .sum::<u128>()
    );
    assert_eq!(
        group.pnl_pos_tot,
        accounts
            .iter()
            .map(|account| account.pnl.get().max(0) as u128)
            .sum::<u128>()
    );
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 300_012);
    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply,
        300_012
    );
    (price, profile.price_move_remainder_bps_num, values)
}

#[test]
fn v16_program_fractional_target_reversal_commutes_with_neutral_reduction() {
    use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use solana_sdk::{
        fee::FeeStructure, instruction::InstructionError, transaction::TransactionError,
    };

    let mut max_cu = 0;
    for direction in [-1i128, 1] {
        let mut baseline = None;
        for split in [false, true] {
            for trade_first in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        initial_price: 100,
                        max_price_move_bps_per_slot: 24,
                        max_accrual_dt_slots: 6,
                        min_funding_lifetime_slots: 6,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(0);
                env.configure_auth_mark_with_cu(0, 100);
                let owners = [Keypair::new(), Keypair::new()];
                let mut portfolios = [Pubkey::default(); 2];
                let mut tokens = [Pubkey::default(); 2];
                for actor in 0..2 {
                    env.svm
                        .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                        .unwrap();
                    let portfolio = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &portfolio,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    portfolios[actor] = portfolio.pubkey();
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio.pubkey(), false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                    env.portfolios.push(portfolio.pubkey());
                    tokens[actor] = create_ata_for_test(
                        &mut env.svm,
                        &env.payer,
                        owners[actor].pubkey(),
                        env.mint,
                    );
                    let principal = [100_003, 200_009][actor];
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &tokens[actor],
                            &env.admin.pubkey(),
                            &[],
                            principal,
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();
                    env.send(
                        env.deposit_ix(portfolio.pubkey(), principal as u128),
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolio.pubkey(), false),
                            AccountMeta::new(tokens[actor], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&owners[actor]],
                    )
                    .unwrap();
                }
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
                .unwrap();
                env.trade_with_cu(
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    7 * POS_SCALE as i128,
                    100,
                    0,
                );
                let old_target = (100 + 20 * direction) as u64;
                env.push_auth_mark_with_cu(0, old_target);
                let owner_order = if trade_first { [0, 1] } else { [1, 0] };
                let slots: &[u64] = if split { &[1, 2, 3, 4, 5, 6] } else { &[6] };
                for &slot in slots {
                    env.svm.warp_to_slot(slot);
                    for actor in owner_order {
                        let cu = env.crank_if_actionable(
                            portfolios[actor],
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations(0),
                            },
                        );
                        max_cu = max_cu.max(cu.unwrap_or(0));
                        inv045_reversal_checkpoint(
                            &env,
                            portfolios,
                            slot,
                            100,
                            old_target,
                            2_400 * slot,
                            7,
                            7 * direction * (2_400 * slot / 10_000) as i128,
                        );
                    }
                }
                let reversal_price = (100 + direction) as u64;
                let new_target = (100 - 20 * direction) as u64;
                let mut lots = 7;
                // Both same-slot orders reduce one lot at the already committed price.
                // Publication resets 4,400 old-target units; a trade cannot do so itself.
                for publish in if trade_first {
                    [false, true]
                } else {
                    [true, false]
                } {
                    if publish {
                        let sequences = env.control_sequences(0);
                        let push = |mark_e6, observation_sequence| Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(env.admin.pubkey(), true),
                                AccountMeta::new(env.market, false),
                            ],
                            data: ProgInstruction::PushAuthMark {
                                market_id: env.asset_market_id(0),
                                asset_index: 0,
                                now_slot: 6,
                                mark_e6,
                                observation_sequence,
                                authority_epoch: sequences.authority_epoch,
                            }
                            .encode(),
                        };
                        let sequence = next_control_sequence(sequences.oracle_observation);
                        let valid = push(new_target, sequence);
                        let invalid = push(0, next_control_sequence(sequence));
                        let keys: Vec<_> = [
                            env.market,
                            env.mint,
                            env.vault,
                            env.admin.pubkey(),
                            env.payer.pubkey(),
                        ]
                        .into_iter()
                        .chain(portfolios)
                        .chain(tokens)
                        .chain(owners.each_ref().map(Signer::pubkey))
                        .collect();
                        let mut before: Vec<_> = keys
                            .iter()
                            .map(|key| env.svm.get_account(key).unwrap())
                            .collect();
                        env.svm.expire_blockhash();
                        let tx = Transaction::new_signed_with_payer(
                            &[heap_ix(), cu_ix(), valid.clone(), invalid],
                            Some(&env.payer.pubkey()),
                            &[&env.payer, &env.admin],
                            env.svm.latest_blockhash(),
                        );
                        before[4].lamports -= u64::from(tx.message.header.num_required_signatures)
                            * FeeStructure::default().lamports_per_signature;
                        let failure = env
                            .svm
                            .send_transaction(tx)
                            .expect_err("zero mark suffix must reject");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                3,
                                InstructionError::Custom(PercolatorError::OracleInvalid as u32)
                            )
                        );
                        assert_eq!(
                            failure
                                .meta
                                .logs
                                .iter()
                                .filter(
                                    |line| **line == format!("Program {} success", env.program_id)
                                )
                                .count(),
                            1
                        );
                        max_cu = max_cu.max(failure.meta.compute_units_consumed);
                        assert_eq!(keys.iter().map(|key| env.svm.get_account(key).unwrap()).collect::<Vec<_>>(), before, "publication, carry reset and control sequence must roll back with only the calculated network fee");
                        inv045_reversal_checkpoint(
                            &env,
                            portfolios,
                            6,
                            100,
                            old_target,
                            14_400,
                            lots,
                            7 * direction,
                        );
                        env.svm.expire_blockhash();
                        max_cu = max_cu.max(
                            send_raw_ixs(
                                &mut env.svm,
                                &env.payer,
                                vec![heap_ix(), cu_ix(), valid],
                                &[&env.admin],
                            )
                            .expect("identical valid publication remains usable"),
                        );
                    } else {
                        env.svm.expire_blockhash();
                        max_cu = max_cu.max(env.trade_with_cu(
                            &owners[0],
                            portfolios[0],
                            &owners[1],
                            portfolios[1],
                            -(POS_SCALE as i128),
                            reversal_price,
                            0,
                        ));
                        lots -= 1;
                    }
                    let published = publish || !trade_first;
                    inv045_reversal_checkpoint(
                        &env,
                        portfolios,
                        6,
                        if published { reversal_price } else { 100 },
                        if published { new_target } else { old_target },
                        if published { 0 } else { 14_400 },
                        lots,
                        7 * direction,
                    );
                }
                let slots: &[u64] = if split { &[7, 8, 9, 10, 11] } else { &[11] };
                for &slot in slots {
                    env.svm.warp_to_slot(slot);
                    let numerator = 100 * 24 * (slot - 6);
                    let profit = direction * (7 - 6 * (numerator / 10_000) as i128);
                    for actor in owner_order {
                        let cu = env.crank_if_actionable(
                            portfolios[actor],
                            ProgInstruction::PermissionlessCrank {
                                now_slot: slot,
                                observations: crank_observations(0),
                            },
                        );
                        max_cu = max_cu.max(cu.unwrap_or(0));
                        inv045_reversal_checkpoint(
                            &env,
                            portfolios,
                            slot,
                            reversal_price,
                            new_target,
                            numerator,
                            6,
                            profit,
                        );
                    }
                }
                let endpoint = inv045_reversal_checkpoint(
                    &env,
                    portfolios,
                    11,
                    reversal_price,
                    new_target,
                    100 * 24 * 5,
                    6,
                    direction,
                );
                assert_eq!(endpoint.0, 100);
                assert_eq!(endpoint.1, 2_000);
                assert_eq!(tokens.map(|token| env.token_amount(token)), [0, 0]);
                if let Some(expected) = baseline {
                    assert_eq!(
                        endpoint, expected,
                        "split={split}, trade_first={trade_first}, direction={direction}"
                    );
                } else {
                    baseline = Some(endpoint);
                }
            }
        }
    }
    assert_cu_within("fractional target reversal", max_cu, 1_400_000);
    eprintln!("INV-045 fractional target reversal: 8 histories, 8 exact suffix rollbacks, peak {max_cu} CU");
}

#[test]
fn v16_probe_ewma_fee_covers_large_passive_oi_moved_by_small_wash_trades() {
    const MARK: u64 = 100;
    const VICTIM_Q: i128 = 100 * POS_SCALE as i128;
    const WASH_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);

    let attacker = Keypair::new();
    let victim = Keypair::new();
    let wash_long = Keypair::new();
    let wash_short = Keypair::new();
    let attacker_account = env.create_portfolio(&attacker);
    let victim_account = env.create_portfolio(&victim);
    let wash_long_account = env.create_portfolio(&wash_long);
    let wash_short_account = env.create_portfolio(&wash_short);
    for (owner, portfolio) in [
        (&attacker, attacker_account),
        (&victim, victim_account),
        (&wash_long, wash_long_account),
        (&wash_short, wash_short_account),
    ] {
        env.deposit(owner, portfolio, 10_000_000_000);
    }
    env.trade_asset_with_cu(
        0,
        &attacker,
        attacker_account,
        &victim,
        victim_account,
        VICTIM_Q,
        MARK,
        0,
    );
    let insurance_before = env.market_state().1.insurance;
    let coalition_before = [attacker_account, wash_long_account, wash_short_account]
        .into_iter()
        .map(|portfolio| {
            let account = env.portfolio_state(portfolio);
            i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
        })
        .sum::<i128>();

    for slot in 2..=20u64 {
        env.svm.warp_to_slot(slot);
        if slot > 2 {
            env.svm.expire_blockhash();
            env.crank(
                wash_long_account,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
            for portfolio in [attacker_account, victim_account] {
                let state = env.portfolio_state(portfolio);
                let leg = active_leg_for_asset(&state, 0);
                let asset = env.market_state().1.assets[0];
                let cohort_epoch = match leg.side {
                    SideV16::Long => asset.kf_epoch_long,
                    SideV16::Short => asset.kf_epoch_short,
                };
                if leg.kf_epoch_snap == cohort_epoch {
                    continue;
                }
                env.svm.expire_blockhash();
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: vec![],
                    },
                );
            }
        }
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            WASH_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("small wash trade at slot {slot} failed: {err}"));
        let mark_after_open = env.market_state().0.mark_ewma_e6;
        // Target staging may block another risk increase, but cannot strand either side. Both
        // accounts can return to zero immediately while the just-published target is pending.
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            -WASH_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash exit at slot {slot} failed: {err}"));
        let (cfg_after_exit, group_after_exit) = env.market_state();
        assert_eq!(
            cfg_after_exit.mark_ewma_e6, mark_after_open,
            "slot {slot}: same-slot exit compounded or reversed the paid mark movement"
        );
        assert_eq!(
            group_after_exit.assets[0].raw_oracle_target_price, mark_after_open,
            "slot {slot}: exit staged a second target"
        );
    }

    let (cfg_before_crank, group_before_crank) = env.market_state();
    let fees_paid = group_before_crank.insurance - insurance_before;
    assert!(group_before_crank.assets[0].effective_price > MARK);
    assert!(
        cfg_before_crank.mark_ewma_e6 >= group_before_crank.assets[0].effective_price,
        "the paid upward target cannot reverse before catch-up"
    );
    assert_eq!(
        group_before_crank.assets[0].raw_oracle_target_price, cfg_before_crank.mark_ewma_e6,
        "exactly one paid target may remain pending"
    );
    env.svm.warp_to_slot(21);
    for portfolio in [attacker_account, victim_account] {
        env.svm.expire_blockhash();
        env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 21,
                observations: crank_observations(0),
            },
        );
    }
    let (cfg_after_crank, group_after_crank) = env.market_state();
    assert_eq!(
        group_after_crank.assets[0].effective_price, cfg_after_crank.mark_ewma_e6,
        "the next bounded public crank must catch up the one pending segment"
    );

    let attacker_pnl = env.portfolio_state(attacker_account).pnl.get();
    let coalition_after = [attacker_account, wash_long_account, wash_short_account]
        .into_iter()
        .map(|portfolio| {
            let account = env.portfolio_state(portfolio);
            i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
        })
        .sum::<i128>();
    assert!(
        attacker_pnl > 0,
        "wash prints must actually move the large book"
    );
    assert!(
        attacker_pnl as u128 <= fees_paid,
        "small wash fills must pay for the full passive-book transfer: pnl={attacker_pnl}, fees={fees_paid}"
    );
    assert!(
        coalition_after <= coalition_before,
        "the mark manipulator coalition must be EV-neutral or worse: {coalition_before}->{coalition_after}"
    );
}

#[test]
fn v16_attack_repeated_ewma_moves_require_catchup_and_remain_fee_covered() {
    const MARK: u64 = 100;
    const BASE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);

    let attacker = Keypair::new();
    let victim = Keypair::new();
    let wash_long = Keypair::new();
    let wash_short = Keypair::new();
    let attacker_account = env.create_portfolio(&attacker);
    let victim_account = env.create_portfolio(&victim);
    let wash_long_account = env.create_portfolio(&wash_long);
    let wash_short_account = env.create_portfolio(&wash_short);
    for (owner, portfolio) in [
        (&attacker, attacker_account),
        (&victim, victim_account),
        (&wash_long, wash_long_account),
        (&wash_short, wash_short_account),
    ] {
        env.deposit(owner, portfolio, 10_000_000_000);
    }

    env.trade_asset_with_cu(
        0,
        &attacker,
        attacker_account,
        &victim,
        victim_account,
        BASE_Q,
        MARK,
        0,
    );
    let insurance_before = env.market_state().1.insurance;
    let victim_capital_before = env.portfolio_state(victim_account).capital.get();
    let coalition_before = [attacker_account, wash_long_account, wash_short_account]
        .into_iter()
        .map(|portfolio| {
            let account = env.portfolio_state(portfolio);
            i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
        })
        .sum::<i128>();

    for slot in 2..=20u64 {
        env.svm.warp_to_slot(slot);
        if slot > 2 {
            env.svm.expire_blockhash();
            env.crank(
                wash_long_account,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
            for portfolio in [attacker_account, victim_account] {
                let state = env.portfolio_state(portfolio);
                let leg = active_leg_for_asset(&state, 0);
                let asset = env.market_state().1.assets[0];
                let cohort_epoch = match leg.side {
                    SideV16::Long => asset.kf_epoch_long,
                    SideV16::Short => asset.kf_epoch_short,
                };
                if leg.kf_epoch_snap == cohort_epoch {
                    continue;
                }
                env.svm.expire_blockhash();
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: vec![],
                    },
                );
            }
        }
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            BASE_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash trade at slot {slot} failed: {err}"));
        let mark_after_open = env.market_state().0.mark_ewma_e6;
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            -BASE_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash exit at slot {slot} failed: {err}"));
        let (cfg_after_exit, group_after_exit) = env.market_state();
        assert_eq!(
            cfg_after_exit.mark_ewma_e6, mark_after_open,
            "slot {slot}: same-slot exit compounded or reversed the paid mark movement"
        );
        assert_eq!(
            group_after_exit.assets[0].raw_oracle_target_price, mark_after_open,
            "slot {slot}: exit staged a second target"
        );
    }

    let (cfg_before_crank, group_before_crank) = env.market_state();
    let fees_paid = group_before_crank.insurance - insurance_before;
    assert!(group_before_crank.assets[0].effective_price > MARK);
    assert!(
        cfg_before_crank.mark_ewma_e6 >= group_before_crank.assets[0].effective_price,
        "the paid upward target cannot reverse before catch-up"
    );
    assert_eq!(
        group_before_crank.assets[0].raw_oracle_target_price, cfg_before_crank.mark_ewma_e6,
        "exactly one paid target may remain pending"
    );

    env.svm.warp_to_slot(21);
    for portfolio in [attacker_account, victim_account] {
        env.svm.expire_blockhash();
        env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 21,
                observations: crank_observations(0),
            },
        );
    }
    let (cfg_after_crank, group_after_crank) = env.market_state();
    assert_eq!(
        group_after_crank.assets[0].effective_price, cfg_after_crank.mark_ewma_e6,
        "the next bounded public crank must catch up the one pending segment"
    );

    let attacker_after = env.portfolio_state(attacker_account);
    let victim_after = env.portfolio_state(victim_account);
    let attacker_pnl = attacker_after.pnl.get();
    let coalition_after = [attacker_account, wash_long_account, wash_short_account]
        .into_iter()
        .map(|portfolio| {
            let account = env.portfolio_state(portfolio);
            i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
        })
        .sum::<i128>();
    assert!(attacker_pnl > 0);
    assert_eq!(
        victim_after.pnl.get(),
        0,
        "the losing short settles its negative PnL"
    );
    let victim_loss = victim_capital_before
        .checked_sub(victim_after.capital.get())
        .expect("upward paid mark cannot credit the passive short");
    assert_eq!(
        victim_loss, attacker_pnl as u128,
        "the passive loss and attacker claim must attribute exactly"
    );
    assert!(
        attacker_pnl as u128 <= fees_paid,
        "accumulated mark-created claim must remain covered by wash fees: pnl={}, fees={fees_paid}",
        attacker_pnl
    );
    assert!(
        coalition_after <= coalition_before,
        "the mark manipulator coalition must be EV-neutral or worse: {coalition_before}->{coalition_after}"
    );
}

#[test]
fn v16_attack_nocpi_positive_mark_min_fee_does_not_dos_or_move_ewma_for_free() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const MAX_FEE_BPS: u64 = 37;
    const TRADE_SLOT: u64 = 5;
    const SIZE_Q: i128 = (1000u128 * POS_SCALE) as i128;
    const HIGH_PRINT: u64 = MARK * 19 / 10;
    const MARK_MIN_FEE: u64 = 100_000_000;

    let accepted_price = oracle_v16::clamp_toward_engine_dt(MARK, HIGH_PRINT, CAP_BPS, 4);
    let candidate_mark = percolator_prog::policy_v16::ewma_update(
        MARK,
        accepted_price,
        1,
        1,
        TRADE_SLOT,
        MARK_MIN_FEE,
        MARK_MIN_FEE,
    );
    let candidate_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(MARK, candidate_mark)
        .expect("candidate move bps");
    assert!(
        candidate_move_bps > MAX_FEE_BPS,
        "setup must make the full mark-min-fee EWMA candidate exceed the market fee cap"
    );

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: MAX_FEE_BPS,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(1);
        env.configure_ewma_mark_with_cu(1, MARK, 1, MARK_MIN_FEE);
        env.svm.warp_to_slot(TRADE_SLOT);
        let (owner_a, account_a, owner_b, account_b) =
            funded_no_cpi_reported_price_pair(&mut env, 4_000_000_000);
        let insurance_before = env.market_state().1.insurance;

        env.svm.expire_blockhash();
        let trade = try_no_cpi_reported_price_trade_with_cu(
            &mut env, path, &owner_a, account_a, &owner_b, account_b, SIZE_Q, HIGH_PRINT, 0,
        );
        assert!(
            trade.is_ok(),
            "{path:?}: positive mark_min_fee must not DoS a valid off-mark trade: {trade:?}"
        );

        let (cfg, group) = env.market_state();
        let fee_paid = group.insurance - insurance_before;
        assert!(
            fee_paid > 0 && fee_paid < MARK_MIN_FEE as u128,
            "{path:?}: setup must pay a nonzero fee below mark_min_fee"
        );
        let mark_move_bps =
            percolator_prog::policy_v16::price_move_bps_ceil(MARK, cfg.mark_ewma_e6)
                .expect("actual mark move bps");
        assert_eq!(
            mark_move_bps, MAX_FEE_BPS,
            "{path:?}: mark movement should bind at paid fee headroom, not at mark_min_fee"
        );
        let trade_notional = SIZE_Q.unsigned_abs() * accepted_price as u128 / POS_SCALE;
        let paid_move_bps = fee_paid * 10_000 / (trade_notional * 2);
        assert!(
            mark_move_bps <= paid_move_bps as u64,
            "{path:?}: positive mark_min_fee EWMA move ({mark_move_bps} bps) must be paid by fees ({paid_move_bps} bps)"
        );
        assert_eq!(group.assets[0].oi_eff_long_q, SIZE_Q.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, SIZE_Q.unsigned_abs());
    }
}

#[test]
fn v16_attack_cpi_matcher_price_caps_ewma_move_without_dos() {
    for path in [CpiEwmaTradePath::Single, CpiEwmaTradePath::Batch] {
        assert_cpi_matcher_price_caps_paid_ewma_move(path, (1000u128 * POS_SCALE) as i128);
        assert_cpi_matcher_price_caps_paid_ewma_move(path, -((1000u128 * POS_SCALE) as i128));
        assert_cpi_matcher_price_caps_paid_ewma_move(path, 1);
        assert_cpi_matcher_price_caps_paid_ewma_move(path, -1);
    }
}

#[test]
fn v16_attack_underfunded_exit_cannot_move_ewma_with_uncollectible_fee() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        assert_underfunded_ewma_exit_uses_collected_fee(path);
    }
}

#[test]
fn v16_attack_underfunded_cpi_exit_cannot_move_ewma_with_uncollectible_fee() {
    for path in [CpiEwmaTradePath::Single, CpiEwmaTradePath::Batch] {
        assert_underfunded_cpi_ewma_exit_uses_collected_fee(path);
    }
}

// Attacker criterion (must FAIL): declaring exec_price << mark charges a smaller fee than the mark.
#[test]
fn v16_program_tradenocpi_fee_cannot_be_evaded_via_exec_price() {
    // fee charged (insurance delta) for the SAME mark-valued position at different declared exec_prices.
    let fee_for = |exec_price: u64| -> u128 {
        let mut env = V16CuEnv::new();
        env.configure_auth_mark_with_cu(0, 100); // mark = 100
        let oa = Keypair::new();
        let a = env.create_portfolio(&oa);
        let ob = Keypair::new();
        let b = env.create_portfolio(&ob);
        env.deposit(&oa, a, 100_000_000_000);
        env.deposit(&ob, b, 100_000_000_000);
        let ins0 = env.market_state().1.insurance;
        env.trade_asset_with_cu(
            0,
            &oa,
            a,
            &ob,
            b,
            (1000 * POS_SCALE) as i128,
            exec_price,
            100,
        );
        env.market_state().1.insurance - ins0
    };
    let fee_at_mark = fee_for(100);
    assert!(
        fee_at_mark > 0,
        "non-vacuous: a real fee is charged at the mark"
    );
    // The whole point of the fix: a tiny declared exec_price does NOT reduce the fee anymore.
    assert_eq!(
        fee_for(1),
        fee_at_mark,
        "exec_price=1 is billed the SAME mark-based fee (no evasion)"
    );
    assert_eq!(
        fee_for(50),
        fee_at_mark,
        "exec_price below mark is billed the mark-based fee"
    );
    // Declaring a HIGH exec_price must not over-bill either — fee is pinned to the mark, not the caller.
    assert_eq!(
        fee_for(100_000),
        fee_at_mark,
        "exec_price above mark is also billed the mark-based fee"
    );
}

// reported price used to bypass the per-slot clamp and move the mark toward zero via a wash trade.
#[test]
fn v16_program_nocpi_zero_reported_price_cannot_drive_ewma_or_hybrid_mark() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        assert_zero_reported_price_rejects_atomically(
            zero_reported_price_ewma_env(),
            path,
            "EWMA mark",
        );
        assert_zero_reported_price_rejects_atomically(
            zero_reported_price_hybrid_after_hours_env(),
            path,
            "Hybrid after-hours mark",
        );
    }
}

// different mark than the charged price justified.
#[test]
fn v16_program_nocpi_epsilon_reported_price_uses_dt_clamped_fee_and_ewma_price() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const DT: u64 = 4;
    let dt_clamped_epsilon = oracle_v16::clamp_toward_engine_dt(MARK, 1, CAP_BPS, DT);
    assert_eq!(
        dt_clamped_epsilon, 980_000,
        "test setup must exercise a multi-slot engine dt clamp"
    );

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let epsilon = ewma_no_cpi_fee_and_mark_for_reported_price(path, 1);
        let clamped = ewma_no_cpi_fee_and_mark_for_reported_price(path, dt_clamped_epsilon);
        let at_mark = ewma_no_cpi_fee_and_mark_for_reported_price(path, MARK);

        assert_eq!(
            epsilon, clamped,
            "{path:?}: epsilon report must be normalized to the accepted dt-clamped price for fee and EWMA"
        );
        assert_ne!(
            epsilon.0, at_mark.0,
            "{path:?}: non-vacuous fee assertion; the accepted below-mark print must not be billed as old mark"
        );
        assert_ne!(
            epsilon.1, at_mark.1,
            "{path:?}: non-vacuous EWMA assertion; the accepted below-mark print must move the mark"
        );
        assert_eq!(
            at_mark.1, MARK,
            "{path:?}: at-mark control should not move EWMA"
        );
        assert!(
            epsilon.1 < at_mark.1,
            "{path:?}: lower accepted print should move EWMA below at-mark control"
        );
    }
}

// unit against this price. The trade must still execute, but the unpaid print must move no EWMA.
#[test]
fn v16_program_nocpi_trade_not_dosed_by_extreme_reported_price() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        for reported_price in [1, percolator::MAX_ORACLE_PRICE] {
            assert_no_cpi_tiny_exit_accepts_extreme_reported_price(path, reported_price);
            assert_no_cpi_tiny_open_accepts_extreme_reported_price(path, reported_price);
        }
    }
}

// is reduced to what the capped fee actually pays for.
#[test]
fn v16_program_nocpi_extreme_price_caps_ewma_move_without_dos() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        for reported_price in [1, percolator::MAX_ORACLE_PRICE] {
            assert_no_cpi_extreme_reported_price_caps_paid_ewma_move(path, reported_price);
        }
    }
}

// the fee is computed on the mark, so the matcher's price-quoting knobs (spread) cannot change the fee.
#[test]
fn v16_program_tradecpi_fee_is_mark_pinned_not_matcher_quoted() {
    let fee_for_spread = |base_spread: u32, max_total: u32| -> u128 {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            trade_fee_base_bps: 100,
            ..V16CuMarketParams::default()
        });
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let to = Keypair::new();
        let t = env.create_portfolio(&to);
        let mo = Keypair::new();
        let m = env.create_portfolio(&mo);
        env.deposit(&to, t, 100_000_000);
        env.deposit(&mo, m, 100_000_000);
        let (ctx, del, _) = env.init_matcher_context_with_passive_spread_authorized(
            matcher_program,
            &mo,
            m,
            base_spread,
            max_total,
        );
        let ins0 = env.market_state().1.insurance;
        let _ = env.trade_cpi_with_cu_on_asset(
            &to,
            t,
            &mo,
            m,
            matcher_program,
            ctx,
            del,
            0,
            (10 * POS_SCALE) as i128,
            100,
        );
        // entry is at the mark (effective_price==100) regardless of the matcher's spread.
        assert_eq!(
            env.market_state().1.assets[0].effective_price,
            100,
            "position priced at the mark"
        );
        assert_eq!(
            env.portfolio_state(t).pnl.get(),
            0,
            "no off-market PnL from the matcher quote"
        );
        env.market_state().1.insurance - ins0
    };
    // fee at the mark (no spread): 10*POS_SCALE @ 100 = notional 1000 -> 100bps -> 10/side -> 20 total.
    let fee_no_spread = fee_for_spread(0, 100);
    assert_eq!(fee_no_spread, 20, "baseline CPI fee is the mark-based fee");
    // a WIDE matcher spread (20%) charges the IDENTICAL fee -> the fee is on the mark, NOT the matcher's
    // (potentially manipulated) exec_price. A malicious matcher cannot under-bill the CPI fee.
    assert_eq!(
        fee_for_spread(2_000, 4_000),
        fee_no_spread,
        "matcher spread does not change the CPI fee (mark-pinned)"
    );
    assert_eq!(
        fee_for_spread(500, 1_000),
        fee_no_spread,
        "fee invariant to a moderate spread too"
    );
}

// per-slot cap. Complements #10533 with the EWMA mode.
#[test]
fn v16_program_ewma_mark_respects_per_slot_circuit_breaker() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const MOVE_BPS: u64 = 1_000; // 10% / slot circuit breaker
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: MOVE_BPS,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    // EWMA mark mode with an aggressive halflife (1 slot) so the smoothed target races toward any push.
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 50_000_000);
    env.deposit(&sh_owner, sh, 50_000_000);
    // open matched OI so equity is active (the move gate at v16.rs:8060 only fires when equity_active).
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    let mut prev_price = INITIAL_PRICE;
    // push the EWMA mark to 100x EVERY slot and crank; the effective price must never jump past the cap.
    for slot in 1..=4u64 {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, INITIAL_PRICE * 100); // 100,000,000 — 100x
        env.svm.expire_blockhash();
        // crank may succeed (clamped move) or be refused (RecoveryRequired) — either way price is bounded.
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lo, false),
            ],
            &[],
        );
        let g = env.market_state().1;
        let ep = g.assets[0].effective_price;
        // PER-SLOT CAP: the move from the previous settled price is at most MOVE_BPS (10%); a small
        // tolerance covers rounding in the EWMA target. The mark authority CANNOT jump the price 100x.
        let cap = prev_price + prev_price * (MOVE_BPS + 1) / 10_000;
        assert!(
            ep <= cap,
            "slot {}: effective price {} clamped to <= per-slot cap {} (prev {})",
            slot,
            ep,
            cap,
            prev_price
        );
        prev_price = ep;
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "senior conservation under clamped EWMA move"
        );
        assert_eq!(
            g.vault as u64,
            env.token_amount(env.vault),
            "accounting vault == real on-chain vault"
        );
    }
    // NON-VACUITY + the headline: after FOUR slots of pushing to 100,000,000, the effective settlement
    // price is still nowhere near it — the EWMA mode did NOT bypass the circuit breaker.
    let final_price = env.market_state().1.assets[0].effective_price;
    assert!(final_price < INITIAL_PRICE * 2, "after 4 clamped slots the price ({}) is far below the 100x push (circuit breaker held across EWMA mode)", final_price);
    // NON-VACUITY: the price DID move toward the push (the clamp throttled a real move, it wasn't a no-op).
    assert!(
        final_price > INITIAL_PRICE,
        "the EWMA mark actually moved the price (clamped, not frozen): {} > {}",
        final_price,
        INITIAL_PRICE
    );
}

// hyperp-index Bug #9 fix, which has its own test.)
#[test]
fn v16_program_push_ewma_mark_same_slot_does_not_compound() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(1, 100, 10, 0); // asset 0 EWMA, mark 100, halflife 10
    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    env.push_ewma_mark_with_cu(5, 1000); // push toward 1000 -> partial (alpha) move, not a jump
    let m1 = env.market_state().0.mark_ewma_e6;
    assert!(
        m1 > 100 && m1 < 1000,
        "first push moves EWMA partially toward target (no instant jump): {m1}"
    );
    // SECOND push, SAME slot, same target: dt==0 -> must NOT move (no compounding).
    env.svm.expire_blockhash();
    env.push_ewma_mark_with_cu(5, 1000);
    let m2 = env.market_state().0.mark_ewma_e6;
    assert_eq!(
        m2, m1,
        "same-slot repeated PushEwmaMark must not compound (dt==0 -> no movement)"
    );
    // a genuine slot advance resumes movement (time-gated, not frozen).
    env.svm.warp_to_slot(6);
    env.svm.expire_blockhash();
    env.push_ewma_mark_with_cu(6, 1000);
    let m3 = env.market_state().0.mark_ewma_e6;
    assert!(
        m3 > m2,
        "advancing a slot resumes EWMA movement: {m3} vs {m2}"
    );
}

// security.md sweep — oracle/mark bounds (#37/#39): the auth-mark push feeds settlement. An extreme
// mark (0 or u64::MAX) must be rejected/clamped, never corrupt pnl or panic the program.
#[test]
fn v16_attack_extreme_auth_mark_push_rejected_or_safe() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(5);
    // push extreme marks; each must reject or be clamped — never panic, never corrupt state.
    for mark in [0u64, 1, u64::MAX, u64::MAX / 2] {
        env.svm.expire_blockhash();
        let market_before_push = env.svm.get_account(&env.market).unwrap();
        let push = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 0,
                now_slot: 5,
                mark_e6: mark,
                authority_epoch: 0,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        );
        match push {
            Ok(_) => assert_ne!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_push,
                "an accepted extreme mark push must commit authenticated profile state"
            ),
            Err(_) => assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_push,
                "a rejected extreme mark push must roll back exactly"
            ),
        }
        // Crank against whatever mark landed; it must make bounded progress or
        // reject as an exactly framed fixed point.
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lo, false),
            ],
            &[],
        );
        let (_, g) = env.market_state();
        assert_eq!(
            g.vault, 2_000_000,
            "vault intact under extreme mark {}",
            mark
        );
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "senior conservation under extreme mark {}",
            mark
        );
        assert!(
            g.assets[0].effective_price > 0
                && g.assets[0].effective_price <= percolator::MAX_ORACLE_PRICE,
            "effective price stays in valid bounds under extreme mark {} (got {})",
            mark,
            g.assets[0].effective_price
        );
    }
    // state still decodes and positions intact (no corruption). Vault holds exactly the deposits
    // (checked per-iteration); accounted equity + insurance never EXCEEDS the vault (the small
    // difference is the in-vault §6.2 residual buffer, not lost value).
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let accounted = (a.capital.get() as i128 + a.pnl.get())
        + (b.capital.get() as i128 + b.pnl.get())
        + env.market_state().1.insurance as i128;
    assert!(
        accounted <= 2_000_000,
        "no value created by extreme mark pushes (accounted {})",
        accounted
    );
    assert!(
        accounted >= 2_000_000 - 1_000,
        "value not materially destroyed; remainder is in-vault residual (accounted {})",
        accounted
    );
}

// security.md sweep — circuit breaker on mark push (#9 oracle manipulation): a push to a far-away
// mark must move the effective price by at most max_price_move_bps_per_slot per slot. An attacker
// (mark authority) cannot jump the settlement price arbitrarily in one slot.
#[test]
fn v16_attack_mark_push_clamped_per_slot() {
    let mut env = V16CuEnv::new(); // max_price_move_bps_per_slot = 10_000 (100%/slot)
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    let mut prev_price = 100u64;
    for slot in [10u64, 11, 12] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 1_000_000); // push to a huge mark (10000x) every slot
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lo, false),
            ],
            &[],
        );
        let (_, g) = env.market_state();
        let ep = g.assets[0].effective_price;
        // the per-slot move is clamped to <= 100% (price at most doubles per slot).
        assert!(
            ep <= prev_price * 2,
            "slot {}: effective price {} clamped to <= 2x prev {} (circuit breaker)",
            slot,
            ep,
            prev_price
        );
        assert!(
            ep > prev_price,
            "slot {}: price moved toward the pushed mark (non-vacuous)",
            slot
        );
        prev_price = ep;
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "senior conservation under clamped move"
        );
    }
    // even after 3 slots of pushing to 1,000,000, the effective price is nowhere near it (clamped).
    let (_, g) = env.market_state();
    assert!(
        g.assets[0].effective_price <= 800,
        "after 3 clamped slots, price is far below the 1,000,000 push (got {})",
        g.assets[0].effective_price
    );
}

// security.md sweep — EWMA mark halflife edge (#37): configuring the EWMA mark with halflife 0
// (instant) must be handled cleanly — no div-by-zero/panic, no settlement corruption. The mark/price
// stays in valid bounds and conservation holds.
#[test]
fn v16_attack_ewma_mark_halflife_zero_safe() {
    let mut env = V16CuEnv::new();
    // configure with halflife = 0 (instant). If accepted, settlement must stay safe.
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 0,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 0,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    let lo = Keypair::new();
    let plo = env.create_portfolio(&lo);
    let sh = Keypair::new();
    let psh = env.create_portfolio(&sh);
    env.deposit(&lo, plo, 1_000_000);
    env.deposit(&sh, psh, 1_000_000);
    env.trade_with_cu(&lo, plo, &sh, psh, POS_SCALE as i128, 100, 0);
    // if the halflife=0 config was accepted, push a mark and crank — must not panic/corrupt.
    if r.is_ok() {
        env.svm.warp_to_slot(1);
        env.push_ewma_mark_with_cu(1, 150);
        for slot in [1u64, 2] {
            env.svm.warp_to_slot(slot);
            for p in [psh, plo] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                );
            }
        }
    }
    // regardless of accept/reject: no corruption, state decodes, conservation holds, price in bounds.
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault intact");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert!(
        g.assets[0].effective_price > 0
            && g.assets[0].effective_price <= percolator::MAX_ORACLE_PRICE,
        "price in valid bounds (no corruption)"
    );
    let _ = r;
}

// security.md sweep — EWMA-mark mode respects the per-slot circuit breaker (#6/#9/#37): the auth-mark
// test (#10533) proves AUTH_MARK clamps to <= max_price_move_bps_per_slot; EWMA_MARK is a SEPARATE
// pricing mode (halflife smoothing toward a pushed mark) and could, if the clamp lived only in the
// auth-mark path, let the mark authority move the effective settlement price arbitrarily fast in one
// slot. Attacker goal: as mark authority, push the EWMA mark to 100x and have the NEXT crank settle the
// effective price (used for liquidation/funding/PnL) at the full 100x in a single slot. Protection: the
// per-slot move gate is mode-INDEPENDENT — it lives in accrue_asset_to_not_atomic (percolator/src/v16.rs:
// 8052: price_diff*MAX_MARGIN_BPS <= max_price_move_bps_per_slot*segment_dt*old_price, else RecoveryRequired),
// so an EWMA push beyond the budget either clamps or is refused; the effective price cannot jump past the
// security.md sweep — crank dt-clamp prevents retroactive one-shot settlement after a long gap (#9/#22):
// the per-crank segment is clamped to max_accrual_dt_slots (percolator/src/v16.rs:8037), and a crank
// advances slot_last by only that clamped segment (v16.rs:8089), NOT all the way to now_slot. This dt is
// the multiplier on EVERY time-scaled accrual (funding: v16.rs:8072 funding_rate*segment_dt*price; and the
// price-move budget: v16.rs:8057 cap*segment_dt). Attacker goal: leave the market un-cranked for a long
// time so a large accrual window builds up, then fire ONE crank to retroactively settle the FULL elapsed
// window in a single shot — a one-block funding windfall to the favoured side, or a surprise margin blow-up
// of the other side that skips the per-slot price-move circuit breaker. Protection: each crank settles at
// most max_accrual_dt_slots; slot_last creeps forward by the clamp per crank, so catch-up is bounded and
// many cranks (each itself re-clamped) are required to close the gap — no one-shot retroactive settlement.
// (Not covered by the conservation tests, which all crank every slot so dt is always 1.)
#[test]
fn v16_attack_crank_dt_clamp_blocks_retroactive_settle() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DT_CLAMP: u64 = 3; // max_accrual_dt_slots
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: DT_CLAMP,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: DT_CLAMP, // lifetime >= accrual window (config validity)
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 50_000_000);
    env.deposit(&sh_owner, sh, 50_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    let slot_last_0 = env.market_state().1.assets[0].slot_last;
    assert_eq!(
        slot_last_0, 0,
        "asset starts settled at slot 0 (open position is live)"
    );

    // ATTACK: leave the market UNCRANKED for a huge window, then fire ONE crank that tries to settle it all.
    const GAP_SLOT: u64 = 500;
    env.svm.warp_to_slot(GAP_SLOT);
    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: GAP_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
        ],
        &[],
    );
    assert!(r.is_ok(), "the catch-up crank itself succeeds: {:?}", r);

    let g1 = env.market_state().1;
    // ANTI-RETROACTIVITY (the headline): the settled segment is the dt CLAMP (3), NOT the 500-slot gap.
    // A naive retroactive settle would jump slot_last to 500 and apply ~166x the funding/move budget in one
    // block; the clamp holds the advance to exactly max_accrual_dt_slots.
    assert_eq!(
        g1.assets[0].slot_last,
        slot_last_0 + DT_CLAMP,
        "one crank settles only max_accrual_dt_slots ({}), NOT the full {}-slot gap (slot_last={})",
        DT_CLAMP,
        GAP_SLOT,
        g1.assets[0].slot_last
    );
    assert!(GAP_SLOT > 50 * DT_CLAMP, "non-vacuous: the elapsed gap ({}) dwarfs the clamp ({}) — the clamp is the binding constraint", GAP_SLOT, DT_CLAMP);
    assert_eq!(
        g1.current_slot, GAP_SLOT,
        "header current_slot advances to now (the asset is intentionally left 'behind' by design)"
    );
    // conservation: whatever was settled is internal redistribution; nothing minted.
    assert_eq!(
        g1.vault, 100_000_000,
        "vault unchanged (settlement mints nothing)"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation under the clamped catch-up"
    );

    // A SECOND crank at the same now_slot advances slot_last by ANOTHER clamp window — catch-up is bounded
    // PER crank, so closing a 500-slot gap needs ~167 separately-clamped cranks, each re-subject to the
    // per-slot price-move circuit breaker. There is no single transaction that settles the whole window.
    env.svm.expire_blockhash();
    let r2 = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: GAP_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
        ],
        &[],
    );
    assert!(r2.is_ok(), "second catch-up crank succeeds: {:?}", r2);
    let g2 = env.market_state().1;
    assert_eq!(
        g2.assets[0].slot_last,
        slot_last_0 + 2 * DT_CLAMP,
        "second crank advances another clamp window (bounded per-crank catch-up)"
    );
    assert!(g2.assets[0].slot_last < GAP_SLOT, "even after two cranks the asset is still far short of the gap (catch-up is throttled, not instant)");
    assert!(
        g2.vault >= g2.c_tot + g2.insurance,
        "senior conservation after second crank"
    );
}

fn configure_max_shape_ewma_asset(
    env: &mut V16CuEnv,
    asset_index: u16,
    now_slot: u64,
    mark_e6: u64,
) {
    let observation_sequence = next_control_sequence(
        env.control_sequences(asset_index as usize)
            .oracle_observation,
    );
    let market_id = env.asset_market_id(asset_index);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id,
            asset_index,
            now_slot,
            initial_mark_e6: mark_e6,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("configure maximum-shape EWMA asset");
}

#[test]
fn v16_program_max_shape_ewma_movement_is_paid_and_drain_only_exit_stays_bounded() {
    const ASSET_COUNT: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const MARK: u64 = 1_000_000;
    const RAW_UP: u64 = 2_000_000;
    const DEPOSIT: u128 = 25_000_000;
    const EXPECTED_MARK: u64 = 1_005_000;
    const EXPECTED_FEE_PER_ASSET: u128 = 10_100;
    const EXPECTED_RELEASED_PNL_PER_ASSET: u128 = 5_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: ASSET_COUNT,
        initial_price: MARK,
        max_trading_fee_bps: 100,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        ..V16CuMarketParams::default()
    });
    for asset_index in 0..ASSET_COUNT {
        configure_max_shape_ewma_asset(&mut env, asset_index, 0, MARK);
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    let vault_before = env.token_amount(env.vault);

    env.svm.warp_to_slot(1);
    let open_legs = (0..ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: env.asset_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: RAW_UP,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let open_cu = env
        .send(
            env.batch_trade_no_cpi_ix(long, short, open_legs),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
                AccountMeta::new(short, false),
            ],
            &[&long_owner, &short_owner],
        )
        .expect("maximum-shape paid EWMA batch");

    let after_open = env.market_state().1;
    assert_eq!(after_open.vault as u64, vault_before);
    assert_eq!(
        after_open.insurance,
        EXPECTED_FEE_PER_ASSET * u128::from(ASSET_COUNT)
    );
    assert_eq!(
        after_open.insurance_domain_budget.iter().sum::<u128>(),
        0,
        "trade-driven mark fees remain nonwithdrawable"
    );
    assert_eq!(after_open.c_tot + after_open.insurance, after_open.vault);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long))),
        u32::from(ASSET_COUNT)
    );
    for asset_index in 0..ASSET_COUNT {
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index as usize,
        )
        .unwrap();
        assert_eq!(profile.mark_ewma_e6, EXPECTED_MARK);
        assert_eq!(profile.oracle_target_price_e6, EXPECTED_MARK);
        assert_eq!(
            after_open.assets[asset_index as usize].raw_oracle_target_price,
            EXPECTED_MARK
        );
        assert_eq!(
            after_open.assets[asset_index as usize].effective_price,
            MARK
        );
    }

    let asset_indices = (0..ASSET_COUNT).collect::<Vec<_>>();
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations_for_assets(&asset_indices),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[],
        )
        .expect("maximum-shape paid EWMA refresh");
    let after_refresh = env.market_state().1;
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(
            after_refresh.assets[asset_index].effective_price,
            EXPECTED_MARK
        );
        assert_eq!(
            after_refresh.assets[asset_index].raw_oracle_target_price,
            EXPECTED_MARK
        );
    }

    for asset_index in 0..ASSET_COUNT {
        env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_DRAIN_ONLY,
            asset_index,
            0,
            0,
        );
    }
    let mut drain_refresh_cu = 0;
    for _ in 0..(u32::from(ASSET_COUNT) * 2 + 4) {
        for portfolio in [long, short] {
            if !portfolio_certificate_is_current(&env, portfolio) {
                drain_refresh_cu = drain_refresh_cu.max(env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 1,
                        observations: vec![],
                    },
                ));
            }
        }
        if portfolio_certificate_is_current(&env, long)
            && portfolio_certificate_is_current(&env, short)
        {
            break;
        }
    }
    assert!(
        portfolio_certificate_is_current(&env, long)
            && portfolio_certificate_is_current(&env, short),
        "permissionless refresh must reach a two-account certificate fixed point"
    );
    let fee_stock_before_exit = env.market_state().1.insurance;
    let long_capital_before_exit = env.portfolio_state(long).capital.get();
    let short_capital_before_exit = env.portfolio_state(short).capital.get();
    let close_legs = (0..ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: env.asset_market_id(asset_index),
            size_q: -(POS_SCALE as i128),
            exec_price: 1,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            env.batch_trade_no_cpi_ix(long, short, close_legs),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
                AccountMeta::new(short, false),
            ],
            &[&long_owner, &short_owner],
        )
        .expect("maximum-shape DrainOnly extreme-price reduction");

    let after_close = env.market_state().1;
    assert_eq!(after_close.insurance, fee_stock_before_exit);
    assert_eq!(
        env.portfolio_state(long).capital.get(),
        long_capital_before_exit
    );
    assert_eq!(
        env.portfolio_state(short).capital.get(),
        short_capital_before_exit
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long))),
        0
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(short))),
        0
    );
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(after_close.assets[asset_index].oi_eff_long_q, 0);
        assert_eq!(after_close.assets[asset_index].oi_eff_short_q, 0);
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index,
        )
        .unwrap();
        assert_eq!(profile.mark_ewma_e6, EXPECTED_MARK);
    }

    let released_pnl = EXPECTED_RELEASED_PNL_PER_ASSET * u128::from(ASSET_COUNT);
    assert_eq!(env.portfolio_state(long).pnl.get(), released_pnl as i128);
    assert_eq!(env.portfolio_state(short).pnl.get(), 0);
    let convert_cu = env.convert_released_pnl_with_cu(&long_owner, long, released_pnl);
    let long_after_convert = env.portfolio_state(long);
    let short_after_convert = env.portfolio_state(short);
    assert_eq!(long_after_convert.pnl.get(), 0);
    assert_eq!(short_after_convert.pnl.get(), 0);
    assert_eq!(
        long_after_convert.capital.get() + short_after_convert.capital.get(),
        2 * DEPOSIT - fee_stock_before_exit
    );

    let long_dest = env.withdraw(&long_owner, long, long_after_convert.capital.get());
    let short_dest = env.withdraw(&short_owner, short, short_after_convert.capital.get());
    assert_eq!(
        u128::from(env.token_amount(long_dest)) + u128::from(env.token_amount(short_dest)),
        2 * DEPOSIT - fee_stock_before_exit
    );
    env.close_portfolio_with_cu(&long_owner, long);
    env.close_portfolio_with_cu(&short_owner, short);
    let terminal = env.market_state().1;
    assert_eq!(terminal.vault, terminal.insurance);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    assert_eq!(terminal.vault, fee_stock_before_exit);

    println!(
        "INV-045 max-shape paid EWMA open={open_cu} movement-refresh={refresh_cu} \
         drain-refresh={drain_refresh_cu} \
         DrainOnly-close={close_cu} convert={convert_cu}"
    );
    for (label, cu) in [
        ("paid EWMA open", open_cu),
        ("paid EWMA refresh", refresh_cu),
        ("DrainOnly refresh", drain_refresh_cu),
        ("DrainOnly extreme-price close", close_cu),
        ("released PnL conversion", convert_cu),
    ] {
        assert!(cu < 1_400_000, "{label} consumed {cu} CU");
    }
}

const MAX_SHAPE_HYBRID_ASSET_COUNT: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
const MAX_SHAPE_HYBRID_MARK: u64 = 1_000_000;
const MAX_SHAPE_HYBRID_DEPOSIT: u128 = 100_000_000;
// Hybrid becomes stale two slots after configuration. The one-slot price cap is 10_000 and EWMA
// alpha is 2/(2 + halflife=1), so floor(10_000 * 2/3) = 6_666.
const MAX_SHAPE_HYBRID_EXPECTED_MARK: u64 = 1_006_666;
const MAX_SHAPE_HYBRID_FEE_PER_ASSET: u128 = 13_534;

fn portfolio_certificate_is_current(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let group = env.market_state().1;
    let account = env.portfolio_state(portfolio);
    let cert = health_cert(&account);
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&account)
}

struct MaxShapePaidHybridFixture {
    env: V16CuEnv,
    taker_owner: Keypair,
    lp_owner: Keypair,
    taker: Pubkey,
    lp: Pubkey,
    oracle_accounts: Vec<Pubkey>,
    vault_before: u64,
    movement_fees: u128,
    open_cu: u64,
}

fn setup_max_shape_paid_hybrid_cpi() -> MaxShapePaidHybridFixture {
    let asset_count = MAX_SHAPE_HYBRID_ASSET_COUNT;
    let mark = MAX_SHAPE_HYBRID_MARK;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: asset_count,
        initial_price: mark,
        max_trading_fee_bps: 100,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_permissionless_resolve_with_cu(100, 5);
    set_test_clock(&mut env, 1, 100);
    let mut oracle_accounts = Vec::with_capacity(usize::from(asset_count));
    for asset_index in 0..asset_count {
        let mut feed = [0u8; 32];
        feed[0] = u8::try_from(asset_index).unwrap().checked_add(1).unwrap();
        feed[31] = 0xa5;
        let oracle = env.set_pyth_price_with_conf(&feed, mark as i64, -6, 0, 100);
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            1,
            0,
            [feed, [0; 32], [0; 32]],
            &[oracle],
            1,
            100,
            0,
            0,
            1,
            0,
        )
        .unwrap_or_else(|error| {
            panic!("configure maximum-shape Hybrid asset {asset_index}: {error}")
        });
        oracle_accounts.push(oracle);
    }

    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(matcher_program_path()).expect("read matcher BPF"),
    );
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, MAX_SHAPE_HYBRID_DEPOSIT);
    env.deposit(&lp_owner, lp, MAX_SHAPE_HYBRID_DEPOSIT);
    let vault_before = env.token_amount(env.vault);
    let (matcher_context, matcher_delegate, _) = env
        .init_matcher_context_with_passive_spread_authorized(
            matcher_program,
            &lp_owner,
            lp,
            9_000,
            9_000,
        );

    set_test_clock(&mut env, 3, 102);
    let legs = (0..asset_count)
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index,
            market_id: env.asset_market_id(asset_index),
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let open_cu = env
        .send(
            env.batch_trade_cpi_ix(taker, lp, legs),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[&taker_owner],
        )
        .expect("maximum-shape stale-Hybrid paid batch CPI");

    let after_open = env.market_state().1;
    let movement_fees = MAX_SHAPE_HYBRID_FEE_PER_ASSET * u128::from(asset_count);
    let mark_move_bps =
        (u128::from(MAX_SHAPE_HYBRID_EXPECTED_MARK - mark) * 10_000 + u128::from(mark) - 1)
            / u128::from(mark);
    let fee_supported_move_bps = MAX_SHAPE_HYBRID_FEE_PER_ASSET * 10_000 / (2 * u128::from(mark));
    assert_eq!(mark_move_bps, 67, "the stale-Hybrid move is nontrivial");
    assert!(
        mark_move_bps <= fee_supported_move_bps,
        "each asset's {mark_move_bps}-bps movement exceeds its {fee_supported_move_bps}-bps collected-fee support"
    );
    assert_eq!(after_open.vault as u64, vault_before);
    assert_eq!(after_open.insurance, movement_fees);
    assert_eq!(after_open.c_tot + after_open.insurance, after_open.vault);
    assert_eq!(
        after_open.insurance_domain_budget.iter().sum::<u128>(),
        0,
        "Hybrid movement fees are terminal protocol stock, not withdrawable domain insurance"
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(taker))),
        u32::from(asset_count)
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(lp))),
        u32::from(asset_count)
    );
    for asset_index in 0..asset_count as usize {
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index,
        )
        .unwrap();
        assert_eq!(
            profile.oracle_mode,
            percolator_prog::constants::ORACLE_MODE_HYBRID_AFTER_HOURS
        );
        assert_eq!(profile.mark_ewma_e6, MAX_SHAPE_HYBRID_EXPECTED_MARK);
        assert_eq!(after_open.assets[asset_index].effective_price, mark);
        assert_eq!(after_open.assets[asset_index].oi_eff_long_q, POS_SCALE);
        assert_eq!(after_open.assets[asset_index].oi_eff_short_q, POS_SCALE);
    }

    MaxShapePaidHybridFixture {
        env,
        taker_owner,
        lp_owner,
        taker,
        lp,
        oracle_accounts,
        vault_before,
        movement_fees,
        open_cu,
    }
}

#[test]
fn v16_program_max_shape_hybrid_cpi_movement_resolves_with_exact_terminal_value() {
    let MaxShapePaidHybridFixture {
        mut env,
        taker_owner,
        lp_owner,
        taker,
        lp,
        oracle_accounts: _,
        vault_before,
        movement_fees,
        open_cu,
    } = setup_max_shape_paid_hybrid_cpi();

    let resolve_cu = env.resolve();
    let (resolved_cfg, resolved_group) = env.market_state();
    assert_eq!(resolved_group.mode, percolator::MarketModeV16::Resolved);
    let permissionless_slot = resolved_group
        .resolved_slot
        .checked_add(resolved_cfg.force_close_delay_slots)
        .expect("maximum-shape resolved close slot overflow");
    set_test_clock(
        &mut env,
        permissionless_slot,
        102 + resolved_cfg.force_close_delay_slots as i64,
    );
    let (payouts, resolved_close_cu) = drain_resolved_cohort_with_cu_limit(
        &mut env,
        &[(&taker_owner, taker), (&lp_owner, lp)],
        "INV-045 maximum-shape Hybrid resolved close",
        1_375_000,
    );
    let remaining_vault = env.token_amount(env.vault);
    assert_eq!(
        payouts.iter().sum::<u128>() + u128::from(remaining_vault),
        u128::from(vault_before),
        "resolved payouts plus terminal protocol stock conserve exact SPL custody"
    );
    let terminal = env.market_state().1;
    assert_eq!(terminal.vault as u64, remaining_vault);
    assert_eq!(terminal.vault, movement_fees);
    assert_eq!(terminal.c_tot, 0);
    assert_eq!(
        payouts.iter().sum::<u128>(),
        2 * MAX_SHAPE_HYBRID_DEPOSIT - movement_fees,
        "the two-account cohort exits with every non-fee atom"
    );

    println!(
        "INV-045 max-shape stale-Hybrid batch-CPI open={open_cu} resolve={resolve_cu} \
         resolved-close={resolved_close_cu}"
    );
    for (label, cu) in [
        ("stale-Hybrid batch CPI open", open_cu),
        ("maximum-shape resolve", resolve_cu),
        ("maximum-shape resolved close", resolved_close_cu),
    ] {
        assert!(cu < 1_400_000, "{label} consumed {cu} CU");
    }
}

#[test]
fn v16_program_max_shape_paid_hybrid_recovery_allows_atomic_owner_exit() {
    let MaxShapePaidHybridFixture {
        mut env,
        taker_owner,
        lp_owner,
        taker,
        lp,
        oracle_accounts,
        vault_before,
        movement_fees,
        open_cu,
    } = setup_max_shape_paid_hybrid_cpi();

    set_test_clock(&mut env, 4, 200);
    let mut fallback_cu = 0;
    for (asset_index, oracle) in oracle_accounts.iter().copied().enumerate() {
        for portfolio in [taker, lp] {
            fallback_cu = fallback_cu.max(env.crank_with_oracle_tail(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 4,
                    observations: crank_observations(asset_index as u16),
                },
                &[oracle],
            ));
        }
    }
    let mut shutdown_cu = 0;
    for asset_index in 0..MAX_SHAPE_HYBRID_ASSET_COUNT {
        shutdown_cu = shutdown_cu.max(env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            asset_index,
            4,
            0,
        ));
    }
    let recovery = env.market_state().1;
    assert_eq!(recovery.mode, percolator::MarketModeV16::Live);
    for asset_index in 0..MAX_SHAPE_HYBRID_ASSET_COUNT as usize {
        assert_eq!(
            recovery.assets[asset_index].lifecycle,
            AssetLifecycleV16::Recovery
        );
        assert_eq!(
            recovery.assets[asset_index].effective_price,
            MAX_SHAPE_HYBRID_EXPECTED_MARK
        );
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index,
        )
        .unwrap();
        assert_eq!(profile.mark_ewma_e6, MAX_SHAPE_HYBRID_EXPECTED_MARK);
    }

    let mut recovery_refresh_cu = 0;
    for _ in 0..(u32::from(MAX_SHAPE_HYBRID_ASSET_COUNT) * 2 + 4) {
        for portfolio in [taker, lp] {
            if !portfolio_certificate_is_current(&env, portfolio) {
                recovery_refresh_cu = recovery_refresh_cu.max(env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 4,
                        observations: vec![],
                    },
                ));
            }
        }
        if portfolio_certificate_is_current(&env, taker)
            && portfolio_certificate_is_current(&env, lp)
        {
            break;
        }
    }
    assert!(
        portfolio_certificate_is_current(&env, taker) && portfolio_certificate_is_current(&env, lp),
        "permissionless refresh must make both maximum-shape Recovery certificates current"
    );

    let close_legs = (0..MAX_SHAPE_HYBRID_ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: env.asset_market_id(asset_index),
            size_q: -(POS_SCALE as i128),
            exec_price: 1,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            env.batch_trade_no_cpi_ix(taker, lp, close_legs),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
            ],
            &[&taker_owner, &lp_owner],
        )
        .expect("maximum-shape Recovery extreme-price owner reduction");

    let after_close = env.market_state().1;
    assert_eq!(after_close.insurance, movement_fees);
    for asset_index in 0..MAX_SHAPE_HYBRID_ASSET_COUNT as usize {
        assert_eq!(after_close.assets[asset_index].oi_eff_long_q, 0);
        assert_eq!(after_close.assets[asset_index].oi_eff_short_q, 0);
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index,
        )
        .unwrap();
        assert_eq!(profile.mark_ewma_e6, MAX_SHAPE_HYBRID_EXPECTED_MARK);
    }
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(taker)
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(lp)
    )));

    let released_pnl = u128::from(
        (MAX_SHAPE_HYBRID_EXPECTED_MARK - MAX_SHAPE_HYBRID_MARK)
            * u64::from(MAX_SHAPE_HYBRID_ASSET_COUNT),
    );
    assert_eq!(env.portfolio_state(taker).pnl.get(), released_pnl as i128);
    assert_eq!(env.portfolio_state(lp).pnl.get(), 0);
    let convert_cu = env.convert_released_pnl_with_cu(&taker_owner, taker, released_pnl);
    let taker_capital = env.portfolio_state(taker).capital.get();
    let lp_capital = env.portfolio_state(lp).capital.get();
    assert_eq!(
        taker_capital + lp_capital,
        2 * MAX_SHAPE_HYBRID_DEPOSIT - movement_fees
    );

    let taker_destination = env.withdraw(&taker_owner, taker, taker_capital);
    let lp_destination = env.withdraw(&lp_owner, lp, lp_capital);
    assert_eq!(
        u128::from(env.token_amount(taker_destination))
            + u128::from(env.token_amount(lp_destination)),
        2 * MAX_SHAPE_HYBRID_DEPOSIT - movement_fees
    );
    env.close_portfolio_with_cu(&taker_owner, taker);
    env.close_portfolio_with_cu(&lp_owner, lp);
    let terminal = env.market_state().1;
    assert_eq!(terminal.vault, movement_fees);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    assert_eq!(
        terminal.vault as u64,
        vault_before - (2 * MAX_SHAPE_HYBRID_DEPOSIT) as u64 + movement_fees as u64
    );

    println!(
        "INV-045 max-shape paid-Hybrid Recovery open={open_cu} fallback={fallback_cu} \
         shutdown={shutdown_cu} refresh={recovery_refresh_cu} owner-close={close_cu} \
         convert={convert_cu}"
    );
    for (label, cu) in [
        ("stale-Hybrid batch CPI open", open_cu),
        ("maximum-shape stale-fallback accrual", fallback_cu),
        ("maximum-shape Recovery shutdown", shutdown_cu),
        ("maximum-shape Recovery refresh", recovery_refresh_cu),
        ("maximum-shape Recovery owner close", close_cu),
        ("Recovery released-PnL conversion", convert_cu),
    ] {
        assert!(cu < 1_400_000, "{label} consumed {cu} CU");
    }
}

#[derive(Clone, Copy)]
struct Inv045MarkClass {
    class: &'static str,
    witnesses: &'static [(&'static str, &'static str)],
}

fn inv045_source_defines_function(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

fn inv045_function_body<'a>(production: &'a str, function: &str) -> &'a str {
    let start = production
        .find(&format!("fn {function}"))
        .unwrap_or_else(|| panic!("missing production function {function}"));
    let tail = &production[start..];
    let end = tail[1..]
        .find("\n    fn ")
        .or_else(|| tail[1..].find("\n    #["))
        .map_or(tail.len(), |offset| offset + 1);
    &tail[..end]
}

#[test]
fn v16_program_mark_writer_and_trade_exit_composition_is_source_complete() {
    const ENGINE_PIN: &str = "394fd0bf2cb7d73df425eb3754dc3be1a0c44336";
    const CLASSES: &[Inv045MarkClass] = &[
        Inv045MarkClass {
            class: "full-width mark and fee arithmetic",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs",
                    "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
                ),
                (
                    "tests/invariants/cu/inv_085_proven_arithmetic_equals_deployed_arithmetic.rs",
                    "v16_program_dynamic_externality_fee_matches_exhaustive_search_on_generated_inputs",
                ),
                (
                    "tests/invariants/kani/inv_045_no_free_mark_movement.rs",
                    "kani_v16_fee_supported_mark_clamp_is_directional_and_zero_support_is_noop",
                ),
            ],
        },
        Inv045MarkClass {
            class: "all trade routes and mark regimes",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_045_no_free_mark_movement.rs",
                    "v16_program_accepted_mark_boundary_matrix_is_paid_atomic_and_exit_live",
                ),
                (
                    "tests/invariants/stateful/inv_045_no_free_mark_movement.rs",
                    "v16_program_trade_driven_mark_route_orders_converge_economically",
                ),
            ],
        },
        Inv045MarkClass {
            class: "fee support and coalition non-reclaimability",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_045_no_free_mark_movement.rs",
                    "v16_probe_ewma_fee_covers_large_passive_oi_moved_by_small_wash_trades",
                ),
                (
                    "tests/invariants/public_sbf/inv_045_no_free_mark_movement.rs",
                    "v16_program_pr225_mark_movement_fee_is_nonwithdrawable_and_terminally_burned",
                ),
                (
                    "tests/invariants/public_sbf/inv_045_no_free_mark_movement.rs",
                    "v16_program_pr280_trade_driven_liquidation_penalty_is_not_reclaimable",
                ),
            ],
        },
        Inv045MarkClass {
            class: "clock, same-slot, and pending-target sequencing",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_045_no_free_mark_movement.rs",
                    "v16_program_low_price_ewma_discovery_is_not_pinned_by_clock_first_cranks",
                ),
                (
                    "tests/invariants/stateful/inv_045_no_free_mark_movement.rs",
                    "v16_program_pending_trade_mark_replacement_preserves_funding_fee_and_exit",
                ),
                (
                    "tests/invariants/stateful/inv_045_no_free_mark_movement.rs",
                    "v16_program_repeated_trade_driven_mark_steps_are_paid_and_exit_live",
                ),
            ],
        },
        Inv045MarkClass {
            class: "unsafe-price lifecycle exit availability",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_046_trade_availability_without_unsafe_mark_admission.rs",
                    "v16_program_extreme_price_route_lifecycle_matrix_preserves_exit_or_terminal_fallback",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_oracle_failure_seeded_frontier_is_exact_and_terminal",
                ),
            ],
        },
        Inv045MarkClass {
            class: "activation, Recovery freeze, and restart identity",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_089_activation_reactivation_and_initialization_equivalence.rs",
                    "v16_program_reused_slot_matches_fresh_persisted_state_after_public_history",
                ),
                (
                    "tests/invariants/stateful/inv_065_reset_recovery_and_retired_state_isolation.rs",
                    "v16_program_generated_shutdown_reaches_recovery_then_all_positions_exit",
                ),
            ],
        },
        Inv045MarkClass {
            class: "maximum supported account and oracle shapes",
            witnesses: &[
                (
                    "tests/invariants/cu/inv_045_no_free_mark_movement.rs",
                    "v16_program_max_shape_ewma_movement_is_paid_and_drain_only_exit_stays_bounded",
                ),
                (
                    "tests/invariants/cu/inv_045_no_free_mark_movement.rs",
                    "v16_program_max_shape_hybrid_cpi_movement_resolves_with_exact_terminal_value",
                ),
                (
                    "tests/invariants/cu/inv_045_no_free_mark_movement.rs",
                    "v16_program_max_shape_paid_hybrid_recovery_allows_atomic_owner_exit",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-045/046 composition must be reviewed on every engine pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the mark-certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut classes = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate mark class");
        assert!(!row.witnesses.is_empty());
        for (path, witness) in row.witnesses {
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv045_source_defines_function(source, witness),
                "mark class '{}' lacks executable witness {path}#{witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 7, "mark/availability class roster drift");

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");

    // Four public trade variants must collapse into exactly two common implementations.
    for variant in [
        "Instruction::TradeNoCpi",
        "Instruction::TradeCpi",
        "Instruction::BatchTradeNoCpi",
        "Instruction::BatchTradeCpi",
    ] {
        assert!(
            production.contains(variant),
            "missing public route {variant}"
        );
    }
    assert_eq!(
        production.matches("handle_trade_nocpi_zero_copy(").count(),
        2
    );
    assert_eq!(
        production
            .matches("handle_batch_execute_zero_copy(")
            .count(),
        2
    );

    let single = inv045_function_body(production, "handle_trade_nocpi_zero_copy");
    let batch = inv045_function_body(production, "handle_batch_execute_zero_copy");
    for body in [single, batch] {
        let accepted = body.find("accepted_reported_trade_price_view").unwrap();
        let quoted = body.find("hybrid_trade_fee_quote_view").unwrap();
        let collected = body.find("collected_fee_supported_mark_view").unwrap();
        let updated = body.find("update_hybrid_mark_after_trade_view").unwrap();
        assert!(accepted < quoted && quoted < collected && collected < updated);
        assert!(body.contains("exec_price: fee_basis_price"));
    }
    assert!(single.contains("execute_trade_with_fee_loss_stale_scoped_not_atomic"));
    assert!(single.contains("stage_trade_driven_mark_target_view"));
    assert!(batch.contains("execute_batch_with_fee_loss_stale_scoped_not_atomic"));
    assert!(batch.contains("set_asset_raw_oracle_targets_not_atomic"));

    let accepted = inv045_function_body(production, "accepted_reported_trade_price_view");
    assert!(accepted.contains("ensure_valid_reported_trade_price(reported_exec_price)?"));
    assert!(accepted.contains("return Ok(effective_price)"));
    assert!(accepted.contains("clamp_toward_engine_dt"));
    assert!(
        !accepted.contains("abs_diff"),
        "off-mark distance must not become an availability gate",
    );

    let quote = inv045_function_body(production, "hybrid_trade_fee_quote_view");
    assert!(quote.contains(".min(max_trading_fee_bps)"));
    assert!(quote.contains(".min(fee_supported_move_bps)"));
    assert!(quote.contains("mark_externality_notional"));
    assert!(quote.contains("fee_bps_for_two_sided_fee_paid"));

    // This exact production census includes four authoritative profile writes, two simulation
    // writes, and four compatibility mirrors. Initialization and retirement literals are pinned.
    assert_eq!(
        production.matches(".mark_ewma_e6 = ").count(),
        10,
        "a new mark writer requires an invariant disposition",
    );
    for writer in [
        "profile.mark_ewma_e6 = frozen_mark",
        "profile.mark_ewma_e6 = price",
        "profile.mark_ewma_e6 = next_mark",
        "profile.mark_ewma_e6 = post_trade_mark_e6",
        "simulated_profile.mark_ewma_e6 = next_price",
        "simulated_profile.mark_ewma_e6 = effective_price",
    ] {
        assert!(
            production.contains(writer),
            "missing classified writer {writer}"
        );
    }
    assert_eq!(production.matches("mark_ewma_e6: initial_price").count(), 2);
    assert_eq!(production.matches("mark_ewma_e6: 0").count(), 1);
}
