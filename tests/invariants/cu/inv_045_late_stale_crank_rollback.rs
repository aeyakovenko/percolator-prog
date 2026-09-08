//! INV-045, reopening 422/425: a stale observation in a later instruction cannot
//! retain an earlier CPI's paid mark, fee debit, position change, or matcher write.
//! A fresh retry and opposite-direction no-CPI batch must preserve per-asset price
//! envelopes and charge only the accepted print, including the passive book.
//! Requires the checked-in auth matcher fixture's public spread configuration.
//! This is bounded public-wrapper evidence on the supplied SBF, not row closure.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const ENTRY: [u64; 2] = [1_000_000, 2_000_000];
const CAP_BPS: u64 = 100;
const MAX_DT: u64 = 3;
const BASE_BPS: u64 = 37;
const DEPOSIT: u128 = 100_000_000;

fn profiles(env: &V16CuEnv) -> [state::AssetOracleProfileV16; 2] {
    let account = env.svm.get_account(&env.market).unwrap();
    [0, 1].map(|i| state::read_asset_oracle_profile(&account.data, i).unwrap())
}

fn frame(env: &V16CuEnv, keys: &[Pubkey]) -> Vec<Option<Account>> {
    keys.iter().map(|key| env.svm.get_account(key)).collect()
}

fn values(env: &V16CuEnv, portfolios: [Pubkey; 4]) -> [i128; 4] {
    portfolios.map(|key| {
        let account = env.portfolio_state(key);
        i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
    })
}

// Independent bounded oracle: one POS_SCALE reduction, halflife one, fully funded
// fees. No production clamp, EWMA, externality, or fee helper is used here.
fn paid_move(
    old: u64,
    effective: u64,
    oi_units: u128,
    elapsed: u64,
    raw: u64,
) -> (u64, u128, u128) {
    let delta = old * CAP_BPS * elapsed.min(MAX_DT) / 10_000;
    let accepted = raw.clamp(old - delta, old + delta);
    assert_ne!(accepted, raw, "the raw print must be outside the envelope");
    let alpha = 10_000 * elapsed / (elapsed + 1);
    let movement = old.abs_diff(accepted) * alpha / 10_000;
    let mark = if accepted < old {
        old - movement
    } else {
        old + movement
    };
    assert!(0 < movement && movement <= delta);
    let externality = 2 * (oi_units * u128::from(old.max(effective))).max(u128::from(accepted));
    let move_bps = (u128::from(movement) * 10_000).div_ceil(u128::from(old));
    let movement_fee = (externality * move_bps).div_ceil(10_000);
    let base_fee = 2 * (u128::from(accepted) * u128::from(BASE_BPS)).div_ceil(10_000);
    let fee = (BASE_BPS..=10_000)
        .map(|bps| 2 * (u128::from(accepted) * u128::from(bps)).div_ceil(10_000))
        .find(|fee| *fee >= base_fee + movement_fee)
        .expect("fully funded mark fee fits the configured ceiling");
    assert!(fee - base_fee >= movement_fee);
    (mark, fee, base_fee)
}

fn crank_ix(env: &V16CuEnv, signer: Pubkey, portfolio: Pubkey, oracle: Pubkey) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new_readonly(oracle, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 1,
                },
            ],
        }
        .encode(),
    }
}

fn refresh_all(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolios: [Pubkey; 4],
    oracle: Pubkey,
) -> u64 {
    let mut max_cu = 0;
    for portfolio in portfolios {
        let ix = crank_ix(env, owner.pubkey(), portfolio, oracle);
        let before = env.market_state().1;
        let marks = profiles(env).map(|p| (p.mark_ewma_e6, p.mark_ewma_last_slot));
        let cu = env.send_crank_if_actionable(
            ProgInstruction::decode(&ix.data).unwrap(),
            ix.accounts,
            &[owner],
        );
        max_cu = max_cu.max(cu.unwrap_or(0));
        let after = env.market_state().1;
        for i in 0..2 {
            let a = before.assets[i];
            let b = after.assets[i];
            let elapsed = b.slot_last - a.slot_last;
            let envelope = a.fund_px_last * CAP_BPS * elapsed / 10_000;
            assert!(b.effective_price.abs_diff(a.effective_price) <= envelope);
        }
        assert_eq!(
            profiles(env).map(|p| (p.mark_ewma_e6, p.mark_ewma_last_slot)),
            marks
        );
        assert_eq!(
            after.insurance, before.insurance,
            "catchup must not collect another mark fee"
        );
    }
    max_cu
}

