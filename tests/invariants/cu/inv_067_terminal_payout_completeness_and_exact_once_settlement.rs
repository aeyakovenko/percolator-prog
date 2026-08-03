//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: every terminal claim is paid, forfeited for its own episode, or converted
//! exactly once, and no settled value remains ownerless after every claimant exits.
//!
//! Evidence in this file (I/C):
//! `v16_program_terminal_bankruptcy_residual_matrix_discovers_provider_double_charge` executes a
//! complete public bankruptcy lifecycle with real insurance and backing principal. It independently
//! reconciles user SPL payouts, provider withdrawals, and remaining custody. The matrix fails the
//! invariant only when the provider bears the same 20M deficit twice and the duplicate charge is
//! left ownerless in the canonical vault.
//! `v16_program_prior_claim_forfeit_prerequisite_matrix_preserves_withdrawable_value` creates a
//! closed, backed claim followed by a new position episode and proves that the pinned predecessor
//! preserves and pays the historical claim after counterparty-first Recovery.
//!
//! Guarantee boundary: this is a public counterexample on the vulnerable engine pin, not a proof of
//! the corrected terminal residual transition.

use super::*;

#[test]
fn v16_program_prior_claim_forfeit_prerequisite_matrix_preserves_withdrawable_value() {
    const PRICE: u64 = 1_000_000;
    const PROFIT_MARK: u64 = 952_000;
    const ASSET: u16 = 1;
    const SOURCE_DOMAIN: u16 = ASSET * 2;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.configure_auth_mark_for_asset_as_admin(ASSET, 0, PRICE);
    env.top_up_backing_bucket(SOURCE_DOMAIN, 100_000, 10_000);

    let victim_owner = Keypair::new();
    let first_peer_owner = Keypair::new();
    let attacker_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let first_peer = env.create_portfolio(&first_peer_owner);
    let attacker = env.create_portfolio(&attacker_owner);
    for (owner, portfolio) in [
        (&victim_owner, victim),
        (&first_peer_owner, first_peer),
        (&attacker_owner, attacker),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }

    env.trade_asset_with_cu(
        ASSET,
        &first_peer_owner,
        first_peer,
        &victim_owner,
        victim,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    env.svm.warp_to_slot(20);
    env.push_auth_mark_for_asset_as_admin(ASSET, 20, PROFIT_MARK);
    env.crank(
        victim,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations(ASSET),
        },
    );
    env.trade_asset_with_cu(
        ASSET,
        &first_peer_owner,
        first_peer,
        &victim_owner,
        victim,
        -(POS_SCALE as i128),
        PROFIT_MARK,
        0,
    );
    let first_episode = env.portfolio_state(victim);
    assert!(!has_active_leg_for_asset(&first_episode, ASSET as usize));
    let historical_pnl = first_episode.pnl.get();
    let historical_claim = state::portfolio_source_domain(&first_episode, SOURCE_DOMAIN as usize)
        .source_claim_bound_num
        .get();
    assert_eq!(historical_pnl, 48_000);
    assert!(historical_claim > 0);

    env.trade_asset_with_cu(
        ASSET,
        &attacker_owner,
        attacker,
        &victim_owner,
        victim,
        POS_SCALE as i128,
        PROFIT_MARK,
        0,
    );
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, ASSET, 20, 0);
    env.forfeit_recovery_leg_with_cu(&attacker_owner, attacker, ASSET, u128::MAX);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(attacker),
        ASSET as usize
    ));

    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            asset_index: ASSET,
            b_delta_budget: u128::MAX,
        },
        vec![
            AccountMeta::new(victim_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
        ],
        &[&victim_owner],
    )
    .expect("the historical claim changes the current-pin forfeit route");
    let after = env.portfolio_state(victim);
    assert!(!has_active_leg_for_asset(&after, ASSET as usize));
    assert_eq!(after.pnl.get(), historical_pnl);
    assert_eq!(
        state::portfolio_source_domain(&after, SOURCE_DOMAIN as usize)
            .source_claim_bound_num
            .get(),
        historical_claim
    );

    env.svm.warp_to_slot(1_000);
    env.push_auth_mark_for_asset_as_admin(0, 1_000, PRICE);
    env.crank(
        first_peer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1_000,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    let live_conversion = env.send(
        ProgInstruction::ConvertReleasedPnl {
            amount: historical_pnl as u128,
        },
        vec![
            AccountMeta::new(victim_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
        ],
        &[&victim_owner],
    );
    assert!(live_conversion.is_err());

    env.resolve();
    let destination = env.token_account(victim_owner.pubkey(), 0);
    env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&victim_owner],
    )
    .expect("terminal route must pay the preserved prior claim");
    assert_eq!(env.token_amount(destination), 1_048_000);
}

