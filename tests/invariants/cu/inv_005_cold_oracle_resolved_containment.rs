//! Row 416: cold oracle replacement commutes with resolution without transferring
//! funded backing or the distinct live/terminal insurance recipients. Cold-admin
//! burn then preserves unsigned reserve recovery and invalidates retained control.

use super::*;

const INSURANCE: [u128; 2] = [13, 17];
const LIVE_PAID: u128 = 5;
const BACKING_PREFIX: u128 = 3;
const ROLES: [u8; 3] = [
    processor::ASSET_AUTH_BACKING_BUCKET,
    processor::ASSET_AUTH_INSURANCE,
    processor::ASSET_AUTH_INSURANCE_OPERATOR,
];

fn manage(
    env: &V16CuEnv,
    asset: u16,
    from: Pubkey,
    to: Option<Pubkey>,
    kind: u8,
    epoch: u64,
) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(from, true),
            AccountMeta::new_readonly(to.unwrap_or(env.payer.pubkey()), to.is_some()),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            kind,
            new_pubkey: to.map(|key| key.to_bytes()).unwrap_or_default(),
        }
        .encode(),
    }
}

fn insurance_exit(
    env: &V16CuEnv,
    asset: u16,
    holder: Pubkey,
    wallet: Pubkey,
    epoch: u64,
    amount: u128,
    signed: bool,
) -> Instruction {
    let mut ix = withdrawal(env, holder, wallet, asset * 2, amount, epoch);
    ix.accounts[0].is_signer = signed;
    ix.data = ProgInstruction::WithdrawInsuranceAsset {
        asset_index: asset,
        market_id: env.asset_market_id(asset),
        authority_epoch: epoch,
        amount,
    }
    .encode();
    ix
}

fn completed(meta: &litesvm::types::TransactionMetadata, program: Pubkey, count: usize) {
    assert_eq!(
        meta.logs
            .iter()
            .filter(|line| **line == format!("Program {program} success"))
            .count(),
        count,
        "{meta:?}"
    );
}

