//! INV-071/073/078: Recovery claims and deferred live liabilities retain keeper-only exits.
//! Public System/SPL/ATA/wrapper genesis; no economic account bytes are installed by the test.
//! A locally nonprogressing claimant does not prevent an idle payout or unsigned debtor settlement.
//! This is a bounded economic-disposition witness, not an owner-reduction or retirement proof.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;

const PRINCIPAL: [u128; 4] = [1_000, 1_000, 1_000, 137];
const UNITS: [u128; 2] = [7, 11];
const BACKING: [u128; 2] = [17, 23];
const RESOLVE_SLOT: u64 = 40;
const PAYOUT_SLOT: u64 = RESOLVE_SLOT + 3;
const CALL_BOUND: usize = 8;

struct FundedWorld {
    env: V16CuEnv,
    owners: [Keypair; 4],
    portfolios: [Pubkey; 4],
    tokens: [Pubkey; 4],
    provider_token: Pubkey,
}

fn public_world(recovery_asset: usize, expiry: u64) -> FundedWorld {
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
    env.svm.warp_to_slot(1);
    for asset in [0, 1] {
        env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
    }
    env.configure_permissionless_resolve_with_cu(20, 3);
    let owners = std::array::from_fn(|_| Keypair::new());
    let portfolios = std::array::from_fn(|actor| {
        env.svm
            .airdrop(&owners[actor].pubkey(), 1_000_000_000)
            .unwrap();
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
                AccountMeta::new(owners[actor].pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(key.pubkey(), false),
            ],
            &[&owners[actor]],
        )
        .unwrap();
        env.portfolios.push(key.pubkey());
        key.pubkey()
    });
    let tokens = std::array::from_fn(|actor| {
        create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint)
    });
    let provider_token =
        create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
    for (token, amount) in tokens
        .into_iter()
        .zip(PRINCIPAL)
        .chain([(provider_token, BACKING.iter().sum())])
    {
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &env.admin.pubkey(),
                &[],
                u64::try_from(amount).unwrap(),
            )
            .unwrap(),
            &[&env.admin],
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
            &env.admin.pubkey(),
            &[],
        )
        .unwrap(),
        &[&env.admin],
    )
    .unwrap();
    for actor in 0..4 {
        env.send(
            env.deposit_ix(portfolios[actor], PRINCIPAL[actor]),
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
    for role in 0..2 {
        let asset = if role == 0 {
            recovery_asset
        } else {
            1 - recovery_asset
        };
        env.top_up_backing_bucket_from_admin_token_with_cu(
            provider_token,
            (2 * asset + 1) as u16,
            BACKING[role],
            if role == 0 { expiry } else { 100 },
        );
        env.trade_asset_with_cu(
            asset as u16,
            &owners[0],
            portfolios[0],
            &owners[role + 1],
            portfolios[role + 1],
            (UNITS[role] * POS_SCALE) as i128,
            100,
            0,
        );
    }
    FundedWorld {
        env,
        owners,
        portfolios,
        tokens,
        provider_token,
    }
}

// The only transaction signer in the terminal suffix is an unrelated fee payer.
// Check the actual message, packet size and signatures before delivering it to SBF.
fn keeper_call(world: &mut FundedWorld, actor: usize, use_crank: bool) -> Result<u64, String> {
    let env = &mut world.env;
    env.svm.expire_blockhash();
    let instruction = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(world.owners[actor].pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(world.portfolios[actor], false),
            AccountMeta::new(world.tokens[actor], false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: if use_crank {
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            }
        } else {
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
        }
        .encode(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(CRANK_CU_LIMIT as u32),
            instruction,
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer],
        env.svm.latest_blockhash(),
    );
    assert_eq!(transaction.message.header.num_required_signatures, 1);
    assert_eq!(transaction.message.account_keys[0], env.payer.pubkey());
    assert!(world
        .owners
        .iter()
        .all(|owner| owner.pubkey() != env.payer.pubkey()));
    assert_ne!(env.admin.pubkey(), env.payer.pubkey());
    transaction.verify().unwrap();
    assert!(bincode::serialized_size(&transaction).unwrap() <= 1_232);
    env.svm
        .send_transaction(transaction)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|error| format!("{error:?}"))
}

