//! Row 416: cold oracle replacement across valid -> impaired source backing.
//! The actual oracle also owns the liened backing. Oracle succession must commute
//! with expiry while open exposure, owner-local liens and paid exits stay attributed.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census,
};

const PAID_PREFIX: u128 = 5;
const SUPPLY: u128 = WINNER_DEPOSIT + COUNTERPARTY_DEPOSIT + BYSTANDER_DEPOSIT + BACKING;

fn manage(
    env: &V16CuEnv,
    asset: u16,
    from: Pubkey,
    to: Pubkey,
    kind: u8,
    epoch: u64,
) -> Instruction {
    wrap(
        env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            kind,
            new_pubkey: to.to_bytes(),
        },
        vec![
            AccountMeta::new(from, true),
            AccountMeta::new_readonly(to, true),
            AccountMeta::new(env.market, false),
        ],
    )
}

fn report(env: &V16CuEnv, asset: u16, signer: Pubkey, epoch: u64, mark: u64) -> Instruction {
    wrap(
        env,
        ProgInstruction::PushAuthMark {
            asset_index: asset,
            market_id: env.asset_market_id(asset),
            authority_epoch: epoch,
            observation_sequence: next_control_sequence(
                env.control_sequences(asset as usize).oracle_observation,
            ),
            now_slot: u64::MAX,
            mark_e6: mark,
        },
        vec![
            AccountMeta::new(signer, true),
            AccountMeta::new(env.market, false),
        ],
    )
}

fn cashout(
    env: &V16CuEnv,
    owner: Pubkey,
    portfolio: Pubkey,
    wallet: Pubkey,
    amount: u128,
) -> Instruction {
    wrap(
        env,
        env.withdraw_ix(portfolio, amount),
        vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(wallet, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    )
}

fn oracle_profile(env: &V16CuEnv, asset: u16) -> state::AssetOracleProfileV16 {
    state::read_asset_oracle_profile(
        &env.svm.get_account(&env.market).unwrap().data,
        asset as usize,
    )
    .unwrap()
}

fn expire_and_refresh(
    env: &mut V16CuEnv,
    portfolios: [Pubkey; 3],
    winner_long: bool,
    asset: u16,
    oracle: &Keypair,
    peak: &mut [u64; 3],
) {
    expire_lien(env, portfolios, winner_long, asset, oracle, peak);
    // Lien normalization and leg refresh are separate public crank actions.
    for portfolio in [portfolios[0], portfolios[1]] {
        for _ in 0..4 {
            if assert_current_certificate_matches_independent(
                "impaired oracle expiry refresh",
                &env.market_state().1,
                &env.portfolio_state(portfolio),
            )
            .unwrap()
            {
                break;
            }
            if let Some(cu) = env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: EXPIRY,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            ) {
                peak[0] = peak[0].max(cu);
            } else {
                break;
            }
        }
        assert!(assert_current_certificate_matches_independent(
            "impaired oracle bounded refresh",
            &env.market_state().1,
            &env.portfolio_state(portfolio),
        )
        .unwrap());
    }
}

