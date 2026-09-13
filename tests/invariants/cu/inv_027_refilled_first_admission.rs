//! INV-027 / row 413: clipped maintenance leaves a self-rewarded flat account
//! that can be replenished, but the next elapsed interval must precede explicitly
//! refreshed first admission. Forgiven fees must not be resurrected or new fees omitted. INV-040
//! owns same-slot clipped-fee refill/withdrawal, without this later risk boundary.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use solana_sdk::fee::FeeStructure;

#[test]
fn v16_program_refilled_flat_account_pays_new_interval_before_first_admission() {
    const EXHAUSTED: u64 = 4;
    const NOW: u64 = 6;
    const SEED: u128 = 8;
    const SHARE_BPS: u16 = 3_333;
    const REWARD: u128 = SEED * SHARE_BPS as u128 / 10_000;
    const RETAINED: u128 = SEED - REWARD;
    const PEER: u128 = 300;
    const IM: u128 = 100;
    const NEW_FEE: u128 = (NOW - EXHAUSTED) as u128 * FEE_RATE;
    const PEER_FEE: u128 = (NOW - START) as u128 * FEE_RATE;
    const REFILL: u128 = IM + NEW_FEE - REWARD;
    const SUPPLY: u128 = SEED + PEER + REFILL;
    const SIZE: i128 = POS_SCALE as i128;
    const EXCESS: i128 = SIZE + 1;
    let margin = |size: i128| (size.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE);
    assert_eq!((NEW_FEE, PEER_FEE, REFILL, SUPPLY), (14, 35, 112, 420));
    assert_eq!((REWARD, RETAINED), (2, 6));
    assert!(SEED < (EXHAUSTED - START) as u128 * FEE_RATE);
    assert_eq!((margin(SIZE), margin(EXCESS)), (IM, IM + 1));
    assert!(REFILL >= margin(EXCESS));

    let mut peak_cu = 0;
    let mut rollbacks = 0;
    for thin in 0..2 {
        for atomic in [false, true] {
            let label = format!("refilled first admission/thin={thin}/atomic={atomic}");
            let initial = std::array::from_fn::<_, 2, _>(|i| if i == thin { SEED } else { PEER });
            let fees =
                std::array::from_fn::<_, 2, _>(|i| if i == thin { NEW_FEE } else { PEER_FEE });
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
            env.update_maintenance_fee_policy_with_cu(SHARE_BPS);
            let owners = [Keypair::new(), Keypair::new()];
            let portfolios = owners
                .each_ref()
                .map(|owner| public_portfolio(&mut env, owner));
            let tokens = std::array::from_fn::<_, 2, _>(|i| {
                public_deposit(&mut env, &owners[i], portfolios[i], initial[i])
            });
            let keeper_owner = Keypair::new();
            let keeper = public_portfolio(&mut env, &keeper_owner);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[thin],
                    &env.admin.pubkey(),
                    &[],
                    REFILL.try_into().unwrap(),
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
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

            let funded = portfolios.map(|key| env.svm.get_account(&key));
            for slot in START + 1..=EXHAUSTED {
                env.svm.warp_to_slot(slot);
                env.crank(
                    keeper,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                );
            }
            assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), funded);
            let custody = |env: &V16CuEnv| {
                [env.vault, env.mint, tokens[0], tokens[1]].map(|key| env.svm.get_account(&key))
            };
            let before_fee = custody(&env);
            env.sync_maintenance_fee_with_cu(portfolios[thin], Some(portfolios[thin]), EXHAUSTED);
            assert_eq!(custody(&env), before_fee);
            let exhausted = env.portfolio_state(portfolios[thin]);
            assert_eq!(
                (exhausted.capital.get(), exhausted.fee_credits.get()),
                (REWARD, 0)
            );
            assert_eq!(exhausted.last_fee_slot.get(), EXHAUSTED);
            assert_eq!(env.market_state().1.insurance, RETAINED);
            assert_eq!(env.svm.get_account(&portfolios[1 - thin]), funded[1 - thin]);
            let aged = portfolios.map(|key| env.svm.get_account(&key));
            for slot in EXHAUSTED + 1..=NOW {
                env.svm.warp_to_slot(slot);
                env.crank(
                    keeper,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                );
            }
            assert_eq!(portfolios.map(|key| env.svm.get_account(&key)), aged);

            let stable = [
                keeper,
                keeper_owner.pubkey(),
                env.mint,
                env.admin.pubkey(),
                owners[0].pubkey(),
                owners[1].pubkey(),
                env.vault_authority,
                env.program_id,
                spl_token::ID,
            ];
            let stable_before = stable.map(|key| env.svm.get_account(&key));
            let mut tracked = stable.to_vec();
            tracked.extend([
                env.market,
                portfolios[0],
                portfolios[1],
                env.vault,
                tokens[0],
                tokens[1],
            ]);
            let snapshot = |env: &V16CuEnv| {
                tracked
                    .iter()
                    .map(|key| env.svm.get_account(key))
                    .collect::<Vec<_>>()
            };
            let ids = portfolios.map(|key| env.portfolio_id(key));
            let sequences = portfolios.map(|key| env.portfolio_matcher_sequence(key));
            let check = |env: &V16CuEnv,
                         refilled: bool,
                         charged: [bool; 2],
                         open: bool,
                         paid: [u128; 2]| {
                let group = env.market_state().1;
                let accounts =
                    [portfolios[0], portfolios[1], keeper].map(|key| env.portfolio_state(key));
                let insurance = RETAINED
                    + (0..2)
                        .filter(|&i| charged[i])
                        .map(|i| fees[i])
                        .sum::<u128>();
                let vault =
                    SEED + PEER + if refilled { REFILL } else { 0 } - paid.iter().sum::<u128>();
                assert_eq!((group.mode, group.current_slot), (MarketModeV16::Live, NOW));
                assert_eq!(
                    (group.insurance, group.c_tot, group.vault),
                    (insurance, vault - insurance, vault),
                    "{label}"
                );
                assert_eq!(u128::from(env.token_amount(env.vault)), vault);
                assert_eq!(
                    (group.pnl_pos_tot, group.source_claim_bound_total_num),
                    (0, 0)
                );
                let long_budget = RETAINED / 2
                    + (0..2)
                        .filter(|&i| charged[i])
                        .map(|i| fees[i] / 2)
                        .sum::<u128>();
                assert_eq!(
                    &group.insurance_domain_budget[..2],
                    &[long_budget, insurance - long_budget]
                );
                assert!(group.insurance_domain_budget[2..].iter().all(|&x| x == 0));
                for i in 0..2 {
                    let account = &accounts[i];
                    let old_fee = if i == thin { RETAINED } else { 0 };
                    let new_deposit = if i == thin && refilled { REFILL } else { 0 };
                    let new_fee = if charged[i] { fees[i] } else { 0 };
                    let capital = initial[i] + new_deposit - old_fee - new_fee - paid[i];
                    let external = paid[i] + if i == thin && !refilled { REFILL } else { 0 };
                    assert_eq!(account.capital.get(), capital, "{label}: owner {i}");
                    assert_eq!(u128::from(env.token_amount(tokens[i])), external);
                    assert_eq!(
                        capital + external + old_fee + new_fee,
                        initial[i] + if i == thin { REFILL } else { 0 }
                    );
                    assert_eq!((account.fee_credits.get(), account.pnl.get()), (0, 0));
                    assert_eq!(
                        account.last_fee_slot.get(),
                        if charged[i] {
                            NOW
                        } else if i == thin {
                            EXHAUSTED
                        } else {
                            START
                        }
                    );
                    assert_eq!(env.portfolio_id(portfolios[i]), ids[i]);
                    assert_eq!(
                        env.portfolio_matcher_sequence(portfolios[i]),
                        sequences[i] + u64::from(i == thin && refilled) + u64::from(paid[i] != 0)
                    );
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
                        assert_eq!(
                            (
                                cert.certified_equity,
                                cert.certified_initial_req,
                                cert.certified_maintenance_req,
                                cert.certified_liq_deficit
                            ),
                            (capital as i128, IM, IM / 2, 0)
                        );
                    }
                }
                let asset = group.assets[0];
                assert_eq!(
                    (
                        asset.effective_price,
                        asset.raw_oracle_target_price,
                        asset.slot_last
                    ),
                    (PRICE, PRICE, NOW)
                );
                let oi = if open { SIZE as u128 } else { 0 };
                assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (oi, oi));
                assert_eq!(
                    (asset.stored_pos_count_long, asset.stored_pos_count_short),
                    (u64::from(open), u64::from(open))
                );
                assert_market_stock_census(
                    &label,
                    &group,
                    &env.svm.get_account(&env.market).unwrap().data,
                    &accounts,
                    vault,
                )
                .unwrap();
                assert_reservation_encumbrance_census(&label, &group, &accounts).unwrap();
                assert_eq!(stable.map(|key| env.svm.get_account(&key)), stable_before);
                assert_eq!(
                    Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
                        .unwrap()
                        .supply as u128,
                    SUPPLY
                );
                assert_eq!(
                    vault
                        + tokens
                            .iter()
                            .map(|key| u128::from(env.token_amount(*key)))
                            .sum::<u128>(),
                    SUPPLY
                );
            };
            check(&env, false, [false; 2], false, [0; 2]);

            let program_id = env.program_id;
            let ix = |instruction: ProgInstruction, accounts| Instruction {
                program_id,
                accounts,
                data: instruction.encode(),
            };
            let mut prefix = vec![ix(
                env.deposit_ix(portfolios[thin], REFILL),
                vec![
                    AccountMeta::new(owners[thin].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[thin], false),
                    AccountMeta::new(tokens[thin], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            )];
            for key in portfolios {
                prefix.push(ix(
                    ProgInstruction::SyncMaintenanceFee { now_slot: NOW },
                    vec![
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key, false),
                    ],
                ));
                prefix.push(ix(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: NOW,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(keeper_owner.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(key, false),
                    ],
                ));
            }
            let trade = |env: &V16CuEnv, size| {
                ix(
                    env.trade_no_cpi_ix(portfolios[0], portfolios[1], 0, size, PRICE, 0),
                    vec![
                        AccountMeta::new(owners[0].pubkey(), true),
                        AccountMeta::new(owners[1].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[0], false),
                        AccountMeta::new(portfolios[1], false),
                    ],
                )
            };
            let submit =
                |env: &mut V16CuEnv,
                 instructions: Vec<Instruction>,
                 rejected: Option<(usize, InstructionError, [usize; 2])>| {
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
                    let before = tracked
                        .iter()
                        .chain(&tx.message.account_keys)
                        .map(|key| (*key, env.svm.get_account(key)))
                        .collect::<Vec<_>>();
                    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let result = env.svm.send_transaction(tx);
                    let meta = if let Some((index, error, successes)) = rejected {
                        let failure = result
                            .expect_err("liability boundary or late suffix rejects atomically");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError((index + 2) as u8, error),
                            "{label}: {failure:?}"
                        );
                        for (program, count) in
                            [program_id, spl_token::ID].into_iter().zip(successes)
                        {
                            assert_eq!(
                                failure
                                    .meta
                                    .logs
                                    .iter()
                                    .filter(|line| **line == format!("Program {program} success"))
                                    .count(),
                                count,
                                "{label}: executed prefix"
                            );
                        }
                        for (key, account) in before {
                            if key != env.payer.pubkey() {
                                assert_eq!(
                                    env.svm.get_account(&key),
                                    account,
                                    "{label}: complete Account rollback {key}"
                                );
                            }
                        }
                        failure.meta
                    } else {
                        result.expect("bounded public continuation")
                    };
                    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                    meta.compute_units_consumed
                };
            if !atomic {
                let mut charged = [false; 2];
                for (index, instruction) in prefix.iter().enumerate() {
                    peak_cu = peak_cu.max(submit(&mut env, vec![instruction.clone()], None));
                    if index == 1 || index == 3 {
                        charged[index / 2] = true;
                    }
                    check(&env, true, charged, false, [0; 2]);
                    if index == 2 || index == 4 {
                        let account = env.portfolio_state(portfolios[(index - 2) / 2]);
                        assert!(assert_current_certificate_matches_independent(
                            &label,
                            &env.market_state().1,
                            &account
                        )
                        .unwrap());
                        assert_eq!(
                            (
                                health_cert(&account).certified_equity,
                                health_cert(&account).certified_initial_req
                            ),
                            (account.capital.get() as i128, 0)
                        );
                    }
                }
            }
            let admission_prefix = if atomic { prefix } else { vec![] };
            let mut excessive = admission_prefix.clone();
            excessive.push(trade(&env, EXCESS));
            peak_cu = peak_cu.max(submit(
                &mut env,
                excessive,
                Some((
                    admission_prefix.len(),
                    InstructionError::Custom(PercolatorError::EngineInvalidConfig as u32),
                    [admission_prefix.len(), usize::from(atomic)],
                )),
            ));
            rollbacks += 1;
            check(&env, !atomic, [!atomic; 2], false, [0; 2]);

            // An ordinary SPL failure follows a successful exact-margin first open.
            // The unchanged deposit/fee/admission prefix must remain usable on retry.
            let mut exact = admission_prefix.clone();
            exact.push(trade(&env, SIZE));
            let mut late = exact.clone();
            late.push(
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &tokens[thin],
                    &env.vault,
                    &owners[thin].pubkey(),
                    &[],
                    u64::MAX,
                )
                .unwrap(),
            );
            peak_cu = peak_cu.max(submit(
                &mut env,
                late,
                Some((
                    exact.len(),
                    InstructionError::Custom(
                        spl_token::error::TokenError::InsufficientFunds as u32,
                    ),
                    [exact.len(), usize::from(atomic)],
                )),
            ));
            rollbacks += 1;
            check(&env, !atomic, [!atomic; 2], false, [0; 2]);
            peak_cu = peak_cu.max(submit(&mut env, exact, None));
            check(&env, true, [true; 2], true, [0; 2]);
            for key in portfolios {
                let before = snapshot(&env);
                env.svm.expire_blockhash();
                env.sync_maintenance_fee_with_cu(key, None, NOW);
                assert_eq!(
                    snapshot(&env),
                    before,
                    "{label}: no repeated or resurrected fee"
                );
            }
            let close = trade(&env, -SIZE);
            peak_cu = peak_cu.max(submit(&mut env, vec![close], None));
            check(&env, true, [true; 2], false, [0; 2]);
            let mut paid = [0; 2];
            for i in 0..2 {
                let amount = if i == thin { IM } else { PEER - PEER_FEE };
                let withdraw = ix(
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
                );
                peak_cu = peak_cu.max(submit(&mut env, vec![withdraw], None));
                paid[i] = amount;
                check(&env, true, [true; 2], false, paid);
            }
            assert_eq!(
                (env.market_state().1.c_tot, env.token_amount(env.vault)),
                (0, 55)
            );
        }
    }
    assert_eq!(rollbacks, 8);
    assert_cu_within(
        "refilled first admission including deposit/fee/refresh",
        peak_cu,
        600_000,
    );
    eprintln!("INV-027 refilled first admission: 4 worlds, {rollbacks} full rollbacks, 4 exact admissions, 8 owner payouts; peak CU={peak_cu}");
}
