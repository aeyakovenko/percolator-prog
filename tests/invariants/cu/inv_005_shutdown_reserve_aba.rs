//! INV-005/007/024/027/055/080/081: oracle consent does not move reserve ownership.
//! Committed and rolled-back ABA histories commute with shutdown for incumbent
//! reserve exits. The market-authority fallback is outside this local-role oracle.

use super::*;

const BACKING: u128 = 41;
const INSURANCE: u128 = 59;
const PEER: u128 = 23;
const USER: u128 = 31;

fn insurance_exit(
    env: &V16CuEnv,
    signer: Pubkey,
    wallet: Pubkey,
    ledger: Pubkey,
    asset: u16,
    epoch: u64,
    amount: u128,
) -> Instruction {
    let mut ix = withdrawal(env, signer, wallet, asset * 2, amount, epoch);
    ix.accounts.push(AccountMeta::new(ledger, false));
    ix.data = ProgInstruction::WithdrawInsuranceAsset {
        asset_index: asset,
        market_id: env.asset_market_id(asset),
        authority_epoch: epoch,
        amount,
    }
    .encode();
    ix
}

fn backing_exit(
    env: &V16CuEnv,
    signer: Pubkey,
    wallet: Pubkey,
    ledger: Pubkey,
    domain: u16,
    epoch: u64,
    amount: u128,
) -> Instruction {
    let mut ix = withdrawal(env, signer, wallet, domain, amount, epoch);
    ix.accounts.push(AccountMeta::new(ledger, false));
    ix
}

fn rotate(env: &V16CuEnv, from: Pubkey, to: Pubkey, asset: u16, epoch: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(from, true),
            AccountMeta::new_readonly(to, true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            kind: processor::ASSET_AUTH_ORACLE,
            new_pubkey: to.to_bytes(),
        }
        .encode(),
    }
}

