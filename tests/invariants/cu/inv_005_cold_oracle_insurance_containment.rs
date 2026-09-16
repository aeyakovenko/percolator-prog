//! Row 416: cold-admin oracle replacement while the incumbent oracle is funded
//! only by insurance-domain budgets. This complements the backing-funded oracle
//! test: an accepted observation transfers no insurance-beneficiary rights, and
//! a funded-role suffix must roll back the oracle update, mark observation, and
//! unrelated user SPL prefix.
//! The open-position selector crosses insurance coholders and AuthMark/Hybrid
//! refreshes through an oracle round trip. Its price/size cash-flow oracle is
//! independent of engine PnL; this is bounded live evidence, not row closure.

use super::*;

const INSURANCE: [u128; 2] = [13, 17];
const USER_PREFIX: u128 = 5;

const OPEN_CAPITAL: u128 = 1_000;
const OPEN_SIZE: i128 = 2 * POS_SCALE as i128;
const OPEN_FEED: [u8; 32] = [0x14; 32];

fn rotation_request(
    env: &V16CuEnv,
    asset: u16,
    signer: Pubkey,
    epoch: u64,
    sequence: u64,
    mark: u64,
    hybrid: bool,
) -> Instruction {
    let accounts = if hybrid {
        vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(signer, true),
            AccountMeta::new(env.market, false),
        ]
    } else {
        vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(env.market, false),
        ]
    };
    let data = if hybrid {
        // Hybrid reports are permissionless and open positions prohibit oracle
        // reconfiguration. Retain a self-handoff to test the role's epoch instead.
        ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            kind: processor::ASSET_AUTH_ORACLE,
            new_pubkey: signer.to_bytes(),
        }
    } else {
        ProgInstruction::PushAuthMark {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            observation_sequence: sequence,
            now_slot: u64::MAX,
            mark_e6: mark,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: data.encode(),
    }
}

fn rotation_cashout(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    wallet: Pubkey,
    amount: u128,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(portfolio, amount).encode(),
    }
}

fn rotation_refresh(
    env: &V16CuEnv,
    asset: u16,
    signer: Pubkey,
    portfolio: Pubkey,
    report: Option<Pubkey>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(signer, true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(portfolio, false),
    ];
    accounts.extend(report.map(|key| AccountMeta::new_readonly(key, false)));
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::PermissionlessCrank {
            now_slot: u64::MAX,
            observations: crank_observations_with_accounts(asset, u8::from(report.is_some())),
        }
        .encode(),
    }
}

