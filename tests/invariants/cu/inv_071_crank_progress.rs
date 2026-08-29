//! INV-071 - Crank progress.
//!
//! Normative obligation: Every successful crank strictly decreases a finite liveness rank or enters a lower terminal mode.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): expiry matrices, bankruptcy
//! escalation, no-op crank detection, resolved cranks, stale liquidation-budget progress, public
//! same-account close/B priority composition, and current solvent partial-liquidation progress. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

fn inv071_close_pending(ledger: CloseProgressLedgerV16) -> bool {
    ledger.active && !ledger.finalized && !ledger.canceled && ledger.residual_remaining != 0
}

struct Inv071PublicCloseBOverlap {
    env: V16CuEnv,
    target_owner: Keypair,
    target: Pubkey,
    target_b: u128,
    b_before: percolator::PortfolioLegV16,
    close_before: CloseProgressLedgerV16,
    shutdown_slot: u64,
}

struct Inv071PublicBAdverseOverlap {
    env: V16CuEnv,
    target_owner: Keypair,
    target: Pubkey,
    target_b: u128,
    b_before: percolator::PortfolioLegV16,
    adverse_before: percolator::PortfolioLegV16,
    now_slot: u64,
}

fn inv071_public_b_adverse_overlap() -> Inv071PublicBAdverseOverlap {
    let PublicActiveCloseFixture {
        mut env,
        loss,
        asset1_counterparty_owner: target_owner,
        asset1_counterparty: target,
        live_counterparty,
        ..
    } = public_asset1_bankrupt_close_fixture_with_counterparty_asset0_short();

    let target_asset0_before = active_leg_for_asset(&env.portfolio_state(target), 0);
    assert_eq!(target_asset0_before.side, SideV16::Short);

    // Complete the first public close far enough to put two loss atoms into the
    // asset-1 B index. The target's owner then discovers and settles one atom,
    // leaving a publicly reachable B-stale obligation on its asset-1 leg.
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: vec![],
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[],
    )
    .expect("public close continuation must grow the asset-1 B index");
    let target_b = env.market_state().1.assets[1].b_long_num;
    let target_asset1_before = active_leg_for_asset(&env.portfolio_state(target), 1);
    assert!(target_b > target_asset1_before.b_snap);
    env.forfeit_recovery_leg_with_cu(&target_owner, target, 1, 1);
    let b_stale = active_leg_for_asset(&env.portfolio_state(target), 1);
    assert!(b_stale.b_stale && b_stale.b_snap < target_b);

    // Make the already-open asset-0 short adverse using only authenticated mark
    // updates while leaving its account uncranked. This creates a real public
    // B prerequisite in front of the separately actionable live-risk leg.
    for (slot, mark) in [(5u64, 200u64), (6, 400)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(0, slot, mark);
        env.crank(
            live_counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    let shutdown_slot = env
        .svm
        .get_sysvar::<Clock>()
        .slot
        .max(env.market_state().1.current_slot);
    env.svm.warp_to_slot(shutdown_slot);
    for _ in 0..8 {
        let market_data = env.svm.get_account(&env.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        let profile = state::read_asset_oracle_profile(&market_data, 0).unwrap();
        if profile.funding_mark_pending_e6 == 0
            || profile.funding_mark_pending_slot <= group.assets[0].slot_last
        {
            break;
        }
        env.crank(
            live_counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: shutdown_slot,
                observations: crank_observations(0),
            },
        );
    }
    let caught_up_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, caught_up_group) = state::read_market(&caught_up_data).unwrap();
    let caught_up_profile = state::read_asset_oracle_profile(&caught_up_data, 0).unwrap();
    assert!(
        caught_up_profile.funding_mark_pending_e6 == 0
            || caught_up_profile.funding_mark_pending_slot <= caught_up_group.assets[0].slot_last,
        "bounded public accrual must make the authenticated checkpoint replayable"
    );

    let overlap = env.portfolio_state(target);
    let b_before = active_leg_for_asset(&overlap, 1);
    let adverse_before = active_leg_for_asset(&overlap, 0);
    assert!(b_before.b_stale && b_before.b_snap < target_b);
    assert_eq!(adverse_before.side, SideV16::Short);

    Inv071PublicBAdverseOverlap {
        env,
        target_owner,
        target,
        target_b,
        b_before,
        adverse_before,
        now_slot: shutdown_slot,
    }
}

fn inv071_public_close_b_overlap() -> Inv071PublicCloseBOverlap {
    let Inv071PublicBAdverseOverlap {
        mut env,
        target_owner,
        target,
        target_b,
        b_before,
        now_slot: shutdown_slot,
        ..
    } = inv071_public_b_adverse_overlap();

    // Freeze and forfeit the adverse leg to turn the lower-priority live-risk
    // work into a durable close ledger without touching the independent B leg.
    let admin = Keypair::from_bytes(&env.admin.to_bytes()).expect("copy market authority");
    env.try_shutdown_asset_with_authority(&admin, 0, shutdown_slot)
        .expect("market authority shuts down asset 0");
    env.forfeit_recovery_leg_with_cu(&target_owner, target, 0, 1);

    let overlap = env.portfolio_state(target);
    let close_before = close_progress(&overlap);
    assert!(inv071_close_pending(close_before));
    assert_eq!(active_leg_for_asset(&overlap, 1), b_before);

    Inv071PublicCloseBOverlap {
        env,
        target_owner,
        target,
        target_b,
        b_before,
        close_before,
        shutdown_slot,
    }
}

