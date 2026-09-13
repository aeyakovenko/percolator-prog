//! INV-024/027/044/053/060/081: accrued funding and maintenance jointly precede
//! first exposure on another asset. A rounded funding debtor cannot reuse stale
//! equity through a route switch after rollback; senior principal can then exit
//! while the peer's junior funding claim remains unconverted.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_funding_and_maintenance_precede_new_asset_after_route_rollback() {
    const RATE: u64 = 10_000;
    const CAP_BPS: u64 = 1;
    const FUNDING_SLOTS: u64 = ADMISSION - 2;
    // Negative premium rounds toward minus infinity before position scaling.
    let funding_per_lot =
        (-(-i128::from(RATE) * i128::from(PRICE)).div_euclid(1_000_000_000)) as u128;
    let funding_num = OLD_SIZE.unsigned_abs() * funding_per_lot * u128::from(FUNDING_SLOTS);
    let debt = funding_num.div_ceil(POS_SCALE);
    let claim = funding_num / POS_SCALE;
    let exact_req = requirement(OLD_SIZE) + requirement(NEW_SIZE);
    let thin_deposit = exact_req + FEE + debt;
    assert_eq!(
        (funding_per_lot, debt, claim, FEE, exact_req),
        (1, 21, 20, 21, 111)
    );
    assert_eq!(
        requirement(OLD_SIZE) + requirement(NEW_SIZE + 1),
        exact_req + 1
    );
    assert!(thin_deposit - FEE >= exact_req + 1);
    assert!(thin_deposit - debt >= exact_req + 1);

    let routes = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];
    let mut peak_cu = 0;
    let mut worlds = 0;
    for debtor in 0..2 {
        let target = PRICE - 1;
        let old_signed_size = if debtor == 0 { -OLD_SIZE } else { OLD_SIZE };
        let deposits =
            std::array::from_fn::<_, 2, _>(|i| if i == debtor { thin_deposit } else { 10_000 });
        let principal = std::array::from_fn::<_, 2, _>(|i| {
            deposits[i] - FEE - if i == debtor { debt } else { 0 }
        });
        let mut reference = None;
        for (route_index, route) in routes.into_iter().enumerate() {
            // Every failed transport retries in the opposite CPI and batching family.
            let retry_route = routes[3 - route_index];
            for explicit in [true, false] {
                let label = format!("funding admission/debtor={debtor}/{route:?}->{retry_route:?}/explicit={explicit}");
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        maintenance_margin_bps: MARGIN_BPS,
                        initial_margin_bps: MARGIN_BPS,
                        max_price_move_bps_per_slot: CAP_BPS,
                        maintenance_fee_per_slot: FEE_RATE,
                        max_abs_funding_e9_per_slot: RATE,
                        ..V16CuMarketParams::default()
                    },
                );
                env.svm.warp_to_slot(START);
                for asset in 0..2 {
                    env.configure_auth_mark_for_asset_as_admin(asset, START, PRICE);
                }
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
                    1,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    old_signed_size,
                    PRICE,
                    0,
                );
                let (matcher, context, delegate) =
                    auth_matcher_for_lp_via_system_create(&mut env, &owners[1], portfolios[1]);
                let original = portfolios.map(|key| env.svm.get_account(&key));
                let opening = env.market_state().1;
                env.svm.warp_to_slot(2);
                env.push_auth_mark_for_asset_as_admin(1, 2, target);
                for slot in 2..=ADMISSION {
                    env.svm.warp_to_slot(slot);
                    peak_cu = peak_cu.max(env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations_for_assets(&[0, 1]),
                        },
                    ));
                    let group = env.market_state().1;
                    let funding_index =
                        (slot - 2) as i128 * funding_per_lot as i128 * ADL_ONE as i128;
                    assert_eq!(
                        (group.assets[1].f_long_num, group.assets[1].f_short_num),
                        (funding_index, -funding_index),
                        "{label}"
                    );
                    assert_eq!(group.assets[1].effective_price, PRICE);
                    assert_eq!(group.assets[1].raw_oracle_target_price, target);
                    assert_eq!(
                        (group.assets[1].k_long, group.assets[1].k_short),
                        (opening.assets[1].k_long, opening.assets[1].k_short)
                    );
                    let profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        1,
                    )
                    .unwrap();
                    assert_eq!(
                        u64::from(profile.price_move_remainder_bps_num),
                        PRICE * CAP_BPS * (slot - START)
                    );
                    assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), original);
                }
                let pending = env.market_state().1;
                assert_eq!(pending.insurance, 0);
                for i in 0..2 {
                    let account = env.portfolio_state(portfolios[i]);
                    assert_eq!(
                        (
                            account.capital.get(),
                            account.last_fee_slot.get(),
                            account.pnl.get()
                        ),
                        (deposits[i], START, 0)
                    );
                    assert!(!has_active_leg_for_asset(&account, 0));
                    assert!(health_cert(&account).cert_funding_epoch < pending.funding_epoch);
                }

                let tracked = [
                    env.market,
                    portfolios[0],
                    portfolios[1],
                    keeper,
                    env.vault,
                    env.mint,
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
                ];
                let snapshot = |env: &V16CuEnv| tracked.map(|key| env.svm.get_account(&key));
                let untouched = [
                    keeper,
                    env.vault,
                    env.mint,
                    tokens[0],
                    tokens[1],
                    owners[0].pubkey(),
                    owners[1].pubkey(),
                    keeper_owner.pubkey(),
                    env.admin.pubkey(),
                    env.vault_authority,
                    matcher,
                    delegate,
                ];
                let untouched_before = untouched.map(|key| env.svm.get_account(&key));

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
                let submit = |env: &mut V16CuEnv, ix: Instruction| {
                    let mut signers = vec![&env.payer];
                    for owner in &owners {
                        if ix
                            .accounts
                            .iter()
                            .any(|meta| meta.pubkey == owner.pubkey() && meta.is_signer)
                        {
                            signers.push(owner);
                        }
                    }
                    env.svm.expire_blockhash();
                    let tx = Transaction::new_signed_with_payer(
                        &[heap_ix(), cu_ix(), ix],
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    let mut keys = tracked.to_vec();
                    keys.extend(tx.message.account_keys.iter().copied());
                    keys.sort_unstable();
                    keys.dedup();
                    let mut before: Vec<_> =
                        keys.iter().map(|key| env.svm.get_account(key)).collect();
                    let fee = u64::from(tx.message.header.num_required_signatures)
                        * FeeStructure::default().lamports_per_signature;
                    let result = env.svm.send_transaction(tx);
                    if result.is_err() {
                        let payer = keys
                            .iter()
                            .position(|key| *key == env.payer.pubkey())
                            .unwrap();
                        before[payer].as_mut().unwrap().lamports -= fee;
                        assert_eq!(keys.iter().map(|key| env.svm.get_account(key)).collect::<Vec<_>>(), before,
                            "{label}: complete transaction-account rollback including the runtime fee");
                    }
                    result
                };
                let census = |env: &V16CuEnv| {
                    let group = env.market_state().1;
                    assert_eq!(group.current_slot, ADMISSION);
                    assert_eq!(
                        (group.assets[1].f_long_num, group.assets[1].f_short_num),
                        (pending.assets[1].f_long_num, pending.assets[1].f_short_num)
                    );
                    assert_eq!(
                        (
                            group.assets[1].effective_price,
                            group.assets[1].raw_oracle_target_price
                        ),
                        (PRICE, target)
                    );
                    let profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        1,
                    )
                    .unwrap();
                    assert_eq!(
                        u64::from(profile.price_move_remainder_bps_num),
                        PRICE * CAP_BPS * (ADMISSION - START)
                    );
                    let accounts =
                        [portfolios[0], portfolios[1], keeper].map(|key| env.portfolio_state(key));
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
                };
                let check = |env: &V16CuEnv, admitted: bool| {
                    census(env);
                    let group = env.market_state().1;
                    assert_eq!(group.c_tot, principal.iter().sum::<u128>());
                    assert_eq!(group.vault, deposits.iter().sum::<u128>());
                    assert_eq!(group.pnl_pos_tot, claim);
                    assert_eq!(group.insurance, 2 * FEE);
                    assert_eq!(
                        &group.insurance_domain_budget[..2],
                        &[2 * (FEE / 2), 2 * (FEE - FEE / 2)]
                    );
                    assert!(group.insurance_domain_budget[2..]
                        .iter()
                        .all(|&amount| amount == 0));
                    let certs = std::array::from_fn::<_, 2, _>(|i| {
                        let account = env.portfolio_state(portfolios[i]);
                        assert_eq!(
                            account.capital.get(),
                            principal[i],
                            "{label}: principal {i}"
                        );
                        assert_eq!(
                            account.pnl.get(),
                            if i == debtor { 0 } else { claim as i128 },
                            "{label}: claim {i}"
                        );
                        assert_eq!(
                            (account.last_fee_slot.get(), account.fee_credits.get()),
                            (ADMISSION, 0)
                        );
                        assert!(assert_current_certificate_matches_independent(
                            &label, &group, &account
                        )
                        .unwrap());
                        let cert = health_cert(&account);
                        if i == debtor {
                            assert_eq!(cert.certified_equity, principal[i] as i128);
                            assert_eq!(
                                cert.certified_initial_req,
                                requirement(OLD_SIZE)
                                    + if admitted { requirement(NEW_SIZE) } else { 0 }
                            );
                            assert_eq!(cert.certified_maintenance_req, cert.certified_initial_req);
                            assert_eq!(cert.certified_liq_deficit, 0);
                        }
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(&account)),
                            if admitted { 2 } else { 1 }
                        );
                        for (asset, size) in
                            [(1, OLD_SIZE), (0, if admitted { NEW_SIZE } else { 0 })]
                        {
                            if size != 0 {
                                assert_eq!(
                                    active_leg_for_asset(&account, asset).basis_pos_q,
                                    if asset == 1 {
                                        if i == 0 {
                                            old_signed_size
                                        } else {
                                            -old_signed_size
                                        }
                                    } else if i == 0 {
                                        size
                                    } else {
                                        -size
                                    }
                                );
                            }
                        }
                        cert
                    });
                    assert_eq!(
                        untouched.map(|key| env.svm.get_account(&key)),
                        untouched_before
                    );
                    for (asset, size) in [(1, OLD_SIZE), (0, if admitted { NEW_SIZE } else { 0 })] {
                        assert_eq!(
                            (
                                group.assets[asset].oi_eff_long_q,
                                group.assets[asset].oi_eff_short_q
                            ),
                            (size as u128, size as u128)
                        );
                    }
                    certs
                };
                census(&env);
                if explicit {
                    for i in [debtor, 1 - debtor] {
                        peak_cu = peak_cu.max(env.crank(
                            portfolios[i],
                            ProgInstruction::PermissionlessCrank {
                                now_slot: ADMISSION,
                                observations: crank_observations_for_assets(&[0, 1]),
                            },
                        ));
                        census(&env);
                    }
                    // Booking the peer's claim may invalidate the earlier certificate.
                    for portfolio in portfolios {
                        let group = env.market_state().1;
                        let account = env.portfolio_state(portfolio);
                        if !assert_current_certificate_matches_independent(&label, &group, &account)
                            .unwrap()
                        {
                            peak_cu = peak_cu.max(env.crank(
                                portfolio,
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: ADMISSION,
                                    observations: crank_observations_for_assets(&[0, 1]),
                                },
                            ));
                        }
                    }
                    check(&env, false);
                }
                let before = snapshot(&env);
                let ix = trade(&env, route, NEW_SIZE + 1);
                let failure = submit(&mut env, ix)
                    .expect_err("new asset must fit equity after both accrued liabilities");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32)
                    ),
                    "{label}: {failure:?}"
                );
                peak_cu = peak_cu.max(failure.meta.compute_units_consumed);
                assert_eq!(
                    snapshot(&env),
                    before,
                    "{label}: funding/fee/certificate/matcher rollback"
                );
                let ix = trade(&env, retry_route, NEW_SIZE);
                let meta =
                    submit(&mut env, ix).expect("other route admits exact post-liability boundary");
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                let certs = check(&env, true);
                assert_eq!(
                    certs[debtor].certified_equity,
                    certs[debtor].certified_initial_req as i128
                );
                if let Some(expected) = reference {
                    assert_eq!(certs, expected, "{label}: settlement and route equivalence");
                } else {
                    reference = Some(certs);
                }

                let close = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[0].pubkey(), true),
                        AccountMeta::new(owners[1].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ],
                    data: env
                        .batch_trade_no_cpi_ix(
                            portfolios[0],
                            portfolios[1],
                            [(0, NEW_SIZE), (1, old_signed_size)]
                                .map(|(asset_index, size)| BatchTradeLeg {
                                    asset_index,
                                    market_id: env.asset_market_id(asset_index),
                                    size_q: -size,
                                    exec_price: PRICE,
                                    fee_bps: 0,
                                })
                                .to_vec(),
                        )
                        .encode(),
                };
                peak_cu = peak_cu.max(
                    submit(&mut env, close)
                        .expect("both owners close at the unchanged effective price")
                        .compute_units_consumed,
                );
                census(&env);
                for i in [debtor, 1 - debtor] {
                    let account = env.portfolio_state(portfolios[i]);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    assert_eq!(account.capital.get(), principal[i]);
                    let ix = Instruction {
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
                        submit(&mut env, ix)
                            .expect("senior principal exits ahead of the junior funding claim")
                            .compute_units_consumed,
                    );
                    assert_eq!(u128::from(env.token_amount(tokens[i])), principal[i]);
                    assert_eq!(env.portfolio_state(portfolios[i]).capital.get(), 0);
                    census(&env);
                }
                assert_eq!(env.market_state().1.c_tot, 0);
                assert_eq!(env.market_state().1.pnl_pos_tot, claim);
                assert_eq!(env.market_state().1.insurance, 2 * FEE);
                for i in 0..2 {
                    assert_eq!(env.portfolio_state(portfolios[i]).pnl.get(),
                        if i == debtor { 0 } else { claim as i128 },
                        "{label}: the unconverted claim retains its original owner after principal exit");
                }
                assert_eq!(env.token_amount(env.vault) as u128, 2 * FEE + debt);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_cu_within(
        "funding and maintenance admission with route rollback",
        peak_cu,
        600_000,
    );
    println!(
        "funding admission: {worlds} worlds, 16 rollbacks, 32 principal payouts, peak {peak_cu} CU"
    );
}
