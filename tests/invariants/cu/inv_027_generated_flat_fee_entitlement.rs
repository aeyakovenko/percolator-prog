//! INV-027/044/053/060/081: generated flat fee histories before public admission.
//! Explicit sync and implicit withdrawal collection must produce the same owner
//! entitlement, rounded fee destination and exact margin boundary on four routes.
//! Standalone admission with deferred fees and junior claim support are outside scope.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

#[derive(Clone, Copy, Debug)]
struct FeeCase {
    rate: u128,
    elapsed: u64,
    quantity: i128,
    bps: u64,
    partial: u128,
    thin: usize,
    prior_episode: bool,
}

#[derive(Clone, Copy, Default)]
struct FeeBook {
    maintenance: [u128; 3],
    trading: [u128; 3],
    paid: [u128; 3],
    cursors: [u64; 3],
    budgets: [u128; 2],
    position: i128,
}

impl FeeBook {
    fn collect(&mut self, case: FeeCase, owner: usize, slot: u64) {
        let amount = case.rate * u128::from(slot - self.cursors[owner]);
        self.maintenance[owner] += amount;
        self.cursors[owner] = slot;
        self.budgets[0] += amount / 2;
        self.budgets[1] += amount - amount / 2;
    }

    fn trade(&mut self, fee: u128, position: i128) {
        for owner in 0..2 {
            self.trading[owner] += fee;
        }
        // Equal per-owner trade charges supply one full fee to each side.
        self.budgets[0] += fee;
        self.budgets[1] += fee;
        self.position = position;
    }
}