#[test]
fn v16_program_public_b_prerequisite_preserves_later_liquidation_progress() {
    let Inv071PublicBAdverseOverlap {
        mut env,
        target,
        target_b,
        b_before,
        adverse_before,
        now_slot,
        ..
    } = inv071_public_b_adverse_overlap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let group_before = env.market_state().1;
    let cert_before = health_cert(&env.portfolio_state(target));
    assert!(
        !cert_before.valid
            || cert_before.cert_oracle_epoch != group_before.oracle_epoch
            || cert_before.cert_funding_epoch != group_before.funding_epoch
            || cert_before.cert_risk_epoch != group_before.risk_epoch
            || cert_before.cert_asset_set_epoch != group_before.asset_set_epoch
            || cert_before.active_bitmap_at_cert != active_bitmap(&env.portfolio_state(target)),
        "the public B/adverse prefix must expose the certificate prerequisite"
    );

    // The adverse leg cannot make B settlement disappear or redirect it. Even
    // with the asset-0 observation present, the first public call must consume
    // the account-local B prerequisite and frame the separate live-risk leg.
    env.svm.expire_blockhash();
    let settle_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("B prerequisite must be publicly dispatchable before adverse-risk work");
    assert_cu_within(
        "public B prerequisite before liquidation",
        settle_cu,
        CRANK_CU_LIMIT,
    );
    let after_b = env.portfolio_state(target);
    let b_after = active_leg_for_asset(&after_b, 1);
    assert!(b_after.b_snap > b_before.b_snap);
    assert!(b_after.b_snap <= target_b);
    assert_eq!(
        active_leg_for_asset(&after_b, 0),
        adverse_before,
        "B settlement must frame the independent adverse leg"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "B settlement must move no SPL custody"
    );

    // Once B reaches its target, the same observation-bearing public route must
    // recertify and reduce the adverse leg in finitely many bounded calls. This
    // proves the real sequential composition even though this public prefix's B
    // work precedes the health-certificate state needed for liquidation.
    let adverse_abs_before = adverse_before.basis_pos_q.unsigned_abs();
    let mut reduced = false;
    for _ in 0..8 {
        let account_before = env.svm.get_account(&target).unwrap();
        let market_before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let step_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                ],
                &[],
            )
            .expect("post-B adverse account must retain a public progress step");
        assert_cu_within(
            "public post-B liquidation continuation",
            step_cu,
            CRANK_CU_LIMIT,
        );
        assert!(
            env.svm.get_account(&target).unwrap() != account_before
                || env.svm.get_account(&env.market).unwrap() != market_before,
            "every accepted post-B crank must mutate the account or market"
        );
        let after = env.portfolio_state(target);
        reduced = !has_active_leg_for_asset(&after, 0)
            || active_leg_for_asset(&after, 0).basis_pos_q.unsigned_abs() < adverse_abs_before;
        if reduced {
            break;
        }
    }
    assert!(
        reduced,
        "the publicly adverse leg must become liquidatable after its B prerequisite clears"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_program_public_retained_source_lien_does_not_hide_adverse_leg_progress() {
    const PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 105;
    const LOSING_PRICE: u64 = 95;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);
    env.top_up_backing_bucket(1, 150, 100);

    let owner = Keypair::new();
    let peer_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let peer = env.create_portfolio(&peer_owner);
    env.deposit(&owner, portfolio, 313);
    env.deposit(&peer_owner, peer, 5_000);
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &peer_owner,
        peer,
        20 * POS_SCALE as i128,
        PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &peer_owner,
        peer,
        10 * POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, WINNING_PRICE);
    env.push_auth_mark_for_asset_as_admin(1, 2, LOSING_PRICE);
    for account in [peer, portfolio] {
        for _ in 0..4 {
            if env
                .crank_if_actionable(
                    account,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations_for_assets(&[0, 1]),
                    },
                )
                .is_none()
            {
                break;
            }
        }
    }

    // Create one real source-backed risk increase, then flatten the original
    // episodes. The resulting label is retained in an otherwise flat account.
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &peer_owner,
        peer,
        POS_SCALE as i128,
        LOSING_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &peer_owner,
        peer,
        -(11 * POS_SCALE as i128),
        LOSING_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &peer_owner,
        peer,
        -(20 * POS_SCALE as i128),
        WINNING_PRICE,
        0,
    );
    let source_lien_count = |env: &V16CuEnv| {
        env.portfolio_state(portfolio)
            .source_domains
            .iter()
            .filter(|source| source.source_claim_liened_num.get() != 0)
            .count()
    };
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(portfolio)
    )));
    let lien_count_before = source_lien_count(&env);
    assert!(lien_count_before > 0);

    // Reopen a separate episode and move an authenticated mark against it while
    // only the peer is cranked. The target therefore reaches a public state with
    // a retained source label, a stale certificate, and an adverse active leg.
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &peer_owner,
        peer,
        -(20 * POS_SCALE as i128),
        LOSING_PRICE,
        0,
    );
    for slot in 3u64..=5 {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(1, slot, 110);
        env.crank(
            peer,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(1),
            },
        );
    }
    assert_eq!(source_lien_count(&env), lien_count_before);
    let adverse_before = active_leg_for_asset(&env.portfolio_state(portfolio), 1);
    let adverse_abs_before = adverse_before.basis_pos_q.unsigned_abs();

    // A stale account is refreshed before liquidation. That first bounded call
    // may normalize the obsolete source label, but it cannot hide, resize, or
    // redirect the independently adverse leg.
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(1),
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("retained source labels must not block adverse-account refresh");
    assert_cu_within(
        "public source-lien/adverse refresh",
        refresh_cu,
        CRANK_CU_LIMIT,
    );
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before
            || env.svm.get_account(&portfolio).unwrap() != portfolio_before
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(portfolio), 1)
            .basis_pos_q
            .unsigned_abs(),
        adverse_abs_before,
        "refresh must frame the adverse episode's signed quantity"
    );

    let mut reduced = false;
    for _ in 0..8 {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 5,
                    observations: crank_observations(1),
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            )
            .expect("refreshed adverse account must retain a bounded public continuation");
        assert_cu_within("public source-lien/adverse liquidation", cu, 400_000);
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_before
                || env.svm.get_account(&portfolio).unwrap() != portfolio_before
        );
        let state = env.portfolio_state(portfolio);
        reduced = !has_active_leg_for_asset(&state, 1)
            || active_leg_for_asset(&state, 1).basis_pos_q.unsigned_abs() < adverse_abs_before;
        if reduced {
            break;
        }
    }
    assert!(
        reduced,
        "the refreshed adverse leg must become liquidatable"
    );

    // Finish the owner's remaining risk through the ordinary signed reduction.
    // Once flat, any obsolete source label must already be gone or become a
    // bounded observation-free crank continuation.
    let state = env.portfolio_state(portfolio);
    if has_active_leg_for_asset(&state, 1) {
        let remaining = active_leg_for_asset(&state, 1).basis_pos_q.unsigned_abs();
        env.svm.expire_blockhash();
        let reduce_cu = env
            .send(
                ProgInstruction::RebalanceReduce {
                    portfolio_id: env.portfolio_id(portfolio),
                    position_epoch: env.portfolio_position_epoch(portfolio),
                    asset_index: 1,
                    reduce_q: remaining,
                },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[&owner],
            )
            .expect("owner must retain the canonical full reduction after liquidation");
        assert_cu_within(
            "post-liquidation owner reduction",
            reduce_cu,
            CRANK_CU_LIMIT,
        );
    }
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(portfolio)
    )));
    for _ in 0..8 {
        if source_lien_count(&env) < lien_count_before {
            break;
        }
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: vec![],
            },
        );
    }
    assert!(source_lien_count(&env) < lien_count_before);
    let remaining_capital = env.portfolio_state(portfolio).capital.get();
    assert!(remaining_capital > 0);
    env.withdraw_with_cu(&owner, portfolio, remaining_capital);
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_program_public_pending_close_preempts_b_stale_then_exposes_b_progress() {
    let Inv071PublicCloseBOverlap {
        mut env,
        target,
        target_b,
        b_before,
        close_before,
        shutdown_slot,
        ..
    } = inv071_public_close_b_overlap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    // The selector's concrete priority is pending close before B settlement. The
    // selected step must reduce the durable close rank without consuming or
    // rewriting the independent B obligation.
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: shutdown_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("pending-close/B-stale overlap must advance the close first");
    assert_cu_within("public pending-close/B-stale overlap", cu, CRANK_CU_LIMIT);
    let after_close_step = env.portfolio_state(target);
    let close_after = close_progress(&after_close_step);
    let b_after_close = active_leg_for_asset(&after_close_step, 1);
    assert!(
        close_after.residual_remaining < close_before.residual_remaining,
        "the higher-priority close rank must strictly decrease"
    );
    assert_eq!(
        b_after_close, b_before,
        "close progress must frame the B leg"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "close bookkeeping must not move SPL custody"
    );

    // Finish the bounded close, then prove the previously deferred B step is
    // still discoverable. This is the composition property absent from the
    // standalone per-class witnesses.
    let mut previous = close_after.residual_remaining;
    for _ in 0..32 {
        if !inv071_close_pending(close_progress(&env.portfolio_state(target))) {
            break;
        }
        env.svm.expire_blockhash();
        let step_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: shutdown_slot,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                ],
                &[],
            )
            .expect("each pending-close continuation must remain live");
        assert_cu_within("public pending-close continuation", step_cu, CRANK_CU_LIMIT);
        let remaining = close_progress(&env.portfolio_state(target)).residual_remaining;
        assert!(
            remaining < previous,
            "every close continuation must decrease rank"
        );
        assert_eq!(
            active_leg_for_asset(&env.portfolio_state(target), 1),
            b_before
        );
        previous = remaining;
    }
    assert!(
        !inv071_close_pending(close_progress(&env.portfolio_state(target))),
        "the public close rank must terminate within its explicit bound"
    );

    env.svm.expire_blockhash();
    let settle_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: shutdown_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("deferred B settlement must become the next public continuation");
    assert_cu_within("public deferred B settlement", settle_cu, CRANK_CU_LIMIT);
    let state_after_b = env.portfolio_state(target);
    let b_after = active_leg_for_asset(&state_after_b, 1);
    assert!(b_after.b_snap > b_before.b_snap);
    assert_eq!(
        b_after.b_snap, target_b,
        "the B prerequisite must be exhausted before probing the third class"
    );
    assert!(
        !inv071_close_pending(close_progress(&state_after_b)),
        "close work must remain exhausted before probing the third class"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery,
        "the deferred third class must be a real Recovery-lifecycle leg"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );

    // The same public prefix also leaves the asset-1 Recovery leg needing
    // committed-state account refresh after its close and B prerequisites are
    // gone. This third class must become dispatchable without an oracle hint;
    // otherwise the pairwise priority witnesses would miss a real fixed point.
    let account_before_refresh = env.svm.get_account(&target).unwrap();
    let market_before_refresh = env.svm.get_account(&env.market).unwrap();
    let vault_before_refresh = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: shutdown_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("deferred Recovery-leg refresh must follow close and B progress");
    assert_cu_within(
        "public deferred Recovery-leg refresh",
        refresh_cu,
        CRANK_CU_LIMIT,
    );
    assert!(
        env.svm.get_account(&target).unwrap() != account_before_refresh
            || env.svm.get_account(&env.market).unwrap() != market_before_refresh,
        "the accepted third-class continuation must mutate liveness state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_refresh,
        "committed-state Recovery-leg refresh must not move SPL custody"
    );
    let group_after_refresh = env.market_state().1;
    let state_after_refresh = env.portfolio_state(target);
    let cert_after_refresh = health_cert(&state_after_refresh);
    assert!(cert_after_refresh.valid);
    assert_eq!(
        cert_after_refresh.cert_oracle_epoch,
        group_after_refresh.oracle_epoch
    );
    assert_eq!(
        cert_after_refresh.cert_funding_epoch,
        group_after_refresh.funding_epoch
    );
    assert_eq!(
        cert_after_refresh.cert_risk_epoch,
        group_after_refresh.risk_epoch
    );
    assert_eq!(
        cert_after_refresh.cert_asset_set_epoch,
        group_after_refresh.asset_set_epoch
    );
    assert_eq!(
        cert_after_refresh.active_bitmap_at_cert,
        active_bitmap(&state_after_refresh),
        "the third-class step must discharge the deferred certificate rank"
    );
}

