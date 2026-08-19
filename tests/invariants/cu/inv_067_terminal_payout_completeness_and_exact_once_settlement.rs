//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: every terminal claim is paid, forfeited for its own episode, or converted
//! exactly once, and no settled value remains ownerless after every claimant exits.
//!
//! Evidence in this file (I/C):
//! `v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value` executes a complete
//! public bankruptcy lifecycle with real insurance and backing principal. It independently
//! reconciles user SPL payouts, provider withdrawals, and remaining custody, requiring every atom
//! to end either in a user payout or the provider destination with zero ownerless vault residue.
//! `v16_program_prior_claim_forfeit_prerequisite_matrix_preserves_withdrawable_value` creates a
//! closed, backed claim followed by a new position episode and proves that the pinned predecessor
//! preserves and pays the historical claim after counterparty-first Recovery.
//! `v16_program_retained_recovery_haircut_prerequisite_matrix_keeps_prior_claim_floor` lands a
//! retained forfeit after a real B haircut and proves the predecessor still pays at least the
//! complete earlier claim.
//!
//! Guarantee boundary: this is one adversarial public lifecycle matrix, not an exhaustive proof of
//! every terminal residual partition.

use super::*;

#[test]
fn v16_program_retained_recovery_haircut_prerequisite_matrix_keeps_prior_claim_floor() {
    const PRICE: u64 = 1_000_000;
    const FIRST_PROFIT_MARK: u64 = 952_000;
    const ASSET: u16 = 1;
    const SOURCE_DOMAIN: u16 = ASSET * 2;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    params.max_abs_funding_e9_per_slot = 0;
    params.public_b_chunk_atoms = percolator::MAX_VAULT_TVL;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.configure_auth_mark_for_asset_as_admin(ASSET, 0, PRICE);
    env.top_up_backing_bucket(SOURCE_DOMAIN, 200_000, 10_000);

    let victim_owner = Keypair::new();
    let first_peer_owner = Keypair::new();
    let attacker_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let first_peer = env.create_portfolio(&first_peer_owner);
    let attacker = env.create_portfolio(&attacker_owner);
    env.deposit(&victim_owner, victim, 1_000_000);
    env.deposit(&first_peer_owner, first_peer, 1_000_000);
    env.deposit(&attacker_owner, attacker, 51_000);

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
    env.push_auth_mark_for_asset_as_admin(ASSET, 20, FIRST_PROFIT_MARK);
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
        FIRST_PROFIT_MARK,
        0,
    );
    let first_episode = env.portfolio_state(victim);
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
        FIRST_PROFIT_MARK,
        0,
    );
    for (slot, mark) in [(40, 904_000), (45, 892_000), (50, 892_576)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(ASSET, slot, mark);
        env.crank(
            victim,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(ASSET),
            },
        );
    }
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, ASSET, 50, 0);
    let before_attack = env.portfolio_state(victim);
    assert_eq!(
        active_leg_for_asset(&before_attack, ASSET as usize).b_snap,
        env.market_state().1.assets[ASSET as usize].b_short_num
    );
    let retained_forfeit = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(victim_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(victim, false),
                ],
                data: ProgInstruction::ForfeitRecoveryLeg {
                    portfolio_id: env.portfolio_id(victim),
                    position_epoch: env.portfolio_position_epoch(victim),
                    asset_index: ASSET,
                    b_delta_budget: u128::MAX,
                }
                .encode(),
            },
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &victim_owner],
        env.svm.latest_blockhash(),
    );

    env.forfeit_recovery_leg_with_cu(&attacker_owner, attacker, ASSET, u128::MAX);
    let before_forfeit = env.portfolio_state(victim);
    assert!(
        active_leg_for_asset(&before_forfeit, ASSET as usize).b_snap
            < env.market_state().1.assets[ASSET as usize].b_short_num
    );
    assert_eq!(before_forfeit.pnl.get(), historical_pnl + 59_424);

    env.svm
        .send_transaction(retained_forfeit)
        .expect("unprivileged relayer lands retained signed forfeit");
    let after = env.portfolio_state(victim);
    assert!(!has_active_leg_for_asset(&after, ASSET as usize));
    assert_eq!(
        after.pnl.get(),
        99_000,
        "the pinned predecessor's retained-value behavior changed"
    );
    assert!(
        state::portfolio_source_domain(&after, SOURCE_DOMAIN as usize)
            .source_claim_bound_num
            .get()
            >= historical_claim,
        "the pinned predecessor consumed the older closed claim"
    );

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
    .expect("terminal payout remains bounded");
    assert_eq!(env.token_amount(destination), 1_099_000);
}

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
            portfolio_id: env.portfolio_id(victim),
            position_epoch: env.portfolio_position_epoch(victim),
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
        env.convert_released_pnl_ix(victim, historical_pnl as u128),
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
fn v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value() {
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
    let market_id = env.asset_market_id(0);
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
                market_id,
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
    if has_active_leg_for_asset(&env.portfolio_state(long), 0) {
        env.send(
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id: env.portfolio_id(long),
                position_epoch: env.portfolio_position_epoch(long),
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
    }
    for side in [0u8, 1] {
        let asset = env.market_state().1.assets[0];
        let mode = if side == 0 {
            asset.mode_long
        } else {
            asset.mode_short
        };
        if mode == SideModeV16::ResetPending {
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
                env.close_portfolio_ix(portfolios[index]),
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
    let backing_after_users: u128 = [0usize, 1]
        .into_iter()
        .map(|domain| {
            after_users.source_backing_buckets[domain].fresh_unliened_backing_num / BOUND_SCALE
        })
        .sum();
    let claim_free_residual = after_users
        .vault
        .checked_sub(after_users.insurance + backing_after_users)
        .expect("terminal classified stock cannot exceed custody");
    assert_eq!(
        claim_free_residual, 20_000_000,
        "the provider withdrawal must lazily recover the exact duplicated insurance charge"
    );

    let provider_destination = env.token_account(provider.pubkey(), 0);
    env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 2 * DOMAIN_TRANCHE,
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
            let market_id = env.asset_market_id(domain / 2);
            env.send(
                ProgInstruction::WithdrawBackingBucket {
                    domain,
                    market_id,
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
    assert_eq!(env.token_amount(provider_destination) as u128, 380_000_000);
    assert_eq!(env.market_state().1.vault, 0);
    assert_eq!(env.token_amount(env.vault), 0);
}

#[test]
fn v16_attack_close_resolved_requires_owner_signature_during_exit_window() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    let unsigned_dest = env.token_account(owner.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let dest_before = env.svm.get_account(&unsigned_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let unsigned = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(unsigned_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        unsigned.is_err(),
        "third-party CloseResolved must reject during the owner exit window"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "unsigned close inside the exit window must not mutate resolved market accounting"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "unsigned close inside the exit window must not burn the owner's payout state"
    );
    assert_eq!(
        env.svm.get_account(&unsigned_dest).unwrap(),
        dest_before,
        "unsigned close inside the exit window must not pay the destination"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "unsigned close inside the exit window must not move vault tokens"
    );

    let signed_dest = env.token_account(owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let signed = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(signed_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .expect("owner-signed CloseResolved works during the exit window");
    assert_cu_within(
        "owner-signed CloseResolved during exit window",
        signed,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(signed_dest),
        1_000,
        "the owner can still recover during the protected exit window"
    );
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
}

#[test]
fn v16_attack_close_resolved_after_exit_window_is_permissionless_but_not_stealable() {
    let mut env = V16CuEnv::new();
    const EXIT_DELAY: u64 = 5;
    env.configure_permissionless_resolve_with_cu(100, EXIT_DELAY);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    env.svm.warp_to_slot(EXIT_DELAY + 1);
    env.svm.expire_blockhash();

    let attacker = Keypair::new();
    let attacker_dest = env.token_account(attacker.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let attacker_dest_before = env.svm.get_account(&attacker_dest).unwrap();
    let steal = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(attacker_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        steal.is_err(),
        "post-timeout CloseResolved is permissionless, but payout must still go to the portfolio owner"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&attacker_dest).unwrap(),
        attacker_dest_before
    );

    let owner_dest = env.token_account(owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let permissionless = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(owner_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("post-timeout CloseResolved can be cranked without owner signature");
    assert_cu_within(
        "post-timeout permissionless CloseResolved",
        permissionless,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(owner_dest),
        1_000,
        "permissionless close pays the owner-controlled destination"
    );
    assert_eq!(
        env.token_amount(attacker_dest),
        0,
        "attacker-controlled destination receives nothing"
    );
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
}

// security.md sweep — CloseResolved caller-supplied fee_rate_per_slot must be IGNORED (Copenhagen
// SOL-001-class spoofed-param / SOL-023 fee-rounding-away-from-user): CloseResolved is permissionless
// after the exit window (force_close_delay_slots==0 -> always permissionless), and it carries a
// caller-supplied `fee_rate_per_slot`. handle_close_resolved (v16_program 10285) names it `_fee_rate_
// per_slot` and passes cfg.maintenance_fee_per_slot (10317) to the engine instead. If the param were
// honored, a hostile third party finalizing a victim's resolved account could pass a huge rate to
// over-charge the victim's accrued maintenance fee at terminal close, draining the payout into
// insurance (victim LOF). Every existing CloseResolved test passes fee_rate_per_slot: 0, so this
// ignore property is unpinned. With cfg maintenance_fee=0 and slots elapsed, the victim must receive
// the FULL deposit regardless of a u128::MAX spoofed rate; a regression that wired the param in would
// drain it to ~0.
#[test]
fn v16_attack_close_resolved_ignores_spoofed_fee_rate_param() {
    let mut env = V16CuEnv::new(); // default maintenance_fee_per_slot = 0, force_close_delay_slots = 0
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    env.deposit(&victim_owner, victim, 1_000_000);
    env.resolve();
    // Advance many slots so elapsed_slots is large: a leaked spoofed rate would charge rate*elapsed.
    env.svm.warp_to_slot(10_000);

    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, victim_owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    env.svm.expire_blockhash();
    // Permissionless finalize (no signer) with a maximally-spoofed fee rate.
    env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: u128::MAX,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("permissionless close-resolved with spoofed fee rate");

    assert_eq!(
        env.token_amount(dest),
        1_000_000,
        "victim must receive the FULL deposit; the caller-supplied fee_rate_per_slot must be ignored"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        env.portfolio_state(victim).capital.get(),
        0,
        "account fully closed"
    );
    assert_eq!(
        g.vault,
        g.c_tot + g.insurance,
        "conservation after terminal close"
    );
}

#[test]
fn v16_attack_claim_resolved_topup_rejects_live_market_without_mutation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let attacker = Keypair::new();
    let attacker_dest = env.token_account(attacker.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&attacker_dest).unwrap();

    env.svm.expire_blockhash();
    let live_claim = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(attacker_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        live_claim.is_err(),
        "unsigned resolved-payout top-up must reject while the market is still Live"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.svm.get_account(&attacker_dest).unwrap(), dest_before);

    let (owner_dest, _) = env.withdraw_with_cu(&owner, portfolio, 1_000);
    assert_eq!(
        env.token_amount(owner_dest),
        1_000,
        "owner can still withdraw after rejected live top-up attempt"
    );
    assert_eq!(
        env.token_amount(attacker_dest),
        0,
        "attacker destination receives nothing"
    );
}

// engine backing_double_claim_fuzz port (LoF) — Fresh-bucket counterparty backing principal is
// provider-recoverable: WithdrawBackingBucket has NO resolved-payout-snapshot gate. residual()
// (the junior payout pool feeding the resolved snapshot) must therefore EXCLUDE that principal.
// The currently-pinned engine counts it, so a resolved junior winner with ZERO honest residual is
// still paid out of the provider's backing — the same vault atoms the provider can still withdraw.
// Whoever closes second is robbed (loss of funds). This drives the bug end-to-end through the
// public CloseResolved + WithdrawBackingBucket handlers; the winner closing first captures the
// payout snapshot via the buggy residual(). FAILS against the pinned (pre-fix) engine, PASSES once
// residual() excludes recoverable counterparty backing principal.
#[test]
fn v16_attack_resolved_junior_winner_double_claims_provider_backing() {
    const CAPITAL: u128 = 1_000;
    const BACKING: u128 = 1_000; // provider principal (B >= F)
    const FACE: u128 = 500; // winner junior positive pnl
    let domain: u16 = 1;
    let mut env = V16CuEnv::new();

    // Provider deposits recoverable Fresh-bucket backing principal (Live mode).
    env.top_up_backing_bucket(domain, BACKING, 10_000);

    // A winner deposits capital, then (synthetically) holds plain JUNIOR positive pnl with NO
    // source claim of its own, and we resolve with ZERO honest junior residual: vault =
    // CAPITAL + BACKING and c_tot = CAPITAL, so the only vault atoms above capital ARE the
    // provider's recoverable backing.
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, CAPITAL);
    {
        let mut market_account = env.svm.get_account(&env.market).unwrap();
        let mut portfolio_account = env.svm.get_account(&portfolio).unwrap();
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.pnl_pos_tot = FACE;
        group.pnl_matured_pos_tot = FACE;
        group.pnl_pos_bound_tot = FACE;
        group.pnl_pos_bound_tot_num = FACE * BOUND_SCALE;
        account.pnl = percolator::V16PodI128::new(FACE as i128);
        account.last_fee_slot = percolator::V16PodU64::new(1);
        // payout_snapshot_captured left false: the first CloseResolved captures it via residual().
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    // Honest junior residual (vault - capital - backing) is ZERO, so the winner is NOT entitled to
    // any pnl payout: it must recover EXACTLY its capital. A larger payout is financed by the
    // provider's recoverable backing — the double-claim.
    let dest = env.close_resolved(&owner, portfolio);
    let _ = env.close_resolved(&owner, portfolio); // finalize receipt
    let winner_payout = env.token_amount(dest) as u128;
    assert_eq!(
        winner_payout, CAPITAL,
        "winner must recover ONLY capital (honest junior residual is zero); a larger payout \
         ({winner_payout}) was financed out of the provider's recoverable backing (double-claim)"
    );

    // Wind the winner down so the provider can reclaim (resolved-mode withdraw requires
    // materialized_portfolio_count == 0 && c_tot == 0).
    env.close_portfolio_with_cu(&owner, portfolio);
    let (_, g) = env.market_state();
    assert_eq!(g.c_tot, 0, "winner capital fully wound down");
    assert_eq!(g.materialized_portfolio_count, 0, "winner dematerialized");

    // The provider must recover its FULL principal — recoverable, no snapshot gate. (On the
    // pinned engine the run already failed the winner_payout assertion above, before reaching
    // here; on the fixed engine the backing is intact and this withdrawal succeeds.)
    let admin_pubkey = env.admin.pubkey();
    let provider_dest = env.token_account(admin_pubkey, 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(provider_dest, domain, BACKING);
    let provider_got = env.token_amount(provider_dest) as u128;
    assert_eq!(
        provider_got, BACKING,
        "provider recovers exactly its principal"
    );

    // Global conservation: nothing minted, nothing stranded.
    assert_eq!(
        winner_payout + provider_got,
        CAPITAL + BACKING,
        "value conserved end to end (no mint, no strand)"
    );
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 0, "vault fully drained, no funds stranded");
}

// Issue #88: resolved bound refinement is internal engine accounting. Raw tag
// 47 must reject before any resolved payout or custody state can move.
#[test]
fn v16_attack_refine_resolved_bound_tag_is_not_public() {
    let mut env = V16CuEnv::new();
    env.mutate_market(|_, group| {
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 0,
            terminal_claim_bound_unreceipted_num: 100 * BOUND_SCALE,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
    });

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let mut data = Vec::with_capacity(17);
    data.push(47);
    data.extend_from_slice(&(10 * BOUND_SCALE).to_le_bytes());
    let admin = env.admin.insecure_clone();
    env.svm.expire_blockhash();
    let rejected = send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            data,
        },
        &[&admin],
    );

    assert!(rejected.is_err(), "raw refine tag must be unavailable");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
}

// Engine-selected liquidation must reduce no more than the live position and conserve value. The
// A configured absolute liquidation-fee floor can make every partial chunk inadmissible even when
// the account is liquidatable. The public auto-crank must fall back to closing the selected leg in
// full instead of repeatedly returning NonProgress on the same engine-selected asset.

// security.md sweep — insurance makes winner whole at resolution (#33/#9): with a funded insurance
// backstop, a winner facing a loser's bad debt should recover their full claim at resolution (insurance
// absorbs the deficit), bounded by available insurance. Value conserved; insurance only spent.
#[test]
fn v16_attack_insurance_makes_winner_whole_at_resolution() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000); // backstop
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner); // long winner
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner); // short loser (thin)
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 250);
    env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    let ins_before = env.market_state().1.insurance;
    let vault_before = env.market_state().1.vault;
    // price up over slots -> short insolvent.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        for acct in [lo, sh] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(acct, false),
                ],
                &[],
            );
        }
    }
    // Liquidate the insolvent short, then resolve and wind down.
    env.crank(
        sh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.resolve();
    let lo_dest = env.close_resolved(&lo_owner, lo);
    let _ = env.close_resolved(&sh_owner, sh);
    let won = env.token_amount(lo_dest) as u128;
    let (_, g) = env.market_state();
    // winner recovered MORE than just their capital — insurance covered the loser's bad debt.
    assert!(
        won > 1_000_000,
        "winner made (more) whole by insurance backstop: got {}",
        won
    );
    // insurance was SPENT (not conjured), bounded by what was available.
    assert!(
        g.insurance <= ins_before,
        "insurance only spent, never conjured ({} <= {})",
        g.insurance,
        ins_before
    );
    // no value printed: total tokens out + remaining vault accounted, no creation.
    assert!(g.vault <= vault_before, "vault not over-credited");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
}

