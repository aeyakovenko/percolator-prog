//! INV-019: a retained two-CPI bundle binds each response to its own economic request.
//! Distinct portfolio pairs prevent the first fill's position epoch from masking return checks.
//! INV-012 expiry and renewal are composed with prefix rollback, without stale-sequence probes.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_retained_cpi_bundle_binds_fresh_returns_across_expiry_and_rollback() {
    const CAPITAL: u128 = 1_000_000;
    const EXPIRY: u64 = 2;
    const BUNDLE_CU_LIMIT: u64 = 1_400_000;
    let mut peak_cu = 0;

    for batch in [false, true] {
        for expired in [false, true] {
            let label = format!("batch={batch} expired={expired}");
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
            env.svm.warp_to_slot(1);
            let prices = [100, 200];
            for (asset, price) in prices.into_iter().enumerate() {
                env.configure_auth_mark_for_asset_as_admin(asset as u16, 1, price);
            }

            let owners = [
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
                Keypair::new(),
            ];
            let mut portfolios = [Pubkey::default(); 4];
            let mut tokens = [Pubkey::default(); 4];
            for actor in 0..4 {
                env.svm
                    .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                    .unwrap();
                let portfolio = Keypair::new();
                inv019_init_portfolio_at(&mut env, &owners[actor], &portfolio);
                portfolios[actor] = portfolio.pubkey();
                env.portfolios.push(portfolio.pubkey());
                tokens[actor] =
                    create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &tokens[actor],
                        &env.admin.pubkey(),
                        &[],
                        CAPITAL as u64,
                    )
                    .unwrap(),
                    &[&env.admin],
                )
                .expect("public SPL collateral mint");
                env.send(
                    env.deposit_ix(portfolios[actor], CAPITAL),
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
                .expect("public deposit");
            }

            let matcher = Pubkey::new_unique();
            env.svm.add_program(
                matcher,
                &std::fs::read(hostile_matcher_program_path()).unwrap(),
            );
            let context = Keypair::new();
            inv019_system_create_account(&mut env, &context, matcher, MATCHER_CONTEXT_LEN);
            let context = context.pubkey();
            inv019_hostile_context_control(&mut env, matcher, &owners[1], context, vec![10], None);
            let delegates = [1, 3].map(|lp| {
                matcher_delegate_key(
                    &env.program_id,
                    &env.market,
                    &portfolios[lp],
                    &owners[lp].pubkey(),
                    &matcher,
                    &context,
                )
            });
            assert_ne!(delegates[0], delegates[1]);
            for (pair, lp) in [1, 3].into_iter().enumerate() {
                env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
                    matcher,
                    &owners[lp],
                    portfolios[lp],
                    context,
                    delegates[pair],
                    1,
                    0,
                    if pair == 0 { 100 } else { EXPIRY },
                )
                .expect("owner grants the shared context a portfolio-specific capability");
            }
            let sequences = [1, 3].map(|lp| env.portfolio_matcher_sequence(portfolios[lp]));
            let sizes = [2 * POS_SCALE as i128, -3 * POS_SCALE as i128];
            let instructions: Vec<_> = (0..2)
                .map(|pair| {
                    let taker = portfolios[2 * pair];
                    let lp = portfolios[2 * pair + 1];
                    if batch {
                        // Equal response lengths ensure an earlier response is no less well formed.
                        env.batch_trade_cpi_ix_with_caps(
                            taker,
                            lp,
                            vec![BatchTradeCpiLeg {
                                asset_index: pair as u16,
                                market_id: env.asset_market_id(pair as u16),
                                size_q: sizes[pair],
                                fee_bps: 0,
                                limit_price: prices[pair],
                            }],
                            0,
                            0,
                        )
                    } else {
                        env.trade_cpi_ix(taker, lp, pair as u16, sizes[pair], 0, prices[pair])
                    }
                })
                .collect();
            let sign = |env: &V16CuEnv, requests: &[ProgInstruction]| {
                let mut ixs = vec![heap_ix(), cu_ix()];
                for (pair, request) in requests.iter().enumerate() {
                    ixs.push(Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[2 * pair].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[2 * pair], false),
                            AccountMeta::new(portfolios[2 * pair + 1], false),
                            AccountMeta::new_readonly(matcher, false),
                            AccountMeta::new(context, false),
                            AccountMeta::new_readonly(delegates[pair], false),
                        ],
                        data: request.encode(),
                    });
                }
                let tx = Transaction::new_signed_with_payer(
                    &ixs,
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &owners[0], &owners[2]],
                    env.svm.latest_blockhash(),
                );
                tx.verify().unwrap();
                assert!(
                    bincode::serialized_size(&tx).unwrap()
                        <= solana_sdk::packet::PACKET_DATA_SIZE as u64
                );
                tx
            };
            // Complete data/owner/lamport frames include both pairs and all SPL custody accounts.
            // Only the independent network fee payer is excluded.
            let mut keys = vec![env.market, env.vault, env.mint, context, env.admin.pubkey()];
            keys.extend(portfolios);
            keys.extend(tokens);
            keys.extend(owners.iter().map(Signer::pubkey));
            keys.extend(delegates);
            let snapshot = |env: &V16CuEnv| {
                keys.iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let assert_calls = |meta: &litesvm::types::TransactionMetadata, calls: usize| {
                for expected in [
                    format!("Program {matcher} invoke [2]"),
                    format!("Program {matcher} success"),
                ] {
                    assert_eq!(
                        meta.logs.iter().filter(|line| **line == expected).count(),
                        calls,
                        "{label}: {expected}; logs={:?}",
                        meta.logs
                    );
                }
                assert_cu_within(&label, meta.compute_units_consumed, BUNDLE_CU_LIMIT);
            };
            let retained = sign(&env, &instructions);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            let before = snapshot(&env);
            let live = env
                .svm
                .simulate_transaction(retained.clone().into())
                .expect("both exact retained requests are live with fresh responses at E-1");
            assert_calls(&live, 2);
            assert_eq!(snapshot(&env), before);

            // The first invocation emits an honest response; the second returns success silently.
            inv019_hostile_context_control(
                &mut env,
                matcher,
                &owners[1],
                context,
                vec![11, 13, 0],
                None,
            );
            if expired {
                env.svm.warp_to_slot(EXPIRY);
            }
            let before = snapshot(&env);
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            let failed = env
                .svm
                .send_transaction(retained)
                .expect_err("an earlier CPI response cannot authorize the second retained request");
            let expected_error = if expired {
                InstructionError::Custom(PercolatorError::Unauthorized as u32)
            } else if batch {
                InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
            } else {
                InstructionError::InvalidAccountData
            };
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(3, expected_error),
                "{label}"
            );
            assert_calls(&failed.meta, if expired { 1 } else { 2 });
            assert_eq!(
                failed
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", env.program_id))
                    .count(),
                1,
                "{label}: the first wrapper trade succeeded before the second rejected"
            );
            assert_eq!(snapshot(&env), before, "{label}: whole bundle rolls back");
            assert_eq!(env.market_state().0.matcher_req_seq, 0);
            assert_eq!(
                [1, 3].map(|lp| env.portfolio_matcher_sequence(portfolios[lp])),
                sequences
            );
            assert_eq!(env.portfolio_matcher_expiry(portfolios[3]), EXPIRY);
            peak_cu = peak_cu.max(failed.meta.compute_units_consumed);

            let mut fresh = instructions.clone();
            if expired {
                env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
                    matcher,
                    &owners[3],
                    portfolios[3],
                    context,
                    delegates[1],
                    1,
                    0,
                    EXPIRY + 2,
                )
                .expect("owner renews the second capability after the rolled-back prefix");
                assert_eq!(
                    env.portfolio_matcher_sequence(portfolios[3]),
                    sequences[1] + 1
                );
                match &mut fresh[1] {
                    ProgInstruction::TradeCpi {
                        account_b_matcher_sequence,
                        ..
                    }
                    | ProgInstruction::BatchTradeCpi {
                        account_b_matcher_sequence,
                        ..
                    } => {
                        *account_b_matcher_sequence = sequences[1] + 1;
                    }
                    _ => unreachable!(),
                }
            }
            inv019_hostile_context_control(
                &mut env,
                matcher,
                &owners[1],
                context,
                vec![11, 9, 0],
                None,
            );
            env.svm.expire_blockhash();
            let before_retry = snapshot(&env);
            let retry = sign(&env, &fresh);
            assert_calls(
                &env.svm
                    .simulate_transaction(retry.clone().into())
                    .expect("fresh current responses keep the retained economic requests live"),
                2,
            );
            assert_eq!(snapshot(&env), before_retry);

            // A fresh, correctly bound response still cannot override the signed sale price.
            let mut tight = fresh.clone();
            match &mut tight[1] {
                ProgInstruction::TradeCpi { limit_price, .. } => *limit_price += 1,
                ProgInstruction::BatchTradeCpi { legs, .. } => legs[0].limit_price += 1,
                _ => unreachable!(),
            }
            let tight = sign(&env, &tight);
            let failed = env
                .svm
                .send_transaction(tight)
                .expect_err("one-atom tighter price rejects");
            assert_eq!(
                failed.err,
                TransactionError::InstructionError(
                    3,
                    InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                )
            );
            assert_calls(&failed.meta, 2);
            assert_eq!(
                snapshot(&env),
                before_retry,
                "{label}: post-CPI price rollback"
            );
            peak_cu = peak_cu.max(failed.meta.compute_units_consumed);

            let committed = env
                .svm
                .send_transaction(retry)
                .expect("exact-price retry commits both fills");
            assert_calls(&committed, 2);
            peak_cu = peak_cu.max(committed.compute_units_consumed);
            assert_eq!(env.market_state().0.matcher_req_seq, 2);
            let response = if batch {
                assert_eq!(committed.return_data.program_id, matcher);
                assert_eq!(committed.return_data.data.len(), 64);
                committed.return_data.data.clone()
            } else {
                env.svm.get_account(&context).unwrap().data[..64].to_vec()
            };
            let ret = percolator_prog::matcher_abi::read_matcher_return(&response).unwrap();
            assert_eq!(ret.abi_version, MATCHER_ABI_VERSION);
            assert_eq!(ret.flags, percolator_prog::matcher_abi::FLAG_VALID);
            assert_eq!(ret.req_id, 2);
            assert_eq!(
                ret.lp_account_id,
                u64::from_le_bytes(delegates[1].to_bytes()[..8].try_into().unwrap())
            );
            assert_eq!(ret.asset_index, 1);
            assert_eq!(ret.exec_size, sizes[1]);
            assert_eq!(ret.oracle_price_e6, prices[1]);
            assert_eq!(ret.exec_price_e6, prices[1]);
            let group = env.market_state().1;
            for pair in 0..2 {
                for (actor, size) in [(2 * pair, sizes[pair]), (2 * pair + 1, -sizes[pair])] {
                    let state = env.portfolio_state(portfolios[actor]);
                    let leg = active_leg_for_asset(&state, pair);
                    assert_eq!(leg.basis_pos_q, size);
                    assert_eq!(leg.market_id, env.asset_market_id(pair as u16));
                    assert!(!has_active_leg_for_asset(&state, 1 - pair));
                    assert_eq!(state.capital.get(), CAPITAL);
                    assert_eq!(state.pnl.get(), 0);
                    assert_eq!(env.portfolio_position_epoch(portfolios[actor]), 1);
                }
                assert_eq!(group.assets[pair].oi_eff_long_q, sizes[pair].unsigned_abs());
                assert_eq!(
                    group.assets[pair].oi_eff_short_q,
                    sizes[pair].unsigned_abs()
                );
            }
            assert_eq!(env.portfolio_matcher_sequence(portfolios[1]), sequences[0]);
            assert_eq!(
                env.portfolio_matcher_sequence(portfolios[3]),
                sequences[1] + u64::from(expired)
            );
            assert_eq!(
                env.portfolio_matcher_expiry(portfolios[3]),
                if expired { EXPIRY + 2 } else { EXPIRY }
            );
            assert_eq!(group.c_tot, 4 * CAPITAL);
            assert_eq!(group.vault, 4 * CAPITAL);
            assert_eq!(env.token_amount(env.vault) as u128, group.vault);
            for (index, key) in keys.iter().enumerate() {
                if *key != env.market && *key != context && !portfolios.contains(key) {
                    assert_eq!(
                        env.svm.get_account(key),
                        before_retry[index],
                        "{label}: custody frame {key}"
                    );
                }
            }
        }
    }
    println!("INV-019 retained CPI bundles: 4 worlds, 8 exact rollbacks, 8 committed fills, peak CU {peak_cu}");
}