#[test]
fn v16_program_public_expired_close_preempts_b_stale_and_preserves_terminal_progress() {
    let Inv071PublicCloseBOverlap {
        mut env,
        target_owner,
        target,
        target_b,
        b_before,
        close_before,
        ..
    } = inv071_public_close_b_overlap();

    let expired_slot = close_before
        .max_close_slot
        .checked_add(1)
        .expect("fixture close expiry fits u64");
    env.svm.warp_to_slot(expired_slot);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let target_before = env.svm.get_account(&target).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let group_before = env.market_state().1;

    // Expiration is a market-terminal condition and must outrank account-local
    // B settlement. The authenticated Clock drives the decision; the caller's
    // now_slot is deliberately stale and no discovery hint is supplied.
    env.svm.expire_blockhash();
    let declare_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("expired-close/B-stale overlap must declare Recovery first");
    assert_cu_within(
        "public expired-close/B-stale recovery declaration",
        declare_cu,
        CRANK_CU_LIMIT,
    );
    let recovered = env.market_state().1;
    assert_eq!(recovered.mode, MarketModeV16::Recovery);
    assert_eq!(
        recovered.recovery_reason,
        Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
    );
    assert_ne!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&target).unwrap(), target_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(recovered.vault, group_before.vault);
    assert_eq!(recovered.c_tot, group_before.c_tot);
    assert_eq!(recovered.insurance, group_before.insurance);
    assert_eq!(close_progress(&env.portfolio_state(target)), close_before);
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(target), 1),
        b_before
    );

    // Recovery itself has a bounded public continuation and must not consume the
    // deferred account-local obligation while changing the market mode.
    let recovery_market = env.svm.get_account(&env.market).unwrap();
    let recovery_target = env.svm.get_account(&target).unwrap();
    env.svm.expire_blockhash();
    let finalize_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("Recovery must retain a permissionless Resolved continuation");
    assert_cu_within(
        "public expired-close/B-stale Recovery-to-Resolved",
        finalize_cu,
        CRANK_CU_LIMIT,
    );
    let resolved = env.market_state().1;
    assert_eq!(resolved.mode, MarketModeV16::Resolved);
    assert_ne!(env.svm.get_account(&env.market).unwrap(), recovery_market);
    assert_eq!(env.svm.get_account(&target).unwrap(), recovery_target);
    assert_eq!(resolved.vault as u64, env.token_amount(env.vault));

    // After the owner exit window, a third party can drive the same account's
    // resolved continuation. The deferred B leg must be processed or removed in
    // bounded successful calls rather than becoming hidden by global Recovery.
    let force_close_delay = env.market_state().0.force_close_delay_slots;
    let resolved_crank_slot = resolved
        .resolved_slot
        .checked_add(force_close_delay)
        .and_then(|slot| slot.checked_add(1))
        .expect("resolved permissionless slot fits u64");
    env.svm.warp_to_slot(resolved_crank_slot);
    let destination = env.token_account(target_owner.pubkey(), 0);
    let mut b_disposed = false;
    for _ in 0..16 {
        let before = env.portfolio_state(target);
        if !has_active_leg_for_asset(&before, 1) {
            b_disposed = true;
            break;
        }
        let current_b = active_leg_for_asset(&before, 1);
        if current_b.b_snap >= target_b {
            b_disposed = true;
            break;
        }
        let market_step_before = env.svm.get_account(&env.market).unwrap();
        let target_step_before = env.svm.get_account(&target).unwrap();
        let vault_step_before = env.svm.get_account(&env.vault).unwrap();
        let destination_step_before = env.svm.get_account(&destination).unwrap();
        env.svm.expire_blockhash();
        let step_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: resolved_crank_slot,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new_readonly(target_owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            )
            .expect("Resolved must keep processing the deferred B-bearing account");
        assert_cu_within(
            "public expired-close/B-stale resolved continuation",
            step_cu,
            CRANK_CU_LIMIT,
        );
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_step_before
                || env.svm.get_account(&target).unwrap() != target_step_before
                || env.svm.get_account(&env.vault).unwrap() != vault_step_before
                || env.svm.get_account(&destination).unwrap() != destination_step_before,
            "a successful resolved continuation must mutate persistent state or custody"
        );
        assert_eq!(
            env.market_state().1.vault as u64,
            env.token_amount(env.vault)
        );
    }
    let terminal_target = env.portfolio_state(target);
    b_disposed |= !has_active_leg_for_asset(&terminal_target, 1)
        || active_leg_for_asset(&terminal_target, 1).b_snap >= target_b;
    assert!(
        b_disposed,
        "global recovery must not hide the pre-existing B obligation from bounded resolved progress"
    );
}

