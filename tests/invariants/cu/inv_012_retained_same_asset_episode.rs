//! INV-004/005/010/012/024/081: retained exits across a same-asset LP episode ABA.
//!
//! A third portfolio closes and reopens the LP through its live matcher. The original
//! taker's Account, asset generation, portfolio identities and grant stay fixed. Both
//! retained CPI exits must reject; refreshing only the LP episode restores owner exit.
//! This covers position scope under an unchanged authority, not authority rotation or
//! automatic revocation. All economic transitions use public instructions.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const PRINCIPAL: u128 = 10_000_000;
const PRICE: u64 = 1_000_000;

fn assert_episode_economics(env: &V16CuEnv, portfolios: [Pubkey; 3], sizes: [i128; 3]) {
    let group = env.market_state().1;
    assert_eq!(group.c_tot, 3 * PRINCIPAL);
    assert_eq!(group.vault, 3 * PRINCIPAL);
    assert_eq!(env.token_amount(env.vault), (3 * PRINCIPAL) as u64);
    assert_eq!(sizes.iter().sum::<i128>(), 0);
    let expected_oi: u128 = sizes.iter().filter(|q| **q > 0).map(|q| *q as u128).sum();
    assert_eq!(group.assets[0].oi_eff_long_q, expected_oi);
    assert_eq!(group.assets[0].oi_eff_short_q, expected_oi);

    let mut market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, market) = state::market_view_mut(&mut market_data).unwrap();
    market.validate_shape().unwrap();
    for (portfolio, size) in portfolios.into_iter().zip(sizes) {
        let account = env.portfolio_state(portfolio);
        assert_eq!(account.capital.get(), PRINCIPAL);
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&account)),
            u32::from(size != 0),
        );
        if size != 0 {
            assert_eq!(active_leg_for_asset(&account, 0).basis_pos_q, size);
        }
        let mut data = env.svm.get_account(&portfolio).unwrap().data;
        state::portfolio_view_mut_for_market_slots(&mut data, 1)
            .unwrap()
            .validate_with_market(&market.as_view())
            .unwrap();
    }
}