#[test]
fn v16_program_paid_cpi_then_stale_crank_rolls_back_before_fresh_batch_reversal() {
    let (first_mark, first_fee, first_base) = paid_move(ENTRY[0], ENTRY[0], 14, 4, 700_000);
    assert_eq!(first_mark, 976_000);
    let (second_mark, second_fee, second_base) = paid_move(first_mark, 980_000, 13, 1, 1_300_000);
    assert_eq!(second_mark, 980_880);
    let hybrid_fee = 2 * (u128::from(ENTRY[1]) * u128::from(BASE_BPS)).div_ceil(10_000);
    let mut outcomes = Vec::new();

    for rejected_prefix in [false, true] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets: 2,
            initial_price: ENTRY[0],
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            ..V16CuMarketParams::default()
        });
        set_test_clock(&mut env, 1, 100);
        env.configure_ewma_mark_with_cu(1, ENTRY[0], 1, 0);
        let feed = [0x45; 32];
        let initial = env.set_pyth_price_with_conf(&feed, ENTRY[1] as i64, -6, 0, 100);
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
            100,
            0,
        )
        .expect("public Hybrid configuration");
        let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        let sources: Vec<_> = (0..4)
            .map(|i| env.deposit(&owners[i], portfolios[i], DEPOSIT))
            .collect();
        // Ten passive units plus four trading units on each side, on both assets.
        for (a, b, units) in [(0, 1, 10), (2, 3, 4)] {
            let legs = (0..2)
                .map(|i| BatchTradeLeg {
                    asset_index: i as u16,
                    market_id: env.asset_market_id(i as u16),
                    size_q: units * POS_SCALE as i128,
                    exec_price: ENTRY[i],
                    fee_bps: 0,
                })
                .collect();
            env.svm.expire_blockhash();
            env.send(
                env.batch_trade_no_cpi_ix(portfolios[a], portfolios[b], legs),
                vec![
                    AccountMeta::new(owners[a].pubkey(), true),
                    AccountMeta::new(owners[b].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[a], false),
                    AccountMeta::new(portfolios[b], false),
                ],
                &[&owners[a], &owners[b]],
            )
            .expect("public initial positions");
        }
        env.update_trade_fee_policy_with_cu(BASE_BPS);
        let (matcher, context, delegate) =
            auth_matcher_for_lp_via_system_create(&mut env, &owners[3], portfolios[3]);
        // The authorized matcher emits ordinary 30% off-mark quotes, not zero or extremes.
        let mut spread = vec![4];
        spread.extend_from_slice(&3_000u64.to_le_bytes());
        spread.extend_from_slice(&3_000u64.to_le_bytes());
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            Instruction {
                program_id: matcher,
                accounts: vec![
                    AccountMeta::new_readonly(owners[3].pubkey(), true),
                    AccountMeta::new(context, false),
                ],
                data: spread,
            },
            &[&owners[3]],
        )
        .expect("owner configures matcher spread publicly");

        set_test_clock(&mut env, 4, 103);
        let mut max_crank_cu = refresh_all(&mut env, &owners[0], portfolios, initial);
        assert_eq!(
            profiles(&env).map(|p| (p.mark_ewma_e6, p.mark_ewma_last_slot)),
            [(ENTRY[0], 1), (ENTRY[1], 1)]
        );
        assert_eq!(
            [0, 1].map(|i| env.market_state().1.assets[i].slot_last),
            [4, 4]
        );
        assert_eq!(values(&env, portfolios), [DEPOSIT as i128; 4]);
        assert_eq!(env.market_state().1.insurance, 0);

        set_test_clock(&mut env, 5, 200);
        let hybrid = profiles(&env)[1];
        assert!(5 - hybrid.last_good_oracle_slot < hybrid.hybrid_soft_stale_slots);
        assert!(
            200 - hybrid.oracle_target_publish_time
                > i64::try_from(hybrid.max_staleness_secs).unwrap()
        );
        let fresh = env.set_pyth_price_with_conf(&feed, ENTRY[1] as i64, -6, 0, 200);
        let cpi = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(owners[2].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[2], false),
                AccountMeta::new(portfolios[3], false),
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new(context, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            data: env
                .trade_cpi_ix(
                    portfolios[2],
                    portfolios[3],
                    0,
                    -(POS_SCALE as i128),
                    BASE_BPS,
                    0,
                )
                .encode(),
        };
        let mut keys = vec![
            env.market,
            env.mint,
            env.vault,
            env.admin.pubkey(),
            matcher,
            context,
            delegate,
            initial,
            fresh,
        ];
        keys.extend(portfolios);
        keys.extend(owners.each_ref().map(Signer::pubkey));
        keys.extend(sources);
        let before = frame(&env, &keys);
        let matcher_before = env.svm.get_account(&context).unwrap();
        let request_seq = env.market_state().0.matcher_req_seq;
        let mut rejected_cu = 0;
        if rejected_prefix {
            let stale_crank = crank_ix(&env, owners[0].pubkey(), portfolios[0], initial);
            env.svm.expire_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[heap_ix(), cu_ix(), cpi.clone(), stale_crank],
                Some(&env.payer.pubkey()),
                &[&env.payer, &owners[2], &owners[0]],
                env.svm.latest_blockhash(),
            );
            let failure = env
                .svm
                .send_transaction(tx)
                .expect_err("late stale observation must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    3,
                    InstructionError::Custom(PercolatorError::OracleStale as u32)
                )
            );
            assert!(
                failure
                    .meta
                    .logs
                    .contains(&format!("Program {matcher} success")),
                "matcher CPI must have executed"
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", env.program_id))
                    .count(),
                1,
                "the paid wrapper instruction must succeed before the second instruction rejects"
            );
            rejected_cu = failure.meta.compute_units_consumed;
            assert_eq!(frame(&env, &keys), before, "all account bytes, owners, flags and lamports roll back; only the unrelated transaction fee payer is excluded");
        }

        // The exact retained CPI payload remains usable. Only the oracle account in
        // the second instruction changes, with the same authenticated slot and time.
        let fresh_crank = crank_ix(&env, owners[0].pubkey(), portfolios[0], fresh);
        env.svm.expire_blockhash();
        let retry_cu = send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![heap_ix(), cu_ix(), cpi, fresh_crank],
            &[&owners[2], &owners[0]],
        )
        .expect("paid CPI plus fresh crank progresses after exact rollback");
        let after = env.market_state().1;
        let p = profiles(&env);
        assert_eq!(p.map(|p| p.mark_ewma_e6), [first_mark, ENTRY[1]]);
        assert_eq!(p[0].mark_ewma_last_slot, 5);
        assert_eq!(p[1].last_good_oracle_slot, 5);
        assert_eq!(after.assets[0].raw_oracle_target_price, first_mark);
        assert_eq!(after.assets[0].effective_price, 990_000);
        assert_eq!(after.assets[1].effective_price, ENTRY[1]);
        assert_eq!(after.insurance, first_fee);
        assert_eq!(env.market_state().0.matcher_req_seq, request_seq + 1);
        let matcher_after = env.svm.get_account(&context).unwrap();
        assert_ne!(
            matcher_after, matcher_before,
            "successful single CPI really writes matcher context"
        );
        let matcher_return = percolator_prog::matcher_abi::read_matcher_return(
            &matcher_after.data[..percolator_prog::matcher_abi::MATCHER_RETURN_BYTES],
        )
        .unwrap();
        assert_eq!(matcher_return.exec_price_e6, 700_000);
        assert_eq!(
            [0, 1].map(|i| after.assets[i].oi_eff_long_q),
            [13 * POS_SCALE, 14 * POS_SCALE]
        );
        max_crank_cu = max_crank_cu.max(refresh_all(&mut env, &owners[0], portfolios, fresh));
        assert_eq!(
            values(&env, portfolios),
            [
                DEPOSIT as i128 - 100_000,
                DEPOSIT as i128 + 100_000,
                DEPOSIT as i128 - 30_000 - (first_fee / 2) as i128,
                DEPOSIT as i128 + 30_000 - (first_fee / 2) as i128
            ]
        );

        set_test_clock(&mut env, 6, 201);
        max_crank_cu = max_crank_cu.max(refresh_all(&mut env, &owners[0], portfolios, fresh));
        assert_eq!(env.market_state().1.assets[0].effective_price, 980_000);
        // Reverse the pending EWMA target through a two-asset no-CPI batch. The
        // fresh Hybrid leg must use its own committed price and only its base fee.
        let legs = (0..2)
            .map(|i| BatchTradeLeg {
                asset_index: i as u16,
                market_id: env.asset_market_id(i as u16),
                size_q: -(POS_SCALE as i128),
                exec_price: [1_300_000, 2_600_000][i],
                fee_bps: BASE_BPS,
            })
            .collect();
        env.svm.expire_blockhash();
        let batch_cu = env
            .send(
                env.batch_trade_no_cpi_ix(portfolios[2], portfolios[3], legs),
                vec![
                    AccountMeta::new(owners[2].pubkey(), true),
                    AccountMeta::new(owners[3].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[2], false),
                    AccountMeta::new(portfolios[3], false),
                ],
                &[&owners[2], &owners[3]],
            )
            .expect("freshly refreshed mixed-mode batch reduction");
        let total_fee = first_fee + second_fee + hybrid_fee;
        assert_eq!(
            profiles(&env).map(|p| p.mark_ewma_e6),
            [second_mark, ENTRY[1]]
        );
        assert_eq!(profiles(&env)[0].mark_ewma_last_slot, 6);
        assert_eq!(
            env.market_state().1.assets[0].raw_oracle_target_price,
            second_mark
        );
        assert_eq!(
            env.market_state().1.assets[0].effective_price,
            980_000,
            "trade stages, but does not retroactively accrue, the reversal"
        );
        assert_eq!(env.market_state().1.insurance, total_fee);

        set_test_clock(&mut env, 7, 202);
        max_crank_cu = max_crank_cu.max(refresh_all(&mut env, &owners[0], portfolios, fresh));
        let final_group = env.market_state().1;
        let actor_values = values(&env, portfolios);
        assert_eq!(final_group.assets[0].effective_price, second_mark);
        assert_eq!(final_group.assets[1].effective_price, ENTRY[1]);
        assert_eq!(
            actor_values,
            [
                DEPOSIT as i128 - 191_200,
                DEPOSIT as i128 + 191_200,
                DEPOSIT as i128 - 58_240 - (total_fee / 2) as i128,
                DEPOSIT as i128 + 58_240 - (total_fee / 2) as i128
            ]
        );
        assert!(191_200 <= first_fee - first_base + second_fee - second_base, "passive winner's gain is covered by collected movement fees, excluding both assets' base fees");
        assert_eq!(
            actor_values.iter().sum::<i128>() + total_fee as i128,
            4 * DEPOSIT as i128
        );
        assert_eq!(final_group.insurance, total_fee);
        assert_eq!(final_group.vault, 4 * DEPOSIT);
        assert_eq!(u128::from(env.token_amount(env.vault)), final_group.vault);
        for (i, units) in [(0, 12), (1, 13)] {
            assert_eq!(final_group.assets[i].oi_eff_long_q, units * POS_SCALE);
            assert_eq!(final_group.assets[i].oi_eff_short_q, units * POS_SCALE);
        }
        outcomes.push((
            actor_values,
            final_group.insurance,
            profiles(&env).map(|p| (p.mark_ewma_e6, p.mark_ewma_last_slot)),
            env.market_state().0.matcher_req_seq,
        ));
        eprintln!("INV-045 late stale crank rejected_prefix={rejected_prefix}: rejected CU={rejected_cu}, fresh CPI+crank CU={retry_cu}, batch CU={batch_cu}, max crank CU={max_crank_cu}, fees=[{first_fee}, {second_fee}, {hybrid_fee}]");
        assert_cu_within(
            "CPI plus observation transaction",
            retry_cu.max(rejected_cu),
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
        assert_cu_within(
            "mixed-mode no-CPI batch",
            batch_cu,
            MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        );
        assert_cu_within("two-asset public refresh", max_crank_cu, CRANK_CU_LIMIT);
    }
    assert_eq!(outcomes[0], outcomes[1], "late rejection cannot alter the fresh continuation's economics, marks, or matcher request sequence");
}
