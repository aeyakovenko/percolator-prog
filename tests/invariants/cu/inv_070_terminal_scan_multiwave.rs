//! Row 424: a paid first rediscovery must not certify the prefix for a later expiry.

use super::*;

const DEADLINES: [u64; 2] = [EXPIRY, EXPIRY + 4];
const FIRST: u64 = 37;
const NATIVE_SURPLUS: u64 = 53;

// Both later-asset buckets must be funded while Live. The existing single-wave
// fixture returns Resolved, where adding a second source is correctly forbidden.
fn two_wave_fixture(side: usize, backing: [u64; 2], native: bool) -> RecreditFixture {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;
    let params = V16CuMarketParams {
        max_portfolio_assets: 2,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    };
    let mut env = if native {
        let mut env = inv081_public_native_market_with_params(2, params);
        env.portfolio_account_len = state::portfolio_account_len_for_market_slots(2).unwrap();
        env
    } else {
        inv018_public_spl_market_with_params(0, params)
    };
    let admin = env.admin.insecure_clone();
    let beneficiary = Keypair::new();
    env.svm
        .airdrop(&beneficiary.pubkey(), 1_000_000_000)
        .unwrap();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&beneficiary),
        0,
        processor::ASSET_AUTH_INSURANCE,
        beneficiary.pubkey().to_bytes(),
    )
    .unwrap();
    env.svm.warp_to_slot(1);
    for asset in [0, 1] {
        env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
    }
    let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
    let portfolios = owners.each_ref().map(|owner| {
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key.pubkey(), false),
            ],
            &[owner],
        )
        .unwrap();
        env.portfolios.push(key.pubkey());
        key.pubkey()
    });
    let tokens = owners
        .each_ref()
        .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
    let reserve = create_ata_for_test(&mut env.svm, &env.payer, beneficiary.pubkey(), env.mint);
    let destination = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), env.mint);
    for (token, amount) in tokens
        .into_iter()
        .zip(CAPITAL)
        .chain([(reserve, SPENT), (destination, backing.iter().sum())])
    {
        if native {
            send_raw_ixs(
                &mut env.svm,
                &env.payer,
                vec![
                    system_instruction::transfer(&admin.pubkey(), &token, amount),
                    spl_token::instruction::sync_native(&spl_token::ID, &token).unwrap(),
                ],
                &[&admin],
            )
            .unwrap();
            continue;
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &admin.pubkey(),
                &[],
                amount,
            )
            .unwrap(),
            &[&admin],
        )
        .unwrap();
    }
    if !native {
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
    }
    for actor in 0..3 {
        env.send(
            env.deposit_ix(portfolios[actor], CAPITAL[actor].into()),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
                AccountMeta::new(tokens[actor], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
    }
    env.send(
        ProgInstruction::TopUpInsuranceDomain {
            domain: side as u16,
            market_id: env.asset_market_id(0),
            authority_epoch: env.control_sequences(0).authority_epoch,
            intent_id: 0,
            amount: SPENT.into(),
        },
        vec![
            AccountMeta::new(beneficiary.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(reserve, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&beneficiary],
    )
    .unwrap();
    for wave in 0..2 {
        env.top_up_backing_bucket_from_admin_token_with_cu(
            destination,
            [2 + side, 3 - side][wave] as u16,
            backing[wave].into(),
            DEADLINES[wave],
        );
    }
    env.trade_asset_with_cu(
        0,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        (10 * POS_SCALE) as i128 * if side == 0 { 1 } else { -1 },
        100,
        0,
    );
    for offset in 0..5 {
        let slot = offset + 2;
        let delta = 5 * (offset + 1).min(4);
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(
            0,
            slot,
            if side == 0 { 100 + delta } else { 100 - delta },
        );
        env.crank(
            portfolios[2],
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for actor in [0, 1] {
        env.crank(
            portfolios[actor],
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), 200);
    assert_eq!(
        env.portfolio_state(portfolios[1]).pnl.get(),
        -i128::from(SPENT)
    );
    env.svm.warp_to_slot(40);
    env.resolve();
    env.svm.warp_to_slot(EXPIRY - 1);
    let mut peak = 0;
    for actor in [1, 0, 2] {
        for _ in 0..8 {
            if resolved_portfolio_is_terminal(&env, portfolios[actor]) {
                break;
            }
            env.svm.expire_blockhash();
            peak = peak.max(
                env.send(
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    },
                    vec![
                        AccountMeta::new_readonly(owners[actor].pubkey(), false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[actor], false),
                        AccountMeta::new(tokens[actor], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    &[],
                )
                .unwrap(),
            );
        }
        assert!(resolved_portfolio_is_terminal(&env, portfolios[actor]));
        assert_eq!(env.token_amount(tokens[actor]), PAYOUTS[actor]);
        env.send(
            env.close_portfolio_ix(portfolios[actor]),
            vec![
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[actor], false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
    }
    assert_cu_within("two-wave user payouts", peak, 400_000);
    RecreditFixture {
        env,
        admin,
        beneficiary,
        owners,
        portfolios,
        tokens,
        reserve,
        destination,
        peak,
    }
}

#[test]
fn v16_program_terminal_scan_rediscovers_remaining_insurance_across_two_expiry_waves() {
    run_two_wave_scan(false);
}

#[test]
fn v16_program_native_terminal_scan_caps_second_recredit_after_paid_prefix_and_sync() {
    run_two_wave_scan(true);
}

fn run_two_wave_scan(native: bool) {
    let mut peak = 0;
    let mut commits = 0;
    let mut rollbacks = 0;
    let mut rediscoveries = 0;
    for side in 0..2 {
        for second in [41u64, 107] {
            let backing = [FIRST, second];
            let total: u64 = backing.iter().sum();
            let RecreditFixture {
                mut env,
                admin,
                beneficiary,
                owners,
                portfolios,
                tokens,
                reserve,
                destination,
                peak: fixture_peak,
            } = two_wave_fixture(side, backing, native);
            peak = peak.max(fixture_peak);
            let insurer = beneficiary.pubkey();
            drop(beneficiary);
            let initial_sequences = env.control_sequences(0);
            let initial_ledger = env.market_state().1.resolved_payout_ledger;
            let mut tracked = vec![
                env.market,
                env.vault,
                env.mint,
                reserve,
                destination,
                admin.pubkey(),
                insurer,
                solana_sdk::sysvar::clock::ID,
            ];
            tracked.extend(tokens);
            tracked.extend(portfolios);
            tracked.extend(owners.each_ref().map(Signer::pubkey));
            let total_lamports = |env: &V16CuEnv| {
                tracked
                    .iter()
                    .filter_map(|key| env.svm.get_account(key))
                    .map(|account| account.lamports)
                    .sum::<u64>()
            };
            let initial_lamports = total_lamports(&env);
            let initial_mint = env.svm.get_account(&env.mint);
            let custody_keys = [env.vault, reserve, destination];
            let initial_custody = custody_keys.map(|key| env.svm.get_account(&key).unwrap());
            let initial_tokens = tokens.map(|key| env.svm.get_account(&key));
            let native_custody = |env: &V16CuEnv, amounts: [u64; 3], closed: bool| {
                if !native {
                    return;
                }
                for (index, amount) in amounts.into_iter().enumerate() {
                    if closed && index == 0 {
                        continue;
                    }
                    let mut expected = initial_custody[index].clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    assert_eq!(token.amount, if index == 0 { total } else { 0 });
                    let COption::Some(rent) = token.is_native else {
                        panic!("native custody must retain its rent reserve");
                    };
                    assert_eq!(expected.lamports, rent + token.amount);
                    token.amount = amount;
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    expected.lamports = rent + amount;
                    assert_eq!(env.svm.get_account(&custody_keys[index]), Some(expected));
                }
                assert_eq!(tokens.map(|key| env.svm.get_account(&key)), initial_tokens);
                assert_eq!(env.svm.get_account(&env.mint), initial_mint);
                assert_eq!(total_lamports(env), initial_lamports);
            };
            let close = |env: &V16CuEnv| {
                let mut ix = wrap(
                    env,
                    ProgInstruction::CloseSlab {
                        authority_epoch: env.control_sequences(0).authority_epoch,
                    },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new(destination, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(env.mint, false),
                    ],
                );
                if native {
                    ix.accounts.push(AccountMeta::new(reserve, false));
                }
                ix
            };
            let bad = Instruction {
                program_id: system_program::ID,
                accounts: vec![],
                data: vec![255],
            };
            let lock = InstructionError::Custom(PercolatorError::EngineLockActive as u32);
            let mut send = |env: &mut V16CuEnv,
                            ixs: &[Instruction],
                            signers: &[&Keypair],
                            changed: &[Pubkey],
                            rejection: Option<(u8, InstructionError)>,
                            successes| {
                if rejection.is_some() {
                    rollbacks += 1;
                } else {
                    commits += 1;
                }
                peak = peak.max(land(
                    env, ixs, signers, &tracked, changed, rejection, successes,
                ));
            };
            let check = |env: &V16CuEnv,
                         expired: usize,
                         restored: u64,
                         paid: u64,
                         payments: u64,
                         cursor: u128,
                         surplus: u64| {
                let (cfg, group) = env.market_state();
                let market = env.svm.get_account(&env.market).unwrap();
                let header = market_group_header_bytes(&market.data);
                let released: u64 = backing[..expired].iter().sum();
                let fresh = u128::from(total - released) * BOUND_SCALE;
                assert_eq!(cfg.terminal_slab_scan_progress, cursor);
                assert_eq!(group.mode, MarketModeV16::Resolved);
                assert_eq!(
                    (
                        group.c_tot,
                        group.pnl_pos_tot,
                        group.materialized_portfolio_count
                    ),
                    (0, 0, 0)
                );
                assert!(group
                    .assets
                    .iter()
                    .all(|a| a.oi_eff_long_q == 0 && a.oi_eff_short_q == 0));
                assert_eq!(
                    (group.vault, group.insurance),
                    ((total - paid).into(), (restored - paid).into())
                );
                assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
                assert_eq!(env.token_amount(reserve), paid);
                assert_eq!(env.token_amount(destination), 0);
                assert_eq!(env.token_amount(env.vault), total - paid + surplus);
                let supply = CAPITAL.iter().sum::<u64>() + SPENT + total;
                if !native {
                    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                    assert_eq!(mint.supply, supply);
                    assert_eq!(mint.mint_authority, COption::None);
                }
                assert_eq!(
                    PAYOUTS.iter().sum::<u64>() + paid + env.token_amount(env.vault),
                    supply + surplus
                );
                native_custody(env, [total - paid + surplus, paid, 0], false);
                for domain in 0..4 {
                    let bucket = group.source_backing_buckets[domain];
                    let source = group.source_credit[domain];
                    let expected_fresh = if domain >= 2 {
                        let wave = usize::from(domain != 2 + side);
                        assert_eq!(bucket.expiry_slot, DEADLINES[wave]);
                        assert_eq!(
                            bucket.status,
                            if wave < expired {
                                BackingBucketStatusV16::Expired
                            } else {
                                BackingBucketStatusV16::Fresh
                            }
                        );
                        if wave < expired {
                            0
                        } else {
                            u128::from(backing[wave]) * BOUND_SCALE
                        }
                    } else {
                        assert_ne!(bucket.status, BackingBucketStatusV16::Fresh);
                        0
                    };
                    assert_eq!(bucket.fresh_unliened_backing_num, expected_fresh);
                    assert_eq!(source.fresh_reserved_backing_num, expected_fresh);
                    assert_eq!(
                        (
                            bucket.valid_liened_backing_num,
                            bucket.utilization_fee_earnings,
                            source.valid_liened_backing_num,
                            source.positive_claim_bound_num
                        ),
                        (0, 0, 0, 0)
                    );
                    assert_eq!(
                        source.provider_receivable_num,
                        if domain == 1 - side {
                            u128::from(CAPITAL[1]) * BOUND_SCALE
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        group.insurance_domain_budget[domain],
                        if domain == side {
                            (SPENT - paid).into()
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        group.insurance_domain_spent[domain],
                        if domain == side {
                            (SPENT - restored).into()
                        } else {
                            0
                        }
                    );
                }
                assert_eq!(
                    group
                        .source_backing_buckets
                        .iter()
                        .map(|b| b.fresh_unliened_backing_num)
                        .sum::<u128>(),
                    fresh
                );
                assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
                let remaining = group
                    .insurance_domain_budget
                    .iter()
                    .zip(&group.insurance_domain_spent)
                    .map(|(budget, spent)| budget.checked_sub(*spent).unwrap())
                    .sum::<u128>();
                assert_eq!(remaining, group.insurance);
                assert_eq!(
                    header.insurance_domain_budget_remaining_total.get(),
                    remaining
                );
                let residual = group
                    .vault
                    .checked_sub(group.insurance + fresh / BOUND_SCALE)
                    .unwrap();
                let overlap = (group.source_credit[1 - side].provider_receivable_num / BOUND_SCALE)
                    .min(group.insurance_domain_spent[side])
                    .min(residual);
                assert_eq!(
                    group.insurance + overlap,
                    u128::from(released.min(SPENT) - paid)
                );
                let mut ledger = initial_ledger;
                ledger.snapshot_residual += u128::from(released);
                assert_eq!(group.resolved_payout_ledger, ledger);
                let mut sequences = initial_sequences;
                sequences.authority_epoch += payments;
                assert_eq!(env.control_sequences(0), sequences);
                crate::support::fuzz_model::assert_market_stock_census(
                    "two-wave scanner",
                    &group,
                    &market.data,
                    &[],
                    u128::from(env.token_amount(env.vault) - surplus),
                )
                .unwrap();
                crate::support::fuzz_model::assert_reservation_encumbrance_census(
                    "two-wave scanner",
                    &group,
                    &[],
                )
                .unwrap();
            };
            let market_only = [env.market];
            let payment = [env.market, env.vault, reserve];
            let mut paid = 0;
            let mut surplus = 0;
            for wave in 0..2 {
                let scan = close(&env);
                send(
                    &mut env,
                    &[scan.clone()],
                    &[&admin],
                    &market_only,
                    None,
                    (1, 0),
                );
                check(&env, wave, paid, paid, wave as u64, 1, surplus);
                if native && wave == 1 {
                    // The first payment stays final while new native custody crosses a saved prefix.
                    let donation =
                        system_instruction::transfer(&admin.pubkey(), &env.vault, NATIVE_SURPLUS);
                    let sync =
                        spl_token::instruction::sync_native(&spl_token::ID, &env.vault).unwrap();
                    send(
                        &mut env,
                        &[donation, sync],
                        &[&admin],
                        &[admin.pubkey(), custody_keys[0]],
                        None,
                        (0, 1),
                    );
                    surplus = NATIVE_SURPLUS;
                    check(&env, wave, paid, paid, wave as u64, 1, surplus);
                }
                let before_clock = env.svm.get_account(&env.market).unwrap();
                env.svm.warp_to_slot(DEADLINES[wave]);
                assert_eq!(env.svm.get_account(&env.market), Some(before_clock.clone()));
                check(&env, wave, paid, paid, wave as u64, 1, surplus);

                // In wave two, rollback must preserve the already committed first payment.
                send(
                    &mut env,
                    &[scan.clone(), scan.clone(), bad.clone()],
                    &[&admin],
                    &[],
                    Some((4, InstructionError::InvalidInstructionData)),
                    (2, 0),
                );
                check(&env, wave, paid, paid, wave as u64, 1, surplus);
                send(
                    &mut env,
                    &[scan.clone()],
                    &[&admin],
                    &market_only,
                    None,
                    (1, 0),
                );
                check(&env, wave + 1, paid, paid, wave as u64, 0, surplus);
                let start = MARKET_GROUP_OFF
                    + std::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
                let end =
                    start + std::mem::size_of::<percolator::Market<state::AssetOracleStorageV16>>();
                assert_eq!(
                    &env.svm.get_account(&env.market).unwrap().data[start..end],
                    &before_clock.data[start..end]
                );

                let restored = backing[..=wave].iter().sum::<u64>().min(SPENT);
                assert!(restored > paid);
                send(
                    &mut env,
                    &[scan.clone()],
                    &[&admin],
                    &market_only,
                    None,
                    (1, 0),
                );
                check(&env, wave + 1, restored, paid, wave as u64, 0, surplus);
                rediscoveries += 1;
                // A Fresh sibling permits prefix progress before its wait rejects.
                let blocked_scan = if wave == 0 {
                    vec![scan.clone(), scan]
                } else {
                    vec![scan]
                };
                send(
                    &mut env,
                    &blocked_scan,
                    &[&admin],
                    &[],
                    Some((1 + blocked_scan.len() as u8, lock.clone())),
                    (blocked_scan.len() - 1, 0),
                );
                check(&env, wave + 1, restored, paid, wave as u64, 0, surplus);
                let payout = wrap(
                    &env,
                    ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        authority_epoch: env.control_sequences(0).authority_epoch,
                        amount: (restored - paid).into(),
                    },
                    vec![
                        AccountMeta::new_readonly(insurer, false),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(reserve, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                send(
                    &mut env,
                    &[payout.clone(), bad.clone()],
                    &[],
                    &[],
                    Some((3, InstructionError::InvalidInstructionData)),
                    (1, 1),
                );
                check(&env, wave + 1, restored, paid, wave as u64, 0, surplus);
                send(&mut env, &[payout], &[], &payment, None, (1, 1));
                paid = restored;
                check(&env, wave + 1, paid, paid, wave as u64 + 1, 0, surplus);
            }
            assert_eq!(paid, total.min(SPENT));
            let final_close = close(&env);
            let market = env.svm.get_account(&env.market).unwrap();
            let vault = env.svm.get_account(&env.vault).unwrap();
            let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
            let mut expected_mint = env.svm.get_account(&env.mint).unwrap();
            if !native {
                let mut mint = Mint::unpack(&expected_mint.data).unwrap();
                mint.supply -= total - paid;
                Mint::pack(mint, &mut expected_mint.data).unwrap();
            }
            let retired = if native { total - paid } else { 0 };
            let spl_successes = 1 + usize::from(total != paid) + usize::from(surplus != 0);
            send(
                &mut env,
                &[final_close.clone(), bad],
                &[&admin],
                &[],
                Some((3, InstructionError::InvalidInstructionData)),
                (1, spl_successes),
            );
            check(&env, 2, paid, paid, 2, 0, surplus);
            let mut closing = vec![
                env.market,
                env.vault,
                if native { destination } else { env.mint },
                admin.pubkey(),
            ];
            if native && retired != 0 {
                closing.push(reserve);
            }
            send(
                &mut env,
                &[final_close],
                &[&admin],
                &closing,
                None,
                (1, spl_successes),
            );
            let tombstone = env.svm.get_account(&env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                env.svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            expected_admin.lamports +=
                market.lamports - tombstone.lamports + vault.lamports - surplus - retired;
            assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
            assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
            assert!(env
                .svm
                .get_account(&env.vault)
                .map_or(true, |a| a.lamports == 0));
            assert_eq!(tokens.map(|key| env.token_amount(key)), PAYOUTS);
            assert_eq!(env.token_amount(reserve), paid + retired);
            assert_eq!(env.token_amount(destination), surplus);
            native_custody(&env, [0, paid + retired, surplus], true);
        }
    }
    assert_eq!(
        (commits, rollbacks, rediscoveries),
        (36 + if native { 4 } else { 0 }, 28, 8)
    );
    println!("INV-070 two-wave scan: native={native}, 4 histories, {commits} commits, {rollbacks} exact rollbacks, {rediscoveries} scanner rediscoveries, peak={peak} CU");
}