#[test]
fn v16_program_terminal_bankruptcy_residual_matrix_discovers_provider_double_charge() {
    const DOMAIN_TRANCHE: u128 = 100_000_000;
    const PROVIDER_PRINCIPAL: u128 = 4 * DOMAIN_TRANCHE;
    const TRADER_CAPITAL: u128 = 10_000_000;
    const POSITION_Q: i128 = 1_000_000_000_000;
    const INITIAL_MARK: u64 = 100;
    const FINAL_MARK: u64 = 130;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    });
    let provider = Keypair::new();
    let oracle = Keypair::new();
    env.ensure_signer_account(provider.pubkey());
    env.ensure_signer_account(oracle.pubkey());
    let admin = env.admin.insecure_clone();
    for (kind, incoming) in [
        (processor::ASSET_AUTH_INSURANCE, &provider),
        (processor::ASSET_AUTH_INSURANCE_OPERATOR, &provider),
        (processor::ASSET_AUTH_BACKING_BUCKET, &provider),
        (processor::ASSET_AUTH_ORACLE, &oracle),
    ] {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::UpdateAssetAuthority {
                asset_index: 0,
                kind,
                new_pubkey: incoming.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new_readonly(incoming.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&admin, incoming],
        )
        .expect("delegate independent funded roles");
    }

    env.configure_auth_mark_for_asset_with_authority(0, &oracle, 100, INITIAL_MARK);
    for domain in [0u16, 1] {
        env.top_up_insurance_domain_with_authority(&provider, domain, DOMAIN_TRANCHE);
        env.top_up_backing_bucket_with_authority(&provider, domain, DOMAIN_TRANCHE, 10_000);
    }
    assert_eq!(env.market_state().1.vault, PROVIDER_PRINCIPAL);

    let observer_owner = Keypair::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let observer = env.create_portfolio(&observer_owner);
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, TRADER_CAPITAL);
    env.deposit(&short_owner, short, TRADER_CAPITAL);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POSITION_Q,
        INITIAL_MARK,
        0,
    );

    let public_crank = |env: &mut V16CuEnv, portfolio: Pubkey, slot: u64, observe: bool| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: if observe {
                    crank_observations(0)
                } else {
                    vec![]
                },
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
    };

    let mut slot = 101u64;
    env.svm.warp_to_slot(slot);
    env.push_auth_mark_for_asset_with_authority(0, &oracle, slot, FINAL_MARK);
    for _ in 0..64 {
        env.svm.warp_to_slot(slot);
        public_crank(&mut env, observer, slot, true).expect("advance authenticated mark");
        if env.market_state().1.assets[0].effective_price == FINAL_MARK {
            break;
        }
        slot += 1;
    }
    assert_eq!(env.market_state().1.assets[0].effective_price, FINAL_MARK);

    public_crank(&mut env, short, slot, true).expect("settle losing account");
    for _ in 0..4 {
        public_crank(&mut env, short, slot, false).expect("liquidation progress");
    }
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(short)
    )));

    public_crank(&mut env, long, slot, false).expect("refresh reset winner");
    env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            asset_index: 0,
            b_delta_budget: percolator::MAX_VAULT_TVL,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
        ],
        &[&long_owner],
    )
    .expect("winner clears recovery leg");
    for side in [0u8, 1] {
        env.send(
            ProgInstruction::FinalizeResetSide {
                asset_index: 0,
                side,
            },
            vec![AccountMeta::new(env.market, false)],
            &[],
        )
        .expect("finalize side reset");
    }
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(long)
    )));

    env.resolve();
    let owners = [&observer_owner, &long_owner, &short_owner];
    let portfolios = [observer, long, short];
    let destinations = owners.map(|owner| env.token_account(owner.pubkey(), 0));
    for _ in 0..512 {
        let mut open = 0usize;
        for index in 0..portfolios.len() {
            if env
                .svm
                .get_account(&portfolios[index])
                .map_or(true, |account| account.lamports == 0)
            {
                continue;
            }
            open += 1;
            let payout_accounts = vec![
                AccountMeta::new_readonly(owners[index].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolios[index], false),
                AccountMeta::new(destinations[index], false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];
            let _ = env.send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                payout_accounts.clone(),
                &[owners[index]],
            );
            let _ = env.send(
                ProgInstruction::ClaimResolvedPayoutTopup,
                payout_accounts,
                &[owners[index]],
            );
            let _ = env.send(
                ProgInstruction::ClosePortfolio,
                vec![
                    AccountMeta::new(owners[index].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[index], false),
                ],
                &[owners[index]],
            );
        }
        if open == 0 {
            break;
        }
    }
    assert!(portfolios.iter().all(|portfolio| env
        .svm
        .get_account(portfolio)
        .map_or(true, |account| account.lamports == 0)));
    assert_eq!(
        destinations.map(|destination| env.token_amount(destination) as u128),
        [0, 40_000_000, 0]
    );

    let after_users = env.market_state().1;
    assert_eq!(after_users.materialized_portfolio_count, 0);
    assert_eq!(after_users.c_tot, 0);
    assert_eq!(
        after_users.insurance, 180_000_000,
        "the vulnerable transition must omit exactly one 20M residual recredit"
    );

    let provider_destination = env.token_account(provider.pubkey(), 0);
    env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            amount: after_users.insurance,
        },
        vec![
            AccountMeta::new(provider.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(provider_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&provider],
    )
    .expect("withdraw terminal insurance");
    for domain in [0u16, 1] {
        let backing = env.market_state().1.source_backing_buckets[domain as usize]
            .fresh_unliened_backing_num
            / BOUND_SCALE;
        if backing != 0 {
            env.send(
                ProgInstruction::WithdrawBackingBucket {
                    domain,
                    amount: backing,
                },
                vec![
                    AccountMeta::new(provider.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(provider_destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&provider],
            )
            .expect("withdraw remaining source backing");
        }
    }
    assert_eq!(env.token_amount(provider_destination) as u128, 360_000_000);
    assert_eq!(env.market_state().1.vault, 20_000_000);
    assert_eq!(env.token_amount(env.vault), 20_000_000);
}