// security.md sweep — haircut proportionality with disparate claims (#33/#37): two resolved winners
// with very different positive-pnl faces (100 vs 900) sharing insufficient backing must be paid
// PROPORTIONALLY to their claim size (~1:9), and the total must not exceed the backing.
#[test]
fn v16_attack_haircut_proportional_to_claim_size() {
    const BACKING: u128 = 100;
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACKING, 10_000);
    let o1 = Keypair::new();
    let p1 = env.create_portfolio(&o1);
    let o2 = Keypair::new();
    let p2 = env.create_portfolio(&o2);
    env.deposit(&o1, p1, 1_000);
    env.deposit(&o2, p2, 1_000);
    env.add_source_positive_pnl(p1, 1, 100); // small claim
    env.add_source_positive_pnl(p2, 1, 900); // 9x larger claim
    env.resolve();
    // two close passes to converge the terminal haircut rate.
    let mut out1 = 0u128;
    let mut out2 = 0u128;
    for _ in 0..2 {
        let d1 = env.close_resolved(&o1, p1);
        out1 += env.token_amount(d1) as u128;
        let d2 = env.close_resolved(&o2, p2);
        out2 += env.token_amount(d2) as u128;
    }
    let hc1 = out1.saturating_sub(1_000); // haircut payout above senior capital
    let hc2 = out2.saturating_sub(1_000);
    // proportionality: the larger claim (9x) gets ~9x the haircut payout.
    assert!(
        hc1 > 0 && hc2 > 0,
        "both winners got some haircut payout (hc1={} hc2={})",
        hc1,
        hc2
    );
    assert!(
        hc2 >= hc1 * 8 && hc2 <= hc1 * 10,
        "payout ~proportional to claim size (9x): hc1={} hc2={}",
        hc1,
        hc2
    );
    // total haircut paid never exceeds the backing (no over-pay).
    assert!(
        hc1 + hc2 <= BACKING,
        "summed haircut payout {} <= backing {}",
        hc1 + hc2,
        BACKING
    );
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — haircut rounding across MANY tiny winners (#22/#33/#35): when the backing residual
// is smaller than total positive-pnl claims, the haircut rate h = residual / total_claims is applied to
// each winner's claim. The classic precision attack on any proportional split is per-winner round-UP: with
// N winners each rounding their floor(claim_i * h) UP, the summed payout can exceed the residual, MINTING
// value from rounding dust. The 2-winner test (#11314) checks proportionality; this stresses the rounding
// edge with EIGHT winners holding coprime claims against a tiny residual (max cumulative rounding error).
// Attacker goal: split a claim across many small accounts so the rounded-up dust sums to more than the
// backing actually held — a free mint. Protection: the engine rounds haircut payouts conservatively
// (down/toward the protocol), so sum(payouts) <= residual no matter how the claims are partitioned.
#[test]
fn v16_attack_haircut_rounding_many_winners_no_mint() {
    const BACKING: u128 = 100; // tiny residual -> aggressive haircut, max rounding pressure
                               // eight coprime/awkward claims so claim_i * h rarely lands on an integer (forces rounding every winner).
    const CLAIMS: [u128; 8] = [7, 11, 13, 17, 19, 23, 29, 31]; // sum = 150 >> 100 backing
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACKING, 10_000);
    let mut owners = Vec::new();
    let mut ports = Vec::new();
    for &claim in CLAIMS.iter() {
        let o = Keypair::new();
        let p = env.create_portfolio(&o);
        env.deposit(&o, p, 1_000);
        env.add_source_positive_pnl(p, 1, claim);
        owners.push(o);
        ports.push(p);
    }
    let total_claims: u128 = CLAIMS.iter().sum();
    assert!(
        total_claims > BACKING,
        "non-vacuous: claims ({}) exceed backing ({}) so the haircut bites",
        total_claims,
        BACKING
    );

    env.resolve();
    // converge the terminal haircut rate across all winners (a few passes, like the 2-winner test).
    let mut hc_total = 0u128;
    let mut payouts = vec![0u128; CLAIMS.len()];
    for _ in 0..3 {
        for (i, (o, p)) in owners.iter().zip(ports.iter()).enumerate() {
            let d = env.close_resolved(o, *p);
            payouts[i] += env.token_amount(d) as u128;
        }
    }
    // haircut payout = anything paid ABOVE each winner's own senior capital (1_000 deposited).
    for (i, &paid) in payouts.iter().enumerate() {
        let hc = paid.saturating_sub(1_000);
        hc_total += hc;
        // each winner's haircut payout is bounded by its own claim (never paid MORE than it claimed).
        assert!(
            hc <= CLAIMS[i],
            "winner {} haircut payout {} <= its claim {}",
            i,
            hc,
            CLAIMS[i]
        );
    }
    // THE HEADLINE: summed haircut payout across all eight winners never exceeds the backing residual —
    // per-winner rounding dust cannot accumulate into a mint, however the claims are partitioned.
    assert!(
        hc_total <= BACKING,
        "summed haircut payout {} must not exceed backing {} (no rounding mint)",
        hc_total,
        BACKING
    );
    // non-vacuity: the haircut actually paid out a meaningful fraction of the backing (not a degenerate zero).
    assert!(
        hc_total > BACKING / 2,
        "non-vacuous: the winners collectively received a real haircut payout ({} of {})",
        hc_total,
        BACKING
    );
    // conservation: no vault minting, senior invariant intact.
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation after many-winner haircut"
    );
}