#[test]
fn v16_program_cold_oracle_resolution_and_admin_burn_preserve_split_funded_recipients() {
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    for asset in [0u16, 1] {
        for oracle_is_beneficiary in [false, true] {
            let mut outcomes = Vec::new();
            for resolve_first in [false, true] {
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                // 0: oracle/provider; 1: other insurance role; 2: cold; 3: new oracle.
                let actors: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
                for actor in &actors {
                    env.ensure_signer_account(actor.pubkey());
                }
                let beneficiary = usize::from(!oracle_is_beneficiary);
                let operator = 1 - beneficiary;
                env.svm.warp_to_slot(1);
                for index in [0u16, 1] {
                    env.configure_auth_mark_for_asset_as_admin(index, 1, 100);
                }
                for (role, holder) in [
                    (processor::ASSET_AUTH_BACKING_BUCKET, 0),
                    (processor::ASSET_AUTH_INSURANCE, beneficiary),
                    (processor::ASSET_AUTH_INSURANCE_OPERATOR, operator),
                    (processor::ASSET_AUTH_ORACLE, 0),
                    (processor::ASSET_AUTH_ADMIN, 2),
                ] {
                    env.try_update_per_asset_authority_with_cu(
                        &admin,
                        Some(&actors[holder]),
                        asset,
                        role,
                        actors[holder].pubkey().to_bytes(),
                    )
                    .unwrap();
                }
                let wallets = actors.each_ref().map(|actor| {
                    create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint)
                });
                for (holder, amount) in [(0, PRINCIPAL.iter().sum()), (beneficiary, 30)] {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &wallets[holder],
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
                for side in 0..2 {
                    for backing in [true, false] {
                        let seq = env.control_sequences(asset as usize);
                        let holder = if backing { 0 } else { beneficiary };
                        let data = if backing {
                            ProgInstruction::TopUpBackingBucket {
                                domain: asset * 2 + side as u16,
                                market_id: env.asset_market_id(asset),
                                authority_epoch: seq.authority_epoch,
                                intent_id: next_control_sequence(seq.backing_top_up),
                                backing_fee_bps: 0,
                                insurance_share_bps: 0,
                                amount: PRINCIPAL[side],
                                expiry_slot: 10_000,
                            }
                        } else {
                            ProgInstruction::TopUpInsuranceDomain {
                                domain: asset * 2 + side as u16,
                                market_id: env.asset_market_id(asset),
                                authority_epoch: seq.authority_epoch,
                                intent_id: next_control_sequence(seq.insurance_top_up),
                                amount: INSURANCE[side],
                            }
                        };
                        env.send(
                            data,
                            vec![
                                AccountMeta::new(actors[holder].pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(wallets[holder], false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            &[&actors[holder]],
                        )
                        .unwrap();
                    }
                }
                let market = env.market;
                let vault = env.vault;
                let mut tracked = vec![market, env.vault, env.mint, env.vault_authority];
                tracked.extend(wallets);
                tracked.extend(actors.each_ref().map(Signer::pubkey));
                tracked.push(admin.pubkey());
                let peer = (1 - asset) as usize;
                let peer_profile = profile(&env, peer);
                let peer_sequences = env.control_sequences(peer);
                let mut expected_profile = profile(&env, asset as usize);
                let mut sequences = env.control_sequences(asset as usize);
                let mut paid = [0u128; 4];
                let mut remaining = PRINCIPAL;
                let assert_book = |env: &V16CuEnv,
                                   mode: MarketModeV16,
                                   remaining: [u128; 2],
                                   insurance: [u128; 2],
                                   paid: [u128; 4]| {
                    let (cfg, group) = env.market_state();
                    assert_eq!(cfg.marketauth, admin.pubkey().to_bytes());
                    assert_eq!(group.mode, mode);
                    assert_eq!(group.c_tot, 0);
                    assert_eq!(group.materialized_portfolio_count, 0);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(group.backing_provider_earnings_total, 0);
                    assert_eq!(profile(env, peer), peer_profile);
                    assert_eq!(env.control_sequences(peer), peer_sequences);
                    for asset in &group.assets {
                        assert_eq!((asset.oi_eff_long_q, asset.oi_eff_short_q), (0, 0));
                    }
                    for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
                        let own = domain / 2 == asset as usize;
                        let backing = if own { remaining[domain % 2] } else { 0 };
                        assert_eq!(bucket.fresh_unliened_backing_num, backing * BOUND_SCALE);
                        assert_eq!(bucket.valid_liened_backing_num, 0);
                        assert_eq!(bucket.consumed_liened_backing_num, 0);
                        assert_eq!(bucket.impaired_liened_backing_num, 0);
                        assert_eq!(bucket.utilization_fee_earnings, 0);
                        assert_eq!(
                            group.insurance_domain_budget[domain],
                            if own { insurance[domain % 2] } else { 0 }
                        );
                        assert_eq!(group.insurance_domain_spent[domain], 0);
                    }
                    let reserve = insurance.iter().sum::<u128>();
                    assert_eq!(group.insurance, reserve);
                    assert_eq!(group.insurance_domain_budget_remaining_total, reserve);
                    let vault = remaining.iter().sum::<u128>() + reserve;
                    assert_eq!(group.vault, vault);
                    assert_eq!(env.token_amount(env.vault) as u128, vault);
                    assert_eq!(wallets.map(|key| env.token_amount(key) as u128), paid);
                    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
                    assert_eq!(mint.mint_authority, COption::None);
                    assert_eq!(mint.supply, 76);
                    assert_eq!(vault + paid.iter().sum::<u128>(), 76);
                };
                assert_book(&env, MarketModeV16::Live, remaining, INSURANCE, paid);
                let live = insurance_exit(
                    &env,
                    asset,
                    actors[operator].pubkey(),
                    wallets[operator],
                    sequences.authority_epoch,
                    LIVE_PAID,
                    true,
                );
                let meta = land(
                    &mut env,
                    &[live.clone()],
                    &[&actors[operator]],
                    &tracked,
                    &[market, vault, wallets[operator]],
                    None,
                );
                peak = peak.max(meta.compute_units_consumed);
                sequences.authority_epoch += 1;
                paid[operator] += LIVE_PAID;
                let insurance = [INSURANCE[0] - LIVE_PAID, INSURANCE[1]];
                assert_book(&env, MarketModeV16::Live, remaining, insurance, paid);

                // Retain role-local instructions before the unrelated oracle epoch changes.
                let holders = [0, beneficiary, operator];
                let retained: [Instruction; 3] = std::array::from_fn(|index| {
                    let holder = holders[index];
                    manage(
                        &env,
                        asset,
                        actors[holder].pubkey(),
                        Some(actors[holder].pubkey()),
                        ROLES[index],
                        sequences.authority_epoch,
                    )
                });
                for (ix, holder) in retained.iter().zip(holders) {
                    let tx = Transaction::new_signed_with_payer(
                        &[heap_ix(), cu_ix(), ix.clone()],
                        Some(&env.payer.pubkey()),
                        &[&env.payer, &actors[holder]],
                        env.svm.latest_blockhash(),
                    );
                    env.svm
                        .simulate_transaction(tx.into())
                        .expect("funded incumbent self-handoff is valid before oracle replacement");
                }
                let epoch = sequences.authority_epoch;
                let oracle = manage(
                    &env,
                    asset,
                    actors[2].pubkey(),
                    Some(actors[3].pubkey()),
                    processor::ASSET_AUTH_ORACLE,
                    epoch,
                );
                let resolve = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(market, false),
                    ],
                    data: ProgInstruction::ResolveMarket {
                        asset_generation_frontier: env.market_state().1.next_market_id,
                        authority_epoch: env.control_sequences(0).authority_epoch
                            + u64::from(asset == 0 && !resolve_first),
                    }
                    .encode(),
                };
                let mut backing = withdrawal(
                    &env,
                    actors[0].pubkey(),
                    wallets[0],
                    asset * 2,
                    BACKING_PREFIX,
                    epoch + 1,
                );
                backing.accounts[0].is_signer = false;
                let prefix = if resolve_first {
                    vec![resolve, oracle, backing]
                } else {
                    vec![oracle, resolve, backing]
                };
                for role in ROLES {
                    let mut bundle = prefix.clone();
                    bundle.push(manage(
                        &env,
                        asset,
                        actors[2].pubkey(),
                        Some(actors[3].pubkey()),
                        role,
                        epoch + 1,
                    ));
                    let meta = land(
                        &mut env,
                        &bundle,
                        &[&actors[2], &actors[3], &admin],
                        &tracked,
                        &[],
                        Some((5, PercolatorError::EngineLockActive)),
                    );
                    completed(&meta, env.program_id, 3);
                    completed(&meta, spl_token::ID, 1);
                    peak = peak.max(meta.compute_units_consumed);
                    rollbacks += 1;
                    assert_eq!(profile(&env, asset as usize), expected_profile);
                    assert_eq!(env.control_sequences(asset as usize), sequences);
                    assert_book(&env, MarketModeV16::Live, remaining, insurance, paid);
                }
                let meta = land(
                    &mut env,
                    &prefix,
                    &[&actors[2], &actors[3], &admin],
                    &tracked,
                    &[market, vault, wallets[0]],
                    None,
                );
                peak = peak.max(meta.compute_units_consumed);
                remaining[0] -= BACKING_PREFIX;
                paid[0] += BACKING_PREFIX;
                sequences.authority_epoch += 1;
                expected_profile.oracle_authority = actors[3].pubkey().to_bytes();
                assert_eq!(profile(&env, asset as usize), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), sequences);
                assert_book(&env, MarketModeV16::Resolved, remaining, insurance, paid);

                // A retained live payout and its current-epoch form cannot follow the
                // operator into the beneficiary-only terminal route.
                for ix in [
                    live,
                    insurance_exit(
                        &env,
                        asset,
                        actors[operator].pubkey(),
                        wallets[operator],
                        sequences.authority_epoch,
                        LIVE_PAID,
                        true,
                    ),
                ] {
                    let meta = land(
                        &mut env,
                        &[ix],
                        &[&actors[operator]],
                        &tracked,
                        &[],
                        Some((2, PercolatorError::InvalidTokenAccount)),
                    );
                    peak = peak.max(meta.compute_units_consumed);
                    rollbacks += 1;
                }
                let retained_cold = manage(
                    &env,
                    asset,
                    actors[2].pubkey(),
                    Some(actors[3].pubkey()),
                    processor::ASSET_AUTH_ORACLE,
                    sequences.authority_epoch,
                );
                let tx = Transaction::new_signed_with_payer(
                    &[heap_ix(), cu_ix(), retained_cold.clone()],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, &actors[2], &actors[3]],
                    env.svm.latest_blockhash(),
                );
                env.svm
                    .simulate_transaction(tx.into())
                    .expect("cold oracle control is valid immediately before burn");
                let burn = manage(
                    &env,
                    asset,
                    actors[2].pubkey(),
                    None,
                    processor::ASSET_AUTH_ADMIN,
                    sequences.authority_epoch,
                );
                let meta = land(&mut env, &[burn], &[&actors[2]], &tracked, &[market], None);
                peak = peak.max(meta.compute_units_consumed);
                sequences.authority_epoch += 1;
                expected_profile.asset_admin = [0; 32];

                // Every rejection follows an actual terminal backing transfer.
                // The identical backing bytes remain usable after full rollback.
                let mut backing = withdrawal(
                    &env,
                    actors[0].pubkey(),
                    wallets[0],
                    asset * 2,
                    BACKING_PREFIX,
                    sequences.authority_epoch,
                );
                backing.accounts[0].is_signer = false;
                for (ix, holder) in retained.into_iter().zip(holders) {
                    let meta = land(
                        &mut env,
                        &[backing.clone(), ix],
                        &[&actors[holder]],
                        &tracked,
                        &[],
                        Some((3, PercolatorError::EngineStale)),
                    );
                    completed(&meta, spl_token::ID, 1);
                    peak = peak.max(meta.compute_units_consumed);
                    rollbacks += 1;
                }
                let current_cold = manage(
                    &env,
                    asset,
                    actors[2].pubkey(),
                    Some(actors[3].pubkey()),
                    processor::ASSET_AUTH_ORACLE,
                    sequences.authority_epoch,
                );
                for ix in [retained_cold, current_cold] {
                    let meta = land(
                        &mut env,
                        &[backing.clone(), ix],
                        &[&actors[2], &actors[3]],
                        &tracked,
                        &[],
                        Some((3, PercolatorError::Unauthorized)),
                    );
                    completed(&meta, spl_token::ID, 1);
                    peak = peak.max(meta.compute_units_consumed);
                    rollbacks += 1;
                }
                for role in ROLES {
                    let seize = manage(
                        &env,
                        asset,
                        actors[3].pubkey(),
                        Some(actors[3].pubkey()),
                        role,
                        sequences.authority_epoch,
                    );
                    let meta = land(
                        &mut env,
                        &[backing.clone(), seize],
                        &[&actors[3]],
                        &tracked,
                        &[],
                        Some((3, PercolatorError::Unauthorized)),
                    );
                    completed(&meta, spl_token::ID, 1);
                    peak = peak.max(meta.compute_units_consumed);
                    rollbacks += 1;
                }
                assert_eq!(profile(&env, asset as usize), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), sequences);
                assert_book(&env, MarketModeV16::Resolved, remaining, insurance, paid);
                let meta = land(
                    &mut env,
                    &[backing],
                    &[],
                    &tracked,
                    &[market, vault, wallets[0]],
                    None,
                );
                peak = peak.max(meta.compute_units_consumed);
                remaining[0] -= BACKING_PREFIX;
                paid[0] += BACKING_PREFIX;

                for (role, holder) in ROLES.into_iter().zip(holders) {
                    let current = manage(
                        &env,
                        asset,
                        actors[holder].pubkey(),
                        Some(actors[holder].pubkey()),
                        role,
                        sequences.authority_epoch,
                    );
                    let meta = land(
                        &mut env,
                        &[current],
                        &[&actors[holder]],
                        &tracked,
                        &[market],
                        None,
                    );
                    peak = peak.max(meta.compute_units_consumed);
                    sequences.authority_epoch += 1;
                    assert_eq!(profile(&env, asset as usize), expected_profile);
                    assert_eq!(env.control_sequences(asset as usize), sequences);
                }
                // No funded holder, cold admin, market admin or oracle signs recovery.
                let terminal = insurance_exit(
                    &env,
                    asset,
                    actors[beneficiary].pubkey(),
                    wallets[beneficiary],
                    sequences.authority_epoch,
                    30 - LIVE_PAID,
                    false,
                );
                let meta = land(
                    &mut env,
                    &[terminal],
                    &[],
                    &tracked,
                    &[market, vault, wallets[beneficiary]],
                    None,
                );
                peak = peak.max(meta.compute_units_consumed);
                sequences.authority_epoch += 1;
                paid[beneficiary] += 30 - LIVE_PAID;
                for side in 0..2 {
                    let mut ix = withdrawal(
                        &env,
                        actors[0].pubkey(),
                        wallets[0],
                        asset * 2 + side as u16,
                        remaining[side],
                        sequences.authority_epoch,
                    );
                    ix.accounts[0].is_signer = false;
                    let meta = land(
                        &mut env,
                        &[ix],
                        &[],
                        &tracked,
                        &[market, vault, wallets[0]],
                        None,
                    );
                    peak = peak.max(meta.compute_units_consumed);
                    paid[0] += remaining[side];
                    remaining[side] = 0;
                    assert_book(&env, MarketModeV16::Resolved, remaining, [0, 0], paid);
                }
                assert_eq!(profile(&env, asset as usize), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), sequences);
                let mut entitlement = [46, 0, 0, 0];
                entitlement[operator] += LIVE_PAID;
                entitlement[beneficiary] += 30 - LIVE_PAID;
                assert_eq!(paid, entitlement);
                outcomes.push(paid);
                worlds += 1;
            }
            assert_eq!(
                outcomes[0], outcomes[1],
                "oracle replacement and resolution commute for every recipient"
            );
        }
    }
    assert_eq!(worlds, 8);
    assert_eq!(rollbacks, 104);
    eprintln!("INV-005 Lane 24: worlds={worlds}, rollbacks={rollbacks}, SPL-prefix rollbacks=88, oracle replacements=8, resolutions=8, cold burns=8, current funded self-handoffs=24, unsigned payouts=40, peak_cu={peak}");
}