#[test]
fn v16_program_permissionless_crank_closes_capital_only_resolved_account() {
    const DEPOSIT: u128 = 123_456;

    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, DEPOSIT);
    env.resolve();

    let destination = env.token_account(owner.pubkey(), 0);
    let now_slot = env.svm.get_sysvar::<Clock>().slot;
    let market_before = env.market_state().1;
    let spl_vault_before = env.token_amount(env.vault);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), DEPOSIT);

    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("capital-only resolved account must be permissionlessly actionable");
    assert_cu_within("capital-only resolved PermissionlessCrank", cu, 1_375_000);

    let after = env.portfolio_state(portfolio);
    assert_eq!(after.capital.get(), 0);
    assert_eq!(after.pnl.get(), 0);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&after)));
    assert_eq!(env.token_amount(destination), DEPOSIT as u64);
    assert_eq!(
        spl_vault_before - env.token_amount(env.vault),
        DEPOSIT as u64
    );
    assert_eq!(market_before.vault - env.market_state().1.vault, DEPOSIT);

    let market_fixed = env.svm.get_account(&env.market).unwrap();
    let portfolio_fixed = env.svm.get_account(&portfolio).unwrap();
    let vault_fixed = env.svm.get_account(&env.vault).unwrap();
    let destination_fixed = env.svm.get_account(&destination).unwrap();
    env.svm.expire_blockhash();
    let retry = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot,
            observations: vec![],
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    retry.expect_err("a terminal permissionless crank must not land as a successful no-op");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_fixed);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_fixed);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_fixed);
    assert_eq!(
        env.svm.get_account(&destination).unwrap(),
        destination_fixed
    );
}

#[test]
fn v16_program_prospective_loss_expiry_matrix_keeps_resolved_exit_live() {
    const PRICE: u64 = 100;
    const LOW_PRICE: u64 = 98;
    const FINAL_LOW_PRICE: u64 = 96;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        initial_price: PRICE,
        max_price_move_bps_per_slot: 200,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let neutral_owner = Keypair::new();
    let fee_peer_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let neutral = env.create_portfolio(&neutral_owner);
    let fee_peer = env.create_portfolio(&fee_peer_owner);
    for (owner, portfolio) in [
        (&long_owner, long),
        (&short_owner, short),
        (&neutral_owner, neutral),
        (&fee_peer_owner, fee_peer),
    ] {
        env.deposit(owner, portfolio, DEPOSIT);
    }
    env.trade_with_cu(&long_owner, long, &short_owner, short, SIZE_Q, PRICE, 0);
    env.trade_asset_with_cu(
        1,
        &short_owner,
        short,
        &fee_peer_owner,
        fee_peer,
        SIZE_Q,
        PRICE,
        0,
    );
    env.top_up_backing_bucket(0, 1_000_000, 8);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, LOW_PRICE);
    env.push_auth_mark_for_asset_as_admin(1, 2, LOW_PRICE);
    for asset_index in [0, 1] {
        env.crank(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        );
    }
    env.svm.warp_to_slot(3);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert!(env
        .portfolio_state(short)
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        },
    );
    env.trade_with_cu(
        &short_owner,
        short,
        &neutral_owner,
        neutral,
        SIZE_Q,
        LOW_PRICE,
        0,
    );

    env.svm.warp_to_slot(4);
    env.push_auth_mark_for_asset_as_admin(0, 4, FINAL_LOW_PRICE);
    env.crank(
        neutral,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
    );

    env.svm.warp_to_slot(9);
    for _ in 0..8 {
        if env.market_state().1.assets[0].slot_last == 9 {
            break;
        }
        env.crank(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: 9,
                observations: crank_observations(0),
            },
        );
    }
    let long_before = env.portfolio_state(long);
    let market_before = env.market_state().1;
    assert_eq!(long_before.pnl.get(), 0);
    assert!(
        market_before.assets[0].k_long < active_leg_for_asset(&long_before, 0).k_snap,
        "the target must retain a prospective negative K delta"
    );
    assert!(long_before
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    assert_eq!(
        market_before.source_backing_buckets[0].status,
        BackingBucketStatusV16::Expired,
        "the authenticated slot-9 crank must normalize the slot-8 bucket before resolution"
    );
    assert_eq!(market_before.source_backing_buckets[0].expiry_slot, 8);
    env.resolve();

    let destinations = [
        env.token_account(long_owner.pubkey(), 0),
        env.token_account(short_owner.pubkey(), 0),
        env.token_account(neutral_owner.pubkey(), 0),
        env.token_account(fee_peer_owner.pubkey(), 0),
    ];
    let accounts = [
        (&long_owner, long),
        (&short_owner, short),
        (&neutral_owner, neutral),
        (&fee_peer_owner, fee_peer),
    ];
    let mut rejected = 0usize;
    let mut progressed = 0usize;
    for ((owner, portfolio), destination) in accounts.into_iter().zip(destinations).cycle().take(64)
    {
        let state = env.portfolio_state(portfolio);
        let receipt = resolved_receipt(&state);
        if percolator::active_bitmap_is_empty(active_bitmap(&state))
            && state.capital.get() == 0
            && state.pnl.get() == 0
            && (!receipt.present || receipt.finalized)
        {
            continue;
        }
        env.svm.expire_blockhash();
        let before_market = env.svm.get_account(&env.market).unwrap();
        let before_portfolio = env.svm.get_account(&portfolio).unwrap();
        let before_vault = env.svm.get_account(&env.vault).unwrap();
        let before_destination = env.svm.get_account(&destination).unwrap();
        let close = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: vec![CrankObservationHint {
                    asset_index: u16::MAX,
                    oracle_accounts: u8::MAX,
                }],
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        );
        match close {
            Ok(cu) => {
                progressed += 1;
                assert_cu_within("prospective-loss resolved crank", cu, CUSTODY_CU_LIMIT);
                assert!(
                    env.svm.get_account(&env.market).unwrap() != before_market
                        || env.svm.get_account(&portfolio).unwrap() != before_portfolio
                        || env.svm.get_account(&env.vault).unwrap() != before_vault
                        || env.svm.get_account(&destination).unwrap() != before_destination,
                    "accepted preterminal resolved crank was a no-op: portfolio={portfolio}"
                );
            }
            Err(_) => {
                rejected += 1;
                assert_eq!(env.svm.get_account(&env.market).unwrap(), before_market);
                assert_eq!(env.svm.get_account(&portfolio).unwrap(), before_portfolio);
                assert_eq!(env.svm.get_account(&env.vault).unwrap(), before_vault);
                assert_eq!(
                    env.svm.get_account(&destination).unwrap(),
                    before_destination
                );
            }
        }
    }

    let states = [
        env.portfolio_state(long),
        env.portfolio_state(short),
        env.portfolio_state(neutral),
        env.portfolio_state(fee_peer),
    ];
    assert!(
        progressed > accounts.len(),
        "the matrix did not exercise bounded multi-step progress"
    );
    assert_eq!(
        rejected, 0,
        "a funded resolved account lost its crank continuation"
    );
    for portfolio in &states {
        let receipt = resolved_receipt(portfolio);
        assert!(percolator::active_bitmap_is_empty(active_bitmap(portfolio)));
        assert_eq!(portfolio.capital.get(), 0);
        assert_eq!(portfolio.pnl.get(), 0);
        assert!(!receipt.present || receipt.finalized);
    }
    assert_eq!(
        destinations
            .into_iter()
            .map(|destination| u128::from(env.token_amount(destination)))
            .sum::<u128>(),
        DEPOSIT * accounts.len() as u128,
        "resolved progress did not conserve funded user principal"
    );
}

