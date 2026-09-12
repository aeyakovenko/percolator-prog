//! Row 413 / INV-027: first two-asset batch admission after elapsed maintenance
//! must fit the sum of individually rounded trade fees and both IM requirements.
//! Public fee/refresh prefixes are explicit; uncollected standalone opens stay OPEN.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

#[test]
fn v16_program_first_batch_admission_accounts_each_rounded_fee_after_maintenance() {
    const QUANTITIES: [i128; 2] = [
        (48 * POS_SCALE / 100) as i128,
        (49 * POS_SCALE / 100) as i128,
    ];
    const TRADE_BPS: u64 = 100;
    let notional = |q: i128| (q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    let fee = |n: u128| (n * u128::from(TRADE_BPS)).div_ceil(10_000);
    let notionals = QUANTITIES.map(notional);
    let fees = notionals.map(fee);
    let initial_req = notionals.iter().sum::<u128>();
    let trade_fee = fees.iter().sum::<u128>();
    let equity = initial_req + trade_fee;
    let excess_req = notional(QUANTITIES[0]) + notional(QUANTITIES[1] + 1);
    assert_eq!(
        (notionals, fees, initial_req, trade_fee),
        ([48, 49], [1, 1], 97, 2)
    );
    assert_eq!((equity, excess_req, fee(excess_req)), (99, 98, 1));
    assert_eq!(equity - trade_fee, initial_req);
    assert_eq!(equity - fee(excess_req), excess_req);
    assert_eq!(fee(notional(QUANTITIES[1] + 1)), fees[1]);

    let mut worlds = 0;
    let mut peak_cu = 0;
    for thin in 0..2 {
        for order in [[0, 1], [1, 0]] {
            for atomic in [false, true] {
                let label = format!("first batch fees/thin={thin}/order={order:?}/atomic={atomic}");
                let deposits =
                    std::array::from_fn::<_, 2, _>(|i| FEE + if i == thin { equity } else { 500 });
                let total = deposits.iter().sum::<u128>();
                let mut env = inv018_public_spl_market_with_params(
                    0,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        maintenance_fee_per_slot: FEE_RATE,
                        maintenance_margin_bps: 5_000,
                        initial_margin_bps: 10_000,
                        max_price_move_bps_per_slot: 500,
                        max_abs_funding_e9_per_slot: 0,
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
                let funded = portfolios.map(|key| env.svm.get_account(&key));
                for slot in START + 1..=ADMISSION {
                    env.svm.warp_to_slot(slot);
                    env.crank(
                        keeper,
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: crank_observations_for_assets(&[0, 1]),
                        },
                    );
                }
                assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), funded);
                for key in portfolios {
                    let account = env.portfolio_state(key);
                    assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                    assert_eq!(account.last_fee_slot.get(), START);
                    assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
                }
                let stable = [
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
                    env.program_id,
                    spl_token::ID,
                ];
                let stable_before = stable.map(|key| env.svm.get_account(&key));
                let mut tracked = stable.to_vec();
                tracked.extend([env.market, portfolios[0], portfolios[1]]);
                let snapshot = |env: &V16CuEnv| {
                    tracked
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>()
                };
                let prefix: Vec<_> = portfolios
                    .iter()
                    .flat_map(|&key| {
                        [
                            Instruction {
                                program_id: env.program_id,
                                accounts: vec![
                                    AccountMeta::new(env.market, false),
                                    AccountMeta::new(key, false),
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
                                    AccountMeta::new(key, false),
                                ],
                                data: ProgInstruction::PermissionlessCrank {
                                    now_slot: ADMISSION,
                                    observations: crank_observations_for_assets(&[0, 1]),
                                }
                                .encode(),
                            },
                        ]
                    })
                    .collect();
                let batch = |env: &V16CuEnv, quantities: [i128; 2], fee_bps| Instruction {
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
                            order
                                .map(|asset| BatchTradeLeg {
                                    asset_index: asset as u16,
                                    market_id: env.asset_market_id(asset as u16),
                                    size_q: quantities[asset],
                                    exec_price: PRICE,
                                    fee_bps,
                                })
                                .to_vec(),
                        )
                        .encode(),
                };
                let submit = |env: &mut V16CuEnv, instructions: Vec<Instruction>| {
                    let mut all = vec![heap_ix(), cu_ix()];
                    all.extend(instructions);
                    let mut signers = vec![&env.payer];
                    for signer in [&owners[0], &owners[1], &keeper_owner] {
                        if all.iter().any(|ix| {
                            ix.accounts
                                .iter()
                                .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
                        }) {
                            signers.push(signer);
                        }
                    }
                    env.svm.expire_blockhash();
                    let tx = Transaction::new_signed_with_payer(
                        &all,
                        Some(&env.payer.pubkey()),
                        &signers,
                        env.svm.latest_blockhash(),
                    );
                    tx.verify().unwrap();
                    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                    env.svm.send_transaction(tx)
                };
                if !atomic {
                    let result =
                        submit(&mut env, prefix.clone()).expect("public fee/refresh prefix");
                    peak_cu = peak_cu.max(result.compute_units_consumed);
                    let group = env.market_state().1;
                    assert_eq!(group.insurance, 2 * FEE);
                    for i in 0..2 {
                        let account = env.portfolio_state(portfolios[i]);
                        assert_eq!(account.capital.get(), deposits[i] - FEE);
                        assert_eq!(account.last_fee_slot.get(), ADMISSION);
                        assert!(percolator::active_bitmap_is_empty(active_bitmap(&account)));
                        assert!(assert_current_certificate_matches_independent(
                            &label, &group, &account
                        )
                        .unwrap());
                        assert_eq!(
                            health_cert(&account).certified_equity,
                            (deposits[i] - FEE) as i128
                        );
                        assert_eq!(health_cert(&account).certified_initial_req, 0);
                    }
                }

                // Both rounded fees must enter the admission decision, even though rounding
                // the combined notional once would leave enough equity for this larger batch.
                let before = snapshot(&env);
                let mut instructions = if atomic { prefix.clone() } else { vec![] };
                instructions.push(batch(&env, [QUANTITIES[0], QUANTITIES[1] + 1], TRADE_BPS));
                let error = submit(&mut env, instructions)
                    .expect_err("all per-leg fees precede IM admission");
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        if atomic { 6 } else { 2 },
                        InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32),
                    ),
                    "{label}: {error:?}"
                );
                assert_eq!(
                    snapshot(&env),
                    before,
                    "{label}: full economic Account rollback"
                );
                peak_cu = peak_cu.max(error.meta.compute_units_consumed);

                let mut instructions = if atomic { prefix } else { vec![] };
                instructions.push(batch(&env, QUANTITIES, TRADE_BPS));
                let accepted = submit(&mut env, instructions)
                    .expect("exact post-maintenance and per-leg-fee IM");
                peak_cu = peak_cu.max(accepted.compute_units_consumed);
                let group = env.market_state().1;
                let accounts =
                    [portfolios[0], portfolios[1], keeper].map(|key| env.portfolio_state(key));
                let insurance = 2 * (FEE + trade_fee);
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    (group.c_tot, group.insurance, group.vault),
                    (total - insurance, insurance, total)
                );
                assert_eq!(u128::from(env.token_amount(env.vault)), total);
                assert_eq!(
                    (group.pnl_pos_tot, group.source_claim_bound_total_num),
                    (0, 0)
                );
                assert_eq!(
                    &group.insurance_domain_budget[..4],
                    &[2 * (FEE / 2) + 1, 2 * (FEE - FEE / 2) + 1, 1, 1]
                );
                assert!(group.insurance_domain_budget[4..].iter().all(|&x| x == 0));
                for i in 0..2 {
                    let account = &accounts[i];
                    let capital = deposits[i] - FEE - trade_fee;
                    assert_eq!(
                        (account.capital.get(), account.last_fee_slot.get()),
                        (capital, ADMISSION)
                    );
                    assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
                    assert_eq!(
                        percolator::active_bitmap_count_ones(active_bitmap(account)),
                        2
                    );
                    assert!(assert_current_certificate_matches_independent(
                        &label, &group, account
                    )
                    .unwrap());
                    let cert = health_cert(account);
                    assert_eq!(cert.certified_equity, capital as i128);
                    assert_eq!(cert.certified_initial_req, initial_req);
                    assert_eq!(
                        cert.certified_maintenance_req,
                        notionals.map(|n| n.div_ceil(2)).iter().sum()
                    );
                    assert_eq!(cert.certified_liq_deficit, 0);
                    for (asset, quantity) in QUANTITIES.iter().enumerate() {
                        let leg = active_leg_for_asset(account, asset);
                        assert_eq!(leg.basis_pos_q, if i == 0 { *quantity } else { -*quantity });
                        assert_eq!(
                            leg.side,
                            if i == 0 {
                                SideV16::Long
                            } else {
                                SideV16::Short
                            }
                        );
                    }
                }
                assert_eq!(
                    health_cert(&accounts[thin]).certified_equity,
                    initial_req as i128
                );
                for (asset, quantity) in QUANTITIES.iter().enumerate() {
                    let state = group.assets[asset];
                    assert_eq!(
                        (state.effective_price, state.raw_oracle_target_price),
                        (PRICE, PRICE)
                    );
                    assert_eq!(
                        (state.oi_eff_long_q, state.oi_eff_short_q),
                        (quantity.unsigned_abs(), quantity.unsigned_abs())
                    );
                    assert_eq!(
                        (state.stored_pos_count_long, state.stored_pos_count_short),
                        (1, 1)
                    );
                }
                assert_market_stock_census(
                    &label,
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    total,
                )
                .unwrap();
                assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                assert_eq!(stable.map(|key| env.svm.get_account(&key)), stable_before);
                for portfolio in portfolios {
                    let before = snapshot(&env);
                    env.svm.expire_blockhash();
                    let cu = env.sync_maintenance_fee_with_cu(portfolio, None, ADMISSION);
                    peak_cu = peak_cu.max(cu);
                    assert_eq!(
                        snapshot(&env),
                        before,
                        "{label}: no second maintenance collection"
                    );
                }
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert_cu_within(
        "first batch fee boundary including fee/refresh prefix",
        peak_cu,
        600_000,
    );
    eprintln!("INV-027 first batch fee boundary: {worlds} worlds, 8 exact rejections, 8 exact admissions, 16 fee-sync no-ops; peak CU={peak_cu}");
}