#[test]
fn v16_program_funded_insurance_coholders_preserve_open_positions_through_oracle_round_trip() {
    use crate::support::fuzz_model::{
        assert_current_certificate_matches_independent, assert_market_stock_census,
    };

    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for asset in [0u16, 1] {
        for overlap in [1u8, 2, 3] {
            for mark in [96u64, 104] {
                let mut reference = None;
                for hybrid in [false, true] {
                    let mut env = inv018_public_spl_market_with_params(
                        6,
                        V16CuMarketParams {
                            max_portfolio_assets: 2,
                            max_price_move_bps_per_slot: 500,
                            ..V16CuMarketParams::default()
                        },
                    );
                    let admin = env.admin.insecure_clone();
                    // 0/1 trade; 2 is an independent flat owner; 3 is the original
                    // oracle, 4 the disjoint insurance role, 5 cold admin, 6 new oracle.
                    let actors: [Keypair; 7] = std::array::from_fn(|_| Keypair::new());
                    for actor in &actors {
                        env.ensure_signer_account(actor.pubkey());
                    }
                    let beneficiary = if overlap & 1 != 0 { 3 } else { 4 };
                    let operator = if overlap & 2 != 0 { 3 } else { 4 };
                    set_test_clock(&mut env, 1, 100);
                    for index in [0u16, 1] {
                        env.configure_auth_mark_for_asset_as_admin(index, 1, 100);
                    }
                    let initial_report =
                        hybrid.then(|| env.set_pyth_price_with_conf(&OPEN_FEED, 100, -6, 0, 100));
                    if let Some(report) = initial_report {
                        env.try_configure_hybrid_asset_with_conf_filter_cu(
                            asset,
                            1,
                            0,
                            [OPEN_FEED, [0; 32], [0; 32]],
                            &[report],
                            1,
                            100,
                            0,
                            0,
                            1_000,
                            0,
                        )
                        .unwrap();
                    }
                    for (kind, actor) in [
                        (processor::ASSET_AUTH_INSURANCE, beneficiary),
                        (processor::ASSET_AUTH_INSURANCE_OPERATOR, operator),
                        (processor::ASSET_AUTH_ORACLE, 3),
                        (processor::ASSET_AUTH_ADMIN, 5),
                    ] {
                        env.try_update_per_asset_authority_with_cu(
                            &admin,
                            Some(&actors[actor]),
                            asset,
                            kind,
                            actors[actor].pubkey().to_bytes(),
                        )
                        .unwrap();
                    }
                    let wallets = actors.each_ref().map(|actor| {
                        create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
                    });
                    for (actor, amount) in [
                        (0, OPEN_CAPITAL),
                        (1, OPEN_CAPITAL),
                        (2, CAPITAL),
                        (beneficiary, INSURANCE.iter().sum()),
                    ] {
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::mint_to(
                                &spl_token::ID,
                                &env.mint,
                                &wallets[actor],
                                &admin.pubkey(),
                                &[],
                                amount as u64,
                            )
                            .unwrap(),
                            &[&admin],
                        )
                        .unwrap();
                    }
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                    let portfolios: [Pubkey; 3] = std::array::from_fn(|i| {
                        let key = Keypair::new();
                        system_create_account_for_test(
                            &mut env.svm,
                            &env.payer,
                            &key,
                            env.portfolio_account_len,
                            env.program_id,
                        );
                        env.send(
                            ProgInstruction::InitPortfolio,
                            vec![
                                AccountMeta::new(actors[i].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(key.pubkey(), false),
                            ],
                            &[&actors[i]],
                        )
                        .unwrap();
                        env.portfolios.push(key.pubkey());
                        env.send(
                            env.deposit_ix(
                                key.pubkey(),
                                if i < 2 { OPEN_CAPITAL } else { CAPITAL },
                            ),
                            vec![
                                AccountMeta::new(actors[i].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(key.pubkey(), false),
                                AccountMeta::new(wallets[i], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&actors[i]],
                        )
                        .unwrap();
                        key.pubkey()
                    });
                    for (side, amount) in INSURANCE.into_iter().enumerate() {
                        let seq = env.control_sequences(asset as usize);
                        env.send(
                            ProgInstruction::TopUpInsuranceDomain {
                                domain: asset * 2 + side as u16,
                                market_id: env.asset_market_id(asset),
                                authority_epoch: seq.authority_epoch,
                                intent_id: next_control_sequence(seq.insurance_top_up),
                                amount,
                            },
                            vec![
                                AccountMeta::new(actors[beneficiary].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(wallets[beneficiary], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&actors[beneficiary]],
                        )
                        .unwrap();
                    }
                    peak = peak.max(env.trade_asset_with_cu(
                        asset,
                        &actors[0],
                        portfolios[0],
                        &actors[1],
                        portfolios[1],
                        OPEN_SIZE,
                        100,
                        0,
                    ));
                    let market = env.market;
                    let vault = env.vault;
                    let mut tracked = vec![
                        market,
                        vault,
                        env.mint,
                        env.vault_authority,
                        admin.pubkey(),
                        solana_sdk::sysvar::clock::ID,
                    ];
                    tracked.extend(wallets);
                    tracked.extend(portfolios);
                    tracked.extend(actors.each_ref().map(Signer::pubkey));
                    tracked.extend(initial_report);
                    let peer = (asset ^ 1) as usize;
                    let peer_profile = profile(&env, peer);
                    let peer_sequences = env.control_sequences(peer);
                    let peer_asset = env.market_state().1.assets[peer];
                    let mint_frame = env.svm.get_account(&env.mint);
                    let trader_frames =
                        [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key));
                    let initial_seq = env.control_sequences(asset as usize);
                    let mut expected_seq = initial_seq;
                    let insurance_total = INSURANCE.iter().sum::<u128>();
                    let mut paid = [0u128; 7];
                    let mut insurance_paid = 0;
                    // Losing capital becomes fresh backing; winner conversion consumes it.
                    let mut loss_stock = [0u128; 2];
                    let stock = |env: &V16CuEnv,
                                 paid: [u128; 7],
                                 insurance_paid: u128,
                                 loss_stock: [u128; 2]| {
                        let (_, group) = env.market_state();
                        assert_eq!(group.mode, MarketModeV16::Live);
                        assert_eq!(
                            group.assets[asset as usize].lifecycle,
                            AssetLifecycleV16::Active
                        );
                        assert_eq!(group.assets[peer], peer_asset);
                        assert_eq!(profile(env, peer), peer_profile);
                        assert_eq!(env.control_sequences(peer), peer_sequences);
                        let profile = profile(env, asset as usize);
                        assert_eq!(
                            profile.insurance_authority,
                            actors[beneficiary].pubkey().to_bytes()
                        );
                        assert_eq!(
                            profile.insurance_operator,
                            actors[operator].pubkey().to_bytes()
                        );
                        assert_eq!(profile.asset_admin, actors[5].pubkey().to_bytes());
                        assert_eq!(group.insurance, insurance_total - insurance_paid);
                        assert_eq!(
                            group.insurance_domain_budget_remaining_total,
                            insurance_total - insurance_paid
                        );
                        for (domain, amount) in group.insurance_domain_budget.iter().enumerate() {
                            let expected = if domain / 2 == asset as usize && insurance_paid == 0 {
                                INSURANCE[domain % 2]
                            } else {
                                0
                            };
                            assert_eq!(*amount, expected);
                            assert_eq!(group.insurance_domain_spent[domain], 0);
                        }
                        assert_eq!(group.backing_provider_earnings_total, 0);
                        for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                            let loss_domain = asset as usize * 2 + usize::from(mark > 100);
                            assert_eq!(
                                bucket.fresh_unliened_backing_num,
                                if domain == loss_domain {
                                    loss_stock[0] * BOUND_SCALE
                                } else {
                                    0
                                }
                            );
                            assert_eq!(bucket.valid_liened_backing_num, 0);
                            assert_eq!(
                                bucket.consumed_liened_backing_num,
                                if domain == loss_domain {
                                    loss_stock[1] * BOUND_SCALE
                                } else {
                                    0
                                }
                            );
                            assert_eq!(bucket.impaired_liened_backing_num, 0);
                            assert_eq!(bucket.utilization_fee_earnings, 0);
                        }
                        assert_eq!(wallets.map(|key| env.token_amount(key) as u128), paid);
                        assert_eq!(
                            group.vault,
                            2 * OPEN_CAPITAL + CAPITAL + insurance_total
                                - paid.iter().sum::<u128>()
                        );
                        assert_eq!(env.token_amount(vault) as u128, group.vault);
                        assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                        let accounts = portfolios.map(|key| env.portfolio_state(key));
                        assert_market_stock_census(
                            "funded open rotation",
                            &group,
                            &env.svm.get_account(&market).unwrap().data,
                            &accounts,
                            env.token_amount(vault) as u128,
                        )
                        .unwrap();
                    };
                    stock(&env, paid, insurance_paid, loss_stock);
                    let rotate = |env: &V16CuEnv, kind, to: usize, epoch| Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(actors[5].pubkey(), true),
                            AccountMeta::new(actors[to].pubkey(), true),
                            AccountMeta::new(market, false),
                        ],
                        data: ProgInstruction::UpdateAssetAuthority {
                            asset_index: asset,
                            market_id: env.asset_market_id(asset),
                            authority_epoch: epoch,
                            kind,
                            new_pubkey: actors[to].pubkey().to_bytes(),
                        }
                        .encode(),
                    };
                    let retained = rotation_request(
                        &env,
                        asset,
                        actors[3].pubkey(),
                        initial_seq.authority_epoch,
                        next_control_sequence(initial_seq.oracle_observation),
                        mark,
                        hybrid,
                    );
                    let winner = usize::from(mark < 100);
                    let loser = winner ^ 1;
                    let pnl = (OPEN_SIZE / POS_SCALE as i128) * (mark as i128 - 100);
                    for (step, oracle) in [6usize, 3].into_iter().enumerate() {
                        let slot = step as u64 + 2;
                        let now = 101 + step as i64;
                        set_test_clock(&mut env, slot, now);
                        let report = hybrid.then(|| {
                            env.set_pyth_price_with_conf(&OPEN_FEED, mark as i64, -6, 0, now)
                        });
                        tracked.extend(report);
                        let next_observation =
                            next_control_sequence(expected_seq.oracle_observation);
                        let publication = if hybrid {
                            rotation_refresh(
                                &env,
                                asset,
                                actors[oracle].pubkey(),
                                portfolios[2],
                                report,
                            )
                        } else {
                            rotation_request(
                                &env,
                                asset,
                                actors[oracle].pubkey(),
                                expected_seq.authority_epoch + 1,
                                next_observation,
                                mark,
                                false,
                            )
                        };
                        let cashout = rotation_cashout(
                            &env,
                            actors[2].pubkey(),
                            portfolios[2],
                            wallets[2],
                            USER_PREFIX,
                        );
                        let prefix = [
                            cashout.clone(),
                            rotate(
                                &env,
                                processor::ASSET_AUTH_ORACLE,
                                oracle,
                                expected_seq.authority_epoch,
                            ),
                            publication,
                        ];
                        // Both funded roles remain protected, including the role held by
                        // the disjoint key in each single-coholder world.
                        for kind in [
                            processor::ASSET_AUTH_INSURANCE,
                            processor::ASSET_AUTH_INSURANCE_OPERATOR,
                        ] {
                            let mut bundle = prefix.to_vec();
                            bundle.push(rotate(&env, kind, 5, expected_seq.authority_epoch + 1));
                            let meta = land(
                                &mut env,
                                &bundle,
                                &[&actors[2], &actors[5], &actors[oracle]],
                                &tracked,
                                &[],
                                Some((5, PercolatorError::EngineLockActive)),
                            );
                            assert_eq!(
                                meta.logs
                                    .iter()
                                    .filter(|line| **line
                                        == format!("Program {} success", spl_token::ID))
                                    .count(),
                                1
                            );
                            assert_eq!(
                                meta.logs
                                    .iter()
                                    .filter(|line| **line
                                        == format!("Program {} success", env.program_id))
                                    .count(),
                                3
                            );
                            peak = peak.max(meta.compute_units_consumed);
                            rollbacks += 1;
                            stock(&env, paid, insurance_paid, loss_stock);
                        }
                        let meta = land(
                            &mut env,
                            &prefix,
                            &[&actors[2], &actors[5], &actors[oracle]],
                            &tracked,
                            &[market, vault, portfolios[2], wallets[2]],
                            None,
                        );
                        peak = peak.max(meta.compute_units_consumed);
                        paid[2] += USER_PREFIX;
                        expected_seq.authority_epoch += 1;
                        if !hybrid {
                            expected_seq.oracle_observation = next_observation;
                        }
                        assert_eq!(env.control_sequences(asset as usize), expected_seq);
                        let observed = profile(&env, asset as usize);
                        assert_eq!(
                            observed.oracle_authority,
                            actors[oracle].pubkey().to_bytes()
                        );
                        assert_eq!(observed.oracle_target_price_e6, mark);
                        assert_eq!(observed.last_good_oracle_slot, slot);
                        if hybrid {
                            assert_eq!(observed.oracle_target_publish_time, now);
                            assert_eq!(observed.oracle_leg_prices_e6[0], mark);
                            assert_eq!(observed.oracle_leg_publish_times[0], now);
                        }
                        if step == 0 {
                            assert_eq!(
                                [portfolios[0], portfolios[1]].map(|key| env.svm.get_account(&key)),
                                trader_frames
                            );
                        }
                        stock(&env, paid, insurance_paid, loss_stock);
                        let former = if step == 0 { 3 } else { 6 };
                        let revoked = rotation_request(
                            &env,
                            asset,
                            actors[former].pubkey(),
                            expected_seq.authority_epoch,
                            next_control_sequence(expected_seq.oracle_observation),
                            mark,
                            hybrid,
                        );
                        let meta = land(
                            &mut env,
                            &[revoked],
                            &[&actors[former]],
                            &tracked,
                            &[],
                            Some((2, PercolatorError::Unauthorized)),
                        );
                        peak = peak.max(meta.compute_units_consumed);
                        rollbacks += 1;
                        if step == 1 {
                            let cashout = rotation_cashout(
                                &env,
                                actors[2].pubkey(),
                                portfolios[2],
                                wallets[2],
                                USER_PREFIX,
                            );
                            let meta = land(
                                &mut env,
                                &[cashout, retained.clone()],
                                &[&actors[2], &actors[3]],
                                &tracked,
                                &[],
                                Some((3, PercolatorError::EngineStale)),
                            );
                            assert!(meta
                                .logs
                                .contains(&format!("Program {} success", spl_token::ID)));
                            peak = peak.max(meta.compute_units_consumed);
                            rollbacks += 1;
                            let current_request = rotation_request(
                                &env,
                                asset,
                                actors[3].pubkey(),
                                expected_seq.authority_epoch,
                                next_control_sequence(expected_seq.oracle_observation),
                                mark,
                                hybrid,
                            );
                            peak = peak.max(
                                land(
                                    &mut env,
                                    &[current_request],
                                    &[&actors[3]],
                                    &tracked,
                                    &[market],
                                    None,
                                )
                                .compute_units_consumed,
                            );
                            if hybrid {
                                expected_seq.authority_epoch += 1;
                            } else {
                                expected_seq.oracle_observation =
                                    next_control_sequence(expected_seq.oracle_observation);
                            }
                            assert_eq!(env.control_sequences(asset as usize), expected_seq);
                        }
                        for actor in [loser, winner] {
                            let mut current = false;
                            for _ in 0..4 {
                                let group = env.market_state().1;
                                if assert_current_certificate_matches_independent(
                                    "open rotation refresh",
                                    &group,
                                    &env.portfolio_state(portfolios[actor]),
                                )
                                .unwrap()
                                {
                                    current = true;
                                    break;
                                }
                                let crank = rotation_refresh(
                                    &env,
                                    asset,
                                    actors[2].pubkey(),
                                    portfolios[actor],
                                    report,
                                );
                                peak = peak.max(
                                    land(
                                        &mut env,
                                        &[crank],
                                        &[&actors[2]],
                                        &tracked,
                                        &[market, portfolios[actor]],
                                        None,
                                    )
                                    .compute_units_consumed,
                                );
                            }
                            assert!(
                                current,
                                "bounded permissionless refresh after oracle succession"
                            );
                        }
                        let group = env.market_state().1;
                        assert_eq!(group.assets[asset as usize].effective_price, mark);
                        assert_eq!(
                            group.assets[asset as usize].oi_eff_long_q,
                            OPEN_SIZE as u128
                        );
                        assert_eq!(
                            group.assets[asset as usize].oi_eff_short_q,
                            OPEN_SIZE as u128
                        );
                        for actor in 0..2 {
                            let account = env.portfolio_state(portfolios[actor]);
                            let leg = active_leg_for_asset(&account, asset as usize);
                            assert_eq!(
                                leg.basis_pos_q,
                                if actor == 0 { OPEN_SIZE } else { -OPEN_SIZE }
                            );
                            assert_eq!(
                                account.capital.get() as i128 + account.pnl.get(),
                                OPEN_CAPITAL as i128 + if actor == 0 { pnl } else { -pnl }
                            );
                        }
                        loss_stock = [pnl.unsigned_abs(), 0];
                        stock(&env, paid, insurance_paid, loss_stock);
                    }
                    peak = peak.max(env.trade_asset_with_cu(
                        asset,
                        &actors[0],
                        portfolios[0],
                        &actors[1],
                        portfolios[1],
                        -OPEN_SIZE,
                        mark,
                        0,
                    ));
                    peak = peak.max(env.convert_released_pnl_with_cu(
                        &actors[winner],
                        portfolios[winner],
                        pnl.unsigned_abs(),
                    ));
                    loss_stock = [0, pnl.unsigned_abs()];
                    for actor in [loser, winner, 2] {
                        let entitlement = match actor {
                            0 => (OPEN_CAPITAL as i128 + pnl) as u128,
                            1 => (OPEN_CAPITAL as i128 - pnl) as u128,
                            _ => CAPITAL,
                        };
                        let amount = entitlement - paid[actor];
                        let ix = rotation_cashout(
                            &env,
                            actors[actor].pubkey(),
                            portfolios[actor],
                            wallets[actor],
                            amount,
                        );
                        peak = peak.max(
                            land(
                                &mut env,
                                &[ix],
                                &[&actors[actor]],
                                &tracked,
                                &[market, vault, portfolios[actor], wallets[actor]],
                                None,
                            )
                            .compute_units_consumed,
                        );
                        paid[actor] += amount;
                        assert_eq!(env.portfolio_state(portfolios[actor]).capital.get(), 0);
                        assert_eq!(env.portfolio_state(portfolios[actor]).pnl.get(), 0);
                        assert!(!has_active_leg_for_asset(
                            &env.portfolio_state(portfolios[actor]),
                            asset as usize
                        ));
                        stock(&env, paid, insurance_paid, loss_stock);
                    }
                    let payout = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(actors[operator].pubkey(), true),
                            AccountMeta::new(market, false),
                            AccountMeta::new(wallets[operator], false),
                            AccountMeta::new(vault, false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::WithdrawInsuranceAsset {
                            asset_index: asset,
                            market_id: env.asset_market_id(asset),
                            authority_epoch: expected_seq.authority_epoch,
                            amount: insurance_total,
                        }
                        .encode(),
                    };
                    peak = peak.max(
                        land(
                            &mut env,
                            &[payout],
                            &[&actors[operator]],
                            &tracked,
                            &[market, vault, wallets[operator]],
                            None,
                        )
                        .compute_units_consumed,
                    );
                    paid[operator] += insurance_total;
                    insurance_paid = insurance_total;
                    stock(&env, paid, insurance_paid, loss_stock);
                    assert_eq!(env.market_state().1.vault, 0);
                    if let Some(expected) = reference {
                        assert_eq!(
                            paid, expected,
                            "AuthMark and Hybrid preserve the same owner entitlements"
                        );
                    } else {
                        reference = Some(paid);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 24);
    assert_eq!(rollbacks, 168);
    assert_cu_within("funded insurance open rotation", peak, CUSTODY_CU_LIMIT);
    eprintln!("INV-005 funded insurance open rotation: worlds={worlds}, exact_rollbacks={rollbacks}, oracle_handoffs=48, owner_exits=72, insurance_payouts=24, peak_cu={peak}");
}

#[test]
fn v16_program_cold_oracle_replacement_preserves_insurance_funded_coholder() {
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for pushed_mark in [100u64, 104] {
            let mut env = inv018_public_spl_market_with_params(
                6,
                V16CuMarketParams {
                    max_portfolio_assets: 2,
                    ..V16CuMarketParams::default()
                },
            );
            let admin = env.admin.insecure_clone();
            let incumbent = Keypair::new();
            let cold = Keypair::new();
            let incoming = Keypair::new();
            let user = Keypair::new();
            let actors = [&incumbent, &cold, &incoming, &user, &admin];
            for actor in actors {
                env.ensure_signer_account(actor.pubkey());
            }
            for (kind, holder) in [
                (processor::ASSET_AUTH_INSURANCE, &incumbent),
                (processor::ASSET_AUTH_INSURANCE_OPERATOR, &incumbent),
                (processor::ASSET_AUTH_ORACLE, &incumbent),
                (processor::ASSET_AUTH_ADMIN, &cold),
            ] {
                env.try_update_per_asset_authority_with_cu(
                    &admin,
                    Some(holder),
                    asset,
                    kind,
                    holder.pubkey().to_bytes(),
                )
                .unwrap();
            }
            env.svm.warp_to_slot(1);
            for index in [0u16, 1] {
                env.configure_auth_mark_for_asset_with_authority(
                    index,
                    if index == asset { &incumbent } else { &admin },
                    1,
                    100,
                );
            }
            let wallets = actors.map(|actor| {
                create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
            });
            for (wallet, amount) in [
                (wallets[0], INSURANCE.iter().sum::<u128>()),
                (wallets[3], CAPITAL),
            ] {
                send_raw_tx(
                    &mut env.svm,
                    &env.payer,
                    spl_token::instruction::mint_to(
                        &spl_token::ID,
                        &env.mint,
                        &wallet,
                        &admin.pubkey(),
                        &[],
                        amount as u64,
                    )
                    .unwrap(),
                    &[&admin],
                )
                .unwrap();
            }
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            for (side, amount) in INSURANCE.into_iter().enumerate() {
                let seq = env.control_sequences(asset as usize);
                env.send(
                    ProgInstruction::TopUpInsuranceDomain {
                        domain: asset * 2 + side as u16,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.insurance_top_up),
                        amount,
                    },
                    vec![
                        AccountMeta::new(incumbent.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallets[0], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[&incumbent],
                )
                .unwrap();
            }

            let portfolio_key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &portfolio_key,
                env.portfolio_account_len,
                env.program_id,
            );
            let portfolio = portfolio_key.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&user],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolio, CAPITAL),
                vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(wallets[3], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&user],
            )
            .unwrap();

            let market = env.market;
            let mut tracked = vec![market, env.mint, env.vault, env.vault_authority, portfolio];
            tracked.extend(wallets);
            tracked.extend(actors.map(Signer::pubkey));
            let peer = (1 - asset) as usize;
            let peer_profile = profile(&env, peer);
            let peer_sequences = env.control_sequences(peer);
            let peer_asset = env.market_state().1.assets[peer];
            let assert_stock = |env: &V16CuEnv, remaining: [u128; 2], user_paid: u128| {
                let (cfg, group) = env.market_state();
                let insurance = remaining.iter().sum::<u128>();
                let paid = INSURANCE.iter().sum::<u128>() - insurance;
                assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.assets[asset as usize].lifecycle,
                    AssetLifecycleV16::Active
                );
                assert_eq!(group.assets[peer], peer_asset);
                assert_eq!(profile(env, peer), peer_profile);
                assert_eq!(env.control_sequences(peer), peer_sequences);
                assert_eq!(group.c_tot, CAPITAL - user_paid);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.backing_provider_earnings_total, 0);
                assert!(group.source_backing_buckets.iter().all(|bucket| {
                    bucket.fresh_unliened_backing_num == 0
                        && bucket.valid_liened_backing_num == 0
                        && bucket.consumed_liened_backing_num == 0
                        && bucket.impaired_liened_backing_num == 0
                        && bucket.utilization_fee_earnings == 0
                }));
                assert_eq!(group.insurance, insurance);
                assert_eq!(group.insurance_domain_budget_remaining_total, insurance);
                for (domain, budget) in group.insurance_domain_budget.iter().enumerate() {
                    let amount = if domain / 2 == asset as usize {
                        remaining[domain % 2]
                    } else {
                        0
                    };
                    assert_eq!(*budget, amount);
                    assert_eq!(group.insurance_domain_spent[domain], 0);
                }
                let owner = env.portfolio_state(portfolio);
                assert_eq!(owner.owner, user.pubkey().to_bytes());
                assert_eq!(owner.capital.get(), CAPITAL - user_paid);
                assert_eq!(owner.pnl.get(), 0);
                let expected_wallets = [paid, 0, 0, user_paid, 0];
                assert_eq!(
                    wallets.map(|key| env.token_amount(key) as u128),
                    expected_wallets
                );
                let vault = insurance + CAPITAL - user_paid;
                assert_eq!(group.vault, vault);
                assert_eq!(env.token_amount(env.vault) as u128, vault);
                let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                assert_eq!(mint.mint_authority, COption::None);
                assert_eq!(
                    mint.supply as u128,
                    INSURANCE.iter().sum::<u128>() + CAPITAL
                );
                assert_eq!(
                    mint.supply as u128,
                    vault + expected_wallets.iter().sum::<u128>()
                );
            };
            assert_stock(&env, INSURANCE, 0);

            let mut expected_profile = profile(&env, asset as usize);
            let mut expected_sequences = env.control_sequences(asset as usize);
            assert_eq!(expected_profile.asset_admin, cold.pubkey().to_bytes());
            assert_eq!(
                expected_profile.oracle_authority,
                incumbent.pubkey().to_bytes()
            );
            assert_eq!(
                expected_profile.insurance_authority,
                incumbent.pubkey().to_bytes()
            );
            assert_eq!(
                expected_profile.insurance_operator,
                incumbent.pubkey().to_bytes()
            );
            let handoff = |kind, epoch| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(cold.pubkey(), true),
                    AccountMeta::new(incoming.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::UpdateAssetAuthority {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch,
                    kind,
                    new_pubkey: incoming.pubkey().to_bytes(),
                }
                .encode(),
            };
            let user_exit = |env: &V16CuEnv, amount| Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(user.pubkey(), true),
                    AccountMeta::new(market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(wallets[3], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: env.withdraw_ix(portfolio, amount).encode(),
            };
            let epoch = expected_sequences.authority_epoch;
            let oracle = handoff(processor::ASSET_AUTH_ORACLE, epoch);
            let funded = handoff(processor::ASSET_AUTH_INSURANCE, epoch + 1);
            let observation = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(incoming.pubkey(), true),
                    AccountMeta::new(market, false),
                ],
                data: ProgInstruction::PushAuthMark {
                    asset_index: asset,
                    market_id: env.asset_market_id(asset),
                    authority_epoch: epoch + 1,
                    observation_sequence: next_control_sequence(
                        expected_sequences.oracle_observation,
                    ),
                    now_slot: u64::MAX,
                    mark_e6: pushed_mark,
                }
                .encode(),
            };
            let prefix = [user_exit(&env, USER_PREFIX), oracle, observation];
            let bundle = [prefix.to_vec(), vec![funded]].concat();
            env.svm.warp_to_slot(2);

            let meta = land(
                &mut env,
                &bundle,
                &[&user, &cold, &incoming],
                &tracked,
                &[],
                Some((5, PercolatorError::EngineLockActive)),
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            for (program, count) in [(env.program_id, 3), (spl_token::ID, 1)] {
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count
                );
            }
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_stock(&env, INSURANCE, 0);

            let custody_changes = [market, portfolio, env.vault, wallets[3]];
            let meta = land(
                &mut env,
                &prefix,
                &[&user, &cold, &incoming],
                &tracked,
                &custody_changes,
                None,
            );
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            expected_profile.oracle_authority = incoming.pubkey().to_bytes();
            expected_profile.oracle_target_price_e6 = pushed_mark;
            if pushed_mark != 100 {
                expected_profile.mark_ewma_e6 = pushed_mark;
                expected_profile.mark_ewma_last_slot = 2;
                expected_profile.funding_mark_pending_e6 = pushed_mark;
                expected_profile.funding_mark_pending_slot = 2;
            }
            expected_profile.last_good_oracle_slot = 2;
            expected_sequences.authority_epoch += 1;
            expected_sequences.oracle_observation =
                next_control_sequence(expected_sequences.oracle_observation);
            assert_eq!(profile(&env, asset as usize), expected_profile);
            assert_eq!(env.control_sequences(asset as usize), expected_sequences);
            assert_stock(&env, INSURANCE, USER_PREFIX);

            let mut remaining = INSURANCE;
            for side in 0..2 {
                let ix = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(incumbent.pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(wallets[0], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: asset,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: expected_sequences.authority_epoch,
                        amount: remaining[side],
                    }
                    .encode(),
                };
                let changed = [market, env.vault, wallets[0]];
                let meta = land(&mut env, &[ix], &[&incumbent], &tracked, &changed, None);
                assert!(meta
                    .logs
                    .contains(&format!("Program {} success", spl_token::ID)));
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                remaining[side] = 0;
                expected_sequences.authority_epoch += 1;
                assert_eq!(profile(&env, asset as usize), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), expected_sequences);
                assert_stock(&env, remaining, USER_PREFIX);
            }

            let ix = user_exit(&env, CAPITAL - USER_PREFIX);
            let meta = land(&mut env, &[ix], &[&user], &tracked, &custody_changes, None);
            peak_cu = peak_cu.max(meta.compute_units_consumed);
            assert_stock(&env, [0, 0], CAPITAL);
        }
    }
    eprintln!("INV-005 cold oracle insurance containment: 4 live worlds, 4 exact SPL/oracle/observation rollbacks, 4 cold-signed oracle replacements, 8 incumbent insurance payouts, peak {peak_cu} CU");
}
