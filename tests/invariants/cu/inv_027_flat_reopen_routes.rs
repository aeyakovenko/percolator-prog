//! INV-027 / row 434: an already closed fee episode survives route-switching
//! rollback, explicit elapsed-fee settlement, recertification and senior exit.
//! This covers prefixed reopening, not standalone admission with deferred fees.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_flat_reopen_route_switch_preserves_fee_history_and_senior_exit() {
    const CLOSE: u64 = 2;
    const SIZE: i128 = POS_SCALE as i128;
    let collected = FEE_RATE * u128::from(CLOSE - START);
    let pending = FEE_RATE * u128::from(ADMISSION - CLOSE);
    let margin = |size: i128| (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    assert_eq!(
        (collected, pending, margin(SIZE), margin(SIZE + 1)),
        (7, 14, 100, 101)
    );
    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let mut peak_cu = 0;
    let mut worlds = 0;
    for thin in 0..2 {
        let principal = std::array::from_fn::<_, 2, _>(|i| if i == thin { 100 } else { 200 });
        let deposits = principal.map(|amount| amount + collected + pending);
        assert!(deposits[thin] - collected >= margin(SIZE + 1));
        let supply = deposits.iter().sum::<u128>();
        let mut reference = None;
        for (route_index, route) in routes.into_iter().enumerate() {
            let retry = routes[3 - route_index];
            for staged in [false, true] {
                let label = format!("flat reopen/thin={thin}/{route:?}->{retry:?}/staged={staged}");
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        maintenance_fee_per_slot: FEE_RATE,
                        maintenance_margin_bps: 5_000,
                        initial_margin_bps: 10_000,
                        max_price_move_bps_per_slot: 500,
                        max_abs_funding_e9_per_slot: 0,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(START);
                env.configure_auth_mark_for_asset_as_admin(0, START, PRICE);
                let owners = [Keypair::new(), Keypair::new()];
                let portfolios = owners
                    .each_ref()
                    .map(|owner| public_portfolio(&mut env, owner));
                let tokens = std::array::from_fn::<_, 2, _>(|i| {
                    public_deposit(&mut env, &owners[i], portfolios[i], deposits[i])
                });
                let keeper_owner = Keypair::new();
                let keeper = public_portfolio(&mut env, &keeper_owner);
                env.trade_asset_with_cu(
                    0,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    SIZE / 2,
                    PRICE,
                    0,
                );
                env.svm.warp_to_slot(CLOSE);
                env.crank(
                    keeper,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: CLOSE,
                        observations: crank_observations(0),
                    },
                );
                env.trade_asset_with_cu(
                    0,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    -SIZE / 2,
                    PRICE,
                    0,
                );
                for i in 0..2 {
                    let account = env.portfolio_state(portfolios[i]);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    assert_eq!(
                        (account.capital.get(), account.last_fee_slot.get()),
                        (deposits[i] - collected, CLOSE)
                    );
                    assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
                }
                let (matcher, context, delegate) =
                    auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
                let closed = portfolios.map(|key| env.svm.get_account(&key));
                for slot in CLOSE + 1..=ADMISSION {
                    env.svm.warp_to_slot(slot);
                    env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations(0),
                        },
                    );
                }
                assert_eq!(
                    portfolios.map(|key| env.svm.get_account(&key)),
                    closed,
                    "{label}: age only the market"
                );

                let tracked = [
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    keeper,
                    env.mint,
                    env.vault,
                    tokens[0],
                    tokens[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    matcher,
                    context,
                    delegate,
                    env.program_id,
                    spl_token::ID,
                ];
                let immutable = [
                    keeper,
                    env.mint,
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    matcher,
                    delegate,
                    env.program_id,
                    spl_token::ID,
                ];
                let frame = immutable.map(|key| env.svm.get_account(&key));
                let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
                let submit = |env: &mut V16CuEnv, suffix: Vec<Instruction>| {
                    let mut instructions = vec![heap_ix(), cu_ix()];
                    instructions.extend(suffix);
                    let mut signers = vec![&env.payer];
                    for owner in [&owners[0], &owners[1], &keeper_owner] {
                        if instructions
                            .iter()
                            .flat_map(|ix| &ix.accounts)
                            .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
                        {
                            signers.push(owner);
                        }
                    }
                    env.svm.expire_blockhash();
                    let tx = Transaction::new_signed_with_payer(
                        &instructions,
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(
                        bincode::serialized_size(&tx).unwrap() <= 1_232,
                        "{label}: packet bound"
                    );
                    let mut keys = tracked.to_vec();
                    keys.extend(tx.message.account_keys.iter().copied());
                    keys.sort_unstable();
                    keys.dedup();
                    let mut before = keys
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>();
                    let fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    let result = env.svm.send_transaction(tx);
                    if result.is_err() {
                        let payer = keys
                            .iter()
                            .position(|key| *key == env.payer.pubkey())
                            .unwrap();
                        before[payer].as_mut().unwrap().lamports -= fee;
                        assert_eq!(
                            keys.iter()
                                .map(|key| env.svm.get_account(key))
                                .collect::<Vec<_>>(),
                            before,
                            "{label}: every transaction Account rolls back, less the network fee"
                        );
                    }
                    result
                };
                let trade = |env: &V16CuEnv, route, size_q| {
                    let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                    let data = match route {
                        TradeRoute::NoCpi => {
                            env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, size_q, PRICE, 0)
                        }
                        TradeRoute::Cpi => {
                            env.trade_cpi_ix(portfolios[0], portfolios[1], 0, size_q, 0, PRICE)
                        }
                        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q,
                                exec_price: PRICE,
                                fee_bps: 0,
                            }],
                        ),
                        TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeCpiLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q,
                                fee_bps: 0,
                                limit_price: PRICE,
                            }],
                            0,
                            0,
                        ),
                    };
                    let mut accounts = vec![AccountMeta::new(owners[0].pubkey(), true)];
                    if !cpi {
                        accounts.push(AccountMeta::new(owners[1].pubkey(), true));
                    }
                    accounts.extend([
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ]);
                    if cpi {
                        accounts.extend([
                            AccountMeta::new_readonly(matcher, false),
                            AccountMeta::new(context, false),
                            AccountMeta::new_readonly(delegate, false),
                        ]);
                    }
                    Instruction {
                        program_id: env.program_id,
                        accounts,
                        data: data.encode(),
                    }
                };
                let prefix_for = |i: usize| {
                    vec![
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            data: ProgInstruction::SyncMaintenanceFee {
                                now_slot: ADMISSION,
                            }
                            .encode(),
                        },
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(keeper_owner.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(portfolios[i], false),
                            ],
                            data: ProgInstruction::PermissionlessCrank {
                                now_slot: ADMISSION,
                                observations: crank_observations(0),
                            }
                            .encode(),
                        },
                    ]
                };
                let prefixes = [prefix_for(0), prefix_for(1)];
                let prefix: Vec<_> = prefixes.iter().flatten().cloned().collect();
                let check = |env: &V16CuEnv, settled: [bool; 2], open: bool, paid: [u128; 2]| {
                    let group = env.market_state().1;
                    let fees = settled.map(|done| collected + if done { pending } else { 0 });
                    let accounts =
                        [portfolios[0], portfolios[1], keeper].map(|key| env.portfolio_state(key));
                    assert_eq!(
                        (group.current_slot, group.assets[0].slot_last),
                        (ADMISSION, ADMISSION)
                    );
                    assert_eq!(
                        (
                            group.assets[0].effective_price,
                            group.assets[0].raw_oracle_target_price
                        ),
                        (PRICE, PRICE)
                    );
                    assert_eq!(group.insurance, fees.iter().sum::<u128>());
                    assert_eq!(
                        group.c_tot,
                        supply - group.insurance - paid.iter().sum::<u128>()
                    );
                    assert_eq!(group.vault, supply - paid.iter().sum::<u128>());
                    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
                    assert_eq!(
                        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                            .unwrap()
                            .supply as u128,
                        supply
                    );
                    assert_eq!(
                        (group.pnl_pos_tot, group.source_claim_bound_total_num),
                        (0, 0)
                    );
                    let long_fees = 2 * (collected / 2)
                        + settled.iter().filter(|&&done| done).count() as u128 * (pending / 2);
                    assert_eq!(
                        &group.insurance_domain_budget[..2],
                        &[long_fees, group.insurance - long_fees]
                    );
                    assert!(group.insurance_domain_budget[2..]
                        .iter()
                        .all(|&amount| amount == 0));
                    let size = if open { SIZE as u128 } else { 0 };
                    assert_eq!(
                        (
                            group.assets[0].oi_eff_long_q,
                            group.assets[0].oi_eff_short_q
                        ),
                        (size, size)
                    );
                    for i in 0..2 {
                        let account = &accounts[i];
                        assert_eq!(
                            account.capital.get(),
                            deposits[i] - fees[i] - paid[i],
                            "{label}: owner {i}"
                        );
                        assert_eq!(
                            account.last_fee_slot.get(),
                            if settled[i] { ADMISSION } else { CLOSE }
                        );
                        assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
                        assert_eq!(env.token_amount(tokens[i]) as u128, paid[i]);
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(account)),
                            u32::from(open)
                        );
                        if open {
                            assert_eq!(
                                active_leg_for_asset(account, 0).basis_pos_q,
                                if i == 0 { SIZE } else { -SIZE }
                            );
                            assert!(assert_current_certificate_matches_independent(
                                &label, &group, account
                            )
                            .unwrap());
                            let cert = health_cert(account);
                            assert_eq!(cert.certified_equity, principal[i] as i128);
                            assert_eq!(cert.certified_initial_req, margin(SIZE));
                        }
                    }
                    assert_market_stock_census(
                        &label,
                        &group,
                        &env.svm.get_account(&env.market).unwrap().data,
                        &accounts,
                        env.token_amount(env.vault).into(),
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                    assert_source_credit_rates(&label, &group).unwrap();
                    assert_eq!(
                        immutable.map(|key| env.svm.get_account(&key)),
                        frame,
                        "{label}: unrelated Account frame"
                    );
                };
                check(&env, [false; 2], false, [0; 2]);
                let before = snapshot(&env);
                let mut excessive = prefix.clone();
                excessive.push(trade(&env, route, SIZE + 1));
                let failure =
                    submit(&mut env, excessive).expect_err("reopen must fit post-fee equity");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        6,
                        InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)
                    ),
                    "{label}: fee/refresh prefix must succeed: {failure:?}"
                );
                peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
                assert_eq!(snapshot(&env), before);
                check(&env, [false; 2], false, [0; 2]);

                if staged {
                    let mut settled = [false; 2];
                    for i in 0..2 {
                        peak_cu = peak_cu.max(
                            submit(&mut env, prefixes[i].clone())
                                .unwrap()
                                .compute_units_consumed,
                        );
                        settled[i] = true;
                        check(&env, settled, false, [0; 2]);
                    }
                }
                let mut exact = if staged { Vec::new() } else { prefix.clone() };
                exact.push(trade(&env, retry, SIZE));
                peak_cu = peak_cu.max(
                    submit(&mut env, exact)
                        .expect("opposite route admits exact post-fee equity")
                        .compute_units_consumed,
                );
                check(&env, [true; 2], true, [0; 2]);
                let certs = portfolios.map(|key| health_cert(&env.portfolio_state(key)));
                assert_eq!(
                    certs[thin].certified_equity,
                    certs[thin].certified_initial_req as i128
                );
                if let Some(expected) = reference {
                    assert_eq!(
                        certs, expected,
                        "{label}: full certificates agree across routes and settlement schedules"
                    );
                } else {
                    reference = Some(certs);
                }

                // Owner-signed reopening revokes the standing matcher grant.
                if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.set_matcher_config(
                        matcher,
                        &owners[1],
                        portfolios[1],
                        context,
                        delegate,
                        1,
                    );
                    check(&env, [true; 2], true, [0; 2]);
                }
                let close = trade(&env, route, -SIZE);
                peak_cu = peak_cu.max(
                    submit(&mut env, vec![close])
                        .expect("original route closes reopened exposure")
                        .compute_units_consumed,
                );
                check(&env, [true; 2], false, [0; 2]);
                let mut paid = [0; 2];
                for i in [thin, 1 - thin] {
                    let payout = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: env.withdraw_ix(portfolios[i], principal[i]).encode(),
                    };
                    peak_cu = peak_cu.max(
                        submit(&mut env, vec![payout])
                            .expect("full post-fee senior principal exits")
                            .compute_units_consumed,
                    );
                    paid[i] = principal[i];
                    check(&env, [true; 2], false, paid);
                }
                assert_eq!(
                    (
                        env.market_state().1.c_tot,
                        env.token_amount(env.vault) as u128
                    ),
                    (0, 2 * (collected + pending))
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_cu_within("flat reopen route switch", peak_cu, 600_000);
    println!("INV-027 flat reopen routes: {worlds} worlds, 16 prefix rollbacks, 16 route-switch reopens, 32 senior payouts; peak CU={peak_cu}");
}