#[test]
fn v16_program_shutdown_and_funded_oracle_aba_preserve_separate_reserve_owners() {
    let mut worlds = 0;
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for side in [0u16, 1] {
            let mut outcomes = Vec::new();
            for shutdown_first in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let a = Keypair::new();
                let b = Keypair::new();
                let insurer = Keypair::new();
                let user = Keypair::new();
                let actors = [&a, &b, &insurer, &user, &admin];
                for actor in [&a, &b, &insurer, &user] {
                    env.svm.airdrop(&actor.pubkey(), 1_000_000_000).unwrap();
                }
                for index in [0u16, 1] {
                    for (kind, holder) in [
                        (processor::ASSET_AUTH_ORACLE, &a),
                        (processor::ASSET_AUTH_BACKING_BUCKET, &a),
                        (processor::ASSET_AUTH_INSURANCE, &insurer),
                        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &insurer),
                    ] {
                        env.try_update_per_asset_authority_with_cu(
                            &admin,
                            Some(holder),
                            index,
                            kind,
                            holder.pubkey().to_bytes(),
                        )
                        .unwrap();
                    }
                    env.configure_auth_mark_for_asset_with_authority(index, &a, 0, 100);
                }
                env.configure_permissionless_resolve_with_cu(100, 5);
                let wallets = actors.map(|actor| {
                    create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
                });
                for (wallet, amount) in [
                    (wallets[0], BACKING),
                    (wallets[2], INSURANCE + PEER),
                    (wallets[3], USER),
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
                let ledger_keys = [Keypair::new(), Keypair::new()];
                let ledgers = ledger_keys.each_ref().map(Signer::pubkey);
                for (key, len) in ledger_keys.iter().zip([
                    state::backing_domain_ledger_account_len(),
                    state::insurance_ledger_account_len(),
                ]) {
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        key,
                        len,
                        env.program_id,
                    );
                }
                let domain = asset * 2 + side;
                let seq = env.control_sequences(asset as usize);
                env.send(
                    ProgInstruction::TopUpBackingBucket {
                        domain,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: seq.authority_epoch,
                        intent_id: next_control_sequence(seq.backing_top_up),
                        backing_fee_bps: 0,
                        insurance_share_bps: 0,
                        amount: BACKING,
                        expiry_slot: 10_000,
                    },
                    vec![
                        AccountMeta::new(a.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallets[0], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(ledgers[0], false),
                    ],
                    &[&a],
                )
                .unwrap();
                for (funded_domain, amount, ledger) in [
                    (asset * 2, INSURANCE, Some(ledgers[1])),
                    ((1 - asset) * 2, PEER, None),
                ] {
                    let seq = env.control_sequences((funded_domain / 2) as usize);
                    let mut accounts = vec![
                        AccountMeta::new(insurer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(wallets[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ];
                    if let Some(ledger) = ledger {
                        accounts.push(AccountMeta::new(ledger, false));
                    }
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: funded_domain,
                            market_id: env.asset_market_id(funded_domain / 2),
                            authority_epoch: seq.authority_epoch,
                            intent_id: next_control_sequence(seq.insurance_top_up),
                            amount,
                        },
                        accounts,
                        &[&insurer],
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
                env.portfolios.push(portfolio);
                env.send(
                    env.deposit_ix(portfolio, USER),
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
                let vault = env.vault;
                let mut protected = vec![market, vault, env.mint, portfolio];
                protected.extend(wallets);
                protected.extend(ledgers);
                protected.extend(actors.map(Signer::pubkey));
                let initial_profile = profile(&env, asset as usize);
                let peer_profile = profile(&env, (1 - asset) as usize);
                let peer_sequences = env.control_sequences((1 - asset) as usize);
                let old_epoch = env.control_sequences(asset as usize).authority_epoch;
                let assert_stock = |env: &V16CuEnv, paid: [u128; 2], user_paid: u128| {
                    let (cfg, group) = env.market_state();
                    assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.c_tot, USER - user_paid);
                    assert_eq!(group.insurance, INSURANCE + PEER - paid[1]);
                    assert_eq!(
                        group.insurance_domain_budget[asset as usize * 2],
                        INSURANCE - paid[1]
                    );
                    assert_eq!(
                        group.insurance_domain_budget[(1 - asset) as usize * 2],
                        PEER
                    );
                    assert_eq!(
                        group.source_backing_buckets[domain as usize].fresh_unliened_backing_num,
                        (BACKING - paid[0]) * BOUND_SCALE
                    );
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(group.backing_provider_earnings_total, 0);
                    for state in &group.assets {
                        assert_eq!((state.oi_eff_long_q, state.oi_eff_short_q), (0, 0));
                    }
                    let p = env.portfolio_state(portfolio);
                    assert_eq!(p.owner, user.pubkey().to_bytes());
                    assert_eq!(p.capital.get(), USER - user_paid);
                    assert_eq!(p.pnl.get(), 0);
                    assert_eq!(profile(env, (1 - asset) as usize), peer_profile);
                    assert_eq!(env.control_sequences((1 - asset) as usize), peer_sequences);
                    let p = profile(env, asset as usize);
                    assert_eq!(p.oracle_authority, a.pubkey().to_bytes());
                    assert_eq!(p.backing_bucket_authority, a.pubkey().to_bytes());
                    assert_eq!(p.insurance_authority, insurer.pubkey().to_bytes());
                    assert_eq!(p.insurance_operator, insurer.pubkey().to_bytes());
                    assert_eq!(p.asset_admin, initial_profile.asset_admin);
                    let backing = state::read_backing_domain_ledger(
                        &env.svm.get_account(&ledgers[0]).unwrap().data,
                    )
                    .unwrap();
                    assert_eq!(backing.market_group, market.to_bytes());
                    assert_eq!(backing.authority, a.pubkey().to_bytes());
                    assert_eq!(backing.domain, domain);
                    assert_eq!(backing.total_deposited_atoms, BACKING);
                    assert_eq!(backing.total_principal_atoms, BACKING - paid[0]);
                    assert_eq!(backing.total_principal_withdrawn_atoms, paid[0]);
                    assert_eq!(
                        (
                            backing.cumulative_loss_atoms,
                            backing.cumulative_recovery_atoms
                        ),
                        (0, 0)
                    );
                    let insurance = state::read_insurance_ledger(
                        &env.svm.get_account(&ledgers[1]).unwrap().data,
                    )
                    .unwrap();
                    assert_eq!(insurance.market_group, market.to_bytes());
                    assert_eq!(insurance.authority, insurer.pubkey().to_bytes());
                    assert_eq!(insurance.total_deposited_atoms, INSURANCE);
                    assert_eq!(insurance.total_principal_atoms, INSURANCE - paid[1]);
                    assert_eq!(insurance.total_withdrawn_atoms, paid[1]);
                    assert_eq!(insurance.last_observed_insurance_atoms, INSURANCE - paid[1]);
                    assert_eq!(
                        (
                            insurance.cumulative_loss_atoms,
                            insurance.cumulative_profit_atoms
                        ),
                        (0, 0)
                    );
                    let balances = [paid[0], 0, paid[1], user_paid, 0];
                    for ((wallet, actor), amount) in wallets.iter().zip(actors).zip(balances) {
                        let token =
                            TokenAccount::unpack(&env.svm.get_account(wallet).unwrap().data)
                                .unwrap();
                        assert_eq!(token.owner, actor.pubkey());
                        assert_eq!(token.mint, env.mint);
                        assert_eq!(token.amount as u128, amount);
                    }
                    let remaining =
                        BACKING + INSURANCE + PEER + USER - paid.iter().sum::<u128>() - user_paid;
                    assert_eq!(group.vault, remaining);
                    assert_eq!(env.token_amount(vault) as u128, remaining);
                    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                    assert_eq!(mint.mint_authority, COption::None);
                    assert_eq!(
                        mint.supply as u128,
                        remaining + balances.iter().sum::<u128>()
                    );
                    assert_eq!(mint.supply as u128, BACKING + INSURANCE + PEER + USER);
                };
                let shutdown = |env: &mut V16CuEnv| {
                    env.svm.warp_to_slot(1);
                    env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        asset,
                        1,
                        0,
                    );
                    env.svm.warp_to_slot(6);
                    assert_eq!(
                        env.market_state().1.assets[asset as usize].lifecycle,
                        AssetLifecycleV16::Recovery
                    );
                };
                if shutdown_first {
                    shutdown(&mut env);
                }
                assert_stock(&env, [0, 0], 0);
                let insurance = insurance_exit(
                    &env,
                    insurer.pubkey(),
                    wallets[2],
                    ledgers[1],
                    asset,
                    old_epoch,
                    7,
                );
                let backing = backing_exit(
                    &env,
                    a.pubkey(),
                    wallets[0],
                    ledgers[0],
                    domain,
                    old_epoch,
                    5,
                );
                let aba = [
                    rotate(&env, a.pubkey(), b.pubkey(), asset, old_epoch),
                    rotate(&env, b.pubkey(), a.pubkey(), asset, old_epoch + 1),
                ];
                let meta = land(
                    &mut env,
                    &[
                        insurance.clone(),
                        aba[0].clone(),
                        aba[1].clone(),
                        backing.clone(),
                    ],
                    &[&insurer, &a, &b],
                    &protected,
                    &[],
                    Some((5, PercolatorError::EngineStale)),
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                assert_eq!(
                    meta.logs
                        .iter()
                        .filter(|line| **line
                            == format!(
                                "Program {market_program} success",
                                market_program = env.program_id
                            ))
                        .count(),
                    3
                );
                assert!(meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", spl_token::ID)));
                assert_eq!(
                    env.control_sequences(asset as usize).authority_epoch,
                    old_epoch
                );
                assert_stock(&env, [0, 0], 0);
                land(&mut env, &aba, &[&a, &b], &protected, &[market], None);
                assert_eq!(
                    env.control_sequences(asset as usize).authority_epoch,
                    old_epoch + 2
                );
                assert_stock(&env, [0, 0], 0);
                let fresh_insurance = insurance_exit(
                    &env,
                    insurer.pubkey(),
                    wallets[2],
                    ledgers[1],
                    asset,
                    old_epoch + 2,
                    7,
                );
                land(
                    &mut env,
                    &[fresh_insurance.clone(), backing],
                    &[&insurer, &a],
                    &protected,
                    &[],
                    Some((3, PercolatorError::EngineStale)),
                );
                land(
                    &mut env,
                    &[insurance],
                    &[&insurer],
                    &protected,
                    &[],
                    Some((2, PercolatorError::EngineStale)),
                );
                assert_stock(&env, [0, 0], 0);
                let fresh_backing = backing_exit(
                    &env,
                    a.pubkey(),
                    wallets[0],
                    ledgers[0],
                    domain,
                    old_epoch + 2,
                    5,
                );
                let changed = [
                    market, vault, wallets[0], wallets[2], ledgers[0], ledgers[1],
                ];
                land(
                    &mut env,
                    &[fresh_insurance, fresh_backing],
                    &[&insurer, &a],
                    &protected,
                    &changed,
                    None,
                );
                assert_stock(&env, [5, 7], 0);
                if !shutdown_first {
                    shutdown(&mut env);
                }
                assert_stock(&env, [5, 7], 0);
                let exits = [
                    backing_exit(
                        &env,
                        a.pubkey(),
                        wallets[0],
                        ledgers[0],
                        domain,
                        old_epoch + 2,
                        BACKING - 5,
                    ),
                    insurance_exit(
                        &env,
                        insurer.pubkey(),
                        wallets[2],
                        ledgers[1],
                        asset,
                        old_epoch + 2,
                        INSURANCE - 7,
                    ),
                ];
                let ordered = if shutdown_first {
                    [exits[1].clone(), exits[0].clone()]
                } else {
                    exits
                };
                land(
                    &mut env,
                    &ordered,
                    &[&a, &insurer],
                    &protected,
                    &changed,
                    None,
                );
                assert_stock(&env, [BACKING, INSURANCE], 0);
                let owner_exit = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(user.pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(portfolio, false),
                        AccountMeta::new(wallets[3], false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: env.withdraw_ix(portfolio, USER).encode(),
                };
                land(
                    &mut env,
                    &[owner_exit],
                    &[&user],
                    &protected,
                    &[market, vault, portfolio, wallets[3]],
                    None,
                );
                assert_stock(&env, [BACKING, INSURANCE], USER);
                outcomes.push((
                    wallets.map(|wallet| env.token_amount(wallet)),
                    env.token_amount(vault),
                    env.control_sequences(asset as usize).authority_epoch - old_epoch,
                ));
                worlds += 1;
            }
            assert_eq!(
                outcomes[0], outcomes[1],
                "shutdown and payout order preserve owner-indexed outcomes"
            );
        }
    }
    assert_eq!(worlds, 8);
    eprintln!("INV-005/024 shutdown oracle ABA: worlds=8, rejected_bundles=16, rejected_retained_insurance=8, committed_aba=8, reserve_payouts=32, user_exits=8, peak_aba_rollback_cu={peak_cu}");
}