#[test]
fn v16_program_prospective_source_expiry_prerequisite_matrix_keeps_exit_live() {
    const PRICE: u64 = 100;
    const LOW_PRICE: u64 = 98;
    const REBOUND_PRICE: u64 = 99;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: PRICE,
        max_price_move_bps_per_slot: 200,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let neutral_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let neutral = env.create_portfolio(&neutral_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    env.trade_with_cu(&long_owner, long, &short_owner, short, SIZE_Q, PRICE, 0);
    env.top_up_backing_bucket(0, 93, 8);
    env.top_up_backing_bucket(1, 32, 8);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, LOW_PRICE);
    env.crank(
        neutral,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    assert_eq!(env.portfolio_state(long).pnl.get(), 0);
    assert!(env.portfolio_state(long).capital.get() < DEPOSIT);

    env.svm.warp_to_slot(4);
    env.push_auth_mark_for_asset_as_admin(0, 4, REBOUND_PRICE);
    env.crank(
        neutral,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
    );
    let long_before = env.portfolio_state(long);
    let short_before = env.portfolio_state(short);
    assert_eq!(long_before.pnl.get(), 0);
    assert_eq!(short_before.pnl.get(), 0);
    assert!(long_before
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));
    assert!(short_before
        .source_domains
        .iter()
        .all(|source| !source.is_occupied()));

    env.svm.warp_to_slot(9);
    for _ in 0..8 {
        if env.market_state().1.assets[0].slot_last == 9 {
            break;
        }
        env.crank(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: 9,
                observations: crank_observations(0),
            },
        );
    }
    let before_resolve = env.market_state().1;
    assert_eq!(before_resolve.assets[0].slot_last, 9);
    assert_eq!(
        before_resolve.source_backing_buckets[0].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(before_resolve.source_backing_buckets[0].expiry_slot, 8);
    assert_eq!(before_resolve.pnl_matured_pos_tot, 0);
    env.resolve();

    let long_destination = env.token_account(long_owner.pubkey(), 0);
    let short_destination = env.token_account(short_owner.pubkey(), 0);
    let mut rejected = 0usize;
    for (owner, portfolio, destination) in [
        (&long_owner, long, long_destination),
        (&short_owner, short, short_destination),
    ]
    .into_iter()
    .cycle()
    .take(32)
    {
        let state = env.portfolio_state(portfolio);
        let receipt = resolved_receipt(&state);
        if percolator::active_bitmap_is_empty(active_bitmap(&state))
            && state.capital.get() == 0
            && state.pnl.get() == 0
            && (!receipt.present || receipt.finalized)
        {
            continue;
        }
        env.svm.expire_blockhash();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let destination_before = env.svm.get_account(&destination).unwrap();
        let close = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: vec![CrankObservationHint {
                    asset_index: u16::MAX,
                    oracle_accounts: u8::MAX,
                }],
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        );
        match close {
            Ok(cu) => {
                assert_cu_within("prospective source-expiry close", cu, CUSTODY_CU_LIMIT);
                assert!(
                    env.svm.get_account(&env.market).unwrap() != market_before
                        || env.svm.get_account(&portfolio).unwrap() != portfolio_before
                        || env.svm.get_account(&env.vault).unwrap() != vault_before
                        || env.svm.get_account(&destination).unwrap() != destination_before,
                    "accepted preterminal resolved close was a no-op: portfolio={portfolio}, active={}, capital={}, pnl={}",
                    percolator::active_bitmap_count_ones(active_bitmap(&state)),
                    state.capital.get(),
                    state.pnl.get()
                );
            }
            Err(_) => {
                rejected += 1;
                assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
                assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                assert_eq!(
                    env.svm.get_account(&destination).unwrap(),
                    destination_before
                );
            }
        }
    }

    let long_after = env.portfolio_state(long);
    let short_after = env.portfolio_state(short);
    let long_locked = has_active_leg_for_asset(&long_after, 0) || long_after.capital.get() != 0;
    let short_locked = has_active_leg_for_asset(&short_after, 0) || short_after.capital.get() != 0;
    assert_eq!(rejected, 0, "the pinned predecessor unexpectedly locked");
    assert!(!long_locked && !short_locked);
    assert_eq!(env.token_amount(long_destination), 99_900_000);
    assert_eq!(env.token_amount(short_destination), 100_100_000);
}

#[test]
fn v16_program_b_budget_lock_prerequisite_rejects_post_adl_basis_reissue() {
    const SCALE: u64 = 100_000_000;
    const INITIAL_PRICE: u64 = 10_000 * SCALE;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        min_nonzero_mm_req: 1,
        min_nonzero_im_req: 2,
        maintenance_margin_bps: 250,
        initial_margin_bps: 500,
        max_trading_fee_bps: 10,
        liquidation_fee_bps: 0,
        liquidation_fee_cap: 0,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, INITIAL_PRICE);
    let owners = [Keypair::new(), Keypair::new(), Keypair::new()];
    let accounts = [
        env.create_portfolio(&owners[0]),
        env.create_portfolio(&owners[1]),
        env.create_portfolio(&owners[2]),
    ];
    for index in 0..accounts.len() {
        env.deposit(&owners[index], accounts[index], 20_000 * u128::from(SCALE));
    }
    let crank_at = |env: &mut V16CuEnv, portfolio: Pubkey, slot: u64| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("public crank in B-budget setup")
    };

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, 9_976 * SCALE);
    crank_at(&mut env, accounts[2], 1);
    env.trade_asset_with_cu(
        0,
        &owners[0],
        accounts[0],
        &owners[1],
        accounts[1],
        -(29 * POS_SCALE as i128),
        9_976 * SCALE,
        3,
    );
    env.rebalance_reduce_with_cu(&owners[0], accounts[0], 0, 25 * POS_SCALE);
    let adl = env.market_state().1;
    assert!(adl.assets[0].a_long < percolator::ADL_ONE);
    assert_eq!(adl.assets[0].oi_eff_long_q, 4 * POS_SCALE);
    assert_eq!(adl.assets[0].oi_eff_short_q, 4 * POS_SCALE);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let account0_before = env.svm.get_account(&accounts[0]).unwrap();
    let account1_before = env.svm.get_account(&accounts[1]).unwrap();
    let account2_before = env.svm.get_account(&accounts[2]).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let prerequisite = env.try_trade_asset_with_cu(
        0,
        &owners[2],
        accounts[2],
        &owners[0],
        accounts[0],
        32 * POS_SCALE as i128,
        9_976 * SCALE,
        0,
    );
    let error = prerequisite
        .expect_err("the former resolved B-budget lock prefix must stop at post-ADL basis reissue");
    assert!(
        error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
        "B-budget prerequisite reached the wrong gate: {error}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&accounts[0]).unwrap(), account0_before);
    assert_eq!(env.svm.get_account(&accounts[1]).unwrap(), account1_before);
    assert_eq!(env.svm.get_account(&accounts[2]).unwrap(), account2_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let raw_before = active_leg_for_asset(&env.portfolio_state(accounts[0]), 0)
        .basis_pos_q
        .unsigned_abs();
    let exit_cu = env.rebalance_reduce_with_cu(&owners[0], accounts[0], 0, POS_SCALE);
    assert_cu_within("B-budget prefix owner exit", exit_cu, CUSTODY_CU_LIMIT);
    let after_exit = env.market_state().1;
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(accounts[0]), 0)
            .basis_pos_q
            .unsigned_abs(),
        raw_before - POS_SCALE
    );
    assert_eq!(after_exit.assets[0].oi_eff_long_q, 3 * POS_SCALE);
    assert_eq!(after_exit.assets[0].oi_eff_short_q, 3 * POS_SCALE);
}