#[test]
fn v16_program_generated_flat_fee_collection_preserves_first_admission_entitlement() {
    let mut cases = vec![
        FeeCase {
            rate: 1,
            elapsed: 2,
            quantity: POS_SCALE as i128,
            bps: 1,
            partial: 1,
            thin: 0,
            prior_episode: false,
        },
        FeeCase {
            rate: 7,
            elapsed: 3,
            quantity: POS_SCALE as i128 + 1,
            bps: 100,
            partial: 13,
            thin: 1,
            prior_episode: true,
        },
        FeeCase {
            rate: 9,
            elapsed: 5,
            quantity: 2 * POS_SCALE as i128 - 1,
            bps: 137,
            partial: 17,
            thin: 0,
            prior_episode: true,
        },
    ];
    let mut rng = XorShiftRng::from_seed([0x27; 16]);
    for _ in 0..5 {
        cases.push(FeeCase {
            rate: rng.gen_range(1..=11),
            elapsed: rng.gen_range(2..=6),
            quantity: rng.gen_range(1..=3) * POS_SCALE as i128 + rng.gen_range(-1..=1),
            bps: rng.gen_range(1..=199),
            partial: rng.gen_range(1..=19),
            thin: rng.gen_range(0..2),
            prior_episode: rng.gen(),
        });
    }
    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let mut worlds = 0;
    let mut transactions = 0;
    let mut peak_cu = 0;
    for case in cases {
        let admission = START + case.elapsed;
        let notional = |q: i128| (q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
        let fee = |q| (notional(q) * u128::from(case.bps)).div_ceil(10_000);
        let opening_fee = fee(case.quantity);
        let prior_quantity = POS_SCALE as i128 / 2;
        let prior_fees = if case.prior_episode {
            2 * fee(prior_quantity)
        } else {
            0
        };
        let elapsed_fee = case.rate * u128::from(case.elapsed);
        let funds = std::array::from_fn::<_, 3, _>(|i| {
            elapsed_fee
                + if i == 2 {
                    59
                } else {
                    notional(case.quantity)
                        + opening_fee
                        + prior_fees
                        + case.partial
                        + if i == case.thin { 0 } else { 73 }
                }
        });
        let supply = funds.iter().sum::<u128>();
        let excessive = (notional(case.quantity) * POS_SCALE / u128::from(PRICE) + 1) as i128;
        assert!(notional(excessive) + fee(excessive) > notional(case.quantity) + opening_fee);
        assert!(opening_fee > 0 && elapsed_fee > 0);
        let mut reference = None;
        for (route_index, route) in routes.into_iter().enumerate() {
            for explicit in [false, true] {
                let label = format!("generated flat fees/{case:?}/{route:?}/sync={explicit}");
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        maintenance_fee_per_slot: case.rate,
                        trade_fee_base_bps: case.bps,
                        maintenance_margin_bps: 5_000,
                        initial_margin_bps: 10_000,
                        max_price_move_bps_per_slot: 500,
                        max_abs_funding_e9_per_slot: 0,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(START);
                env.configure_auth_mark_for_asset_as_admin(0, START, PRICE);
                let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
                let portfolios = owners
                    .each_ref()
                    .map(|owner| public_portfolio(&mut env, owner));
                let tokens = std::array::from_fn::<_, 3, _>(|i| {
                    public_deposit(&mut env, &owners[i], portfolios[i], funds[i])
                });
                let keeper_owner = Keypair::new();
                let keeper = public_portfolio(&mut env, &keeper_owner);
                let (matcher, context, delegate) =
                    auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
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
                let identities = portfolios.map(|key| env.portfolio_id(key));
                let immutable = [
                    env.mint,
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    owners[2].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    matcher,
                    delegate,
                ];
                let frame = immutable.map(|key| env.svm.get_account(&key));
                let tracked = [
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    portfolios[2],
                    keeper,
                    env.vault,
                    tokens[0],
                    tokens[1],
                    tokens[2],
                    context,
                ];
                let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
                let mut book = FeeBook {
                    cursors: [START; 3],
                    ..FeeBook::default()
                };
                let check = |env: &V16CuEnv, book: FeeBook| {
                    let group = env.market_state().1;
                    let accounts = [portfolios[0], portfolios[1], portfolios[2], keeper]
                        .map(|key| env.portfolio_state(key));
                    let fees =
                        book.maintenance.iter().sum::<u128>() + book.trading.iter().sum::<u128>();
                    let payouts = book.paid.iter().sum::<u128>();
                    assert_eq!(
                        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                            .unwrap()
                            .supply as u128,
                        supply
                    );
                    assert_eq!(
                        (group.insurance, group.c_tot, group.vault),
                        (fees, supply - fees - payouts, supply - payouts),
                        "{label}"
                    );
                    assert_eq!(
                        env.token_amount(env.vault) as u128,
                        supply - payouts,
                        "{label}"
                    );
                    assert_eq!(
                        &group.insurance_domain_budget[..2],
                        &book.budgets,
                        "{label}"
                    );
                    assert!(group.insurance_domain_budget[2..].iter().all(|&v| v == 0));
                    assert_eq!(
                        (group.pnl_pos_tot, group.source_claim_bound_total_num),
                        (0, 0)
                    );
                    assert_eq!(
                        (
                            group.assets[0].effective_price,
                            group.assets[0].raw_oracle_target_price
                        ),
                        (PRICE, PRICE)
                    );
                    assert_eq!(
                        (
                            group.assets[0].oi_eff_long_q,
                            group.assets[0].oi_eff_short_q
                        ),
                        (book.position.unsigned_abs(), book.position.unsigned_abs())
                    );
                    for i in 0..3 {
                        let account = &accounts[i];
                        let capital =
                            funds[i] - book.maintenance[i] - book.trading[i] - book.paid[i];
                        assert_eq!(account.capital.get(), capital, "{label}: owner {i}");
                        assert_eq!(env.portfolio_id(portfolios[i]), identities[i]);
                        assert_eq!(account.owner, owners[i].pubkey().to_bytes());
                        assert_eq!(
                            account.last_fee_slot.get(),
                            book.cursors[i],
                            "{label}: fee cursor {i}"
                        );
                        assert_eq!((account.fee_credits.get(), account.pnl.get()), (0, 0));
                        assert_eq!(env.token_amount(tokens[i]) as u128, book.paid[i]);
                        let q = if i == 0 {
                            book.position
                        } else if i == 1 {
                            -book.position
                        } else {
                            0
                        };
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(account)),
                            u32::from(q != 0)
                        );
                        let current =
                            assert_current_certificate_matches_independent(&label, &group, account)
                                .unwrap();
                        if current {
                            let cert = health_cert(account);
                            assert_eq!(
                                cert.certified_equity, capital as i128,
                                "{label}: fees deducted once"
                            );
                            assert_eq!(cert.certified_initial_req, notional(q));
                            assert_eq!(
                                cert.certified_maintenance_req,
                                (notional(q) * 5_000).div_ceil(10_000)
                            );
                            assert_eq!(cert.certified_liq_deficit, 0);
                            assert_eq!(cert.certified_worst_case_loss, notional(q));
                        }
                        if q != 0 {
                            assert_eq!(active_leg_for_asset(account, 0).basis_pos_q, q);
                        }
                    }
                    assert_eq!(accounts[3].capital.get(), 0);
                    assert_eq!(
                        immutable.map(|key| env.svm.get_account(&key)),
                        frame,
                        "{label}: immutable accounts"
                    );
                    assert_market_stock_census(
                        &label,
                        &group,
                        &env.svm.get_account(&env.market).unwrap().data,
                        &accounts,
                        env.token_amount(env.vault).into(),
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                };
                let instruction = |env: &V16CuEnv, data: ProgInstruction, accounts| Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: data.encode(),
                };
                let sync = |env: &V16CuEnv, i: usize| {
                    instruction(
                        env,
                        ProgInstruction::SyncMaintenanceFee {
                            now_slot: admission,
                        },
                        vec![
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                        ],
                    )
                };
                let refresh = |env: &V16CuEnv, key, now_slot| {
                    instruction(
                        env,
                        ProgInstruction::PermissionlessCrank {
                            now_slot,
                            observations: crank_observations(0),
                        },
                        vec![
                            AccountMeta::new(keeper_owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key, false),
                        ],
                    )
                };
                let withdraw = |env: &V16CuEnv, i: usize, amount| {
                    instruction(
                        env,
                        env.withdraw_ix(portfolios[i], amount),
                        vec![
                            AccountMeta::new(owners[i].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[i], false),
                            AccountMeta::new(tokens[i], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                    )
                };
                let trade = |env: &V16CuEnv, route, q| {
                    let cpi = matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi);
                    let data = match route {
                        TradeRoute::NoCpi => {
                            env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, q, PRICE, case.bps)
                        }
                        TradeRoute::Cpi => {
                            env.trade_cpi_ix(portfolios[0], portfolios[1], 0, q, case.bps, PRICE)
                        }
                        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q: q,
                                exec_price: PRICE,
                                fee_bps: case.bps,
                            }],
                        ),
                        TradeRoute::BatchCpi => env.batch_trade_cpi_ix_with_caps(
                            portfolios[0],
                            portfolios[1],
                            vec![BatchTradeCpiLeg {
                                asset_index: 0,
                                market_id: env.asset_market_id(0),
                                size_q: q,
                                fee_bps: case.bps,
                                limit_price: PRICE,
                            }],
                            0,
                            fee(q),
                        ),
                    };
                    let mut metas = vec![AccountMeta::new(owners[0].pubkey(), true)];
                    if !cpi {
                        metas.push(AccountMeta::new(owners[1].pubkey(), true));
                    }
                    metas.extend([
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ]);
                    if cpi {
                        metas.extend([
                            AccountMeta::new_readonly(matcher, false),
                            AccountMeta::new(context, false),
                            AccountMeta::new_readonly(delegate, false),
                        ]);
                    }
                    instruction(env, data, metas)
                };
                let mut submit = |env: &mut V16CuEnv, ix: Instruction| {
                    env.svm.expire_blockhash();
                    let mut signers = vec![&env.payer];
                    for owner in [&owners[0], &owners[1], &owners[2], &keeper_owner] {
                        if ix
                            .accounts
                            .iter()
                            .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
                        {
                            signers.push(owner);
                        }
                    }
                    let tx = Transaction::new_signed_with_payer(
                        &[heap_ix(), cu_ix(), ix],
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    let result = env.svm.send_transaction(tx);
                    let cu = match &result {
                        Ok(meta) => meta.compute_units_consumed,
                        Err(err) => err.meta.compute_units_consumed,
                    };
                    peak_cu = peak_cu.max(cu);
                    transactions += 1;
                    result
                };
                check(&env, book);
                if case.prior_episode {
                    let ix = trade(&env, TradeRoute::NoCpi, prior_quantity);
                    submit(&mut env, ix).unwrap();
                    book.trade(fee(prior_quantity), prior_quantity);
                    check(&env, book);
                    env.svm.warp_to_slot(START + 1);
                    let ix = refresh(&env, keeper, START + 1);
                    submit(&mut env, ix).unwrap();
                    check(&env, book);
                    let ix = trade(&env, TradeRoute::NoCpi, -prior_quantity);
                    submit(&mut env, ix).unwrap();
                    for i in 0..2 {
                        book.collect(case, i, START + 1);
                    }
                    book.trade(fee(prior_quantity), 0);
                    check(&env, book);
                }
                let flat = portfolios.map(|key| env.svm.get_account(&key));
                let next_slot = START + 1 + u64::from(case.prior_episode);
                for slot in next_slot..=admission {
                    env.svm.warp_to_slot(slot);
                    let ix = refresh(&env, keeper, slot);
                    submit(&mut env, ix).unwrap();
                    assert_eq!(env.market_state().1.assets[0].slot_last, slot);
                    assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), flat);
                    check(&env, book);
                }
                // The unrelated senior owner exits while the traders' fees remain uncollected.
                let ix = withdraw(&env, 2, funds[2] - elapsed_fee);
                submit(&mut env, ix).unwrap();
                book.collect(case, 2, admission);
                book.paid[2] = funds[2] - elapsed_fee;
                check(&env, book);
                for i in [case.thin, 1 - case.thin] {
                    if explicit {
                        let ix = sync(&env, i);
                        submit(&mut env, ix).unwrap();
                        book.collect(case, i, admission);
                        check(&env, book);
                    }
                    let ix = withdraw(&env, i, case.partial);
                    submit(&mut env, ix).unwrap();
                    book.collect(case, i, admission);
                    book.paid[i] += case.partial;
                    check(&env, book);
                    let ix = refresh(&env, portfolios[i], admission);
                    submit(&mut env, ix).unwrap();
                    check(&env, book);
                    assert!(
                        assert_current_certificate_matches_independent(
                            &label,
                            &env.market_state().1,
                            &env.portfolio_state(portfolios[i])
                        )
                        .unwrap(),
                        "{label}: refreshed flat owner {i} must be current"
                    );
                    let before = snapshot(&env);
                    let ix = sync(&env, i);
                    submit(&mut env, ix).unwrap();
                    assert_eq!(
                        snapshot(&env),
                        before,
                        "{label}: same-slot collection is a no-op"
                    );
                    check(&env, book);
                }
                if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.set_matcher_config_with_trade_fee_cap(
                        matcher,
                        &owners[1],
                        portfolios[1],
                        context,
                        delegate,
                        1,
                        case.bps.try_into().unwrap(),
                    );
                    check(&env, book);
                }
                let before = snapshot(&env);
                let ix = trade(&env, route, excessive);
                let failure = submit(&mut env, ix)
                    .expect_err("admission must fit net equity after both fee classes");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)
                    ),
                    "{label}: {failure:?}"
                );
                assert_eq!(snapshot(&env), before, "{label}: admission rollback");
                check(&env, book);
                let ix = trade(&env, route, case.quantity);
                submit(&mut env, ix).unwrap_or_else(|e| panic!("{label}: exact admission: {e:?}"));
                book.trade(opening_fee, case.quantity);
                check(&env, book);
                let certs = [0, 1].map(|i| {
                    let account = env.portfolio_state(portfolios[i]);
                    assert!(assert_current_certificate_matches_independent(
                        &label,
                        &env.market_state().1,
                        &account
                    )
                    .unwrap());
                    health_cert(&account)
                });
                assert_eq!(
                    certs[case.thin].certified_equity,
                    notional(case.quantity) as i128
                );
                if let Some(expected) = reference {
                    assert_eq!(certs, expected, "{label}: route/collection equivalence");
                } else {
                    reference = Some(certs);
                }
                let close_route = routes[3 - route_index];
                if matches!(close_route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.set_matcher_config_with_trade_fee_cap(
                        matcher,
                        &owners[1],
                        portfolios[1],
                        context,
                        delegate,
                        1,
                        case.bps.try_into().unwrap(),
                    );
                    check(&env, book);
                }
                let ix = trade(&env, close_route, -case.quantity);
                submit(&mut env, ix).unwrap();
                book.trade(opening_fee, 0);
                check(&env, book);
                for i in [1 - case.thin, case.thin] {
                    let amount =
                        funds[i] - elapsed_fee - prior_fees - 2 * opening_fee - book.paid[i];
                    let ix = withdraw(&env, i, amount);
                    submit(&mut env, ix).unwrap();
                    book.paid[i] += amount;
                    check(&env, book);
                }
                assert_eq!(env.market_state().1.c_tot, 0);
                assert_eq!(
                    env.token_amount(env.vault) as u128,
                    3 * elapsed_fee + 2 * prior_fees + 4 * opening_fee
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 64);
    assert_cu_within("generated flat fee admission", peak_cu, 400_000);
    eprintln!("INV-027 generated flat fee entitlement: {worlds} worlds, {transactions} checked attempts, 64 margin rollbacks, 128 same-slot sync no-ops, 192 complete owner payouts; peak CU={peak_cu}");
}