#[test]
fn v16_program_cold_oracle_replacement_commutes_with_open_lien_impairment() {
    let mut peak = [0; 3]; // public setup/expiry, exact rollback, committed suffix
    let mut worlds = 0;
    let mut rollbacks = 0;
    for asset in [0u16, 1] {
        for winner_long in [false, true] {
            let mut outcomes = Vec::new();
            for expire_first in [false, true] {
                let LienedWorld {
                    mut env,
                    provider,
                    cold,
                    owners,
                    wallets: funded_wallets,
                    portfolios,
                } = liened_world(winner_long, asset, &mut peak);
                let admin = env.admin.insecure_clone();
                let incoming = Keypair::new();
                env.ensure_signer_account(incoming.pubkey());
                let mut wallets = funded_wallets.to_vec();
                wallets.push(create_ata_for_test(
                    &mut env.svm,
                    &env.payer,
                    incoming.pubkey(),
                    env.mint,
                ));
                let actors = [
                    &owners[0], &owners[1], &owners[2], &provider, &admin, &cold, &incoming,
                ];
                for (kind, holder) in [
                    (processor::ASSET_AUTH_ORACLE, &provider),
                    (processor::ASSET_AUTH_ADMIN, &cold),
                ] {
                    peak[0] = peak[0].max(
                        env.try_update_per_asset_authority_with_cu(
                            &admin,
                            Some(holder),
                            asset,
                            kind,
                            holder.pubkey().to_bytes(),
                        )
                        .unwrap(),
                    );
                }
                // The funded incumbent actually publishes before the cold handoff.
                peak[0] = peak[0].max(env.push_auth_mark_for_asset_with_authority(
                    asset,
                    &provider,
                    2,
                    if winner_long { 105 } else { 95 },
                ));
                let domain = (2 * asset + u16::from(winner_long)) as usize;
                let market = env.market;
                let vault = env.vault;
                let mut tracked = vec![market, vault, env.mint, env.vault_authority];
                tracked.extend(portfolios);
                tracked.extend(&wallets);
                tracked.extend(actors.map(Signer::pubkey));
                let mint_frame = env.svm.get_account(&env.mint);
                let program_frames = std::iter::once(market)
                    .chain(portfolios)
                    .map(|key| (key, env.svm.get_account(&key).unwrap()))
                    .collect::<Vec<_>>();
                let token_frames = wallets
                    .iter()
                    .chain(std::iter::once(&vault))
                    .map(|key| (*key, env.svm.get_account(key).unwrap()))
                    .collect::<Vec<_>>();
                let book = |env: &V16CuEnv, paid: u128, impaired: bool, reduced: bool| {
                    let group = env.market_state().1;
                    let accounts = portfolios.map(|key| env.portfolio_state(key));
                    assert_eq!(group.mode, MarketModeV16::Live);
                    for index in [0, 1] {
                        assert_eq!(group.assets[index].lifecycle, AssetLifecycleV16::Active);
                    }
                    for (key, frame) in &program_frames {
                        let after = env.svm.get_account(key).unwrap();
                        let mut expected = frame.clone();
                        expected.data.clone_from(&after.data);
                        assert_eq!(after.data.len(), frame.data.len());
                        assert_eq!(after, expected, "program Account metadata stays fixed");
                    }
                    for index in 0..3 {
                        assert_eq!(accounts[index].owner, owners[index].pubkey().to_bytes());
                    }
                    assert_eq!(env.svm.get_account(&env.mint), mint_frame);
                    let mint = Mint::unpack(&mint_frame.as_ref().unwrap().data).unwrap();
                    assert_eq!(mint.mint_authority, COption::None);
                    assert_eq!(mint.supply as u128, SUPPLY);
                    assert_eq!(group.vault, SUPPLY - paid);
                    for (key, frame) in &token_frames {
                        let amount = if *key == vault {
                            SUPPLY - paid
                        } else if *key == wallets[2] {
                            paid
                        } else {
                            0
                        };
                        let mut expected = frame.clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = amount as u64;
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(key).unwrap(), expected);
                    }
                    let profit =
                        (WINNING_SIZE_Q / POS_SCALE as i128) * if impaired { 6 } else { 5 };
                    let loss = (ADVERSE_SIZE_Q / POS_SCALE as i128) * 5;
                    for (index, capital) in [WINNER_DEPOSIT, COUNTERPARTY_DEPOSIT]
                        .into_iter()
                        .enumerate()
                    {
                        let sign = if index == 0 { 1 } else { -1 };
                        let direction = if winner_long { sign } else { -sign };
                        assert_eq!(
                            accounts[index].capital.get() as i128 + accounts[index].pnl.get(),
                            capital as i128 + sign * (profit - loss),
                            "input equity: asset={asset}, long={winner_long}, expire_first={expire_first}, paid={paid}, impaired={impaired}, reduced={reduced}, owner={index}"
                        );
                        for (index_asset, size) in [
                            (asset, WINNING_SIZE_Q),
                            (
                                1 - asset,
                                ADVERSE_SIZE_Q + if reduced { 0 } else { RISK_INCREASE_Q },
                            ),
                        ] {
                            assert_eq!(
                                active_leg_for_asset(&accounts[index], index_asset as usize)
                                    .basis_pos_q,
                                direction * size
                            );
                        }
                    }
                    assert_eq!(accounts[2].capital.get(), BYSTANDER_DEPOSIT - paid);
                    let local_lien = accounts
                        .iter()
                        .flat_map(|a| &a.source_domains)
                        .filter(|s| s.is_occupied() && s.domain.get() as usize == domain)
                        .map(|s| s.source_lien_counterparty_backing_num.get())
                        .sum::<u128>();
                    assert!(
                        local_lien > 0,
                        "the funding guard must protect a real owner-local lien"
                    );
                    let bucket = group.source_backing_buckets[domain];
                    let source = group.source_credit[domain];
                    assert_eq!(bucket.consumed_liened_backing_num, 0);
                    assert_eq!(bucket.utilization_fee_earnings, 0);
                    if impaired {
                        assert_eq!(bucket.status, BackingBucketStatusV16::Impaired);
                        assert_eq!(bucket.fresh_unliened_backing_num, 0);
                        assert_eq!(bucket.valid_liened_backing_num, 0);
                        assert_eq!(source.valid_liened_backing_num, 0);
                        assert_eq!(bucket.impaired_liened_backing_num, local_lien);
                        assert_eq!(source.impaired_liened_backing_num, local_lien);
                    } else {
                        assert_eq!(bucket.status, BackingBucketStatusV16::Fresh);
                        assert_eq!(bucket.valid_liened_backing_num, local_lien);
                        assert_eq!(source.valid_liened_backing_num, local_lien);
                        assert_eq!(bucket.impaired_liened_backing_num, 0);
                        assert_eq!(
                            bucket.fresh_unliened_backing_num + local_lien,
                            (BACKING + profit as u128) * BOUND_SCALE
                        );
                    }
                    // At expiry the counterparty nets the additional winning-leg
                    // loss against its peer claim, consuming that much peer backing.
                    let loss_domain = (2 * (1 - asset) + u16::from(!winner_long)) as usize;
                    let peer_consumed = if impaired {
                        WINNING_SIZE_Q / POS_SCALE as i128
                    } else {
                        0
                    };
                    let peer_reserve = loss - peer_consumed;
                    for (index, bucket) in group.source_backing_buckets.iter().enumerate() {
                        if index != domain {
                            assert_eq!(
                                bucket.fresh_unliened_backing_num,
                                if index == loss_domain {
                                    peer_reserve as u128 * BOUND_SCALE
                                } else {
                                    0
                                }
                            );
                            assert_eq!(bucket.valid_liened_backing_num, 0);
                            assert_eq!(bucket.impaired_liened_backing_num, 0);
                            assert_eq!(
                                bucket.consumed_liened_backing_num,
                                if index == loss_domain {
                                    peer_consumed as u128 * BOUND_SCALE
                                } else {
                                    0
                                }
                            );
                            assert_eq!(bucket.utilization_fee_earnings, 0);
                        }
                    }
                    assert_market_stock_census(
                        "cold oracle / open impaired backing",
                        &group,
                        &env.svm.get_account(&market).unwrap().data,
                        &accounts,
                        env.token_amount(vault) as u128,
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(
                        "cold oracle / local lien attribution",
                        &group,
                        &accounts,
                    )
                    .unwrap();
                };
                book(&env, 0, false, false);
                if expire_first {
                    expire_and_refresh(
                        &mut env,
                        portfolios,
                        winner_long,
                        asset,
                        &provider,
                        &mut peak,
                    );
                    book(&env, 0, true, false);
                }
                let mut profile = oracle_profile(&env, asset);
                assert_eq!(profile.oracle_authority, provider.pubkey().to_bytes());
                assert_eq!(
                    profile.backing_bucket_authority,
                    provider.pubkey().to_bytes()
                );
                assert_eq!(profile.asset_admin, cold.pubkey().to_bytes());
                let peer_profile = oracle_profile(&env, 1 - asset);
                let peer_sequences = env.control_sequences((1 - asset) as usize);
                let mut sequences = env.control_sequences(asset as usize);
                let before = env.market_state();
                let mark = match (winner_long, expire_first) {
                    (true, false) => 105,
                    (false, false) => 95,
                    (true, true) => 106,
                    (false, true) => 94,
                };
                let prefix = vec![
                    cashout(
                        &env,
                        owners[2].pubkey(),
                        portfolios[2],
                        wallets[2],
                        PAID_PREFIX,
                    ),
                    manage(
                        &env,
                        asset,
                        cold.pubkey(),
                        incoming.pubkey(),
                        processor::ASSET_AUTH_ORACLE,
                        sequences.authority_epoch,
                    ),
                    report(
                        &env,
                        asset,
                        incoming.pubkey(),
                        sequences.authority_epoch + 1,
                        mark,
                    ),
                ];
                let seize = manage(
                    &env,
                    asset,
                    cold.pubkey(),
                    incoming.pubkey(),
                    processor::ASSET_AUTH_BACKING_BUCKET,
                    sequences.authority_epoch + 1,
                );
                let steal = wrap(
                    &env,
                    ProgInstruction::WithdrawBackingBucket {
                        domain: domain as u16,
                        market_id: env.asset_market_id(asset),
                        authority_epoch: sequences.authority_epoch + 1,
                        amount: 1,
                    },
                    vec![
                        AccountMeta::new(incoming.pubkey(), true),
                        AccountMeta::new(market, false),
                        AccountMeta::new(wallets[6], false),
                        AccountMeta::new(vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                );
                for (suffix, error) in [
                    (seize, PercolatorError::EngineLockActive),
                    (steal, PercolatorError::Unauthorized),
                ] {
                    let mut bundle = prefix.clone();
                    bundle.push(suffix);
                    peak[1] = peak[1].max(land(
                        &mut env,
                        &bundle,
                        &[&owners[2], &cold, &incoming],
                        &tracked,
                        &[],
                        Some((5, error, 1)),
                    ));
                    rollbacks += 1;
                    book(&env, 0, expire_first, false);
                }
                // Neither the funded provider nor the market authority signs this prefix.
                peak[2] = peak[2].max(land(
                    &mut env,
                    &prefix,
                    &[&owners[2], &cold, &incoming],
                    &tracked,
                    &[market, vault, wallets[2], portfolios[2]],
                    None,
                ));
                profile.oracle_authority = incoming.pubkey().to_bytes();
                profile.last_good_oracle_slot = if expire_first { EXPIRY } else { 2 };
                sequences.authority_epoch += 1;
                sequences.oracle_observation += 1;
                assert_eq!(oracle_profile(&env, asset), profile);
                assert_eq!(env.control_sequences(asset as usize), sequences);
                assert_eq!(oracle_profile(&env, 1 - asset), peer_profile);
                assert_eq!(env.control_sequences((1 - asset) as usize), peer_sequences);
                let mut expected = before;
                expected.1.c_tot -= PAID_PREFIX;
                expected.1.vault -= PAID_PREFIX;
                assert_eq!(env.market_state(), expected);
                book(&env, PAID_PREFIX, expire_first, false);

                let reject_old_report = [
                    cashout(&env, owners[2].pubkey(), portfolios[2], wallets[2], 1),
                    report(
                        &env,
                        asset,
                        provider.pubkey(),
                        sequences.authority_epoch,
                        mark,
                    ),
                ];
                peak[1] = peak[1].max(land(
                    &mut env,
                    &reject_old_report,
                    &[&owners[2], &provider],
                    &tracked,
                    &[],
                    Some((3, PercolatorError::Unauthorized, 1)),
                ));
                rollbacks += 1;
                book(&env, PAID_PREFIX, expire_first, false);

                if !expire_first {
                    expire_and_refresh(
                        &mut env,
                        portfolios,
                        winner_long,
                        asset,
                        &incoming,
                        &mut peak,
                    );
                }
                book(&env, PAID_PREFIX, true, false);
                let reject_impaired = [
                    cashout(&env, owners[2].pubkey(), portfolios[2], wallets[2], 1),
                    manage(
                        &env,
                        asset,
                        cold.pubkey(),
                        incoming.pubkey(),
                        processor::ASSET_AUTH_BACKING_BUCKET,
                        sequences.authority_epoch,
                    ),
                ];
                peak[1] = peak[1].max(land(
                    &mut env,
                    &reject_impaired,
                    &[&owners[2], &cold, &incoming],
                    &tracked,
                    &[],
                    Some((3, PercolatorError::EngineLockActive, 1)),
                ));
                rollbacks += 1;
                book(&env, PAID_PREFIX, true, false);

                peak[2] = peak[2].max(env.trade_asset_with_cu(
                    1 - asset,
                    &owners[0],
                    portfolios[0],
                    &owners[1],
                    portfolios[1],
                    if winner_long {
                        -RISK_INCREASE_Q
                    } else {
                        RISK_INCREASE_Q
                    },
                    if winner_long { 95 } else { 105 },
                    0,
                ));
                book(&env, PAID_PREFIX, true, true);
                let before_consent = env.market_state();
                let peer_before_consent = (
                    oracle_profile(&env, 1 - asset),
                    env.control_sequences((1 - asset) as usize),
                );
                let mut expected_profile = oracle_profile(&env, asset);
                let mut expected_sequences = env.control_sequences(asset as usize);
                let consent = manage(
                    &env,
                    asset,
                    provider.pubkey(),
                    incoming.pubkey(),
                    processor::ASSET_AUTH_BACKING_BUCKET,
                    expected_sequences.authority_epoch,
                );
                peak[2] = peak[2].max(land(
                    &mut env,
                    &[consent],
                    &[&provider, &incoming],
                    &tracked,
                    &[market],
                    None,
                ));
                assert_eq!(env.market_state(), before_consent);
                expected_profile.backing_bucket_authority = incoming.pubkey().to_bytes();
                expected_sequences.authority_epoch += 1;
                assert_eq!(oracle_profile(&env, asset), expected_profile);
                assert_eq!(env.control_sequences(asset as usize), expected_sequences);
                assert_eq!(
                    (
                        oracle_profile(&env, 1 - asset),
                        env.control_sequences((1 - asset) as usize)
                    ),
                    peer_before_consent
                );
                book(&env, PAID_PREFIX, true, true);

                let final_exit = cashout(
                    &env,
                    owners[2].pubkey(),
                    portfolios[2],
                    wallets[2],
                    BYSTANDER_DEPOSIT - PAID_PREFIX,
                );
                peak[2] = peak[2].max(land(
                    &mut env,
                    &[final_exit],
                    &[&owners[2]],
                    &tracked,
                    &[market, vault, wallets[2], portfolios[2]],
                    None,
                ));
                book(&env, BYSTANDER_DEPOSIT, true, true);
                let group = env.market_state().1;
                outcomes.push((
                    group.source_credit,
                    group.source_backing_buckets,
                    group.vault,
                    group.c_tot,
                    portfolios.map(|key| {
                        let a = env.portfolio_state(key);
                        (a.capital.get(), a.pnl.get())
                    }),
                ));
                worlds += 1;
            }
            assert_eq!(
                outcomes[0], outcomes[1],
                "oracle replacement and impairment commute economically"
            );
        }
    }
    assert_eq!((worlds, rollbacks), (8, 32));
    assert_cu_within(
        "cold oracle open lien setup/expiry",
        peak[0],
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within("cold oracle open lien rollback", peak[1], 600_000);
    assert_cu_within(
        "cold oracle open lien committed suffix",
        peak[2],
        TRADE_CU_LIMIT,
    );
    eprintln!("INV-005 cold oracle impaired containment: worlds={worlds}, exact SPL-prefix rollbacks={rollbacks}, oracle replacements=8, strict reductions=8, consented funded handoffs=8, owner payouts=16, peak={peak:?}");
}
