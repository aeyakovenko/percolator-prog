//! INV-005/024/027, row416: impaired backing alone keeps a funded backing role
//! incumbent-bound. The earned-reserve and consumed-backing witnesses explicitly
//! run with zero impaired stock; this probe publicly creates an exact-expiry
//! impaired source lien, proves every other funded bucket term is zero, then
//! checks that a correctly signed cold-admin path cannot seize the role.

use super::cold_admin_earned_reserve::{land, wrap};
use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const WINNER_DEPOSIT: u128 = 313;
const COUNTERPARTY_DEPOSIT: u128 = 5_000;
const BYSTANDER_DEPOSIT: u128 = 20;
const BACKING: u128 = 150;
const PRICE: u64 = 100;
const EXPIRY: u64 = 3;
const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
const RISK_INCREASE_Q: i128 = 2 * POS_SCALE as i128;

#[path = "inv_005_cold_oracle_impaired_containment.rs"]
mod cold_oracle_impaired_containment;

struct LienedWorld {
    env: V16CuEnv,
    provider: Keypair,
    cold: Keypair,
    owners: [Keypair; 3],
    wallets: [Pubkey; 6],
    portfolios: [Pubkey; 3],
}

fn liened_world(winner_long: bool, asset: u16, peak: &mut [u64; 3]) -> LienedWorld {
    let direction = if winner_long { 1 } else { -1 };
    let winning_mark = if winner_long { 105 } else { 95 };
    let adverse_mark = if winner_long { 95 } else { 105 };
    let domain = 2 * asset + u16::from(winner_long);

    let mut env = inv018_public_spl_market_with_params(
        0,
        V16CuMarketParams {
            max_portfolio_assets: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            ..V16CuMarketParams::default()
        },
    );
    let admin = env.admin.insecure_clone();
    let provider = Keypair::new();
    let cold = Keypair::new();
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let actors = [&owners[0], &owners[1], &owners[2], &provider, &admin, &cold];
    for actor in actors {
        env.ensure_signer_account(actor.pubkey());
    }
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&provider),
        asset,
        processor::ASSET_AUTH_BACKING_BUCKET,
        provider.pubkey().to_bytes(),
    )
    .expect("install independent backing provider before funding");

    let wallets =
        actors.map(|actor| create_ata_for_test(&mut env.svm, &env.payer, actor.pubkey(), env.mint));
    for (destination, amount) in [
        (wallets[0], WINNER_DEPOSIT),
        (wallets[1], COUNTERPARTY_DEPOSIT),
        (wallets[2], BYSTANDER_DEPOSIT),
        (wallets[3], BACKING),
    ] {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &destination,
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

    let portfolios: [Pubkey; 3] = owners.each_ref().map(|owner| {
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
    for (index, amount) in [WINNER_DEPOSIT, COUNTERPARTY_DEPOSIT, BYSTANDER_DEPOSIT]
        .into_iter()
        .enumerate()
    {
        env.send(
            env.deposit_ix(portfolios[index], amount),
            vec![
                AccountMeta::new(owners[index].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[index], false),
                AccountMeta::new(wallets[index], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owners[index]],
        )
        .unwrap();
    }
    env.send(
        ProgInstruction::TopUpBackingBucket {
            domain,
            market_id: env.asset_market_id(asset),
            authority_epoch: env.control_sequences(asset as usize).authority_epoch,
            intent_id: next_control_sequence(env.control_sequences(asset as usize).backing_top_up),
            amount: BACKING,
            expiry_slot: EXPIRY,
            backing_fee_bps: 0,
            insurance_share_bps: 0,
        },
        vec![
            AccountMeta::new(provider.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(wallets[3], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&provider],
    )
    .unwrap();

    peak[0] = peak[0].max(env.trade_asset_with_cu(
        asset,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        direction * WINNING_SIZE_Q,
        PRICE,
        0,
    ));
    peak[0] = peak[0].max(env.trade_asset_with_cu(
        1 - asset,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        direction * ADVERSE_SIZE_Q,
        PRICE,
        0,
    ));
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(asset, 2, winning_mark);
    env.push_auth_mark_for_asset_as_admin(1 - asset, 2, adverse_mark);
    for portfolio in [portfolios[1], portfolios[0]] {
        for _ in 0..4 {
            if let Some(cu) = env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            ) {
                peak[0] = peak[0].max(cu);
            } else {
                break;
            }
        }
    }
    peak[0] = peak[0].max(env.trade_asset_with_cu(
        1 - asset,
        &owners[0],
        portfolios[0],
        &owners[1],
        portfolios[1],
        direction * RISK_INCREASE_Q,
        adverse_mark,
        0,
    ));
    let (_, liened) = env.market_state();
    assert!(liened.source_credit[domain as usize].valid_liened_backing_num > 0);
    assert!(
        env.portfolio_state(portfolios[0])
            .source_domains
            .iter()
            .any(|source| {
                source.is_occupied()
                    && source.domain.get() == u32::from(domain)
                    && source.source_claim_liened_num.get() > 0
                    && source.source_lien_counterparty_backing_num.get() > 0
            }),
        "winner must hold a public source-backed lien before impairment"
    );
    assert_eq!(
        liened.source_credit[domain as usize].impaired_liened_backing_num,
        0
    );
    assert_eq!(
        liened.source_backing_buckets[domain as usize].status,
        BackingBucketStatusV16::Fresh
    );

    LienedWorld {
        env,
        provider,
        cold,
        owners,
        wallets,
        portfolios,
    }
}

fn expire_lien(
    env: &mut V16CuEnv,
    portfolios: [Pubkey; 3],
    winner_long: bool,
    asset: u16,
    oracle: &Keypair,
    peak: &mut [u64; 3],
) {
    let expiry_winning_mark = if winner_long { 106 } else { 94 };
    let adverse_mark = if winner_long { 95 } else { 105 };
    env.svm.warp_to_slot(EXPIRY);
    env.push_auth_mark_for_asset_with_authority(asset, oracle, EXPIRY, expiry_winning_mark);
    env.push_auth_mark_for_asset_as_admin(1 - asset, EXPIRY, adverse_mark);
    for portfolio in [portfolios[0], portfolios[1]] {
        for asset in [0, 1] {
            if let Some(cu) = env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: EXPIRY,
                    observations: crank_observations(asset),
                },
            ) {
                peak[0] = peak[0].max(cu);
            }
        }
    }
}

#[test]
fn v16_program_cold_admin_cannot_seize_impaired_backing_only_role() {
    let mut peak = [0; 3]; // impairment setup, rollback rejection, consented management/reduction
    for winner_long in [false, true] {
        let LienedWorld {
            mut env,
            provider,
            cold,
            owners,
            wallets,
            portfolios,
        } = liened_world(winner_long, 0, &mut peak);
        let admin = env.admin.insecure_clone();
        let actors = [&owners[0], &owners[1], &owners[2], &provider, &admin, &cold];
        let direction = if winner_long { 1 } else { -1 };
        let adverse_mark = if winner_long { 95 } else { 105 };
        let domain = u16::from(winner_long);
        expire_lien(&mut env, portfolios, winner_long, 0, &admin, &mut peak);

        let before = env.market_state();
        let bucket = before.1.source_backing_buckets[domain as usize];
        let source = before.1.source_credit[domain as usize];
        assert_eq!(bucket.status, BackingBucketStatusV16::Impaired);
        assert_eq!(bucket.fresh_unliened_backing_num, 0);
        assert_eq!(bucket.valid_liened_backing_num, 0);
        assert_eq!(bucket.consumed_liened_backing_num, 0);
        assert!(bucket.impaired_liened_backing_num > 0);
        assert_eq!(bucket.utilization_fee_earnings, 0);
        assert_eq!(source.valid_liened_backing_num, 0);
        assert_eq!(
            source.impaired_liened_backing_num,
            bucket.impaired_liened_backing_num
        );
        let sibling = before.1.source_backing_buckets[(domain ^ 1) as usize];
        assert_eq!(sibling.fresh_unliened_backing_num, 0);
        assert_eq!(sibling.valid_liened_backing_num, 0);
        assert_eq!(sibling.consumed_liened_backing_num, 0);
        assert_eq!(sibling.impaired_liened_backing_num, 0);
        assert_eq!(sibling.utilization_fee_earnings, 0);

        let market = env.market;
        let vault = env.vault;
        let profile_before =
            state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0)
                .unwrap();
        let sequences_before = env.control_sequences(0);
        let epoch = sequences_before.authority_epoch;
        let portfolio_frames = portfolios.map(|key| env.svm.get_account(&key));
        let dest = wallets[2];
        let mut tracked = vec![market, vault, env.mint, env.vault_authority, dest];
        tracked.extend(wallets);
        tracked.extend(portfolios);
        tracked.extend(actors.map(Signer::pubkey));

        let rotation = |kind, from: Pubkey, to: Pubkey, authority_epoch: u64| {
            wrap(
                &env,
                ProgInstruction::UpdateAssetAuthority {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    authority_epoch,
                    kind,
                    new_pubkey: to.to_bytes(),
                },
                vec![
                    AccountMeta::new(from, true),
                    AccountMeta::new_readonly(to, true),
                    AccountMeta::new(market, false),
                ],
            )
        };
        let rotate_admin = rotation(
            processor::ASSET_AUTH_ADMIN,
            admin.pubkey(),
            cold.pubkey(),
            epoch,
        );
        let seize = rotation(
            processor::ASSET_AUTH_BACKING_BUCKET,
            cold.pubkey(),
            cold.pubkey(),
            epoch + 1,
        );
        let consent = rotation(
            processor::ASSET_AUTH_BACKING_BUCKET,
            provider.pubkey(),
            cold.pubkey(),
            epoch + 1,
        );
        let bystander_withdraw = wrap(
            &env,
            env.withdraw_ix(portfolios[2], 1),
            vec![
                AccountMeta::new(owners[2].pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(portfolios[2], false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        );
        peak[1] = peak[1].max(land(
            &mut env,
            &[rotate_admin.clone(), bystander_withdraw, seize.clone()],
            &[&admin, &cold, &owners[2]],
            &tracked,
            &[],
            Some((4, PercolatorError::EngineLockActive, 1)),
        ));
        assert_eq!(env.market_state(), before);
        assert_eq!(
            portfolios.map(|key| env.svm.get_account(&key)),
            portfolio_frames
        );
        assert_eq!(env.token_amount(dest), 0);

        peak[2] = peak[2].max(land(
            &mut env,
            &[rotate_admin, consent],
            &[&admin, &provider, &cold],
            &tracked,
            &[market],
            None,
        ));
        let after = env.market_state();
        assert_eq!(after.0, before.0);
        let mut expected_group = before.1.clone();
        expected_group.current_slot = after.1.current_slot;
        assert_eq!(after.1, expected_group);
        let mut expected_profile = profile_before;
        expected_profile.asset_admin = cold.pubkey().to_bytes();
        expected_profile.backing_bucket_authority = cold.pubkey().to_bytes();
        assert_eq!(
            state::read_asset_oracle_profile(&env.svm.get_account(&market).unwrap().data, 0)
                .unwrap(),
            expected_profile
        );
        let mut expected_sequences = sequences_before;
        expected_sequences.authority_epoch += 2;
        assert_eq!(env.control_sequences(0), expected_sequences);

        peak[2] = peak[2].max(env.trade_asset_with_cu(
            1,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            -direction * RISK_INCREASE_Q,
            adverse_mark,
            0,
        ));
        let reduced = env.market_state().1;
        assert!(
            reduced.source_credit[domain as usize].impaired_liened_backing_num > 0,
            "strict risk reduction preserves still-attributed impaired backing"
        );
    }
    eprintln!("INV-005 impaired backing containment: worlds=2, exact SPL-prefix rollbacks=2, incumbent consents=2, strict reductions=2, peak={peak:?}");
}
