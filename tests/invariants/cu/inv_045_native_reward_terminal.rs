//! INV-045 / row422: optional Hybrid receipts across native sync, unwrap and
//! resolved replay. Public economic construction; the existing native genesis,
//! external Pyth reports, Clock, signer SOL and program loading are harness inputs.

use super::*;
use crate::inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;

const DONATION: u64 = 37;

fn native_fund(env: &mut V16CuEnv, owner: &Keypair, amount: u64) -> (Pubkey, Pubkey) {
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let portfolio = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
        ],
        &[owner],
    )
    .unwrap();
    env.portfolios.push(portfolio.pubkey());
    let tokens = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![
            system_instruction::transfer(&owner.pubkey(), &tokens, amount),
            spl_token::instruction::sync_native(&spl_token::ID, &tokens).unwrap(),
        ],
        &[owner],
    )
    .unwrap();
    env.send(
        env.deposit_ix(portfolio.pubkey(), amount.into()),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio.pubkey(), false),
            AccountMeta::new(tokens, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .unwrap();
    (portfolio.pubkey(), tokens)
}

#[derive(Default)]
struct Meter {
    peak: u64,
    rollback_peak: u64,
    rollbacks: usize,
}

impl Meter {
    fn send(
        &mut self,
        env: &mut V16CuEnv,
        signer: &Keypair,
        instructions: &[Instruction],
        tracked: &[Pubkey],
        rejection: Option<(u8, InstructionError)>,
    ) {
        // Check arbitrary-length successful prefixes before their failing suffix.
        env.svm.expire_blockhash();
        if let Some((index, _)) = &rejection {
            if *index > 2 {
                let mut prefix = vec![heap_ix(), cu_ix()];
                prefix.extend_from_slice(&instructions[..usize::from(*index) - 2]);
                let mut signers = vec![&env.payer];
                if prefix
                    .iter()
                    .flat_map(|ix| &ix.accounts)
                    .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
                {
                    signers.push(signer);
                }
                let tx = Transaction::new_signed_with_payer(
                    &prefix,
                    Some(&env.payer.pubkey()),
                    &signers,
                    env.svm.latest_blockhash(),
                );
                env.svm
                    .simulate_transaction(tx.into())
                    .expect("valid prefix");
            }
        }
        let mut keys = tracked.to_vec();
        keys.push(env.payer.pubkey());
        keys.extend(
            instructions
                .iter()
                .flat_map(|ix| ix.accounts.iter().map(|m| m.pubkey)),
        );
        keys.sort_unstable();
        keys.dedup();
        let lamports = |env: &V16CuEnv| {
            keys.iter()
                .filter_map(|k| env.svm.get_account(k))
                .map(|a| u128::from(a.lamports))
                .sum::<u128>()
        };
        let before = lamports(env);
        let signed = instructions
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|m| m.is_signer && m.pubkey == signer.pubkey());
        let fee =
            (1 + u128::from(signed)) * u128::from(FeeStructure::default().lamports_per_signature);
        let failed = rejection.is_some();
        let cu = submit_with_cu_limit(env, signer, instructions, tracked, rejection, ACTION_CU);
        assert_eq!(
            lamports(env),
            before - fee,
            "native lamports conserve including exact network fee"
        );
        self.peak = self.peak.max(cu);
        if failed {
            self.rollback_peak = self.rollback_peak.max(cu);
            self.rollbacks += 1;
        }
    }

    fn abort(
        &mut self,
        env: &mut V16CuEnv,
        signer: &Keypair,
        prefix: &[Instruction],
        tracked: &[Pubkey],
    ) {
        let mut instructions = prefix.to_vec();
        instructions.push(Instruction {
            program_id: solana_sdk::system_program::ID,
            accounts: vec![],
            data: vec![255],
        });
        self.send(
            env,
            signer,
            &instructions,
            tracked,
            Some((
                (instructions.len() + 1) as u8,
                InstructionError::InvalidInstructionData,
            )),
        );
    }
}

