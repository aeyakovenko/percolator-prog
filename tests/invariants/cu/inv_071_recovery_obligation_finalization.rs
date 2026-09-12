//! INV-071/082: a released obligation precedes market Recovery finalization.
//! Public genesis and transitions only; expected serialized accounts are never installed in SVM.

use super::*;

struct Actor {
    owner: Keypair,
    portfolio: Pubkey,
    token: Pubkey,
}

fn public_world() -> (V16CuEnv, Vec<Actor>) {
    let params = V16CuMarketParams {
        max_portfolio_assets: 2,
        max_bankrupt_close_lifetime_slots: 2,
        public_b_chunk_atoms: 1,
        ..V16CuMarketParams::default()
    };
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    for (id, path) in [
        (program_id, program_path()),
        (spl_token::ID, spl_token_program_path()),
        (
            associated_token_program_id(),
            associated_token_program_path(),
        ),
    ] {
        svm.add_program(id, &std::fs::read(path).unwrap());
    }
    let payer = Keypair::new();
    let admin = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
    let mint = Keypair::new();
    system_create_account_for_test(&mut svm, &payer, &mint, Mint::LEN, spl_token::ID);
    send_raw_tx(
        &mut svm,
        &payer,
        spl_token::instruction::initialize_mint2(
            &spl_token::ID,
            &mint.pubkey(),
            &admin.pubkey(),
            None,
            0,
        )
        .unwrap(),
        &[],
    )
    .unwrap();
    let market = Keypair::new();
    system_create_account_for_test(
        &mut svm,
        &payer,
        &market,
        state::market_account_len_for_capacity(2).unwrap(),
        program_id,
    );
    let vault_authority =
        Pubkey::find_program_address(&[b"vault", market.pubkey().as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint.pubkey());
    let init_market_cu = send_tx(
        &mut svm,
        program_id,
        &payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market.pubkey(), false),
            AccountMeta::new_readonly(mint.pubkey(), false),
        ],
        &[&admin],
    )
    .unwrap();
    let mut env = V16CuEnv {
        svm,
        program_id,
        payer,
        admin,
        init_market_cu,
        market: market.pubkey(),
        mint: mint.pubkey(),
        vault,
        vault_authority,
        portfolio_account_len: state::portfolio_account_len_for_market_slots(2).unwrap(),
        portfolios: Vec::new(),
    };
    let mut actors = Vec::new();
    for amount in [1_000u64, 1_000, 10, 2] {
        let owner = Keypair::new();
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[&owner],
        )
        .unwrap();
        env.portfolios.push(portfolio.pubkey());
        let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &env.admin.pubkey(),
                &[],
                amount,
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        env.send(
            env.deposit_ix(portfolio.pubkey(), u128::from(amount)),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
                AccountMeta::new(token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap();
        actors.push(Actor {
            owner,
            portfolio: portfolio.pubkey(),
            token,
        });
    }
    for asset in 0..2 {
        env.configure_auth_mark_for_asset_as_admin(asset, 0, 100);
    }
    env.configure_permissionless_resolve_with_cu(100, 5);
    (env, actors)
}

// This target-local rank is read from committed state, not a selector or a modeled step.
// Independent bankruptcy work is byte-framed; this is not a whole-market terminal rank.
fn rank(env: &V16CuEnv, target: Pubkey) -> (u8, usize, u128) {
    let phase = match env.market_state().1.mode {
        MarketModeV16::Live => 2,
        MarketModeV16::Recovery => 1,
        MarketModeV16::Resolved => 0,
    };
    let account = env.portfolio_state(target);
    let legs = account.legs.iter().filter(|leg| leg.active != 0).count();
    (phase, legs, account.capital.get())
}

fn crank(env: &mut V16CuEnv, target: Pubkey, caller_slot: u64) -> u64 {
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: caller_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("bounded committed-state continuation");
    assert_cu_within(
        "INV-071 Recovery obligation/finalization",
        cu,
        CRANK_CU_LIMIT,
    );
    cu
}

fn assert_market(env: &V16CuEnv, before: &Account, expected: &MarketGroupV16) {
    let cfg = state::read_market(&before.data).unwrap().0;
    let mut bytes = before.clone();
    state::write_market(&mut bytes.data, &cfg, expected).unwrap();
    assert_eq!(env.svm.get_account(&env.market).unwrap(), bytes);
}