#[test]
fn v16_program_bankruptcy_escalation_matrix_commits_recovery_and_resolves() {
    const OPEN_PRICE: u64 = 1_000_000;
    const ADVERSE_PRICE: u64 = 1_070_000;

    for short_capital in [55_000u128, 56_000] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            public_b_chunk_atoms: 1,
            max_bankrupt_close_lifetime_slots: 1,
            ..production_risk_params()
        });
        env.update_liquidation_fee_policy_with_cu(0);
        env.configure_auth_mark_with_cu(0, OPEN_PRICE);

        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, 100_000_000);
        env.deposit(&short_owner, short, short_capital);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );

        let mut recovery_transition = None;
        for slot in 1..=40u64 {
            env.svm.warp_to_slot(slot);
            let _ = env.push_auth_mark_with_cu(slot, ADVERSE_PRICE);
            let market_before = env.svm.get_account(&env.market).unwrap();
            let (_, group_before) = env.market_state();
            let short_before = env.svm.get_account(&short).unwrap();
            let cert_before = health_cert(&env.portfolio_state(short));
            env.svm.expire_blockhash();
            let cu = env
                .send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(short, false),
                    ],
                    &[],
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "bankruptcy progress crank failed at slot {slot}, cert={cert_before:?}: {error}"
                    )
                });
            if env.market_state().1.mode == MarketModeV16::Recovery {
                recovery_transition =
                    Some((cu, market_before, group_before, short_before, cert_before));
                break;
            }
        }

        let (recovery_cu, market_before, group_before, short_before, cert_before) =
            recovery_transition.expect("bankruptcy must reach Recovery in bounded public cranks");
        assert!(
            cert_before.valid && cert_before.certified_liq_deficit != 0,
            "the recovery transition must start from a current liquidatable account"
        );
        assert_cu_within(
            "bankruptcy escalation recovery declaration",
            recovery_cu,
            CRANK_CU_LIMIT,
        );
        let (_, recovered) = env.market_state();
        assert_eq!(recovered.mode, MarketModeV16::Recovery);
        assert_eq!(
            recovered.recovery_reason,
            Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
        );
        assert_ne!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
        assert_eq!(recovered.vault, group_before.vault);
        assert_eq!(recovered.c_tot, group_before.c_tot);
        assert_eq!(recovered.insurance, group_before.insurance);
        assert_eq!(recovered.vault as u64, env.token_amount(env.vault));

        let recovery_market = env.svm.get_account(&env.market).unwrap();
        let recovery_short = env.svm.get_account(&short).unwrap();
        env.svm.expire_blockhash();
        let finalize_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: u64::MAX,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                ],
                &[],
            )
            .expect("Recovery must have a bounded permissionless Resolved continuation");
        assert_cu_within(
            "bankruptcy escalation Recovery-to-Resolved",
            finalize_cu,
            CRANK_CU_LIMIT,
        );
        let (_, resolved) = env.market_state();
        assert_eq!(resolved.mode, MarketModeV16::Resolved);
        assert_ne!(env.svm.get_account(&env.market).unwrap(), recovery_market);
        assert_eq!(env.svm.get_account(&short).unwrap(), recovery_short);
        assert_eq!(resolved.vault, recovered.vault);
        assert_eq!(resolved.c_tot, recovered.c_tot);
        assert_eq!(resolved.insurance, recovered.insurance);
        assert_eq!(resolved.vault as u64, env.token_amount(env.vault));
    }
}

#[derive(Debug)]
struct MicroPriceScheduleOutcome {
    effective_price: u64,
    raw_target: u64,
    asset_slot_last: u64,
    successful_calls: usize,
    zero_delta_clock_advances: usize,
    vault_tokens: u64,
}

fn run_micro_price_schedule(eager: bool) -> MicroPriceScheduleOutcome {
    const PRICE: u64 = 100;
    const TARGET: u64 = 200;
    const FINAL_SLOT: u64 = 5;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: PRICE,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, PRICE);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 10_000);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    let vault_tokens = env.token_amount(env.vault);

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, TARGET);
    let schedule: Vec<u64> = if eager {
        (1..=FINAL_SLOT).collect()
    } else {
        vec![FINAL_SLOT]
    };
    let mut successful_calls = 0usize;
    let mut zero_delta_clock_advances = 0usize;
    for slot in schedule {
        env.svm.warp_to_slot(slot);
        let (_, before) = env.market_state();
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                ],
                &[],
            )
            .expect("public price crank");
        assert!(cu < 1_400_000);
        successful_calls += 1;
        let (_, after) = env.market_state();
        if after.assets[0].effective_price == before.assets[0].effective_price
            && after.assets[0].slot_last > before.assets[0].slot_last
        {
            zero_delta_clock_advances += 1;
        }
        assert_eq!(env.token_amount(env.vault), vault_tokens);
    }

    let (_, group) = env.market_state();
    MicroPriceScheduleOutcome {
        effective_price: group.assets[0].effective_price,
        raw_target: group.assets[0].raw_oracle_target_price,
        asset_slot_last: group.assets[0].slot_last,
        successful_calls,
        zero_delta_clock_advances,
        vault_tokens: env.token_amount(env.vault),
    }
}

#[test]
fn v16_program_micro_price_schedule_is_partition_invariant_and_eventually_progresses() {
    let delayed = run_micro_price_schedule(false);
    let eager = run_micro_price_schedule(true);
    assert_eq!(delayed.raw_target, 200);
    assert_eq!(eager.raw_target, delayed.raw_target);
    assert!(
        delayed.effective_price > 100,
        "five elapsed slots must make one price atom representable: {delayed:?}"
    );
    assert_eq!(
        eager.effective_price, delayed.effective_price,
        "carried sub-atom movement must make eager and delayed cranks equivalent: eager={eager:?}, delayed={delayed:?}"
    );
    assert_eq!(eager.asset_slot_last, 5);
    assert_eq!(delayed.asset_slot_last, eager.asset_slot_last);
    assert_eq!(eager.successful_calls, 5);
    assert_eq!(delayed.successful_calls, 1);
    assert_eq!(
        eager.zero_delta_clock_advances, 4,
        "four sub-atom steps should carry into the fifth visible price move: {eager:?}"
    );
    assert_eq!(delayed.zero_delta_clock_advances, 0);
    assert_eq!(eager.vault_tokens, delayed.vault_tokens);
}

