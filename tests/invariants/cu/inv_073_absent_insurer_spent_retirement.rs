//! INV-073: fully consumed insurance needs neither reserve signature through retirement.
//! Exact loss settlement exhausts the budget; a one-atom surviving claim must still block close.
//! Economic continuation is permissionless; empty-portfolio deletion and slab close are signed.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_absent_insurance_roles_reach_retirement_only_after_exact_exhaustion() {
    const CAPITAL: [u64; 3] = [1_000, 100, 137];
    const GAIN: u64 = 10 * (120 - 100);
    const DEFICIT: u64 = GAIN - CAPITAL[1];
    const PAYOUTS: [u64; 3] = [CAPITAL[0] + GAIN, 0, CAPITAL[2]];
    const CALL_BOUND: usize = 8;

    for asset in [0usize, 1] {
        for remainder in [0u64, 1] {
            let insurance = DEFICIT + remainder;
            let supply = CAPITAL.iter().sum::<u64>() + insurance;
            let mut env = inv018_public_spl_market_with_params(
                0,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let beneficiary = Keypair::new();
            let operator = Keypair::new();
            for (kind, incoming) in [
                (processor::ASSET_AUTH_INSURANCE, &beneficiary),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &operator),
            ] {
                env.svm.airdrop(&incoming.pubkey(), 1_000_000_000).unwrap();
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(incoming),
                    asset as u16,
                    kind,
                    incoming.pubkey().to_bytes(),
                )
                .unwrap();
            }
            env.svm.warp_to_slot(1);
            for index in [0, 1] {
                env.configure_auth_mark_for_asset_as_admin(index, 1, 100);
            }
            env.configure_permissionless_resolve_with_cu(20, 3);
            let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
            let portfolios = owners.each_ref().map(|owner| {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
                let key = Keypair::new();
                system_create_account_for_test(
                    &mut env.svm,
                    &env.payer,
                    &key,
                    env.portfolio_account_len,
                    env.program_id,
                );
                env.send(
                    ProgInstruction::InitPortfolio,
                    vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key.pubkey(), false),
                    ],
                    &[owner],
                )
                .unwrap();
                env.portfolios.push(key.pubkey());
                key.pubkey()
            });
            let tokens = owners.each_ref().map(|owner| {
                create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint)
            });
            let reserve_token =
                create_ata_for_test(&mut env.svm, &env.payer, beneficiary.pubkey(), env.mint);
            let admin_token =
                create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
            for (token, amount) in tokens
                .into_iter()
                .zip(CAPITAL)
                .chain([(reserve_token, insurance)])
            {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &token,
                        &admin.pubkey(),
                        &[],
                        amount,
                    )
                    .unwrap(),
                    &[&admin],
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
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            for actor in 0..3 {
                env.send(
                    env.deposit_ix(portfolios[actor], CAPITAL[actor].into()),
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
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: (2 * asset) as u16,
                    market_id: env.asset_market_id(asset as u16),
                    authority_epoch: env.control_sequences(asset).authority_epoch,
                    intent_id: 0,
                    amount: insurance.into(),
                },
                vec![
                    AccountMeta::new(beneficiary.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(reserve_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&beneficiary],
            )
            .unwrap();
            let absent = [beneficiary.pubkey(), operator.pubkey()];
            drop(beneficiary);
            drop(operator);
            let absent_frame = absent.map(|key| env.svm.get_account(&key));
            let reserve_frame = env.svm.get_account(&reserve_token);

            env.trade_asset_with_cu(
                asset as u16,
                &owners[0],
                portfolios[0],
                &owners[1],
                portfolios[1],
                (10 * POS_SCALE) as i128,
                100,
                0,
            );
            for (offset, mark) in (105..=120).step_by(5).enumerate() {
                let slot = offset as u64 + 2;
                env.svm.warp_to_slot(slot);
                env.push_auth_mark_for_asset_as_admin(asset as u16, slot, mark);
                env.crank(
                    portfolios[2],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(asset as u16),
                    },
                );
            }
            for actor in [0, 1] {
                env.crank(
                    portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 5,
                        observations: crank_observations(asset as u16),
                    },
                );
            }
            assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), GAIN.into());
            assert_eq!(
                env.portfolio_state(portfolios[1]).pnl.get(),
                -i128::from(DEFICIT)
            );
            assert_eq!(env.portfolio_state(portfolios[1]).capital.get(), 0);
            assert_eq!(env.market_state().1.insurance_domain_spent, vec![0; 4]);
            let profiles = [0, 1].map(|index| {
                state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    index,
                )
                .unwrap()
            });
            let sequences = [0, 1].map(|index| env.control_sequences(index));
            assert_eq!(profiles[asset].insurance_authority, absent[0].to_bytes());
            assert_eq!(profiles[asset].insurance_operator, absent[1].to_bytes());
            let mint_frame = env.svm.get_account(&env.mint).unwrap();
            let mint = Mint::unpack(&mint_frame.data).unwrap();
            assert_eq!(mint.supply, supply);
            assert_eq!(mint.mint_authority, COption::None);
            let mut tracked = vec![
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                reserve_token,
                admin_token,
                admin.pubkey(),
            ];
            tracked.extend(absent);
            tracked.extend(portfolios);
            tracked.extend(tokens);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let wrap = |data: ProgInstruction, accounts| Instruction {
                program_id: percolator_prog::id(),
                accounts,
                data: data.encode(),
            };
            let land = |env: &mut V16CuEnv,
                        instructions: &[Instruction],
                        signers: &[&Keypair],
                        changed: &[Pubkey],
                        rejection: Option<(u8, PercolatorError)>| {
                env.svm.expire_blockhash();
                let mut batch = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(CUSTODY_CU_LIMIT as u32),
                ];
                batch.extend_from_slice(instructions);
                let mut signing = vec![&env.payer];
                signing.extend_from_slice(signers);
                let tx = Transaction::new_signed_with_payer(
                    &batch,
                    Some(&env.payer.pubkey()),
                    &signing,
                    env.svm.latest_blockhash(),
                );
                let required = usize::from(tx.message.header.num_required_signatures);
                assert_eq!(required, 1 + signers.len());
                assert!(absent
                    .iter()
                    .all(|key| !tx.message.account_keys[..required].contains(key)));
                tx.verify().unwrap();
                assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                let mut keys = tx.message.account_keys.clone();
                keys.extend(&tracked);
                keys.sort_unstable();
                keys.dedup();
                let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
                let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                payer.lamports -= required as u64 * FeeStructure::default().lamports_per_signature;
                let result = env.svm.send_transaction(tx);
                let rejected = rejection.is_some();
                let meta = if let Some((index, error)) = rejection {
                    let failure =
                        result.expect_err("a real claim or owner window remains protected");
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            index,
                            InstructionError::Custom(error as u32)
                        )
                    );
                    failure.meta
                } else {
                    result.expect("bounded continuation with absent insurance roles")
                };
                for (key, account) in keys.into_iter().zip(before) {
                    if key != env.payer.pubkey() && (rejected || !changed.contains(&key)) {
                        assert_eq!(
                            env.svm.get_account(&key),
                            account,
                            "Account frame for {key}"
                        );
                    }
                }
                assert_eq!(env.svm.get_account(&env.payer.pubkey()), Some(payer));
                assert_eq!(absent.map(|key| env.svm.get_account(&key)), absent_frame);
                assert_eq!(env.svm.get_account(&reserve_token), reserve_frame);
                assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
                assert_cu_within(
                    "INV-073 absent insurance roles",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                meta.compute_units_consumed
            };
            let custody = |env: &V16CuEnv| {
                let group = env.market_state().1;
                let paid = tokens.map(|key| env.token_amount(key));
                assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
                assert_eq!(
                    env.token_amount(env.vault) + paid.iter().sum::<u64>(),
                    supply
                );
                assert_eq!(env.token_amount(admin_token), 0);
                assert_eq!(env.token_amount(reserve_token), 0);
                assert!(paid
                    .into_iter()
                    .zip(PAYOUTS)
                    .all(|(paid, bound)| paid <= bound));
                for index in [0, 1] {
                    assert_eq!(
                        state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            index
                        )
                        .unwrap(),
                        profiles[index]
                    );
                    assert_eq!(env.control_sequences(index), sequences[index]);
                }
            };
            let payout = |actor: usize| {
                wrap(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(owners[actor].pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                )
            };
            let payouts = [0, 1, 2].map(payout);
            let close = wrap(
                ProgInstruction::CloseSlab {
                    authority_epoch: sequences[0].authority_epoch,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new(admin_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.mint, false),
                ],
            );
            let market = env.market;
            env.svm.warp_to_slot(40);
            let mut peak = land(
                &mut env,
                &[wrap(
                    ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
                    vec![AccountMeta::new(market, false)],
                )],
                &[],
                &[market],
                None,
            );
            assert_eq!(env.market_state().1.resolved_slot, 40);
            env.svm.warp_to_slot(42);
            peak = peak.max(land(
                &mut env,
                &[payouts[0].clone()],
                &[],
                &[],
                Some((2, PercolatorError::ExpectedSigner)),
            ));
            env.svm.warp_to_slot(43);
            let rank = |env: &V16CuEnv, actor: usize| {
                let account = env.portfolio_state(portfolios[actor]);
                (
                    account.pnl.get().min(0).unsigned_abs(),
                    account.legs.iter().filter(|leg| leg.active != 0).count(),
                    account
                        .source_domains
                        .iter()
                        .filter(|source| source.is_occupied())
                        .count(),
                    account.capital.get(),
                    account.pnl.get().max(0) as u128,
                    !resolved_portfolio_is_terminal(env, portfolios[actor]),
                )
            };
            let mut calls = [0; 3];
            // The insolvent debtor consumes insurance before the exact source/receipt payout.
            for actor in [1, 0, 2] {
                while !resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                    assert!(calls[actor] < CALL_BOUND);
                    let before = rank(&env, actor);
                    let changed = [env.market, env.vault, portfolios[actor], tokens[actor]];
                    peak = peak.max(land(
                        &mut env,
                        &[payouts[actor].clone()],
                        &[],
                        &changed,
                        None,
                    ));
                    calls[actor] += 1;
                    assert!(
                        rank(&env, actor) < before,
                        "each economic call strictly progresses"
                    );
                    custody(&env);
                }
                assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
                assert_eq!(
                    env.market_state().1.insurance_domain_spent[2 * asset],
                    DEFICIT.into()
                );
                assert_eq!(env.market_state().1.insurance, remainder.into());
            }
            let paid = env.market_state().1;
            assert_eq!(
                (paid.vault, paid.c_tot, paid.pnl_pos_tot),
                (remainder.into(), 0, 0)
            );
            assert_eq!(paid.source_claim_bound_total_num, 0);
            assert_eq!(paid.backing_provider_earnings_total, 0);
            assert_eq!(
                paid.insurance_domain_budget_remaining_total,
                remainder.into()
            );
            assert_eq!(paid.insurance_domain_budget[2 * asset], insurance.into());
            assert!(paid
                .source_backing_buckets
                .iter()
                .all(|bucket| bucket.fresh_unliened_backing_num == 0
                    && bucket.valid_liened_backing_num == 0));
            assert!(paid
                .assets
                .iter()
                .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
            let ledger = paid.resolved_payout_ledger;
            assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                u128::from(DEFICIT) * BOUND_SCALE
            );
            for actor in 0..3 {
                let expected = if actor == 0 {
                    ResolvedPayoutReceiptV16 {
                        present: true,
                        prior_bound_contribution_num: u128::from(DEFICIT) * BOUND_SCALE,
                        live_released_face_at_receipt: 0,
                        terminal_positive_claim_face: DEFICIT.into(),
                        paid_effective: DEFICIT.into(),
                        finalized: true,
                    }
                } else {
                    ResolvedPayoutReceiptV16::EMPTY
                };
                assert_eq!(
                    resolved_receipt(&env.portfolio_state(portfolios[actor])),
                    expected
                );
                peak = peak.max(land(
                    &mut env,
                    &[payouts[actor].clone()],
                    &[],
                    &[],
                    Some((2, PercolatorError::EngineNonProgress)),
                ));
            }
            assert_eq!(paid.materialized_portfolio_count, 3);
            peak = peak.max(land(
                &mut env,
                &[close.clone()],
                &[&admin],
                &[],
                Some((2, PercolatorError::EngineLockActive)),
            ));
            for actor in 0..3 {
                let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
                let portfolio_rent = env.svm.get_account(&portfolios[actor]).unwrap().lamports;
                let delete = wrap(
                    env.close_portfolio_ix(portfolios[actor]),
                    vec![
                        AccountMeta::new(owners[actor].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                    ],
                );
                let changed = [env.market, portfolios[actor]];
                peak = peak.max(land(&mut env, &[delete], &[&owners[actor]], &changed, None));
                assert_eq!(
                    env.market_state().1.materialized_portfolio_count,
                    (2 - actor) as u64
                );
                assert_eq!(
                    env.svm.get_account(&env.market).unwrap().lamports,
                    market_rent + portfolio_rent
                );
                custody(&env);
            }
            let terminal = env.market_state().1;
            assert_eq!(terminal.insurance_domain_spent[2 * asset], DEFICIT.into());
            assert_eq!(
                terminal.insurance_domain_budget_remaining_total,
                remainder.into()
            );
            if remainder != 0 {
                peak = peak.max(land(
                    &mut env,
                    &[close],
                    &[&admin],
                    &[],
                    Some((2, PercolatorError::EngineLockActive)),
                ));
                custody(&env);
                assert_eq!(env.market_state().1, terminal);
            } else {
                let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
                let vault_rent = env.svm.get_account(&env.vault).unwrap().lamports;
                let rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
                let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
                expected_admin.lamports += market_rent + vault_rent - rent;
                let changed = [env.market, env.vault, admin.pubkey()];
                peak = peak.max(land(&mut env, &[close], &[&admin], &changed, None));
                let tombstone = env.svm.get_account(&env.market).unwrap();
                assert_closed_market_tombstone(&tombstone);
                assert_eq!(tombstone.lamports, rent);
                assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
                assert!(env
                    .svm
                    .get_account(&env.vault)
                    .is_none_or(|account| account.lamports == 0
                        && account.data.iter().all(|byte| *byte == 0)));
            }
            assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
            assert_eq!(env.token_amount(admin_token), 0);
            assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame));
            println!("INV-073 absent insurer asset={asset}, remainder={remainder}: calls={calls:?}/{CALL_BOUND}, paid={PAYOUTS:?}, peak={peak} CU, closed={}", remainder == 0);
        }
    }
}