#[test]
fn v16_program_recovery_releases_obligation_before_exact_finalization_and_payout() {
    let mut max_cu = 0;
    for direction in [1i128, -1] {
        let (mut env, actors) = public_world();
        let target = actors[0].portfolio;
        let peer = actors[1].portfolio;
        let debtor = actors[3].portfolio;
        env.trade_asset_with_cu(
            0,
            &actors[0].owner,
            target,
            &actors[1].owner,
            peer,
            direction * POS_SCALE as i128,
            100,
            0,
        );
        env.trade_asset_with_cu(
            1,
            &actors[2].owner,
            actors[2].portfolio,
            &actors[3].owner,
            debtor,
            (POS_SCALE / 50) as i128,
            100,
            0,
        );
        for (slot, price) in [(1, 200), (2, 300)] {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(1, slot, price);
            env.crank(
                actors[2].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
            );
        }
        env.crank(
            debtor,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(1),
            },
        );
        env.svm.warp_to_slot(3);
        let admin = Keypair::from_bytes(&env.admin.to_bytes()).unwrap();
        for asset in [0, 1] {
            env.try_shutdown_asset_with_authority(&admin, asset, 3)
                .unwrap();
        }
        // At unchanged price, the first exit retains only loss weight until its peer exits.
        env.forfeit_recovery_leg_with_cu(&actors[0].owner, target, 0, 1);
        let obligation = active_leg_for_asset(&env.portfolio_state(target), 0);
        assert_eq!(obligation.basis_pos_q, 0);
        assert_eq!(obligation.loss_weight, POS_SCALE);
        assert!(!obligation.stale && !obligation.b_stale);
        assert_eq!(obligation.b_rem, 0);
        env.forfeit_recovery_leg_with_cu(&actors[1].owner, peer, 0, 1);
        assert!(!has_active_leg_for_asset(&env.portfolio_state(peer), 0));
        env.forfeit_recovery_leg_with_cu(&actors[3].owner, debtor, 1, 1);
        let close = close_progress(&env.portfolio_state(debtor));
        assert!(inv071_close_pending(close));
        assert_eq!(close.residual_remaining, 1);
        assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
        assert_eq!(rank(&env, target), (2, 1, 1_000));

        let extra: Vec<_> = actors
            .iter()
            .flat_map(|a| [a.owner.pubkey(), a.token])
            .chain([env.vault_authority])
            .collect();
        let expiry = close.max_close_slot.checked_add(1).unwrap();
        env.svm.warp_to_slot(expiry);
        let before = inv071_continuation_frame(&env, &extra);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let mut expected_recovery = env.market_state().1;
        let rank_before = rank(&env, target);
        max_cu = max_cu.max(crank(&mut env, debtor, 0));
        inv071_assert_continuation_frame(&env, &before, &[env.market]);
        let recovered = env.market_state().1;
        assert_eq!(
            recovered.recovery_reason,
            Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
        );
        assert_eq!(rank(&env, target), (1, 1, 1_000));
        assert!(rank(&env, target) < rank_before);
        expected_recovery.mode = MarketModeV16::Recovery;
        expected_recovery.recovery_reason =
            Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress);
        expected_recovery.current_slot = expiry;
        assert_market(&env, &market_before, &expected_recovery);

        let before = inv071_continuation_frame(&env, &extra);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let target_before = env.svm.get_account(&target).unwrap();
        let rank_before = rank(&env, target);
        max_cu = max_cu.max(crank(&mut env, target, u64::MAX));
        assert_eq!(rank(&env, target), (1, 0, 1_000));
        assert!(rank(&env, target) < rank_before);
        inv071_assert_continuation_frame(&env, &before, &[env.market, target]);
        let mut expected = recovered.clone();
        let asset = &mut expected.assets[0];
        assert_eq!(asset.oi_eff_long_q, 0);
        assert_eq!(asset.oi_eff_short_q, 0);
        assert_eq!(
            asset.stored_pos_count_long + asset.stored_pos_count_short,
            1
        );
        assert_eq!(
            asset.pending_obligation_count_long + asset.pending_obligation_count_short,
            1
        );
        assert_eq!(
            asset.loss_weight_sum_long + asset.loss_weight_sum_short,
            POS_SCALE
        );
        match obligation.side {
            SideV16::Long => {
                assert_eq!(
                    (
                        asset.stored_pos_count_long,
                        asset.pending_obligation_count_long
                    ),
                    (1, 1)
                );
                assert_eq!(asset.loss_weight_sum_long, POS_SCALE);
                asset.stored_pos_count_long = 0;
                asset.pending_obligation_count_long = 0;
                asset.loss_weight_sum_long = 0;
            }
            SideV16::Short => {
                assert_eq!(
                    (
                        asset.stored_pos_count_short,
                        asset.pending_obligation_count_short
                    ),
                    (1, 1)
                );
                assert_eq!(asset.loss_weight_sum_short, POS_SCALE);
                asset.stored_pos_count_short = 0;
                asset.pending_obligation_count_short = 0;
                asset.loss_weight_sum_short = 0;
            }
        }
        assert_market(&env, &market_before, &expected);
        let mut expected_target = state::read_portfolio(&target_before.data).unwrap();
        expected_target.legs[0] =
            percolator::PortfolioLegV16Account::from_runtime(&percolator::PortfolioLegV16::EMPTY);
        expected_target.active_bitmap =
            percolator::active_bitmap_empty().map(percolator::V16PodU64::new);
        expected_target.health_cert.valid = 0;
        let mut target_bytes = target_before;
        state::write_portfolio(&mut target_bytes.data, &expected_target).unwrap();
        assert_eq!(env.svm.get_account(&target).unwrap(), target_bytes);

        // Cleanup consumed one call without finalizing. A later Clock, not the caller slot,
        // timestamps the next bounded transition; the frozen asset clocks must not advance.
        let final_slot = expiry + 9;
        env.svm.warp_to_slot(final_slot);
        let before = inv071_continuation_frame(&env, &extra);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let rank_before = rank(&env, target);
        max_cu = max_cu.max(crank(&mut env, target, 0));
        assert_eq!(rank(&env, target), (0, 0, 1_000));
        assert!(rank(&env, target) < rank_before);
        expected.mode = MarketModeV16::Resolved;
        expected.current_slot = final_slot;
        expected.resolved_slot = final_slot;
        expected.loss_stale_active = false;
        assert_market(&env, &market_before, &expected);
        inv071_assert_continuation_frame(&env, &before, &[env.market]);
        assert_eq!(env.token_amount(env.vault), 2_012);
        assert_eq!(expected.vault, 2_012);
        let mint = env.svm.get_account(&env.mint).unwrap();
        assert_eq!(Mint::unpack(&mint.data).unwrap().supply, 2_012);
        for actor in &actors {
            assert_eq!(env.token_amount(actor.token), 0);
        }

        let payout_accounts = vec![
            AccountMeta::new_readonly(actors[0].owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(target, false),
            AccountMeta::new(actors[0].token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ];
        assert_eq!(env.market_state().0.force_close_delay_slots, 5);
        let deadline = final_slot + 5;
        env.svm.warp_to_slot(deadline - 1);
        let before = inv071_continuation_frame(&env, &extra);
        env.svm.expire_blockhash();
        let error = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: u64::MAX,
                    observations: vec![],
                },
                payout_accounts.clone(),
                &[],
            )
            .expect_err("finalization must start a fresh authenticated owner window");
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::ExpectedSigner as u32
            )),
            "{error}"
        );
        inv071_assert_continuation_frame(&env, &before, &[]);
        assert_eq!(rank(&env, target), (0, 0, 1_000));

        env.svm.warp_to_slot(deadline);
        env.svm.expire_blockhash();
        let before = inv071_continuation_frame(&env, &extra);
        let capital_before = env.market_state().1.c_tot;
        let rank_before = rank(&env, target);
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: vec![],
                },
                payout_accounts.clone(),
                &[],
            )
            .expect("released-obligation target retains its full permissionless principal exit");
        assert_cu_within("INV-071 post-finalization payout", cu, CRANK_CU_LIMIT);
        max_cu = max_cu.max(cu);
        assert_eq!(rank(&env, target), (0, 0, 0));
        assert!(rank(&env, target) < rank_before);
        assert!(resolved_portfolio_is_terminal(&env, target));
        assert_eq!(env.market_state().1.c_tot, capital_before - 1_000);
        assert_eq!(env.market_state().1.vault, 1_012);
        inv071_assert_continuation_frame(
            &env,
            &before,
            &[env.market, target, env.vault, actors[0].token],
        );
        for (key, amount) in [(env.vault, 1_012), (actors[0].token, 1_000)] {
            let mut expected = before
                .iter()
                .find(|(k, _)| *k == key)
                .unwrap()
                .1
                .clone()
                .unwrap();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(env.svm.get_account(&key).unwrap(), expected);
        }
        let before = inv071_continuation_frame(&env, &extra);
        env.svm.expire_blockhash();
        let error = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: u64::MAX,
                    observations: vec![],
                },
                payout_accounts,
                &[],
            )
            .expect_err("completed target cannot replay finalization or payout");
        assert!(
            error.contains(&format!(
                "Custom({})",
                PercolatorError::EngineNonProgress as u32
            )),
            "{error}"
        );
        inv071_assert_continuation_frame(&env, &before, &[]);
        assert_eq!(rank(&env, target), (0, 0, 0));
    }
    eprintln!("INV-071/082 Recovery obligation: 2 sides, 8 successful cranks, 4 exact rollbacks, 2 exact 1000-atom payouts; max CU={max_cu}");
}