#[test]
fn v16_attack_resolved_permissionless_crank_survives_drained_owner_system_account() {
    let mut env = V16CuEnv::new();
    const EXIT_DELAY: u64 = 5;
    env.configure_permissionless_resolve_with_cu(100, EXIT_DELAY);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    let dest = env.token_account(owner.pubkey(), 0);
    env.resolve();
    env.svm.warp_to_slot(EXIT_DELAY + 1);

    let owner_lamports = env.svm.get_account(&owner.pubkey()).unwrap().lamports;
    env.svm.expire_blockhash();
    send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![system_instruction::transfer(
            &owner.pubkey(),
            &env.payer.pubkey(),
            owner_lamports,
        )],
        &[&owner],
    )
    .expect("owner can publicly drain its system-account lamports");
    assert_eq!(
        env.svm
            .get_account(&owner.pubkey())
            .map(|account| account.lamports)
            .unwrap_or(0),
        0,
        "probe starts after the owner system account is no longer funded"
    );

    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: u64::MAX,
                observations: vec![CrankObservationHint {
                    asset_index: u16::MAX,
                    oracle_accounts: u8::MAX,
                }],
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("post-timeout resolved PermissionlessCrank should not depend on owner lamports");
    assert_cu_within(
        "post-timeout resolved PermissionlessCrank drained owner account",
        cu,
        CRANK_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "resolved public crank still pays the portfolio owner's token account"
    );
    assert_eq!(env.token_amount(env.vault), 0);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
}

#[test]
fn v16_attack_stale_liquidation_budget_observation_crank_progresses_without_reward_or_value() {
    const MARK: u64 = 1_000_000;
    const OPEN_SLOT: u64 = 1;
    const OBS_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);

    set_test_clock(&mut env, OPEN_SLOT, 100);
    let feed0 = [0x46u8; 32];
    let initial0 = env.set_pyth_price_with_conf(&feed0, MARK as i64, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed0, [0u8; 32], [0u8; 32]],
        &[initial0],
        OPEN_SLOT,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure asset-0 hybrid oracle");

    let target_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let target = env.create_portfolio(&target_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&cranker_owner, cranker, 1_000);

    set_test_clock(&mut env, OBS_SLOT, 101);
    let fresh0 = env.set_pyth_price_with_conf(&feed0, (MARK + 10_000) as i64, -6, 0, 101);
    let target_before = env.portfolio_state(target);
    let cranker_before = env.svm.get_account(&cranker).unwrap();

    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: OBS_SLOT,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(target, false),
            AccountMeta::new_readonly(fresh0, false),
            AccountMeta::new(cranker, false),
        ],
        &[&cranker_owner],
    );
    assert!(
        accepted.is_ok(),
        "stale liquidation budget must not roll back otherwise valid observation-only progress: {accepted:?}"
    );
    assert_cu_within(
        "stale close_q observation-only crank",
        accepted.unwrap(),
        CRANK_CU_LIMIT,
    );

    let (_, after_group) = env.market_state();
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        MARK + 10_000,
        "observation-only crank commits the supplied oracle update"
    );
    assert_eq!(
        env.portfolio_state(target).capital.get(),
        target_before.capital.get(),
        "stale-budget observation crank must not credit or debit target capital"
    );
    assert_eq!(
        env.portfolio_state(target).pnl.get(),
        target_before.pnl.get(),
        "stale-budget observation crank must not move target PnL"
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(target))),
        "stale-budget observation crank must not create target exposure"
    );
    assert_eq!(
        env.svm.get_account(&cranker).unwrap(),
        cranker_before,
        "observation-only stale-budget crank pays no liquidation reward"
    );

    let market_fixed = env.svm.get_account(&env.market).unwrap();
    let target_fixed = env.svm.get_account(&target).unwrap();
    let cranker_fixed = env.svm.get_account(&cranker).unwrap();
    env.svm.expire_blockhash();
    let duplicate = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: OBS_SLOT,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(target, false),
            AccountMeta::new_readonly(fresh0, false),
            AccountMeta::new(cranker, false),
        ],
        &[&cranker_owner],
    );
    duplicate.expect_err(
        "an identical same-slot observation at the market/account fixed point must not succeed",
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_fixed);
    assert_eq!(env.svm.get_account(&target).unwrap(), target_fixed);
    assert_eq!(env.svm.get_account(&cranker).unwrap(), cranker_fixed);
}

#[test]
fn v16_attack_auto_crank_prioritizes_b_stale_over_liquidation_reward_tail() {
    const OPEN_MARK: u64 = 100;
    const LIQ_MARK: u64 = 300;
    const OPEN_SLOT: u64 = 1;
    const LIQ_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        public_b_chunk_atoms: 1,
        ..V16CuMarketParams::default()
    });
    env.top_up_insurance(1_000_000);
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.svm.warp_to_slot(OPEN_SLOT);
    env.configure_auth_mark_with_cu(OPEN_SLOT, OPEN_MARK);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 3_000);
    env.deposit(&cranker_owner, cranker, 1_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        OPEN_MARK,
        0,
    );

    env.svm.warp_to_slot(LIQ_SLOT);
    env.push_auth_mark_with_cu(LIQ_SLOT, LIQ_MARK);
    for slot in [LIQ_SLOT, LIQ_SLOT + 1] {
        env.svm.warp_to_slot(slot);
        env.crank(
            short_account,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    let liquidatable_before = env.portfolio_state(short_account);
    let cert_before = health_cert(&liquidatable_before);
    assert!(
        cert_before.valid
            && cert_before.certified_liq_deficit != 0
            && cert_before.certified_equity > 0,
        "setup must produce a current solvent liquidatable short before adding B-stale overlap: {cert_before:?}"
    );

    env.mark_b_stale_gap(short_account, 0, 3);
    let overlapped_before = env.portfolio_state(short_account);
    let leg_before = active_leg_for_asset(&overlapped_before, 0);
    assert_eq!(leg_before.side, SideV16::Short);
    assert!(
        leg_before.b_stale && overlapped_before.b_stale_state != 0,
        "setup must add a real B-stale rank on top of the liquidatable account"
    );
    assert!(
        health_cert(&overlapped_before).certified_liq_deficit != 0,
        "B-stale setup must preserve the liquidatable overlap"
    );

    let (_, group_before) = env.market_state();
    let cranker_before = env.svm.get_account(&cranker).unwrap();
    let cranker_capital_before = env.portfolio_state(cranker).capital.get();
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: LIQ_SLOT + 1,
                observations: vec![],
            },
            vec![
                AccountMeta::new(cranker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_account, false),
                AccountMeta::new(cranker, false),
            ],
            &[&cranker_owner],
        )
        .expect("B-stale/liquidatable overlap must make B-settlement progress");
    assert_cu_within(
        "PermissionlessCrank B-stale/liquidatable overlap",
        cu,
        CRANK_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    let after = env.portfolio_state(short_account);
    let leg_after = active_leg_for_asset(&after, 0);
    let selected_target = group_before.assets[0].b_short_num;
    assert_eq!(
        leg_after.b_snap, selected_target,
        "the higher-priority B-settlement step consumes the bounded loss-atom gap"
    );
    assert!(!leg_after.b_stale && after.b_stale_state == 0);
    assert_eq!(
        leg_after.basis_pos_q, leg_before.basis_pos_q,
        "hostile close_q must not liquidate while B settlement has priority"
    );
    assert_eq!(
        group_after.insurance, group_before.insurance,
        "B-settlement overlap path pays no liquidation fee"
    );
    assert_eq!(
        env.svm.get_account(&cranker).unwrap(),
        cranker_before,
        "non-liquidation overlap path must not rewrite the reward tail account"
    );
    assert_eq!(
        env.portfolio_state(cranker).capital.get(),
        cranker_capital_before,
        "non-liquidation overlap path pays no cranker reward"
    );
    assert_eq!(group_after.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_attack_auto_crank_reaches_later_material_liquidation_past_tiny_first_leg() {
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_040_000;
    const TINY_Q: i128 = 1;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, MARK);
    env.configure_auth_mark_for_asset_as_admin(1, 1, MARK);

    let victim_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&victim_owner, victim, 60_000);
    env.deposit(&counterparty_owner, counterparty, 2_000_000);

    // Asset 0 deliberately occupies the first active slot with the minimum representable public
    // position quantum. It must still be removable rather than shadowing the material asset-1 loss.
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &counterparty_owner,
        counterparty,
        -TINY_Q,
        MARK,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &victim_owner,
        victim,
        &counterparty_owner,
        counterparty,
        -(POS_SCALE as i128),
        MARK,
        0,
    );
    assert_eq!(leg(&env.portfolio_state(victim), 0).asset_index, 0);

    // Reach the adverse price through the production 24-bps/slot circuit breaker while leaving the
    // victim untouched and stale. The counterparty is only the public accrual vehicle.
    for slot in 2..=20u64 {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(1, slot, ADVERSE_MARK);
        env.crank(
            counterparty,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(1),
            },
        );
    }
    env.crank(
        victim,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: vec![],
        },
    );

    let before = env.portfolio_state(victim);
    assert!(health_cert(&before).certified_liq_deficit > 0);
    assert!(has_active_leg_for_asset(&before, 0));
    let material_before = active_leg_for_asset(&before, 1).basis_pos_q.unsigned_abs();

    // Every successful call is engine-selected. The tiny first leg may be removed first, but it
    // must not permanently shadow the later leg that carries the material deficit.
    let mut material_after = material_before;
    for _ in 0..6 {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: vec![],
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(victim, false),
            ],
            &[],
        )
        .expect("one honest auto-crank step");
        let state = env.portfolio_state(victim);
        material_after = if has_active_leg_for_asset(&state, 1) {
            active_leg_for_asset(&state, 1).basis_pos_q.unsigned_abs()
        } else {
            0
        };
        if material_after < material_before {
            break;
        }
    }
    assert!(
        material_after < material_before,
        "tiny first leg must not shadow liquidation of the later losing leg"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(victim), 0),
        "the minimum-quantum first leg must clear before the later material liquidation progresses"
    );
}