// Public max-shape composition: retain both payout-source domains for every active leg, then prove
// a normal owner-signed reduction still fits one transaction without state injection.

#[test]
fn v16_bpf_close_resolved_moves_payout_tokens_with_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    env.resolve();
    let dest = env.close_resolved(&owner, portfolio);
    assert_eq!(env.token_amount(dest), 1_000);
    assert_eq!(env.token_amount(env.vault), 0);

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
}

#[test]
fn v16_bpf_close_resolved_pays_positive_pnl_through_engine_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.top_up_backing_bucket(1, 250, 10);
    env.add_source_positive_pnl(portfolio, 1, 250);

    env.resolve();
    let dest = env.close_resolved(&owner, portfolio);
    assert_eq!(env.token_amount(dest), 1_250);
    assert_eq!(env.token_amount(env.vault), 0);

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
    assert_eq!(account.pnl.get(), 0);
    // Source-backed positive pnl is REALIZED into capital at resolved close (the
    // realize_source_backed_claims step) and paid out as capital — not parked as a junior payout
    // receipt against the backing it is underwritten by. The winner is fully paid (1_250 above)
    // and winds down with NO resolved receipt outstanding.
    assert!(!resolved_receipt(&account).present);
}

// Coverage probe (audit): an INSOLVENT resolved market (residual < positive-PnL
// face, so the resolved payout rate < 1) pays a winner only floor(face*rate) <
// face. The receipt's `finalized` flag is set ONLY when paid_effective ==
// terminal_positive_claim_face (the FULL face), so under a haircut it can never
// finalize. If that is a real gap, the winner's portfolio can never be
// dematerialized (engine dematerialization requires a finalized-or-absent
// receipt), materialized_portfolio_count is stuck >= 1, and the market can never
// WithdrawInsurance or CloseSlab -> permanent fund/rent strand.
//
// This test asserts the CORRECT end-state (the fully-settled winner reaches a
// closable receipt state and the portfolio can be reclaimed).
// GREEN regression: Finding D was fixed in engine b6e23b3
// (clear_fully_diluted_resolved_receipt_if_terminal clears the receipt at the
// terminal rate so the portfolio dematerializes).
#[test]
fn v16_audit_insolvent_resolved_winner_can_dematerialize() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    // Winner carries +250 of positive PnL face, but its domain is backed by only
    // 100, so the resolved junior pool (residual = vault - c_tot) is 100 < 250 ->
    // a permanent haircut: payout rate = 100/250 = 0.4.
    env.top_up_backing_bucket(1, 100, 10_000);
    env.add_source_positive_pnl(portfolio, 1, 250);

    env.resolve();
    let _dest = env.close_resolved(&owner, portfolio);

    let account = state::read_portfolio(&env.svm.get_account(&portfolio).unwrap().data).unwrap();
    assert_eq!(account.capital.get(), 0, "capital paid out");
    assert_eq!(account.pnl.get(), 0, "pnl zeroed by resolved close");
    // A fully-paid (haircut) resolved winner must reach a CLOSABLE receipt state so the
    // portfolio can dematerialize: either finalized, or cleared/absent once it has been
    // paid its full entitlement at the terminal rate. If it can't, materialized_portfolio_count
    // stays >= 1 and the market is permanently un-drainable (no WithdrawInsurance, no CloseSlab).
    assert!(
        !resolved_receipt(&account).present || resolved_receipt(&account).finalized,
        "haircut winner's receipt must be closable (finalized or cleared at the terminal rate); \
         present={} finalized={}",
        resolved_receipt(&account).present,
        resolved_receipt(&account).finalized,
    );

    // The consequence: the owner must be able to reclaim the fully-settled
    // portfolio (this dematerializes it). Panics if the receipt blocks closability.
    env.close_portfolio_with_cu(&owner, portfolio);
}

