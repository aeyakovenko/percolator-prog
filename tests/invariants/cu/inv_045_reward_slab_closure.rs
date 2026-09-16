//! INV-045 / row422: Hybrid fee provenance survives owner deletion and CloseSlab.
//! One paid discovery and fresh-report liquidation, crossed with reward omission
//! and keeper deletion order. Public economic construction; Clock, Pyth reports,
//! signer SOL and program loading are the only harness inputs.

use super::*;

const SURPLUS: u64 = 19;

#[derive(Default)]
struct Evidence {
    peak: u64,
    rollback_peak: u64,
    slab_peak: u64,
    rollbacks: usize,
    redemptions: usize,
}

impl Evidence {
    fn send(
        &mut self,
        env: &mut V16CuEnv,
        signer: &Keypair,
        instructions: &[Instruction],
        tracked: &[Pubkey],
        rejection: Option<(u8, InstructionError)>,
    ) {
        let cu = submit_with_cu_limit(
            env,
            signer,
            instructions,
            tracked,
            rejection.clone(),
            ACTION_CU,
        );
        self.peak = self.peak.max(cu);
        if rejection.is_some() {
            self.rollback_peak = self.rollback_peak.max(cu);
            self.rollbacks += 1;
        }
    }

    fn reject(
        &mut self,
        env: &mut V16CuEnv,
        signer: &Keypair,
        instructions: &[Instruction],
        tracked: &[Pubkey],
        error: PercolatorError,
    ) {
        self.send(
            env,
            signer,
            instructions,
            tracked,
            Some((
                instructions.len() as u8 + 1,
                InstructionError::Custom(error as u32),
            )),
        );
    }
}

fn close_receipt(env: &V16CuEnv, owner: Pubkey, portfolio: Pubkey, token: Pubkey) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    }
}

fn slab(env: &V16CuEnv, destination: Pubkey) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(env.mint, false),
        ],
        data: ProgInstruction::CloseSlab {
            authority_epoch: env.control_sequences(0).authority_epoch,
        }
        .encode(),
    }
}

fn insurance(
    env: &V16CuEnv,
    beneficiary: Pubkey,
    destination: Pubkey,
    amount: u128,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(beneficiary, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            amount,
        }
        .encode(),
    }
}