// the keeper has no liquidation-size input.
#[test]
fn v16_program_auto_crank_current_solvent_partial_liquidation_makes_progress() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 3_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    let position_epoch_after_trade = env.portfolio_position_epoch(short_account);

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );

    let before_group = env.market_state().1;
    let before_short = env.portfolio_state(short_account);
    let before_cert = health_cert(&before_short);
    assert_eq!(
        env.portfolio_position_epoch(short_account),
        position_epoch_after_trade,
        "refresh-only cranks must not invalidate signed position consent"
    );
    assert!(
        before_cert.certified_liq_deficit != 0 && before_cert.certified_equity > 0,
        "setup must be solvent but liquidatable before partial liquidation: {before_cert:?}"
    );
    let oi_pre = before_group.assets[0].oi_eff_short_q;

    env.svm.expire_blockhash();
    let partial = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    assert!(
        partial.is_ok(),
        "current solvent liquidation must not require observations to make progress: {partial:?}"
    );

    let after_group = env.market_state().1;
    let after_short = env.portfolio_state(short_account);
    let closed = oi_pre.saturating_sub(after_group.assets[0].oi_eff_short_q);
    assert!(closed > 0, "partial liquidation must reduce open interest");
    assert_eq!(
        env.portfolio_position_epoch(short_account),
        position_epoch_after_trade + 1,
        "a successful liquidation must advance the position episode exactly once"
    );
    assert!(
        closed < oi_pre,
        "solvent liquidation should preserve the engine-selected remaining position: closed={closed}"
    );
    assert!(
        has_active_leg_for_asset(&after_short, 0),
        "partial close should leave the remaining position active"
    );
    assert_eq!(
        health_cert(&after_short).certified_liq_deficit,
        0,
        "engine-selected partial close restores maintenance health"
    );
    assert_eq!(
        after_group.vault, before_group.vault,
        "liquidation fee is internal accounting, not a vault mint"
    );
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}

// Same-slot retries must not double-realize loss or funding. The sole public crank must make real
// progress until settlement is complete, then reject at the fixed point with exact rollback.
#[test]
fn v16_regression_crank_idempotent_at_settlement_fixed_point() {
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
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    let crank_ix = |slot: u64| ProgInstruction::PermissionlessCrank {
        now_slot: slot,
        observations: crank_observations(0),
    };
    // Settle every reachable cohort. At least one call must mutate; later calls may report the
    // explicit fixed point, but may never land as successful no-ops.
    env.svm.warp_to_slot(11);
    let mut progress_calls = 0usize;
    for _ in 0..8 {
        for p in [sh, lo] {
            progress_calls += usize::from(env.crank_if_actionable(p, crank_ix(11)).is_some());
        }
    }
    assert_ne!(
        progress_calls, 0,
        "settlement fixture must make real progress"
    );

    let fixed_market = env.svm.get_account(&env.market).unwrap();
    let fixed_lo = env.svm.get_account(&lo).unwrap();
    let fixed_sh = env.svm.get_account(&sh).unwrap();
    let lo1 = state::read_portfolio(&fixed_lo.data).unwrap();
    let sh1 = state::read_portfolio(&fixed_sh.data).unwrap();
    let (_, g1) = env.market_state();
    let ep1 = g1.assets[0].effective_price;
    for _ in 0..3 {
        for p in [sh, lo] {
            env.svm.expire_blockhash();
            let error = env
                .send(
                    crank_ix(11),
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                )
                .expect_err("same-slot settlement fixed point must reject");
            assert!(
                is_engine_non_progress_error(&error),
                "fixed point returned the wrong error: {error}"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
            assert_eq!(env.svm.get_account(&lo).unwrap(), fixed_lo);
            assert_eq!(env.svm.get_account(&sh).unwrap(), fixed_sh);
        }
    }
    let lo2 = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let sh2 = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g2) = env.market_state();

    assert_eq!(
        g2.assets[0].effective_price, ep1,
        "effective price unchanged by same-slot fixed-point retry"
    );
    assert_eq!(
        (lo2.capital.get(), lo2.pnl.get()),
        (lo1.capital.get(), lo1.pnl.get()),
        "long pnl/capital not double-accrued"
    );
    assert_eq!(
        (sh2.capital.get(), sh2.pnl.get()),
        (sh1.capital.get(), sh1.pnl.get()),
        "short pnl/capital not double-accrued"
    );
    assert_eq!(
        g2.assets[0].f_long_num, g1.assets[0].f_long_num,
        "funding ledger not double-applied"
    );
    assert_eq!(g2.vault, 2_000_000, "vault conserved");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}

#[test]
fn v16_program_ewma_crank_commits_once_then_rejects_same_slot_fixed_point() {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, 100, 1, 0);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);

    env.svm.warp_to_slot(2);
    env.push_ewma_mark_with_cu(2, 200);
    let before_progress = env.svm.get_account(&env.market).unwrap();
    let progress_cu = env
        .crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        )
        .expect("the first EWMA crank must commit authenticated market progress");
    assert_cu_within("EWMA market-progress crank", progress_cu, CRANK_CU_LIMIT);
    assert_ne!(env.svm.get_account(&env.market).unwrap(), before_progress);

    let fixed_market = env.svm.get_account(&env.market).unwrap();
    let fixed_portfolio = env.svm.get_account(&portfolio).unwrap();
    assert!(
        env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        )
        .is_none(),
        "the identical same-slot EWMA retry must report the fixed point"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), fixed_portfolio);
}