// Coverage probe (audit, Finding G): close_resolved_account_not_atomic charges an
// accrued maintenance fee into group.insurance (handle_close_resolved passes
// cfg.maintenance_fee_per_slot) but the wrapper does NOT credit any per-domain
// insurance budget for it. WithdrawInsurance caps each authority's claim through
// terminal_insurance_withdraw_capacity_for_authority_view,
// not group.insurance, so this fee is withdrawable by NOBODY and permanently
// blocks CloseSlab (requires insurance==0). This asserts the CORRECT invariant
// (all of group.insurance is attributable to a withdrawable domain budget); it
// goes RED iff the strand is real. Confirmed by mainnet evidence (market AWCZ2pK,
// 4060 lamports of stranded dust with every authority = admin).
// GREEN regression: Finding G fixed in handle_close_resolved (the resolved maintenance
// fee is now domain-credited via credit_maintenance_fee_to_active_market_budgets_view).
#[test]
fn v16_audit_resolved_maintenance_fee_insurance_stays_recoverable() {
    // maintenance_fee_per_slot = 5
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 5,
    );
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    // Accrue ~100 slots of maintenance fee, then resolve and close.
    env.svm.warp_to_slot(100);
    env.resolve();
    env.close_resolved(&owner, portfolio);

    let (_, group) = env.market_state();
    let sum_budgets: u128 = group.insurance_domain_budget.iter().sum();
    assert!(
        sum_budgets >= group.insurance,
        "all of group.insurance must be attributable to a withdrawable domain budget so an \
         authority can sweep it and CloseSlab can succeed; insurance={} but \
         sum(domain budgets)={} -> the {} difference is stranded forever",
        group.insurance,
        sum_budgets,
        group.insurance.saturating_sub(sum_budgets),
    );
}

