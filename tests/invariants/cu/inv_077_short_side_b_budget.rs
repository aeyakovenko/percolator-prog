//! INV-077: public short-side B settlement at both portfolio caps, with full and
//! final partial multi-atom chunks. Construction uses only wrapper transitions.

use super::*;

#[test]
fn v16_program_max_shape_short_b_budget_has_exact_public_progress() {
    const ASSETS: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const SOURCES: usize = percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS;
    const CAPITAL: u128 = 10_000;
    const HISTORY_Q: i128 = (POS_SCALE / 50) as i128;
    const SHORT_Q: i128 = (POS_SCALE / 25) as i128;
    const BUDGET: u128 = 4;
    const LOSS_PER_LEG: u128 = 6;
    const HISTORY_SLOT: u64 = 16;
    const OPEN_SLOT: u64 = 40;
    const SETTLE_SLOT: u64 = 81;
    const CU_LIMIT: u64 = 1_375_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: ASSETS,
        h_max: 100,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        public_b_chunk_atoms: BUDGET,
        ..V16CuMarketParams::default()
    });
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.svm.warp_to_slot(1);
    for asset in 0..ASSETS {
        env.configure_auth_mark_for_asset_as_admin(asset, 1, 100);
    }
    let owner = Keypair::new();
    let target = env.create_portfolio(&owner);
    env.deposit(&owner, target, CAPITAL);
    let peer_owner = Keypair::new();
    let peer = env.create_portfolio(&peer_owner);
    env.deposit(&peer_owner, peer, CAPITAL);
    let checkpoint_owner = Keypair::new();
    let checkpoint = env.create_portfolio(&checkpoint_owner);

    let checkpoint_marks = |env: &mut V16CuEnv, slot: u64, price: u64| {
        env.svm.warp_to_slot(slot);
        for asset in 0..ASSETS {
            env.push_auth_mark_for_asset_as_admin(asset, slot, price);
            for _ in 0..env.terminal_accrual_attempt_bound(asset, slot) {
                if env.market_state().1.assets[usize::from(asset)].slot_last == slot {
                    break;
                }
                env.crank(
                    checkpoint,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(asset),
                    },
                );
            }
            let accrued = env.market_state().1.assets[usize::from(asset)];
            assert_eq!(accrued.slot_last, slot);
            assert_eq!(accrued.effective_price, price);
        }
    };

    // Fourteen completed long profits retain one claim atom in each odd domain.
    for asset in 0..ASSETS {
        env.try_trade_asset_with_cu(asset, &owner, target, &peer_owner, peer, HISTORY_Q, 100, 0)
            .unwrap_or_else(|error| panic!("historical long open {asset}: {error}"));
    }
    checkpoint_marks(&mut env, HISTORY_SLOT, 150);
    let cert_current = |env: &V16CuEnv, portfolio: Pubkey| {
        let group = env.market_state().1;
        let account = env.portfolio_state(portfolio);
        let cert = health_cert(&account);
        cert.valid
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == active_bitmap(&account)
    };
    for _ in 0..2 * ASSETS + 2 {
        if [target, peer]
            .into_iter()
            .all(|key| cert_current(&env, key))
        {
            break;
        }
        for portfolio in [target, peer] {
            if !cert_current(&env, portfolio) {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: HISTORY_SLOT,
                        observations: vec![],
                    },
                );
            }
        }
    }
    assert!([target, peer]
        .into_iter()
        .all(|key| cert_current(&env, key)));
    for asset in 0..ASSETS {
        env.try_trade_asset_with_cu(asset, &owner, target, &peer_owner, peer, -HISTORY_Q, 150, 0)
            .unwrap_or_else(|error| panic!("historical long exit {asset}: {error}"));
    }
    let historical = env.portfolio_state(target);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &historical
    )));
    assert_eq!(historical.pnl.get(), i128::from(ASSETS));
    assert_eq!(
        env.portfolio_state(peer).capital.get(),
        CAPITAL - u128::from(ASSETS)
    );

    // Each new short earns eight atoms against two funded long atoms. The six-atom
    // Recovery residual produces two B chunks: four atoms, then a two-atom tail.
    checkpoint_marks(&mut env, OPEN_SLOT, 300);
    let mut counterparties = Vec::new();
    for asset in 0..ASSETS {
        let loss_owner = Keypair::new();
        let loss = env.create_portfolio(&loss_owner);
        env.deposit(&loss_owner, loss, 2);
        env.try_trade_asset_with_cu(asset, &owner, target, &loss_owner, loss, -SHORT_Q, 300, 0)
            .unwrap_or_else(|error| panic!("B cohort short open {asset}: {error}"));
        counterparties.push((loss_owner, loss));
    }
    for (slot, price) in [(60, 200), (80, 100)] {
        checkpoint_marks(&mut env, slot, price);
        env.crank(
            target,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: vec![],
            },
        );
    }
    assert_eq!(
        env.portfolio_state(target).pnl.get(),
        9 * i128::from(ASSETS)
    );
    for (asset, (_, loss)) in counterparties.iter().enumerate() {
        env.crank(
            *loss,
            ProgInstruction::PermissionlessCrank {
                now_slot: SETTLE_SLOT - 1,
                observations: crank_observations(asset as u16),
            },
        );
    }
    env.svm.warp_to_slot(SETTLE_SLOT);
    let admin = env.admin.insecure_clone();
    for (asset, (loss_owner, loss)) in counterparties.iter().enumerate() {
        env.try_shutdown_asset_with_authority(&admin, asset as u16, SETTLE_SLOT)
            .expect("public source-local shutdown");
        env.forfeit_recovery_leg_with_cu(loss_owner, *loss, asset as u16, BUDGET);
        assert_eq!(
            close_progress(&env.portfolio_state(*loss)).residual_remaining,
            2
        );
        env.crank(
            *loss,
            ProgInstruction::PermissionlessCrank {
                now_slot: SETTLE_SLOT,
                observations: vec![],
            },
        );
        assert_eq!(
            close_progress(&env.portfolio_state(*loss)).residual_remaining,
            0
        );
    }

    let initial = env.portfolio_state(target);
    let group_before = env.market_state().1;
    assert_eq!(group_before.mode, MarketModeV16::Live);
    assert_eq!(group_before.config.max_portfolio_assets, ASSETS);
    assert_eq!(group_before.config.public_b_chunk_atoms, BUDGET);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&initial)),
        u32::from(ASSETS)
    );
    let source_count = |account: &PortfolioAccountV16| {
        account
            .source_domains
            .iter()
            .filter(|source| source.is_occupied() && source.source_claim_bound_num.get() > 0)
            .count()
    };
    assert_eq!(source_count(&initial), SOURCES);
    assert!(initial
        .source_domains
        .iter()
        .all(|source| source.source_claim_liened_num.get() == 0));
    let targets: Vec<_> = (0..ASSETS)
        .map(|asset| group_before.assets[usize::from(asset)].b_short_num)
        .collect();
    let remaining = |account: &PortfolioAccountV16| -> Vec<u128> {
        (0..ASSETS)
            .map(|asset| {
                let leg = active_leg_for_asset(account, usize::from(asset));
                let numerator = targets[usize::from(asset)]
                    .checked_sub(leg.b_snap)
                    .and_then(|delta| delta.checked_mul(leg.loss_weight))
                    .and_then(|value| value.checked_add(leg.b_rem))
                    .expect("pending short-side B numerator");
                assert_eq!(numerator % percolator::SOCIAL_LOSS_DEN, 0);
                numerator / percolator::SOCIAL_LOSS_DEN
            })
            .collect()
    };
    assert_eq!(remaining(&initial), vec![LOSS_PER_LEG; usize::from(ASSETS)]);
    for asset in 0..ASSETS {
        let leg = active_leg_for_asset(&initial, usize::from(asset));
        assert_eq!(leg.side, SideV16::Short);
        assert_eq!(leg.basis_pos_q, -SHORT_Q);
        assert_eq!(
            group_before.assets[usize::from(asset)].lifecycle,
            AssetLifecycleV16::Recovery
        );
    }
    let framed_keys: Vec<_> = counterparties
        .iter()
        .map(|(_, key)| *key)
        .chain([peer, checkpoint, env.vault, env.mint])
        .collect();
    let frames: Vec<_> = framed_keys
        .iter()
        .map(|key| env.svm.get_account(key).unwrap())
        .collect();
    assert_ne!(env.payer.pubkey(), owner.pubkey());
    let mut max_cu = 0;
    let mut chunk_counts = [0; 2];
    for call in 0..2 * ASSETS {
        let old = env.portfolio_state(target);
        let pending = remaining(&old);
        let rank: u128 = pending.iter().sum();
        assert!(rank > 0, "call {call} must enter with real B work");
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: SETTLE_SLOT,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                ],
                &[],
            )
            .expect("short-side B must have a bounded unsigned-owner continuation");
        assert_cu_within("max-shape short B multi-atom settlement", cu, CU_LIMIT);
        max_cu = max_cu.max(cu);
        let after = env.portfolio_state(target);
        let next_pending = remaining(&after);
        let changed: Vec<_> = (0..usize::from(ASSETS))
            .filter(|asset| pending[*asset] != next_pending[*asset])
            .collect();
        assert_eq!(changed.len(), 1, "exactly one B leg per public call");
        let asset = changed[0];
        let charged = pending[asset].min(BUDGET);
        assert_eq!(next_pending[asset], pending[asset] - charged);
        assert_eq!(next_pending.iter().sum::<u128>(), rank - charged);
        assert_eq!(old.pnl.get() - after.pnl.get(), charged as i128);
        match charged {
            4 => chunk_counts[0] += 1,
            2 => chunk_counts[1] += 1,
            _ => panic!("unexpected B chunk {charged}"),
        }
        for index in 0..usize::from(ASSETS) {
            let old_leg = active_leg_for_asset(&old, index);
            let new_leg = active_leg_for_asset(&after, index);
            let mut expected = old_leg;
            if index == asset {
                let delta = new_leg.b_snap.checked_sub(old_leg.b_snap).unwrap();
                assert!(delta > 0);
                let settled_num = old_leg
                    .loss_weight
                    .checked_mul(delta)
                    .and_then(|value| value.checked_add(old_leg.b_rem))
                    .unwrap();
                assert_eq!(settled_num / percolator::SOCIAL_LOSS_DEN, charged);
                assert_eq!(new_leg.b_rem, settled_num % percolator::SOCIAL_LOSS_DEN);
                if charged == BUDGET {
                    assert!(
                        new_leg.b_rem > 0,
                        "the full chunk must exercise fractional B carry"
                    );
                }
                assert_eq!(new_leg.b_stale, next_pending[index] > 0);
                expected.b_snap = new_leg.b_snap;
                expected.b_rem = new_leg.b_rem;
                expected.b_stale = new_leg.b_stale;
            }
            assert_eq!(new_leg, expected, "frame exposure and K/F on leg {index}");
        }
        let charged_domain = 2 * asset as u32;
        let mut changed_sources = 0;
        for (old_source, new_source) in old.source_domains.iter().zip(&after.source_domains) {
            let mut expected = *old_source;
            if old_source.is_occupied() && old_source.domain.get() == charged_domain {
                changed_sources += 1;
                expected.source_claim_bound_num = percolator::V16PodU128::new(
                    old_source
                        .source_claim_bound_num
                        .get()
                        .checked_sub(charged * BOUND_SCALE)
                        .unwrap(),
                );
            }
            assert_eq!(
                *new_source, expected,
                "only the matching even-domain claim is debited"
            );
        }
        assert_eq!(changed_sources, 1);
        assert_eq!(active_bitmap(&after), active_bitmap(&initial));
        assert_eq!(source_count(&after), SOURCES);
        assert_eq!(after.capital, initial.capital);
        assert_eq!(after.reserved_pnl, initial.reserved_pnl);
        let group_after = env.market_state().1;
        assert_eq!(group_after.mode, group_before.mode);
        assert_eq!(group_after.assets, group_before.assets);
        assert_eq!(group_after.c_tot, group_before.c_tot);
        assert_eq!(group_after.insurance, group_before.insurance);
        assert_eq!(group_after.vault, group_before.vault);
        assert_eq!(group_after.vault, u128::from(env.token_amount(env.vault)));
        assert!(group_after.vault >= group_after.c_tot + group_after.insurance);
        assert_eq!(
            framed_keys
                .iter()
                .map(|key| env.svm.get_account(key).unwrap())
                .collect::<Vec<_>>(),
            frames
        );
    }
    let settled = env.portfolio_state(target);
    assert_eq!(remaining(&settled), vec![0; usize::from(ASSETS)]);
    assert_eq!(chunk_counts, [ASSETS; 2]);
    assert_eq!(settled.pnl.get(), 3 * i128::from(ASSETS));
    assert_eq!(settled.capital.get(), CAPITAL);
    assert_eq!(settled.b_stale_state, 0);
    assert_eq!(env.market_state().1.b_stale_account_count, 0);
    assert_eq!(
        max_cu, 565_957,
        "remeasure the documented default-feature SBF peak on a new pin"
    );
    println!(
        "INV-077 short B: legs={ASSETS}, sources={SOURCES}, budget={BUDGET}, rank={}->0, chunks={chunk_counts:?}, calls={}, peak_cu={max_cu}",
        LOSS_PER_LEG * u128::from(ASSETS), 2 * ASSETS
    );
}