#[test]
fn v16_program_hybrid_reward_provenance_survives_portfolio_deletion_and_slab_closure() {
    let supply = FUNDS.iter().map(|amount| u128::from(*amount)).sum::<u128>();
    let price = ENTRY - ENTRY * 24 / 10_000;
    let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
    let bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
    let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, bps);
    assert_eq!(discovery, 1_540_072);
    let mut evidence = Evidence::default();
    let mut reference = None;
    let mut liquidations = 0;
    for reward_present in [false, true] {
        for keeper_first in [false, true] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_abs_funding_e9_per_slot: 0,
                    ..production_risk_params()
                },
            );
            set_test_clock(&mut env, 1, 100);
            env.update_liquidation_fee_policy_with_cu(SHARE as u16);
            let admin = env.admin.insecure_clone();
            let beneficiary = Keypair::new();
            env.try_update_per_asset_authority_with_cu(
                &admin,
                Some(&beneficiary),
                0,
                processor::ASSET_AUTH_INSURANCE,
                beneficiary.pubkey().to_bytes(),
            )
            .unwrap();
            let beneficiary_key = beneficiary.pubkey();
            let beneficiary_token =
                create_ata_for_test(&mut env.svm, &env.payer, beneficiary_key, env.mint);
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            drop(beneficiary);
            let feed = [0x5b; 32];
            let initial = env.set_pyth_price_with_conf(&feed, ENTRY as i64, -6, 0, 100);
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
                1,
                0,
            )
            .unwrap();
            let owners: [Keypair; 5] = std::array::from_fn(|_| Keypair::new());
            let funded = std::array::from_fn::<_, 5, _>(|i| fund(&mut env, &owners[i], FUNDS[i]));
            let portfolios = funded.map(|pair| pair.0);
            let tokens = funded.map(|pair| pair.1);
            let [target, peer, trader_a, trader_b, keeper] = portfolios;
            evidence.peak = evidence.peak.max(env.trade_asset_with_cu(
                0,
                &owners[0],
                target,
                &owners[1],
                peer,
                (100 * POS_SCALE) as i128,
                ENTRY,
                0,
            ));
            let mut tracked = vec![
                env.market,
                env.vault,
                env.mint,
                admin.pubkey(),
                admin_token,
                beneficiary_key,
                beneficiary_token,
                initial,
            ];
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let initial_frame = env.svm.get_account(&initial);
            set_test_clock(&mut env, 5, 1_000);
            let advance = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
            evidence.send(&mut env, &owners[4], &[advance], &tracked, None);
            evidence.peak = evidence.peak.max(env.trade_asset_with_cu(
                0,
                &owners[2],
                trader_a,
                &owners[3],
                trader_b,
                POS_SCALE as i128,
                900_000,
                0,
            ));
            treasury(&env, discovery, 0, 0, [0; 2]);
            assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
            assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
            set_test_clock(&mut env, 6, 1_001);
            let report = env.set_pyth_price_with_conf(&feed, MARK as i64, -6, 0, 1_001);
            tracked.push(report);
            let report_frame = env.svm.get_account(&report);
            let retained_crank =
                observe(&env, target, owners[4].pubkey(), Some(report), Some(keeper));
            let mut actual = retained_crank.clone();
            if !reward_present {
                actual.accounts.pop();
            }
            let abort = Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![255],
            };
            let mut episode = None;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_values = values(&env, portfolios);
                evidence.send(
                    &mut env,
                    &owners[4],
                    &[actual.clone(), abort.clone()],
                    &tracked,
                    Some((3, InstructionError::InvalidInstructionData)),
                );
                evidence.send(&mut env, &owners[4], &[actual.clone()], &tracked, None);
                let after = env.market_state().1;
                assert_eq!(after.assets[0].effective_price, price);
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed != 0 {
                    let penalty = fee(closed, price, 5);
                    let eligible = penalty * SHARE / 10_000;
                    let reward = if reward_present { eligible } else { 0 };
                    assert!(closed < 100 * POS_SCALE && eligible > 0);
                    for wrong in [ENTRY, MARK, ACCEPTED_PRINT, 900_000] {
                        assert_ne!(penalty, fee(closed, wrong, 5));
                    }
                    let mut expected = before_values;
                    expected[0] -= penalty as i128;
                    expected[4] += reward as i128;
                    assert_eq!(values(&env, portfolios), expected);
                    episode = Some((closed, penalty, eligible, reward));
                    liquidations += 1;
                    break;
                }
                treasury(&env, discovery, 0, 0, [0; 2]);
            }
            let (closed, penalty, eligible, reward) =
                episode.expect("one real fresh-report liquidation");
            let retained = penalty - reward;
            let budgets = [retained / 2, retained.div_ceil(2)];
            treasury(&env, discovery, penalty, reward, budgets);
            for _ in 0..8 {
                for i in [1, 2, 3, 0] {
                    if !census(&env, portfolios)[i] {
                        let refresh =
                            observe(&env, portfolios[i], owners[4].pubkey(), Some(report), None);
                        evidence.send(&mut env, &owners[4], &[refresh], &tracked, None);
                        treasury(&env, discovery, penalty, reward, budgets);
                    }
                }
                if census(&env, portfolios)[..4].iter().all(|current| *current) {
                    break;
                }
            }
            assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
            evidence.reject(
                &mut env,
                &owners[4],
                &[retained_crank.clone()],
                &tracked,
                PercolatorError::EngineNonProgress,
            );
            // Resolution freezes this price. The initial 100-lot and one-lot
            // pairs realize its exact integer loss; liquidation adds only its fee.
            let loss = u128::from(ENTRY - price);
            let expected_payouts = [
                u128::from(FUNDS[0]) - 100 * loss - penalty,
                u128::from(FUNDS[1]) + 100 * loss,
                u128::from(FUNDS[2]) - discovery / 2 - loss,
                u128::from(FUNDS[3]) - discovery / 2 + loss,
                u128::from(FUNDS[4]) + reward,
            ];
            assert_eq!(
                values(&env, portfolios),
                expected_payouts.map(|value| value as i128)
            );
            let rounding = supply
                .checked_sub(expected_payouts.iter().sum::<u128>() + discovery + retained)
                .unwrap();
            assert_eq!(
                rounding, 0,
                "this integer-price history has no settlement dust"
            );
            let resolve = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                data: ProgInstruction::ResolveMarket {
                    asset_generation_frontier: env.market_state().1.next_market_id,
                    authority_epoch: env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            evidence.send(&mut env, &admin, &[resolve], &tracked, None);
            let frozen_profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            let receipts = std::array::from_fn::<_, 5, _>(|i| {
                close_receipt(&env, owners[i].pubkey(), portfolios[i], tokens[i])
            });
            let mut topup = receipts[4].clone();
            topup.data = ProgInstruction::ClaimResolvedPayoutTopup.encode();
            let mut deleted = [false; 5];
            let order = if keeper_first {
                [4, 0, 1, 2, 3]
            } else {
                [0, 1, 2, 3, 4]
            };
            let mut keeper_checked = false;
            for _ in 0..16 {
                for i in order {
                    if deleted[i] {
                        continue;
                    }
                    // Keep the flat keeper funded until all exposed peers are deleted.
                    if !keeper_first && i == 4 && !deleted[..4].iter().all(|done| *done) {
                        continue;
                    }
                    let before = env.portfolio_state(portfolios[i]);
                    if percolator::active_bitmap_is_empty(active_bitmap(&before))
                        && before.pnl.get() > 0
                        && env.market_state().1.resolved_payout_blocker_count > 0
                    {
                        evidence.reject(
                            &mut env,
                            &owners[i],
                            &[receipts[i].clone()],
                            &tracked,
                            PercolatorError::EngineNonProgress,
                        );
                        continue;
                    }
                    if i == 4 {
                        let premature = slab(&env, admin_token);
                        let reserve = insurance(&env, beneficiary_key, beneficiary_token, retained);
                        evidence.reject(
                            &mut env,
                            &admin,
                            &[premature.clone()],
                            &tracked,
                            PercolatorError::EngineLockActive,
                        );
                        evidence.reject(
                            &mut env,
                            &admin,
                            &[reserve.clone()],
                            &tracked,
                            PercolatorError::EngineLockActive,
                        );
                        evidence.reject(
                            &mut env,
                            &admin,
                            &[receipts[4].clone(), premature],
                            &tracked,
                            PercolatorError::EngineLockActive,
                        );
                        evidence.reject(
                            &mut env,
                            &admin,
                            &[receipts[4].clone(), reserve],
                            &tracked,
                            PercolatorError::EngineLockActive,
                        );
                        keeper_checked = true;
                    }
                    evidence.send(
                        &mut env,
                        &owners[i],
                        &[receipts[i].clone(), abort.clone()],
                        &tracked,
                        Some((3, InstructionError::InvalidInstructionData)),
                    );
                    evidence.send(&mut env, &owners[i], &[receipts[i].clone()], &tracked, None);
                    evidence.redemptions += 1;
                    treasury(&env, discovery, penalty, reward, budgets);
                    custody(&env, tokens, supply);
                    assert_eq!(env.market_state().1.assets[0].effective_price, price);
                    assert_eq!(env.market_state().1.resolved_slot, 6);
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap(),
                        frozen_profile
                    );
                    if !resolved_portfolio_is_terminal(&env, portfolios[i]) {
                        continue;
                    }
                    assert_eq!(u128::from(env.token_amount(tokens[i])), expected_payouts[i]);
                    let delete = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                        ],
                        data: env.close_portfolio_ix(portfolios[i]).encode(),
                    };
                    evidence.reject(
                        &mut env,
                        &owners[i],
                        &[delete.clone(), receipts[i].clone()],
                        &tracked,
                        PercolatorError::NotInitialized,
                    );
                    let wallet = env.svm.get_account(&owners[i].pubkey()).unwrap().lamports;
                    let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
                    let mut portfolio_frame = env.svm.get_account(&portfolios[i]).unwrap();
                    let portfolio_rent = portfolio_frame.lamports;
                    let mut group = env.market_state().1;
                    evidence.send(&mut env, &owners[i], &[delete], &tracked, None);
                    assert_eq!(
                        env.svm.get_account(&owners[i].pubkey()).unwrap().lamports,
                        wallet
                    );
                    assert_eq!(
                        env.svm.get_account(&env.market).unwrap().lamports,
                        market_rent + portfolio_rent
                    );
                    portfolio_frame.lamports = 0;
                    portfolio_frame.data.clear();
                    assert_eq!(env.svm.get_account(&portfolios[i]), Some(portfolio_frame));
                    group.materialized_portfolio_count -= 1;
                    assert_eq!(env.market_state().1, group);
                    deleted[i] = true;
                }
                if deleted.iter().all(|done| *done) {
                    break;
                }
            }
            assert!(keeper_checked && deleted.iter().all(|done| *done));
            assert_eq!(custody(&env, tokens, supply), expected_payouts);
            let terminal = env.market_state().1;
            assert_eq!(
                (
                    terminal.c_tot,
                    terminal.pnl_pos_tot,
                    terminal.materialized_portfolio_count
                ),
                (0, 0, 0)
            );
            assert_eq!(terminal.source_claim_bound_total_num, 0);
            assert_eq!(
                (
                    terminal.assets[0].oi_eff_long_q,
                    terminal.assets[0].oi_eff_short_q
                ),
                (0, 0)
            );
            assert_eq!(terminal.vault, discovery + retained + rounding);
            let premature = slab(&env, admin_token);
            evidence.reject(
                &mut env,
                &admin,
                &[premature],
                &tracked,
                PercolatorError::EngineLockActive,
            );
            let overclaim = insurance(&env, beneficiary_key, beneficiary_token, retained + 1);
            evidence.reject(
                &mut env,
                &admin,
                &[overclaim],
                &tracked,
                PercolatorError::EngineLockActive,
            );
            let reserve = insurance(&env, beneficiary_key, beneficiary_token, retained);
            for replay in [&receipts[4], &topup] {
                evidence.reject(
                    &mut env,
                    &admin,
                    &[reserve.clone(), replay.clone()],
                    &tracked,
                    PercolatorError::NotInitialized,
                );
            }
            evidence.send(&mut env, &admin, &[reserve], &tracked, None);
            assert_eq!(u128::from(env.token_amount(beneficiary_token)), retained);
            let drained = env.market_state().1;
            assert_eq!(drained.insurance, discovery);
            assert_eq!(drained.insurance_domain_budget_remaining_total, 0);
            assert!(drained
                .insurance_domain_budget
                .iter()
                .all(|amount| *amount == 0));
            assert_eq!(drained.vault, discovery + rounding);
            let excess = insurance(&env, beneficiary_key, beneficiary_token, 1);
            evidence.reject(
                &mut env,
                &admin,
                &[excess],
                &tracked,
                PercolatorError::EngineLockActive,
            );

            // External surplus is minted/transferred publicly, outside the engine book.
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &admin_token,
                    &admin.pubkey(),
                    &[],
                    SURPLUS,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &admin_token,
                    &env.vault,
                    &admin.pubkey(),
                    &[],
                    SURPLUS,
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            let burned = discovery + rounding;
            assert_eq!(
                u128::from(env.token_amount(env.vault)),
                burned + u128::from(SURPLUS)
            );
            assert_eq!(env.market_state().1.vault, burned);
            let close = slab(&env, admin_token);
            let mut wrong_destination = close.clone();
            wrong_destination.accounts[4] = AccountMeta::new(tokens[4], false);
            evidence.reject(
                &mut env,
                &admin,
                &[wrong_destination],
                &tracked,
                PercolatorError::InvalidTokenAccount,
            );
            // The successful prefix burns the discovery stock, sweeps only surplus,
            // closes SPL custody and writes the typed tombstone before replay fails.
            for (replay, error) in [
                (&receipts[4], PercolatorError::InvalidAccountLen),
                (&topup, PercolatorError::InvalidAccountKind),
            ] {
                evidence.reject(
                    &mut env,
                    &admin,
                    &[close.clone(), replay.clone()],
                    &tracked,
                    error,
                );
            }
            let mint_before = env.svm.get_account(&env.mint).unwrap();
            let portfolio_frames = portfolios.map(|key| env.svm.get_account(&key));
            let token_frames = tokens.map(|key| env.svm.get_account(&key));
            let beneficiary_frame = env.svm.get_account(&beneficiary_token);
            let wallet_frames = owners
                .each_ref()
                .map(|owner| env.svm.get_account(&owner.pubkey()));
            let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
            let vault_rent = env.svm.get_account(&env.vault).unwrap().lamports;
            let admin_sol = env.svm.get_account(&admin.pubkey()).unwrap().lamports;
            let cu = submit_with_cu_limit(
                &mut env,
                &admin,
                &[close.clone()],
                &tracked,
                None,
                ACTION_CU,
            );
            evidence.slab_peak = evidence.slab_peak.max(cu);
            evidence.peak = evidence.peak.max(cu);
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            let rent = env
                .svm
                .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
            assert_eq!(tombstone.lamports, rent);
            assert_eq!(
                env.svm.get_account(&admin.pubkey()).unwrap().lamports,
                admin_sol + market_rent + vault_rent - rent
            );
            assert!(env
                .svm
                .get_account(&env.vault)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            assert_eq!(env.token_amount(admin_token), SURPLUS);
            let mut expected_mint = mint_before;
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= u64::try_from(burned).unwrap();
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert_eq!(
                portfolios.map(|key| env.svm.get_account(&key)),
                portfolio_frames
            );
            assert_eq!(tokens.map(|key| env.svm.get_account(&key)), token_frames);
            assert_eq!(
                owners
                    .each_ref()
                    .map(|owner| env.svm.get_account(&owner.pubkey())),
                wallet_frames
            );
            assert_eq!(env.svm.get_account(&beneficiary_token), beneficiary_frame);
            assert_eq!(
                expected_payouts.iter().sum::<u128>() + retained + burned,
                supply
            );
            for slot in [6, 100] {
                set_test_clock(&mut env, slot, 995 + slot as i64);
                for (replay, error) in [
                    (&receipts[4], PercolatorError::InvalidAccountLen),
                    (&topup, PercolatorError::InvalidAccountKind),
                    (&close, PercolatorError::InvalidAccountLen),
                ] {
                    evidence.reject(&mut env, &admin, &[replay.clone()], &tracked, error);
                }
            }
            assert_eq!(env.svm.get_account(&initial), initial_frame);
            assert_eq!(env.svm.get_account(&report), report_frame);
            let mut normalized = expected_payouts;
            normalized[4] -= reward;
            let outcome = (
                closed,
                penalty,
                eligible,
                normalized,
                retained + reward,
                burned,
            );
            assert_eq!(*reference.get_or_insert(outcome), outcome);
            println!("row422 slab: rewarded={reward_present}, keeper_first={keeper_first}, closed={closed}, penalty={penalty}, reward={reward}, paid={expected_payouts:?}, beneficiary={retained}, burn={burned}, surplus={SURPLUS}, slab_cu={cu}");
        }
    }
    assert_eq!(
        (liquidations, evidence.redemptions, evidence.rollbacks),
        (4, 28, 136)
    );
    println!("row422 reward slab: 4 histories, liquidations={liquidations}, redemptions={}, exact_rollbacks={}, peak_cu={}, rollback_peak_cu={}, slab_peak_cu={}", evidence.redemptions, evidence.rollbacks, evidence.peak, evidence.rollback_peak, evidence.slab_peak);
}
