//! INV-045 / row 422: optional reward destinations cannot defer or replay a receipt.
//! Public construction, two Hybrid liquidations, malformed-tail rollback, actual
//! catchup and SPL payout. Only Clock, external reports and signer SOL are fixtures.

use super::*;

#[test]
fn v16_program_hybrid_reward_destination_retries_preserve_only_committed_receipts() {
    const SHARE: u128 = 3_333;
    const FINAL: u64 = 980_000;
    let first_price = ENTRY - ENTRY * 24 / 10_000;
    let second_price = first_price - first_price * 24 / 10_000;
    let mut reference = None;
    let mut peak_crank = 0;
    let mut peak_reject = 0;
    let mut peak_payout = 0;
    let mut rollbacks = 0;
    let mut liquidation_rollbacks = 0;
    let mut rewarded = 0;
    let mut omitted = 0;
    for destinations in [[false, false], [false, true], [true, false], [true, true]] {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_abs_funding_e9_per_slot: 0,
                ..production_risk_params()
            },
        );
        set_test_clock(&mut env, 1, 100);
        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
        let feed = [0x75; 32];
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
        let malformed = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &malformed,
            env.portfolio_account_len,
            env.program_id,
        );
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
        env.trade_asset_with_cu(
            0,
            &owners[0],
            target,
            &owners[1],
            peer,
            (100 * POS_SCALE) as i128,
            ENTRY,
            0,
        );
        let mut tracked = vec![
            env.market,
            env.mint,
            env.vault,
            env.admin.pubkey(),
            initial,
            malformed.pubkey(),
        ];
        tracked.extend(portfolios);
        tracked.extend(tokens);
        tracked.extend(owners.each_ref().map(Signer::pubkey));
        let custody = [env.mint, env.vault].map(|key| env.svm.get_account(&key));
        let malformed_before = env.svm.get_account(&malformed.pubkey());
        let supply = FUNDS.iter().map(|amount| u128::from(*amount)).sum::<u128>();

        set_test_clock(&mut env, 5, 1_000);
        let clock_only = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
        submit(&mut env, &owners[4], &[clock_only], &tracked, None);
        env.trade_asset_with_cu(
            0,
            &owners[2],
            trader_a,
            &owners[3],
            trader_b,
            POS_SCALE as i128,
            900_000,
            0,
        );
        let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
        let trade_bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
        let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, trade_bps);
        let mut initial_values = FUNDS.map(i128::from);
        initial_values[2] -= (discovery / 2) as i128;
        initial_values[3] -= (discovery / 2) as i128;
        assert_eq!(values(&env, portfolios), initial_values);
        assert_eq!(env.market_state().1.insurance, discovery);
        assert_eq!(
            env.market_state().1.insurance_domain_budget_remaining_total,
            0
        );
        census(&env, portfolios);

        let mut fees = 0;
        let mut receipts = 0;
        let mut skipped = 0;
        let mut budgets = [0u128; 2];
        let mut episodes = Vec::new();
        let mut final_bad = None;
        for (phase, slot, report_price, price) in [
            (0, 6, MARK, first_price),
            (1, 7, FINAL, second_price),
            (2, 14, FINAL, FINAL),
        ] {
            let now = 995 + slot as i64;
            set_test_clock(&mut env, slot, now);
            let report = env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, now);
            tracked.push(report);
            let destination = phase == 2 || destinations[phase];
            let valid = observe(
                &env,
                target,
                owners[4].pubkey(),
                Some(report),
                destination.then_some(keeper),
            );
            let bad = observe(
                &env,
                target,
                owners[4].pubkey(),
                Some(report),
                Some(malformed.pubkey()),
            );
            let mut liquidated = false;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_values = values(&env, portfolios);
                let before_cert = health_cert(&env.portfolio_state(target));
                if phase == 2
                    && before.assets[0].effective_price == price
                    && before_cert.certified_liq_deficit == 0
                    && census(&env, portfolios)[0]
                {
                    break;
                }
                // The complete valid prefix, including any fee/reward, must roll back
                // when the public uninitialized destination is supplied by the suffix.
                peak_reject = peak_reject.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone(), bad.clone()],
                    &tracked,
                    Some((
                        3,
                        InstructionError::Custom(PercolatorError::NotInitialized as u32),
                    )),
                ));
                rollbacks += 1;
                if before_cert.certified_liq_deficit > 0
                    && before.assets[0].effective_price == price
                {
                    for (wrong, error) in [
                        (Some(peer), PercolatorError::Unauthorized),
                        (Some(target), PercolatorError::InvalidInstruction),
                        (Some(keeper), PercolatorError::ExpectedWritable),
                    ] {
                        let mut invalid =
                            observe(&env, target, owners[4].pubkey(), Some(report), wrong);
                        if matches!(error, PercolatorError::ExpectedWritable) {
                            invalid.accounts.last_mut().unwrap().is_writable = false;
                        }
                        peak_reject = peak_reject.max(submit(
                            &mut env,
                            &owners[4],
                            &[invalid],
                            &tracked,
                            Some((2, InstructionError::Custom(error as u32))),
                        ));
                        rollbacks += 1;
                    }
                }
                let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                let keeper_before = env.portfolio_state(keeper);
                peak_crank = peak_crank.max(submit(
                    &mut env,
                    &owners[4],
                    &[valid.clone()],
                    &tracked,
                    None,
                ));
                let current = census(&env, portfolios);
                let (profile, after) = env.market_state();
                assert_eq!(after.assets[0].effective_price, price);
                assert_eq!(after.assets[0].raw_oracle_target_price, report_price);
                assert_eq!(
                    (
                        profile.last_good_oracle_slot,
                        profile.oracle_target_publish_time
                    ),
                    (slot, now)
                );
                assert_eq!(
                    [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key)),
                    peers
                );
                assert_eq!(
                    [env.mint, env.vault].map(|key| env.svm.get_account(&key)),
                    custody
                );
                assert_eq!(env.svm.get_account(&malformed.pubkey()), malformed_before);
                let keeper_after = env.portfolio_state(keeper);
                assert_eq!(keeper_after.pnl, keeper_before.pnl);
                assert_eq!(keeper_after.legs, keeper_before.legs);
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed == 0 {
                    assert_eq!(keeper_after, keeper_before);
                    assert_eq!(after.insurance, before.insurance);
                    assert_eq!(
                        after.insurance_domain_budget,
                        before.insurance_domain_budget
                    );
                    continue;
                }
                assert!(phase < 2 && current[0] && before_cert.certified_liq_deficit > 0);
                assert!(closed < before.assets[0].oi_eff_long_q);
                assert_eq!(before.assets[0].effective_price, price);
                assert_eq!(
                    health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                    0
                );
                let penalty = fee(closed, price, 5);
                let eligible = penalty * SHARE / 10_000;
                assert!(eligible > 0);
                for wrong_price in [ENTRY, MARK, report_price, ACCEPTED_PRINT, 900_000] {
                    assert_ne!(penalty, fee(closed, wrong_price, 5));
                }
                let receipt = if destination { eligible } else { 0 };
                let mut expected = before_values;
                expected[0] -= penalty as i128;
                expected[4] += receipt as i128;
                assert_eq!(values(&env, portfolios), expected);
                if !destination {
                    assert_eq!(keeper_after, keeper_before);
                    omitted += 1;
                } else {
                    rewarded += 1;
                }
                fees += penalty;
                receipts += receipt;
                skipped += eligible - receipt;
                budgets[0] += (penalty - receipt) / 2;
                budgets[1] += (penalty - receipt).div_ceil(2);
                episodes.push((closed, penalty, eligible));
                liquidation_rollbacks += 1;
                liquidated = true;
                break;
            }
            assert_eq!(
                liquidated,
                phase < 2,
                "two liquidations, no deferred catchup reward"
            );
            for _ in 0..8 {
                for i in [1, 2, 3, 0] {
                    if !census(&env, portfolios)[i] {
                        let ix =
                            observe(&env, portfolios[i], owners[4].pubkey(), Some(report), None);
                        peak_crank =
                            peak_crank.max(submit(&mut env, &owners[4], &[ix], &tracked, None));
                    }
                }
                if census(&env, portfolios)[..4].iter().all(|current| *current) {
                    break;
                }
            }
            assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
            let group = env.market_state().1;
            assert_eq!(group.insurance, discovery + fees - receipts);
            assert_eq!(&group.insurance_domain_budget[..2], &budgets);
            assert!(group.insurance_domain_budget[2..]
                .iter()
                .all(|value| *value == 0));
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                fees - receipts
            );
            assert_eq!(
                group.insurance - group.insurance_domain_budget_remaining_total,
                discovery
            );
            assert_eq!(
                values(&env, portfolios)[4],
                FUNDS[4] as i128 + receipts as i128
            );
            // Supplying or omitting the destination after completion cannot claim the
            // skipped share, replay a committed receipt, or change fee-domain stock.
            for reward_tail in [None, Some(keeper)] {
                let retry = observe(&env, target, owners[4].pubkey(), Some(report), reward_tail);
                peak_reject = peak_reject.max(submit(
                    &mut env,
                    &owners[4],
                    &[retry],
                    &tracked,
                    Some((
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                    )),
                ));
                rollbacks += 1;
            }
            final_bad = Some(bad);
        }
        assert_eq!(episodes.len(), 2);
        assert_eq!(
            receipts + skipped,
            episodes.iter().map(|entry| entry.2).sum::<u128>()
        );
        let payout = FUNDS[4] as u128 + receipts;
        let mut expected_after_payout = values(&env, portfolios);
        expected_after_payout[4] -= payout as i128;
        let withdrawal = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(owners[4].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(keeper, false),
                AccountMeta::new(tokens[4], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env.withdraw_ix(keeper, payout).encode(),
        };
        peak_reject = peak_reject.max(submit(
            &mut env,
            &owners[4],
            &[withdrawal.clone(), final_bad.unwrap()],
            &tracked,
            Some((
                3,
                InstructionError::Custom(PercolatorError::NotInitialized as u32),
            )),
        ));
        rollbacks += 1;
        peak_payout = peak_payout.max(submit(&mut env, &owners[4], &[withdrawal], &tracked, None));
        assert_eq!(values(&env, portfolios), expected_after_payout);
        assert_eq!(expected_after_payout[4], 0);
        assert_eq!(env.token_amount(tokens[4]) as u128, payout);
        assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
        assert_eq!(env.svm.get_account(&env.mint), custody[0]);
        census(&env, portfolios);
        let group = env.market_state().1;
        assert_eq!(group.vault, supply - payout);
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        assert_eq!(group.insurance, discovery + fees - receipts);
        assert_eq!(&group.insurance_domain_budget[..2], &budgets);
        let owner_values = values(&env, portfolios);
        let claims = u128::try_from(owner_values.iter().sum::<i128>()).unwrap();
        let residual = group.vault.checked_sub(claims + group.insurance).unwrap();
        assert_eq!(
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q
        );
        // Destination choices change only who holds the eligible share. Add actual
        // payout back to insurance/custody to compare the complete economic endpoint.
        let endpoint = (
            episodes.clone(),
            owner_values,
            group.insurance + receipts,
            group.vault + payout,
            residual,
        );
        if let Some(reference) = &reference {
            assert_eq!(
                &endpoint, reference,
                "omitting a destination cannot change any target/peer entitlement"
            );
        } else {
            reference = Some(endpoint);
        }
        eprintln!("destination schedule={destinations:?} episodes={episodes:?} receipts={receipts} skipped={skipped} budgets={budgets:?} payout={payout} residual={residual}");
    }
    assert_eq!((rewarded, omitted, liquidation_rollbacks), (4, 4, 8));
    assert_eq!(rollbacks, 72);
    assert_cu_within("destination retry crank", peak_crank, CRANK_CU_LIMIT);
    assert_cu_within(
        "destination retry rejected bundle",
        peak_reject,
        2 * CRANK_CU_LIMIT,
    );
    assert_cu_within("destination retry payout", peak_payout, CUSTODY_CU_LIMIT);
    eprintln!("INV-045 destinations: worlds=4 rewarded={rewarded} omitted={omitted} exact_rollbacks={rollbacks} liquidation_rollbacks={liquidation_rollbacks} payouts=4 CU crank={peak_crank} rejection={peak_reject} payout={peak_payout}");
}
