//! Row 422, INV-020/024/036/045/061/081: retained Hybrid rewards across frozen custody.
//! Net-new beyond malformed reward tails and native terminal redemption: a real SPL
//! freeze follows a committed partial reward payout. A second liquidation succeeds
//! in isolation but its withdrawal suffix fails; thaw must admit the identical bundle
//! without losing the old receipt, duplicating the new receipt, or repricing discovery.
//! Public System/SPL/wrapper paths only; Clock, Pyth reports and signer SOL are inputs.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_freeze_authority;

fn payout(
    env: &V16CuEnv,
    owner: Pubkey,
    keeper: Pubkey,
    tokens: Pubkey,
    amount: u128,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper, false),
            AccountMeta::new(tokens, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(keeper, amount).encode(),
    }
}

#[test]
fn v16_program_frozen_partial_reward_payout_preserves_hybrid_liquidation_retry() {
    const SHARE: u128 = 3_333;
    const FIRST_PAYOUT: u128 = 1_777;
    const SECOND_PAYOUT: u128 = 1_111;
    const FINAL: u64 = 980_000;
    let first = ENTRY - ENTRY * 24 / 10_000;
    let second = first - first * 24 / 10_000;
    let mut reference = None;
    let mut rejected_bundles = 0;
    let mut liquidations = 0;
    let mut peak = 0;

    for freeze_destination in [false, true] {
        let freezer = Keypair::new();
        let params = V16CuMarketParams {
            max_abs_funding_e9_per_slot: 0,
            ..production_risk_params()
        };
        let mut env = inv018_public_spl_market_with_freeze_authority(
            6,
            params,
            params.max_portfolio_assets as usize,
            Some(freezer.pubkey()),
        );
        env.svm.airdrop(&freezer.pubkey(), 1_000_000_000).unwrap();
        set_test_clock(&mut env, 1, 100);
        env.update_liquidation_fee_policy_with_cu(SHARE as u16);
        let feed = [0x76; 32];
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
        let mint = env.svm.get_account(&env.mint);
        let supply = FUNDS.iter().map(|amount| u128::from(*amount)).sum::<u128>();
        let mut tracked = vec![
            env.market,
            env.mint,
            env.vault,
            initial,
            freezer.pubkey(),
            env.admin.pubkey(),
        ];
        tracked.extend(portfolios);
        tracked.extend(tokens);
        tracked.extend(owners.each_ref().map(Signer::pubkey));
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
        let bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
        let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, bps);
        let mut expected = FUNDS.map(i128::from);
        expected[2] -= (discovery / 2) as i128;
        expected[3] -= (discovery / 2) as i128;
        assert_eq!(values(&env, portfolios), expected);
        assert_eq!(env.market_state().1.insurance, discovery);
        assert_eq!(
            env.market_state().1.insurance_domain_budget_remaining_total,
            0
        );
        assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
        assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);

        let mut penalties = 0;
        let mut rewards = 0;
        let mut paid = 0;
        let mut budgets = [0u128; 2];
        let mut episodes = Vec::new();
        for (phase, slot, report_price, price) in [
            (0, 6, MARK, first),
            (1, 7, FINAL, second),
            (2, 14, FINAL, FINAL),
        ] {
            let now = 995 + slot as i64;
            set_test_clock(&mut env, slot, now);
            let report = env.set_pyth_price_with_conf(&feed, report_price as i64, -6, 0, now);
            tracked.push(report);
            let crank = observe(&env, target, owners[4].pubkey(), Some(report), Some(keeper));
            let mut liquidated = false;
            for _ in 0..6 {
                let before = env.market_state().1;
                let before_values = values(&env, portfolios);
                let cert = health_cert(&env.portfolio_state(target));
                if phase == 2
                    && before.assets[0].effective_price == price
                    && cert.certified_liq_deficit == 0
                    && census(&env, portfolios)[0]
                {
                    break;
                }
                let peers = [peer, trader_a, trader_b].map(|key| env.svm.get_account(&key));
                let bundle = phase == 1
                    && before.assets[0].effective_price == price
                    && cert.certified_liq_deficit > 0;
                let mut instructions = vec![crank.clone()];
                if bundle {
                    assert_eq!(paid, FIRST_PAYOUT);
                    assert!(FIRST_PAYOUT > FUNDS[4] as u128 && FIRST_PAYOUT < rewards);
                    assert!(FUNDS[4] as u128 + rewards - paid > SECOND_PAYOUT);
                    instructions.push(payout(
                        &env,
                        owners[4].pubkey(),
                        keeper,
                        tokens[4],
                        SECOND_PAYOUT,
                    ));
                    if freeze_destination {
                        let protocol =
                            [env.market, keeper, env.vault].map(|key| env.svm.get_account(&key));
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::freeze_account(
                                &spl_token::ID,
                                &tokens[4],
                                &env.mint,
                                &freezer.pubkey(),
                                &[],
                            )
                            .unwrap(),
                            &[&freezer],
                        )
                        .unwrap();
                        let frozen =
                            TokenAccount::unpack(&env.svm.get_account(&tokens[4]).unwrap().data)
                                .unwrap();
                        assert_eq!(frozen.state, spl_token::state::AccountState::Frozen);
                        assert_eq!(frozen.amount as u128, FIRST_PAYOUT);
                        assert_eq!(
                            [env.market, keeper, env.vault].map(|key| env.svm.get_account(&key)),
                            protocol
                        );
                        // submit proves the liquidation prefix succeeds alone and checks every
                        // tracked/compiled Account after the late custody rejection, including
                        // the paid prefix, retained reward, OI, fee budgets and withdrawal intent.
                        peak = peak.max(submit(
                            &mut env,
                            &owners[4],
                            &instructions,
                            &tracked,
                            Some((
                                3,
                                InstructionError::Custom(
                                    PercolatorError::InvalidTokenAccount as u32,
                                ),
                            )),
                        ));
                        rejected_bundles += 1;
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::thaw_account(
                                &spl_token::ID,
                                &tokens[4],
                                &env.mint,
                                &freezer.pubkey(),
                                &[],
                            )
                            .unwrap(),
                            &[&freezer],
                        )
                        .unwrap();
                        assert_eq!(
                            [env.market, keeper, env.vault].map(|key| env.svm.get_account(&key)),
                            protocol
                        );
                    }
                }
                // Reuse the exact encoded instructions after thaw, including the old intent.
                peak = peak.max(submit(&mut env, &owners[4], &instructions, &tracked, None));
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
                let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                if closed == 0 {
                    assert!(!bundle);
                    assert_eq!(
                        env.portfolio_state(keeper).capital.get(),
                        FUNDS[4] as u128 + rewards - paid
                    );
                    assert_eq!(after.insurance, before.insurance);
                    assert_eq!(
                        after.insurance_domain_budget,
                        before.insurance_domain_budget
                    );
                    continue;
                }
                assert!(phase < 2 && current[0] && cert.certified_liq_deficit > 0);
                assert!(closed < before.assets[0].oi_eff_long_q);
                assert_eq!(before.assets[0].effective_price, price);
                assert_eq!(
                    health_cert(&env.portfolio_state(target)).certified_liq_deficit,
                    0
                );
                // Only the engine's chosen quantity is observed. Price, fee rate and share
                // come from the history; neither a capital delta nor insurance seeds the book.
                let penalty = fee(closed, price, 5);
                let reward = penalty * SHARE / 10_000;
                assert!(reward > 0);
                for wrong_price in [ENTRY, MARK, report_price, ACCEPTED_PRINT, 900_000] {
                    assert_ne!(penalty, fee(closed, wrong_price, 5));
                }
                let withdrawal = if bundle { SECOND_PAYOUT } else { 0 };
                let mut expected = before_values;
                expected[0] -= penalty as i128;
                expected[4] += reward as i128 - withdrawal as i128;
                assert_eq!(values(&env, portfolios), expected);
                assert_eq!(env.portfolio_state(keeper).pnl.get(), 0);
                penalties += penalty;
                rewards += reward;
                paid += withdrawal;
                budgets[0] += (penalty - reward) / 2;
                budgets[1] += (penalty - reward).div_ceil(2);
                episodes.push((closed, penalty, reward));
                liquidations += 1;
                liquidated = true;
                if phase == 0 {
                    let withdrawal =
                        payout(&env, owners[4].pubkey(), keeper, tokens[4], FIRST_PAYOUT);
                    peak = peak.max(submit(&mut env, &owners[4], &[withdrawal], &tracked, None));
                    paid += FIRST_PAYOUT;
                }
                break;
            }
            assert_eq!(liquidated, phase < 2);
            for _ in 0..8 {
                for i in [1, 2, 3, 0] {
                    if !census(&env, portfolios)[i] {
                        let ix =
                            observe(&env, portfolios[i], owners[4].pubkey(), Some(report), None);
                        submit(&mut env, &owners[4], &[ix], &tracked, None);
                    }
                }
                if census(&env, portfolios)[..4].iter().all(|current| *current) {
                    break;
                }
            }
            assert!(census(&env, portfolios)[..4].iter().all(|current| *current));
            let group = env.market_state().1;
            assert_eq!(group.insurance, discovery + penalties - rewards);
            assert_eq!(&group.insurance_domain_budget[..2], &budgets);
            assert!(group.insurance_domain_budget[2..]
                .iter()
                .all(|value| *value == 0));
            assert_eq!(
                group.insurance_domain_budget_remaining_total,
                penalties - rewards
            );
            assert_eq!(
                group.insurance - group.insurance_domain_budget_remaining_total,
                discovery
            );
            assert_eq!(
                values(&env, portfolios)[4],
                (FUNDS[4] as u128 + rewards - paid) as i128
            );
            assert_eq!(env.token_amount(tokens[4]) as u128, paid);
            assert_eq!(group.vault, supply - paid);
            assert_eq!(env.token_amount(env.vault) as u128, group.vault);
            assert_eq!(env.svm.get_account(&env.mint), mint);
            submit(
                &mut env,
                &owners[4],
                &[crank],
                &tracked,
                Some((
                    2,
                    InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                )),
            );
        }
        assert_eq!(episodes.len(), 2);
        assert_eq!(paid, FIRST_PAYOUT + SECOND_PAYOUT);
        let remainder = FUNDS[4] as u128 + rewards - paid;
        assert!(remainder > 0);
        let withdrawal = payout(&env, owners[4].pubkey(), keeper, tokens[4], remainder);
        peak = peak.max(submit(&mut env, &owners[4], &[withdrawal], &tracked, None));
        paid += remainder;
        assert_eq!(values(&env, portfolios)[4], 0);
        assert_eq!(env.token_amount(tokens[4]) as u128, paid);
        assert!(tokens[..4].iter().all(|key| env.token_amount(*key) == 0));
        census(&env, portfolios);
        let group = env.market_state().1;
        assert_eq!(group.vault, supply - paid);
        assert_eq!(env.token_amount(env.vault) as u128, group.vault);
        assert_eq!(group.insurance, discovery + penalties - rewards);
        assert_eq!(env.svm.get_account(&env.mint), mint);
        let endpoint = (
            episodes,
            values(&env, portfolios),
            paid,
            group.insurance,
            group.insurance_domain_budget,
            group.vault,
        );
        if let Some(reference) = &reference {
            assert_eq!(
                &endpoint, reference,
                "freeze/retry cannot change any owner's final entitlement"
            );
        } else {
            reference = Some(endpoint);
        }
        eprintln!("frozen={freeze_destination} penalties={penalties} rewards={rewards} paid={paid} remainder={remainder}");
    }
    assert_eq!((liquidations, rejected_bundles), (4, 1));
    assert_cu_within("frozen reward bundle", peak, 2 * CRANK_CU_LIMIT);
    eprintln!(
        "INV-045 frozen custody: worlds=2 liquidations=4 late_rollbacks=1 payouts=6 peak={peak}"
    );
}