// Finding-G follow-up (post-#113-fix): the #113 fix routes the account-level maintenance fee solely to
// asset-0 via credit_maintenance_fee_to_active_market_budgets_view, which handle_close_resolved also calls
// (10288). The existing Finding-G regression (v16_audit_resolved_maintenance_fee_insurance_stays_recoverable)
// is SINGLE-asset; this guards the MULTI-asset resolved-close path: with an appended asset-1 present, the
// resolved maintenance fee must still be attributable to a withdrawable domain budget (sum(budgets) >=
// insurance) and land in asset-0, never stranded on the parasite or in a non-credited aggregate.
#[test]
fn v16_audit_resolved_maintenance_fee_multi_asset_stays_recoverable() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 5,
    );
    env.update_market_init_fee_policy_with_cu(1);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    // Append a do-nothing asset-1 so the market is MULTI-asset when the resolved maintenance fee routes.
    let appender = Keypair::new();
    env.ensure_signer_account(appender.pubkey());
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &appender,
        1,
        1,
        100,
        appender.pubkey(),
        appender.pubkey(),
        appender.pubkey(),
        appender.pubkey(),
        1,
    );

    // Accrue ~100 slots of maintenance fee, then resolve and close.
    env.svm.warp_to_slot(100);
    env.resolve();
    env.close_resolved(&owner, portfolio);

    let (_, group) = env.market_state();
    // Finding-G invariant in MULTI-asset mode: all of group.insurance attributable to a withdrawable domain budget.
    let sum_budgets: u128 = group.insurance_domain_budget.iter().sum();
    assert!(
        sum_budgets >= group.insurance,
        "multi-asset resolved: insurance={} but sum(domain budgets)={} -> {} stranded forever",
        group.insurance,
        sum_budgets,
        group.insurance.saturating_sub(sum_budgets),
    );
    // #113 routing in resolved mode: the parasite asset-1 (domains 2/3) earns NOTHING from the maintenance fee.
    assert_eq!(
        group.insurance_domain_budget[2] + group.insurance_domain_budget[3],
        0,
        "resolved maintenance fee must not land on the appended asset-1 (domains 2/3)"
    );
    assert_domain_budget_remaining_total_consistent(&group, "multi-asset resolved maintenance fee");
}