fn economic_frame(world: &FundedWorld) -> Vec<(Pubkey, Option<Account>)> {
    let env = &world.env;
    [
        env.market,
        env.vault,
        env.mint,
        env.admin.pubkey(),
        env.vault_authority,
        world.provider_token,
        solana_sdk::sysvar::clock::id(),
    ]
    .into_iter()
    .chain(world.portfolios)
    .chain(world.tokens)
    .chain(world.owners.iter().map(Signer::pubkey))
    .map(|key| (key, env.svm.get_account(&key)))
    .collect()
}

fn assert_custody(world: &FundedWorld) {
    let env = &world.env;
    let group = env.market_state().1;
    let paid = world
        .tokens
        .iter()
        .map(|&token| u128::from(env.token_amount(token)))
        .sum::<u128>();
    let capital = world
        .portfolios
        .iter()
        .map(|&key| env.portfolio_state(key).capital.get())
        .sum::<u128>();
    let supply = PRINCIPAL.iter().sum::<u128>() + BACKING.iter().sum::<u128>();
    assert_eq!(group.c_tot, capital);
    assert_eq!(group.insurance, 0);
    assert!(group.vault >= capital);
    assert_eq!(group.vault, u128::from(env.token_amount(env.vault)));
    assert_eq!(group.vault + paid, supply);
    assert_eq!(env.token_amount(world.provider_token), 0);
    let mint = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data).unwrap();
    assert_eq!(u128::from(mint.supply), supply);
    assert_eq!(mint.mint_authority, COption::None);
}

// This fixed-clock, two-asset rank measures economic work, not a decoded selector plan.
// Cleanup can create a receipt, so account normalization precedes receipt debt in the tuple.
fn exit_rank(world: &FundedWorld, actor: usize) -> (usize, usize, usize, u128, u128, bool, u128) {
    let account = world.env.portfolio_state(world.portfolios[actor]);
    let receipt = resolved_receipt(&account);
    let group = world.env.market_state().1;
    let expiry_work = group
        .source_backing_buckets
        .iter()
        .filter(|bucket| {
            bucket.status == BackingBucketStatusV16::Fresh && bucket.expiry_slot <= PAYOUT_SLOT
        })
        .count();
    (
        expiry_work,
        account.legs.iter().filter(|leg| leg.active != 0).count(),
        account
            .source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        account.capital.get(),
        account.pnl.get().max(0) as u128,
        !resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]),
        receipt
            .terminal_positive_claim_face
            .checked_sub(receipt.paid_effective)
            .unwrap(),
    )
}

fn advance(world: &mut FundedWorld, actor: usize, use_crank: bool) -> Result<u64, String> {
    let before = economic_frame(world);
    let rank_before = exit_rank(world, actor);
    let cu = match keeper_call(world, actor, use_crank) {
        Ok(cu) => cu,
        Err(error) => {
            assert_eq!(economic_frame(world), before);
            return Err(error);
        }
    };
    assert_cu_within(
        "INV-073 mixed Recovery claim/liability exit",
        cu,
        CRANK_CU_LIMIT,
    );
    let rank_after = exit_rank(world, actor);
    assert!(
        rank_after < rank_before,
        "actor {actor}: {rank_before:?} -> {rank_after:?}"
    );
    for (key, account) in before {
        if ![
            world.env.market,
            world.env.vault,
            world.portfolios[actor],
            world.tokens[actor],
        ]
        .contains(&key)
        {
            assert_eq!(
                world.env.svm.get_account(&key),
                account,
                "unrelated account {key}"
            );
        }
    }
    assert_custody(world);
    Ok(cu)
}