fn payout(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    tokens: Pubkey,
    amount: Option<u128>,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner, amount.is_some()),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(tokens, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: amount
            .map_or(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                |amount| env.withdraw_ix(portfolio, amount),
            )
            .encode(),
    }
}

fn sync(tokens: Pubkey) -> Instruction {
    spl_token::instruction::sync_native(&spl_token::ID, &tokens).unwrap()
}

fn unwrap(tokens: Pubkey, owner: Pubkey) -> Instruction {
    spl_token::instruction::close_account(&spl_token::ID, &tokens, &owner, &owner, &[]).unwrap()
}

fn assert_unwrapped(env: &V16CuEnv, tokens: Pubkey) {
    assert!(env.svm.get_account(&tokens).is_none_or(|account| {
        account.lamports == 0 && account.data.iter().all(|byte| *byte == 0)
    }));
}

fn native_balance(env: &V16CuEnv, tokens: Pubkey, unsynced: u64) -> u128 {
    let account = env.svm.get_account(&tokens).unwrap();
    let token = TokenAccount::unpack(&account.data).unwrap();
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    assert_eq!(account.owner, spl_token::ID);
    assert_eq!(token.mint, spl_token::native_mint::ID);
    assert_eq!(token.is_native, COption::Some(rent));
    assert_eq!(account.lamports, rent + token.amount + unsynced);
    token.amount.into()
}