// regression (security.md sweep): round-trip recovery under the junior-pnl model. A price round-trip
// (100->110->100) leaves the drawdown-first trader's recovery as JUNIOR pnl (realized losses are
// senior/immediate, recoveries park as junior pnl that is not liquid in Live mode). Value is fully
// CONSERVED (vault == deposits) and fully RECOVERABLE at resolution — NOT a loss of funds. Documents
// that per-account LIQUID equity is not symmetric in Live mode, but total value is and resolution pays
// everyone their fair amount.
#[test]
fn v16_regression_roundtrip_recovers_fully_at_resolution() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (1_000 * POS_SCALE) as i128,
        100,
        0,
    );
    let crank_all = |env: &mut V16CuEnv, s: u64| {
        for p in [sh, lo] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: s,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    };
    // up to 110 then back to 100 (round-trip to breakeven).
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for s in [10u64, 11, 12] {
        env.svm.warp_to_slot(s);
        crank_all(&mut env, s);
    }
    env.svm.warp_to_slot(20);
    env.push_auth_mark_with_cu(20, 100);
    for s in [20u64, 21, 22, 23, 24] {
        env.svm.warp_to_slot(s);
        crank_all(&mut env, s);
    }
    // close both, crank to convergence.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        -(1_000 * POS_SCALE as i128),
        100,
        0,
    );
    for s in 25u64..=35 {
        env.svm.warp_to_slot(s);
        crank_all(&mut env, s);
    }
    // Live-mode invariants: value conserved, short's recovery is junior pnl backed by residual.
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault, 2_000_000,
        "vault conserved through the round-trip (no value created/destroyed)"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= b.pnl.get().max(0),
        "short's junior recovery pnl is backed by residual"
    );
    // resolution pays EVERYONE their full fair value — no permanent LoF from the junior-pnl mechanism.
    env.resolve();
    let lo_dest = env.close_resolved(&lo_owner, lo);
    let sh_dest = env.close_resolved(&sh_owner, sh);
    assert_eq!(
        env.token_amount(lo_dest),
        1_000_000,
        "long fully recovered at resolution"
    );
    assert_eq!(
        env.token_amount(sh_dest),
        1_000_000,
        "short fully recovered at resolution (junior pnl realized)"
    );
}