#[test]
fn v16_program_recovery_claim_and_deferred_liability_preserve_keeper_exit() {
    let gains = UNITS.map(|units| units * (105 - 100));
    let total_gain = gains.iter().sum::<u128>();
    for recovery_asset in [0usize, 1] {
        for expiry in [PAYOUT_SLOT, 100] {
            for claimant_first in [true, false] {
                let mut world = public_world(recovery_asset, expiry);
                let live_asset = 1 - recovery_asset;
                let env = &mut world.env;
                env.svm.warp_to_slot(2);
                for asset in [0, 1] {
                    env.push_auth_mark_for_asset_as_admin(asset, 2, 105);
                }
                env.crank(
                    world.portfolios[0],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations_for_assets(&[0, 1]),
                    },
                );
                env.svm.warp_to_slot(3);
                let admin = Keypair::from_bytes(&env.admin.to_bytes()).unwrap();
                env.try_shutdown_asset_with_authority(&admin, recovery_asset as u16, 3)
                    .unwrap();
                env.svm.warp_to_slot(7);
                let keeper = Keypair::new();
                env.force_close_abandoned_asset_with_cu(
                    &keeper,
                    world.portfolios[0],
                    world.portfolios[1],
                    recovery_asset as u16,
                    7,
                    UNITS[0] * POS_SCALE,
                );
                let target = env.portfolio_state(world.portfolios[0]);
                let debtor = env.portfolio_state(world.portfolios[2]);
                let group = env.market_state().1;
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.assets[recovery_asset].lifecycle,
                    AssetLifecycleV16::Recovery
                );
                assert_eq!(
                    group.assets[live_asset].lifecycle,
                    AssetLifecycleV16::Active
                );
                assert!(!has_active_leg_for_asset(&target, recovery_asset));
                assert!(has_active_leg_for_asset(&target, live_asset));
                assert_eq!(target.pnl.get(), total_gain as i128);
                assert_eq!(
                    state::portfolio_source_domain(&target, 2 * recovery_asset + 1)
                        .source_claim_bound_num
                        .get(),
                    gains[0] * BOUND_SCALE
                );
                assert_eq!(debtor.capital.get(), PRINCIPAL[2]);
                assert_eq!(debtor.pnl.get(), 0);
                assert!(
                    group.assets[live_asset].k_short
                        < active_leg_for_asset(&debtor, live_asset).k_snap
                );
                assert_eq!(
                    group.source_backing_buckets[2 * recovery_asset + 1].fresh_unliened_backing_num,
                    (BACKING[0] + gains[0]) * BOUND_SCALE
                );
                assert_eq!(
                    group.source_backing_buckets[2 * live_asset + 1].fresh_unliened_backing_num,
                    BACKING[1] * BOUND_SCALE
                );
                assert_custody(&world);

                // No owner, provider, or admin signs after the mixed lifecycle checkpoint.
                let before = economic_frame(&world);
                world.env.svm.warp_to_slot(RESOLVE_SLOT);
                let cfg = world.env.market_state().0;
                assert!(
                    RESOLVE_SLOT
                        >= cfg.last_good_oracle_slot + cfg.permissionless_resolve_stale_slots
                );
                world.env.svm.expire_blockhash();
                let resolve_cu = world
                    .env
                    .send(
                        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
                        vec![AccountMeta::new(world.env.market, false)],
                        &[],
                    )
                    .expect("authenticated stale resolution of the mixed lifecycle world");
                assert_cu_within(
                    "INV-073 mixed lifecycle resolution",
                    resolve_cu,
                    CRANK_CU_LIMIT,
                );
                assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
                assert_eq!(world.env.market_state().1.resolved_slot, RESOLVE_SLOT);
                for (key, account) in before {
                    if key != world.env.market && key != solana_sdk::sysvar::clock::id() {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                world.env.svm.warp_to_slot(PAYOUT_SLOT - 1);
                let before = economic_frame(&world);
                let error = keeper_call(&mut world, 0, true)
                    .expect_err("owner window remains authenticated");
                assert!(
                    error.contains(&format!(
                        "Custom({})",
                        PercolatorError::ExpectedSigner as u32
                    )),
                    "{error}"
                );
                assert_eq!(economic_frame(&world), before);
                world.env.svm.warp_to_slot(PAYOUT_SLOT);
                let order = if claimant_first {
                    [0, 3, 1, 2]
                } else {
                    [2, 3, 1, 0]
                };
                let mut calls = [0; 4];
                let mut max_cu = 0;
                if claimant_first {
                    let mut waiting_for_debtor = false;
                    for step in 0..CALL_BOUND {
                        match advance(&mut world, 0, step % 2 == 0) {
                            Ok(cu) => {
                                max_cu = max_cu.max(cu);
                                calls[0] += 1;
                            }
                            Err(error) => {
                                assert!(
                                    error.contains(&format!(
                                        "Custom({})",
                                        PercolatorError::EngineNonProgress as u32
                                    )),
                                    "{error}"
                                );
                                waiting_for_debtor = true;
                                break;
                            }
                        }
                    }
                    assert!(waiting_for_debtor && calls[0] > 0);
                    let waiting = world.env.portfolio_state(world.portfolios[0]);
                    assert_eq!(
                        exit_rank(&world, 0),
                        (0, 0, 2, PRINCIPAL[0], total_gain, true, 0)
                    );
                    assert!(!resolved_receipt(&waiting).present);
                    assert_eq!(world.env.token_amount(world.tokens[0]), 0);
                    for role in 0..2 {
                        let asset = if role == 0 {
                            recovery_asset
                        } else {
                            live_asset
                        };
                        assert_eq!(
                            state::portfolio_source_domain(&waiting, 2 * asset + 1)
                                .source_claim_bound_num
                                .get(),
                            gains[role] * BOUND_SCALE
                        );
                    }
                    assert!(!resolved_portfolio_is_terminal(
                        &world.env,
                        world.portfolios[0]
                    ));
                    assert_eq!(
                        world.env.portfolio_state(world.portfolios[2]).capital.get(),
                        PRINCIPAL[2]
                    );
                    for use_crank in [false, true] {
                        let before = economic_frame(&world);
                        let error = keeper_call(&mut world, 0, use_crank)
                            .expect_err("claimant waits for unsettled public liability");
                        assert!(
                            error.contains(&format!(
                                "Custom({})",
                                PercolatorError::EngineNonProgress as u32
                            )),
                            "{error}"
                        );
                        assert_eq!(economic_frame(&world), before);
                    }
                }
                for round in 0..CALL_BOUND {
                    for actor in order {
                        if resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]) {
                            continue;
                        }
                        // The locally blocked claimant waits until this sweep delivers its debtor.
                        if claimant_first && actor == 0 && round == 0 {
                            continue;
                        }
                        assert!(calls[actor] < CALL_BOUND);
                        let cu = advance(&mut world, actor, (actor + round) % 2 == 0).expect(
                            "keeper settlement exposes each remaining terminal continuation",
                        );
                        calls[actor] += 1;
                        max_cu = max_cu.max(cu);
                        if claimant_first && actor == 3 && round == 0 {
                            assert_eq!(
                                u128::from(world.env.token_amount(world.tokens[3])),
                                PRINCIPAL[3]
                            );
                            assert!(has_active_leg_for_asset(
                                &world.env.portfolio_state(world.portfolios[2]),
                                live_asset
                            ));
                            assert_eq!(
                                world.env.portfolio_state(world.portfolios[0]).capital.get(),
                                PRINCIPAL[0]
                            );
                            assert_eq!(
                                world.env.portfolio_state(world.portfolios[0]).pnl.get(),
                                total_gain as i128
                            );
                        }
                    }
                }
                let expected = [
                    PRINCIPAL[0] + total_gain,
                    PRINCIPAL[1] - gains[0],
                    PRINCIPAL[2] - gains[1],
                    PRINCIPAL[3],
                ];
                for actor in 0..4 {
                    assert!(
                        resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]),
                        "actor {actor} exceeded {CALL_BOUND} calls"
                    );
                    let account = world.env.portfolio_state(world.portfolios[actor]);
                    assert_eq!(account.stale_state, 0);
                    assert_eq!(account.b_stale_state, 0);
                    assert_eq!(account.rebalance_lock, 0);
                    assert_eq!(account.liquidation_lock, 0);
                    assert_eq!(account.health_cert.valid, 0);
                    let close = close_progress(&account);
                    assert!(!close.active || (close.finalized && close.residual_remaining == 0));
                    assert_eq!(
                        u128::from(world.env.token_amount(world.tokens[actor])),
                        expected[actor]
                    );
                    for use_crank in [false, true] {
                        let before = economic_frame(&world);
                        let error = keeper_call(&mut world, actor, use_crank)
                            .expect_err("terminal retry cannot pay twice");
                        assert!(
                            error.contains(&format!(
                                "Custom({})",
                                PercolatorError::EngineNonProgress as u32
                            )),
                            "{error}"
                        );
                        assert_eq!(economic_frame(&world), before);
                    }
                }
                assert_eq!(world.env.market_state().1.c_tot, 0);
                for asset in &world.env.market_state().1.assets[..2] {
                    assert_eq!(asset.oi_eff_long_q, 0);
                    assert_eq!(asset.oi_eff_short_q, 0);
                    assert_eq!(asset.stored_pos_count_long, 0);
                    assert_eq!(asset.stored_pos_count_short, 0);
                }
                assert_eq!(
                    world.env.market_state().1.vault,
                    BACKING.iter().sum::<u128>()
                );
                assert_custody(&world);
                eprintln!("INV-073 Recovery claim/liability: recovery_asset={recovery_asset} expiry={expiry} claimant_first={claimant_first} calls={calls:?} payouts={expected:?} max_cu={max_cu}");
            }
        }
    }
}