#[test]
fn v16_program_retained_exit_cannot_follow_lp_through_same_asset_flat_reopen() {
    for writer_batch in [false, true] {
        for direction in [-1i128, 1] {
            let mut env = V16CuEnv::new();
            env.svm.warp_to_slot(1);
            env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
            let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
            let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
            let [taker, lp, bridge] = portfolios;
            let sources = std::array::from_fn::<_, 3, _>(|i| {
                env.deposit(&owners[i], portfolios[i], PRINCIPAL)
            });
            let destinations = owners
                .each_ref()
                .map(|owner| env.token_account(owner.pubkey(), 0));
            let (matcher, context, delegate) =
                auth_matcher_for_lp_via_system_create(&mut env, &owners[1], lp);
            env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
                matcher, &owners[1], lp, context, delegate, 1, 0, 100,
            )
            .expect("owner grants the matcher a live zero-fee capability");

            let request = |env: &V16CuEnv, batch: bool, account_a: Pubkey, size_q: i128| {
                if batch {
                    env.batch_trade_cpi_ix_with_caps(
                        account_a,
                        lp,
                        vec![BatchTradeCpiLeg {
                            asset_index: 0,
                            market_id: env.asset_market_id(0),
                            size_q,
                            fee_bps: 0,
                            limit_price: PRICE,
                        }],
                        0,
                        0,
                    )
                } else {
                    env.trade_cpi_ix(account_a, lp, 0, size_q, 0, PRICE)
                }
            };
            let transaction = |env: &V16CuEnv,
                               owner: &Keypair,
                               account_a: Pubkey,
                               request: &ProgInstruction,
                               prefix: bool| {
                let mut instructions = vec![heap_ix(), cu_ix()];
                if prefix {
                    instructions.push(system_instruction::transfer(
                        &owner.pubkey(),
                        &owners[2].pubkey(),
                        7,
                    ));
                }
                instructions.push(Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(account_a, false),
                        AccountMeta::new(lp, false),
                        AccountMeta::new_readonly(matcher, false),
                        AccountMeta::new(context, false),
                        AccountMeta::new_readonly(delegate, false),
                    ],
                    data: request.encode(),
                });
                Transaction::new_signed_with_payer(
                    &instructions,
                    Some(&env.payer.pubkey()),
                    &[&env.payer, owner],
                    env.svm.latest_blockhash(),
                )
            };
            let q = direction * 2 * POS_SCALE as i128;
            let open = request(&env, writer_batch, taker, q);
            let open = transaction(&env, &owners[0], taker, &open, false);
            env.svm
                .send_transaction(open)
                .expect("initial matcher entry");
            assert_episode_economics(&env, portfolios, [q, -q, 0]);

            let grant = env.portfolio_matcher_config(lp);
            let grant_sequence = env.portfolio_matcher_sequence(lp);
            let grant_expiry = env.portfolio_matcher_expiry(lp);
            let ids = portfolios.map(|key| env.portfolio_id(key));
            let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
            let generation = env.asset_market_id(0);
            let taker_before = env.svm.get_account(&taker).unwrap();
            let frame_keys: Vec<_> = [env.market, env.vault, env.mint, context, delegate]
                .into_iter()
                .chain(portfolios)
                .chain(sources)
                .chain(destinations)
                .chain(owners.iter().map(Signer::pubkey))
                .collect();
            let snapshot = |env: &V16CuEnv| {
                frame_keys
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let retained_requests = [false, true].map(|batch| request(&env, batch, taker, -q));
            let mut retained = Vec::new();
            for retained_request in &retained_requests {
                for prefix in [false, true] {
                    let tx = transaction(&env, &owners[0], taker, retained_request, prefix);
                    let before = snapshot(&env);
                    env.svm
                        .simulate_transaction(tx.clone().into())
                        .expect("the exact retained exit is valid in its original episode");
                    assert_eq!(snapshot(&env), before, "simulation must not commit state");
                    retained.push((prefix, tx));
                }
            }

            // The bridge alone changes counterparties: LP goes -q -> 0 -> -q,
            // while the original taker's position and signed episode never change.
            for (step, size) in [-q, q].into_iter().enumerate() {
                let ix = request(&env, writer_batch, bridge, size);
                let tx = transaction(&env, &owners[2], bridge, &ix, false);
                let result = env
                    .svm
                    .send_transaction(tx)
                    .expect("public LP close/reopen");
                assert!(result
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {matcher} success")));
                assert_episode_economics(
                    &env,
                    portfolios,
                    if step == 0 { [q, 0, -q] } else { [q, -q, 0] },
                );
                assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
                assert_eq!(
                    env.portfolio_position_epoch(lp),
                    epochs[1] + step as u64 + 1
                );
                assert_eq!(
                    env.portfolio_position_epoch(bridge),
                    epochs[2] + step as u64 + 1
                );
                assert_eq!(portfolios.map(|key| env.portfolio_id(key)), ids);
                assert_eq!(env.asset_market_id(0), generation);
                let current = env.portfolio_matcher_config(lp);
                assert_eq!(current.enabled(), 1);
                assert_eq!(current.matcher_program, grant.matcher_program);
                assert_eq!(current.matcher_context, grant.matcher_context);
                assert_eq!(current.matcher_delegate, grant.matcher_delegate);
                assert_eq!(current.trade_fee_cap_bps(), grant.trade_fee_cap_bps());
                assert_eq!(env.portfolio_matcher_sequence(lp), grant_sequence);
                assert_eq!(env.portfolio_matcher_expiry(lp), grant_expiry);
            }

            for (prefix, tx) in retained {
                assert_eq!(tx.message.recent_blockhash, env.svm.latest_blockhash());
                let before = snapshot(&env);
                let error = env
                    .svm
                    .send_transaction(tx)
                    .expect_err("old LP episode must reject");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        if prefix { 3 } else { 2 },
                        InstructionError::Custom(PercolatorError::EngineStale as u32),
                    ),
                );
                assert_eq!(snapshot(&env), before, "exact tracked Account rollback");
                assert!(!error
                    .meta
                    .logs
                    .iter()
                    .any(|line| line.starts_with(&format!("Program {matcher} invoke"))));
                if prefix {
                    assert!(error.meta.logs.iter().any(|line| line
                        == &format!("Program {} success", solana_sdk::system_program::id())));
                }
            }

            let mut fresh_exits = Vec::new();
            for (batch, mut fresh) in [false, true].into_iter().zip(retained_requests) {
                match &mut fresh {
                    ProgInstruction::TradeCpi {
                        account_b_position_epoch,
                        ..
                    }
                    | ProgInstruction::BatchTradeCpi {
                        account_b_position_epoch,
                        ..
                    } => {
                        *account_b_position_epoch = env.portfolio_position_epoch(lp);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(fresh.encode(), request(&env, batch, taker, -q).encode());
                let tx = transaction(&env, &owners[0], taker, &fresh, false);
                let before = snapshot(&env);
                env.svm
                    .simulate_transaction(tx.clone().into())
                    .expect("fresh LP episode alone restores either CPI exit");
                assert_eq!(snapshot(&env), before);
                fresh_exits.push(tx);
            }
            let exit = fresh_exits.swap_remove(usize::from(!writer_batch));
            env.svm
                .send_transaction(exit)
                .expect("fresh cross-route owner exit");
            assert_episode_economics(&env, portfolios, [0, 0, 0]);
            assert_eq!(env.portfolio_matcher_sequence(lp), grant_sequence);
            assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);

            for i in 0..3 {
                env.send(
                    env.withdraw_ix(portfolios[i], PRINCIPAL),
                    vec![
                        AccountMeta::new(owners[i].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[i], false),
                        AccountMeta::new(destinations[i], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&owners[i]],
                )
                .expect("each owner withdraws exactly their own principal");
                assert_eq!(env.token_amount(destinations[i]), PRINCIPAL as u64);
                assert_eq!(env.token_amount(sources[i]), 0);
                assert_eq!(env.portfolio_state(portfolios[i]).capital.get(), 0);
                let remaining = (2 - i) as u128 * PRINCIPAL;
                assert_eq!(env.market_state().1.c_tot, remaining);
                assert_eq!(env.market_state().1.vault, remaining);
                assert_eq!(env.token_amount(env.vault), remaining as u64);
            }
            eprintln!("same-asset episode: writer_batch={writer_batch}, direction={direction}; four retained rejections, three exact principal payouts");
        }
    }
}