#[test]
fn v16_program_native_hybrid_omitted_rewards_stay_unclaimed_through_sync_unwrap_and_terminal_replay(
) {
    let first = ENTRY - ENTRY * 24 / 10_000;
    let second = first - first * 24 / 10_000;
    let supply = FUNDS.iter().map(|v| u128::from(*v)).sum::<u128>();
    let mut meter = Meter::default();
    let mut trade_peak = 0;
    let mut liquidations = 0;
    let mut redemptions = 0;
    let mut provenance_reference = None;
    for mask in 0..4 {
        let mut reference = None;
        for early in [false, true] {
            for reverse in [false, true] {
                let mut env = inv081_public_native_market_with_params(
                    1,
                    V16CuMarketParams {
                        max_abs_funding_e9_per_slot: 0,
                        ..production_risk_params()
                    },
                );
                set_test_clock(&mut env, 1, 100);
                env.update_liquidation_fee_policy_with_cu(SHARE as u16);
                let feed = [0x6d; 32];
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
                let funded =
                    std::array::from_fn::<_, 5, _>(|i| native_fund(&mut env, &owners[i], FUNDS[i]));
                let portfolios = funded.map(|p| p.0);
                let tokens = funded.map(|p| p.1);
                let [target, peer, trader_a, trader_b, keeper] = portfolios;
                let rent = env
                    .svm
                    .minimum_balance_for_rent_exemption(TokenAccount::LEN);
                let keeper_sol = env.svm.get_account(&owners[4].pubkey()).unwrap().lamports;
                let mint = env.svm.get_account(&env.mint);
                let mut tracked =
                    vec![env.market, env.mint, env.vault, env.admin.pubkey(), initial];
                tracked.extend(portfolios);
                tracked.extend(tokens);
                tracked.extend(owners.each_ref().map(Signer::pubkey));
                let mut providers = vec![(initial, env.svm.get_account(&initial))];
                trade_peak = trade_peak.max(env.trade_asset_with_cu(
                    0,
                    &owners[0],
                    target,
                    &owners[1],
                    peer,
                    (100 * POS_SCALE) as i128,
                    ENTRY,
                    0,
                ));
                set_test_clock(&mut env, 5, 1_000);
                let advance = observe(&env, keeper, owners[4].pubkey(), Some(initial), None);
                meter.send(&mut env, &owners[4], &[advance], &tracked, None);
                trade_peak = trade_peak.max(env.trade_asset_with_cu(
                    0,
                    &owners[2],
                    trader_a,
                    &owners[3],
                    trader_b,
                    POS_SCALE as i128,
                    900_000,
                    0,
                ));
                let required = (2 * 100 * u128::from(ENTRY) * 77).div_ceil(10_000);
                let bps = (required * 10_000).div_ceil(2 * u128::from(ACCEPTED_PRINT));
                let discovery = 2 * fee(POS_SCALE, ACCEPTED_PRINT, bps);
                let mut expected = FUNDS.map(i128::from);
                expected[2] -= (discovery / 2) as i128;
                expected[3] -= (discovery / 2) as i128;
                assert_eq!(values(&env, portfolios), expected);
                assert_eq!(env.market_state().1.assets[0].effective_price, ENTRY);
                assert_eq!(env.market_state().1.assets[0].raw_oracle_target_price, MARK);
                let admin = env.admin.insecure_clone();
                let donation = system_instruction::transfer(&admin.pubkey(), &tokens[4], DONATION);
                let economic = [env.market, keeper, env.vault].map(|k| env.svm.get_account(&k));
                meter.send(&mut env, &admin, &[donation], &tracked, None);
                assert_eq!(
                    [env.market, keeper, env.vault].map(|k| env.svm.get_account(&k)),
                    economic
                );
                assert_eq!(native_balance(&env, tokens[4], DONATION), 0);
                let (mut fees, mut rewards, mut skipped, mut early_paid) = (0, 0, 0, 0);
                let mut budgets = [0; 2];
                let mut episodes = Vec::new();
                for (phase, slot, raw, price) in [
                    (0, 6, MARK, first),
                    (1, 7, FINAL, second),
                    (2, 14, FINAL, FINAL),
                ] {
                    let now = 995 + slot as i64;
                    set_test_clock(&mut env, slot, now);
                    let report = env.set_pyth_price_with_conf(&feed, raw as i64, -6, 0, now);
                    tracked.push(report);
                    providers.push((report, env.svm.get_account(&report)));
                    let recipient = phase < 2 && mask & (1 << phase) != 0;
                    let crank = observe(
                        &env,
                        target,
                        owners[4].pubkey(),
                        Some(report),
                        recipient.then_some(keeper),
                    );
                    let mut liquidated = false;
                    for _ in 0..6 {
                        let before = env.market_state().1;
                        if phase == 2
                            && before.assets[0].effective_price == FINAL
                            && census(&env, portfolios)[0]
                            && health_cert(&env.portfolio_state(target)).certified_liq_deficit == 0
                        {
                            break;
                        }
                        let before_values = values(&env, portfolios);
                        let before_keeper = env.portfolio_state(keeper);
                        let foreign = [peer, trader_a, trader_b, env.vault, tokens[4]]
                            .map(|k| env.svm.get_account(&k));
                        // Sync has a real effect until early unwrap, and must also roll back.
                        meter.abort(
                            &mut env,
                            &owners[4],
                            &[sync(tokens[4]), crank.clone()],
                            &tracked,
                        );
                        meter.send(&mut env, &owners[4], &[crank.clone()], &tracked, None);
                        assert_eq!(
                            [peer, trader_a, trader_b, env.vault, tokens[4]]
                                .map(|k| env.svm.get_account(&k)),
                            foreign
                        );
                        let after = env.market_state().1;
                        assert_eq!(after.assets[0].effective_price, price);
                        assert_eq!(after.assets[0].raw_oracle_target_price, raw);
                        let profile = state::read_asset_oracle_profile(
                            &env.svm.get_account(&env.market).unwrap().data,
                            0,
                        )
                        .unwrap();
                        assert_eq!(profile.oracle_target_publish_time, now);
                        assert_eq!(profile.last_good_oracle_slot, slot);
                        let closed = before.assets[0].oi_eff_long_q - after.assets[0].oi_eff_long_q;
                        let penalty = fee(closed, price, 5);
                        let eligible = penalty * SHARE / 10_000;
                        let reward = if recipient { eligible } else { 0 };
                        let mut expected_keeper = before_keeper;
                        expected_keeper.capital =
                            percolator::V16PodU128::new(before_keeper.capital.get() + reward);
                        if reward != 0 {
                            expected_keeper.health_cert.valid = 0;
                        }
                        assert_eq!(env.portfolio_state(keeper), expected_keeper);
                        if closed != 0 {
                            assert!(phase < 2 && eligible > 0 && closed < 100 * POS_SCALE);
                            assert_eq!(before.assets[0].effective_price, price);
                            for wrong in [ENTRY, MARK, raw, ACCEPTED_PRINT, 900_000] {
                                assert_ne!(penalty, fee(closed, wrong, 5));
                            }
                            let mut expected = before_values;
                            expected[0] -= penalty as i128;
                            expected[4] += reward as i128;
                            assert_eq!(values(&env, portfolios), expected);
                            fees += penalty;
                            rewards += reward;
                            skipped += eligible - reward;
                            budgets[0] += (penalty - reward) / 2;
                            budgets[1] += (penalty - reward).div_ceil(2);
                            episodes.push((closed, penalty, reward));
                            liquidations += 1;
                            liquidated = true;
                        }
                        treasury(&env, discovery, fees, rewards, budgets);
                        census(&env, portfolios);
                        if liquidated {
                            break;
                        }
                    }
                    assert_eq!(liquidated, phase < 2);
                    for _ in 0..8 {
                        for i in [1, 2, 3, 0] {
                            if !census(&env, portfolios)[i] {
                                let refresh = observe(
                                    &env,
                                    portfolios[i],
                                    owners[4].pubkey(),
                                    Some(report),
                                    None,
                                );
                                meter.send(&mut env, &owners[4], &[refresh], &tracked, None);
                                treasury(&env, discovery, fees, rewards, budgets);
                            }
                        }
                        if census(&env, portfolios)[..4].iter().all(|v| *v) {
                            break;
                        }
                    }
                    assert!(census(&env, portfolios)[..4].iter().all(|v| *v));
                    for recipient in [None, Some(keeper)] {
                        let replay =
                            observe(&env, target, owners[4].pubkey(), Some(report), recipient);
                        meter.send(
                            &mut env,
                            &owners[4],
                            &[replay],
                            &tracked,
                            Some((
                                2,
                                InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                            )),
                        );
                    }
                    if phase == 0 && early {
                        early_paid = u128::from(FUNDS[4]) + rewards;
                        let withdraw = payout(
                            &env,
                            owners[4].pubkey(),
                            keeper,
                            tokens[4],
                            Some(early_paid),
                        );
                        let prefix = [
                            sync(tokens[4]),
                            withdraw,
                            unwrap(tokens[4], owners[4].pubkey()),
                        ];
                        meter.abort(&mut env, &owners[4], &prefix, &tracked);
                        meter.send(&mut env, &owners[4], &prefix, &tracked, None);
                        assert_unwrapped(&env, tokens[4]);
                        assert_eq!(
                            env.svm.get_account(&owners[4].pubkey()).unwrap().lamports,
                            keeper_sol + early_paid as u64 + DONATION + rent
                        );
                        assert_eq!(
                            create_ata_for_test(
                                &mut env.svm,
                                &env.payer,
                                owners[4].pubkey(),
                                env.mint
                            ),
                            tokens[4]
                        );
                    }
                    assert_eq!(
                        values(&env, portfolios)[4],
                        (u128::from(FUNDS[4]) + rewards - early_paid) as i128
                    );
                    assert_eq!(
                        native_balance(&env, tokens[4], if early_paid > 0 { 0 } else { DONATION }),
                        0
                    );
                    assert_eq!(native_balance(&env, env.vault, 0), supply - early_paid);
                }
                let live_values = values(&env, portfolios);
                let live = env.market_state().1;
                let residual = live
                    .vault
                    .checked_sub(
                        u128::try_from(live_values.iter().sum::<i128>()).unwrap() + live.insurance,
                    )
                    .unwrap();
                let resolve = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                    data: ProgInstruction::ResolveMarket {
                        asset_generation_frontier: live.next_market_id,
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    }
                    .encode(),
                };
                meter.abort(
                    &mut env,
                    &admin,
                    &[sync(tokens[4]), resolve.clone()],
                    &tracked,
                );
                meter.send(&mut env, &admin, &[resolve], &tracked, None);
                let frozen_profile = state::read_asset_oracle_profile(
                    &env.svm.get_account(&env.market).unwrap().data,
                    0,
                )
                .unwrap();
                set_test_clock(&mut env, 30, 1_025);
                let later = env.set_pyth_price_with_conf(&feed, 1_200_000, -6, 0, 1_025);
                tracked.push(later);
                providers.push((later, env.svm.get_account(&later)));
                let closes: [Instruction; 5] = std::array::from_fn(|i| {
                    payout(&env, owners[i].pubkey(), portfolios[i], tokens[i], None)
                });
                let mut paid = [0; 5];
                paid[4] = early_paid;
                let mut keeper_unwrapped = false;
                for round in 0..16 {
                    let mut progressed = false;
                    for i in if reverse {
                        [4, 3, 2, 1, 0]
                    } else {
                        [0, 1, 2, 3, 4]
                    } {
                        if resolved_portfolio_is_terminal(&env, portfolios[i]) {
                            continue;
                        }
                        let before = env.portfolio_state(portfolios[i]);
                        if percolator::active_bitmap_is_empty(active_bitmap(&before))
                            && before.pnl.get() > 0
                            && env.market_state().1.resolved_payout_blocker_count > 0
                        {
                            meter.send(
                                &mut env,
                                &owners[i],
                                &[closes[i].clone()],
                                &tracked,
                                Some((
                                    2,
                                    InstructionError::Custom(
                                        PercolatorError::EngineNonProgress as u32,
                                    ),
                                )),
                            );
                            continue;
                        }
                        let prefix = if i == 4 {
                            vec![
                                sync(tokens[4]),
                                closes[i].clone(),
                                unwrap(tokens[4], owners[4].pubkey()),
                            ]
                        } else {
                            vec![closes[i].clone()]
                        };
                        let vault = env.token_amount(env.vault);
                        let peers: Vec<_> = portfolios
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, key)| (*key, env.svm.get_account(key)))
                            .collect();
                        meter.abort(&mut env, &owners[i], &prefix, &tracked);
                        meter.send(&mut env, &owners[i], &prefix, &tracked, None);
                        let amount = u128::from(vault - env.token_amount(env.vault));
                        paid[i] += amount;
                        progressed = true;
                        redemptions += 1;
                        for (key, account) in peers {
                            assert_eq!(env.svm.get_account(&key), account);
                        }
                        if i == 4 {
                            keeper_unwrapped = true;
                            assert_eq!(paid[4], u128::from(FUNDS[4]) + rewards);
                            assert!(resolved_portfolio_is_terminal(&env, keeper));
                            assert_unwrapped(&env, tokens[4]);
                            assert_eq!(
                                env.svm.get_account(&owners[4].pubkey()).unwrap().lamports,
                                keeper_sol
                                    + paid[4] as u64
                                    + DONATION
                                    + rent * (1 + u64::from(early))
                            );
                        }
                        for j in 0..4 {
                            assert_eq!(native_balance(&env, tokens[j], 0), paid[j]);
                        }
                        assert_eq!(
                            native_balance(&env, env.vault, 0) + paid.iter().sum::<u128>(),
                            supply
                        );
                        treasury(&env, discovery, fees, rewards, budgets);
                        census(&env, portfolios);
                        let group = env.market_state().1;
                        assert_eq!(group.mode, MarketModeV16::Resolved);
                        assert_eq!(group.resolved_slot, 14);
                        assert_eq!(group.assets[0].effective_price, FINAL);
                        assert_eq!(
                            state::read_asset_oracle_profile(
                                &env.svm.get_account(&env.market).unwrap().data,
                                0
                            )
                            .unwrap(),
                            frozen_profile
                        );
                    }
                    if portfolios
                        .iter()
                        .all(|key| resolved_portfolio_is_terminal(&env, *key))
                    {
                        break;
                    }
                    assert!(progressed, "cohort stalled at {round}");
                }
                assert!(keeper_unwrapped);
                assert!(portfolios
                    .iter()
                    .all(|key| resolved_portfolio_is_terminal(&env, *key)));
                // This fully backed cohort never creates a payout snapshot. Recreating
                // custody cannot make the snapshot-gated top-up route reclaim a reward.
                assert_eq!(
                    create_ata_for_test(&mut env.svm, &env.payer, owners[4].pubkey(), env.mint),
                    tokens[4]
                );
                let mut topup = closes[4].clone();
                topup.data = ProgInstruction::ClaimResolvedPayoutTopup.encode();
                for slot in [30, 100] {
                    set_test_clock(&mut env, slot, 995 + slot as i64);
                    meter.send(
                        &mut env,
                        &owners[4],
                        &[closes[4].clone()],
                        &tracked,
                        Some((
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                        )),
                    );
                    assert!(!env.market_state().1.payout_snapshot_captured);
                    meter.send(
                        &mut env,
                        &owners[4],
                        &[sync(tokens[4]), topup.clone()],
                        &tracked,
                        Some((
                            3,
                            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                        )),
                    );
                    assert_eq!(native_balance(&env, tokens[4], 0), 0);
                }
                let group = env.market_state().1;
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.source_claim_bound_total_num
                    ),
                    (0, 0, 0)
                );
                assert_eq!(
                    (
                        group.assets[0].oi_eff_long_q,
                        group.assets[0].oi_eff_short_q
                    ),
                    (0, 0)
                );
                assert_eq!(
                    paid.map(|v| v as i128),
                    std::array::from_fn(
                        |i| live_values[i] + if i == 4 { early_paid as i128 } else { 0 }
                    )
                );
                assert_eq!(group.vault, group.insurance + residual);
                assert_eq!(env.svm.get_account(&env.mint), mint);
                for (key, account) in providers {
                    assert_eq!(env.svm.get_account(&key), account);
                }
                assert!(
                    group.vault > skipped,
                    "replay is tested with funded custody"
                );
                let mut principal_paid = paid;
                principal_paid[4] -= rewards;
                let provenance = (
                    episodes
                        .iter()
                        .map(|&(q, fee, _)| (q, fee))
                        .collect::<Vec<_>>(),
                    principal_paid,
                    group.insurance + rewards,
                    group.vault + rewards,
                    budgets.iter().sum::<u128>() + rewards,
                    rewards + skipped,
                    residual,
                );
                if let Some(expected) = &provenance_reference {
                    assert_eq!(
                        &provenance, expected,
                        "recipient presence changes only the earned share"
                    );
                } else {
                    provenance_reference = Some(provenance);
                }
                let outcome = (
                    episodes,
                    paid,
                    group.vault,
                    group.insurance,
                    budgets,
                    skipped,
                    residual,
                );
                println!("native rewards mask={mask} early={early} reverse={reverse}: {outcome:?}");
                if let Some(expected) = &reference {
                    assert_eq!(&outcome, expected);
                } else {
                    reference = Some(outcome);
                }
            }
        }
    }
    assert_eq!(liquidations, 32);
    assert_cu_within("native reward discovery", trade_peak, TRADE_CU_LIMIT);
    assert_cu_within("native reward terminal bundle", meter.peak, 900_000);
    println!("native reward histories=16 liquidations={liquidations} redemptions={redemptions} rollbacks={} CU[trade,all,rollback]=[{trade_peak},{},{}]", meter.rollbacks, meter.peak, meter.rollback_peak);
}
