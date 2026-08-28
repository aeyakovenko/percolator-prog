//! INV-028 - Source-domain realizability cap (maximum-shape public routes).
//!
//! Normative obligation: admission must reserve enough sparse source capacity for every active
//! leg's later favorable settlement. A publicly admitted leg must never become impossible to
//! settle or close merely because historical source claims occupy the bounded source table.
//!
//! Evidence in this file (I/C/M):
//! `v16_program_source_capacity_admission_order_matrix_rejects_unreserved_risk` fills every
//! wrapper-supported source slot through ordinary trades in forward and reverse asset order, proves
//! already-reserved domains remain tradable, and then attempts to admit a new asset whose long/short
//! source domains are not reserved. The public route must reject before admitting a funded leg and
//! roll back market, portfolios, and SPL custody exactly.
//! `v16_program_expired_source_lien_route_matrix_preserves_bounded_owner_exit` creates a source
//! lien by ordinary risk increase and crosses its authenticated expiry at both equal and late
//! boundaries. One permissionless crank normalizes the global bucket without moving custody; one
//! full owner reduction clears the exposure and preserves withdrawal of all remaining senior
//! capital. The still-funded 5,000-atom claim must reject live conversion with exact rollback,
//! accept one mutating refresh, and then terminate through configured permissionless resolution:
//! exact 5,000/995,000 payouts, zero impaired lien state, and both portfolios dematerialized. This
//! prevents a principal-only exit assertion from hiding a retained-claim lock.
//! `v16_program_shared_expiry_progress_matrix_preserves_terminal_progress` constructs one public
//! four-portfolio world containing a live source lien and a prospective adverse K/F delta. A
//! lien-free winner expires their shared bucket, after which the prospective loser is tested
//! through `CloseResolved` and the sole public crank while every other portfolio is allowed to
//! progress. Every accepted call must mutate a terminal rank, every rejected call must roll back
//! exactly, and all four funded portfolios must reach terminal disposition.
//!
//! Secondary coverage: INV-030 credit-rate fail-closed behavior must still provide a terminal
//! continuation after shared backing becomes impaired; INV-032 requires the exact account-local
//! provider label to retire without consuming a sibling label or an insurance-backed reserve; and
//! INV-073 requires every funded portfolio in the public lifecycle to reach terminal disposition.
//!
//! Guarantee boundary: the wrapper-supported sparse source-domain shape is 2 *
//! WRAPPER_MAX_PORTFOLIO_ASSETS. Risk-reducing exits remain available at that shape; additional
//! risk on an unreserved asset is not an advertised public liveness shape.

use super::*;

#[test]
fn v16_program_shared_expiry_progress_matrix_preserves_terminal_progress() {
    const Q: i128 = 1_000 * POS_SCALE as i128;
    const PRICE: u64 = 100;
    const UP_PRICE: u64 = 105;
    const FINAL_PRICE: u64 = 110;
    const RESOLVE_SLOT: u64 = 4;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 5_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.top_up_backing_bucket(1, 100_000, RESOLVE_SLOT);

    let target_owner = Keypair::new();
    let target_peer_owner = Keypair::new();
    let trigger_owner = Keypair::new();
    let trigger_peer_owner = Keypair::new();
    let target = env.create_portfolio(&target_owner);
    let target_peer = env.create_portfolio(&target_peer_owner);
    let trigger = env.create_portfolio(&trigger_owner);
    let trigger_peer = env.create_portfolio(&trigger_peer_owner);
    for (owner, portfolio, capital) in [
        (&target_owner, target, 52_501),
        (&target_peer_owner, target_peer, 1_000_000),
        (&trigger_owner, trigger, 1_000_000),
        (&trigger_peer_owner, trigger_peer, 1_000_000),
    ] {
        env.deposit(owner, portfolio, capital);
    }
    env.trade_with_cu(
        &target_owner,
        target,
        &target_peer_owner,
        target_peer,
        Q,
        PRICE,
        0,
    );
    env.trade_with_cu(
        &trigger_owner,
        trigger,
        &trigger_peer_owner,
        trigger_peer,
        Q,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, UP_PRICE);
    for portfolio in [target_peer, target, trigger, trigger_peer] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(env.portfolio_state(target).pnl.get(), 5_000);
    assert_eq!(env.portfolio_state(trigger).pnl.get(), 5_000);
    env.trade_with_cu(
        &target_owner,
        target,
        &target_peer_owner,
        target_peer,
        POS_SCALE as i128,
        UP_PRICE,
        0,
    );
    let lien = env.portfolio_state(target).source_domains[0];
    assert!(lien.source_claim_counterparty_liened_num.get() > 0);
    assert!(lien.source_lien_counterparty_backing_num.get() > 0);
    let trigger_source = env.portfolio_state(trigger).source_domains[0];
    assert!(trigger_source.source_claim_bound_num.get() > 0);
    assert_eq!(trigger_source.source_claim_liened_num.get(), 0);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_for_asset_as_admin(0, 3, FINAL_PRICE);
    env.crank(
        trigger,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let prospective_loser = env.portfolio_state(trigger_peer);
    assert_eq!(prospective_loser.pnl.get(), 0);
    assert!(
        env.market_state().1.assets[0].k_short < active_leg_for_asset(&prospective_loser, 0).k_snap,
        "the opposing loser must retain a fresh adverse prospective K/F delta"
    );

    env.svm.warp_to_slot(RESOLVE_SLOT);
    env.resolve();
    let target_destination = env.token_account(target_owner.pubkey(), 0);
    let target_peer_destination = env.token_account(target_peer_owner.pubkey(), 0);
    let trigger_destination = env.token_account(trigger_owner.pubkey(), 0);
    let loser_destination = env.token_account(trigger_peer_owner.pubkey(), 0);
    let send_resolved = |env: &mut V16CuEnv,
                         owner: Pubkey,
                         portfolio: Pubkey,
                         destination: Pubkey,
                         use_crank: bool| {
        env.svm.expire_blockhash();
        let instruction = if use_crank {
            ProgInstruction::PermissionlessCrank {
                now_slot: RESOLVE_SLOT,
                observations: crank_observations(0),
            }
        } else {
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
        };
        env.send(
            instruction,
            vec![
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
    };
    let is_terminal = |env: &V16CuEnv, portfolio: Pubkey| {
        let state = env.portfolio_state(portfolio);
        let receipt = resolved_receipt(&state);
        percolator::active_bitmap_is_empty(active_bitmap(&state))
            && state.capital.get() == 0
            && state.pnl.get() == 0
            && (!receipt.present || receipt.finalized)
    };
    send_resolved(
        &mut env,
        trigger_owner.pubkey(),
        trigger,
        trigger_destination,
        false,
    )
    .expect("the lien-free winner must commit one bounded foreign-expiry step");
    let impaired_market = env.market_state().1;
    assert_eq!(
        impaired_market.source_backing_buckets[1].status,
        BackingBucketStatusV16::Impaired,
        "the lien-free winner must expose the shared impaired-backing state"
    );
    assert!(
        impaired_market.source_backing_buckets[1].impaired_liened_backing_num
            >= lien.source_lien_counterparty_backing_num.get(),
        "the aggregate impaired reserve must include the target's live local lien"
    );

    let mut target_progressed = false;
    let mut target_errors = Vec::new();
    let mut loser_progressed = false;
    let mut loser_errors = Vec::new();
    for round in 0..8 {
        for (owner, portfolio, destination) in [
            (target_owner.pubkey(), target, target_destination),
            (
                target_peer_owner.pubkey(),
                target_peer,
                target_peer_destination,
            ),
            (trigger_owner.pubkey(), trigger, trigger_destination),
        ] {
            if is_terminal(&env, portfolio) {
                continue;
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_before = env.svm.get_account(&portfolio).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let destination_before = env.svm.get_account(&destination).unwrap();
            match send_resolved(&mut env, owner, portfolio, destination, round % 2 != 0) {
                Ok(cu) => {
                    assert_cu_within("nonblocked resolved peer", cu, 1_000_000);
                    let mutated = env.svm.get_account(&env.market).unwrap() != market_before
                        || env.svm.get_account(&portfolio).unwrap() != portfolio_before
                        || env.svm.get_account(&env.vault).unwrap() != vault_before
                        || env.svm.get_account(&destination).unwrap() != destination_before;
                    assert!(mutated, "accepted resolved peer continuation was a no-op");
                    if portfolio == target && mutated {
                        target_progressed = true;
                    }
                }
                Err(error) => {
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
                    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                    assert_eq!(
                        env.svm.get_account(&destination).unwrap(),
                        destination_before
                    );
                    if portfolio == target {
                        target_errors.push(error);
                    }
                }
            }
        }

        for use_crank in [false, true] {
            if is_terminal(&env, trigger_peer) {
                continue;
            }
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_before = env.svm.get_account(&trigger_peer).unwrap();
            let vault_before = env.svm.get_account(&env.vault).unwrap();
            let destination_before = env.svm.get_account(&loser_destination).unwrap();
            match send_resolved(
                &mut env,
                trigger_peer_owner.pubkey(),
                trigger_peer,
                loser_destination,
                use_crank,
            ) {
                Ok(cu) => {
                    assert_cu_within("impaired prospective-loss progress", cu, 1_000_000);
                    let mutated = env.svm.get_account(&env.market).unwrap() != market_before
                        || env.svm.get_account(&trigger_peer).unwrap() != portfolio_before
                        || env.svm.get_account(&env.vault).unwrap() != vault_before
                        || env.svm.get_account(&loser_destination).unwrap() != destination_before;
                    assert!(
                        mutated,
                        "accepted prospective-loss continuation was a no-op"
                    );
                    loser_progressed |= mutated;
                }
                Err(error) => {
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                    assert_eq!(
                        env.svm.get_account(&trigger_peer).unwrap(),
                        portfolio_before
                    );
                    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
                    assert_eq!(
                        env.svm.get_account(&loser_destination).unwrap(),
                        destination_before
                    );
                    loser_errors.push(error);
                }
            }
        }
    }
    assert!(
        loser_progressed,
        "the impaired-domain prospective loss made no terminal progress: {loser_errors:?}"
    );
    assert!(
        target_progressed,
        "the foreign-impaired lien path did not reach its staged progress state"
    );
    assert!(
        target_errors.is_empty(),
        "the account-local foreign-impaired lien path must remain live: {target_errors:?}"
    );
    assert!(
        loser_errors.is_empty(),
        "the prospective-loss path must remain live after source impairment: {loser_errors:?}"
    );

    for (label, portfolio) in [
        ("foreign-impaired winner", target),
        ("foreign-impaired counterparty", target_peer),
        ("expiry-trigger winner", trigger),
        ("prospective loser", trigger_peer),
    ] {
        let state = env.portfolio_state(portfolio);
        let receipt = resolved_receipt(&state);
        assert!(
            percolator::active_bitmap_is_empty(active_bitmap(&state))
                && state.capital.get() == 0
                && state.pnl.get() == 0
                && (!receipt.present || receipt.finalized),
            "{label} must reach terminal disposition: capital={}, pnl={}, bitmap={:?}, receipt={receipt:?}",
            state.capital.get(),
            state.pnl.get(),
            active_bitmap(&state),
        );
    }
    let terminal_market = env.market_state().1;
    assert_eq!(
        terminal_market.source_backing_buckets[1].status,
        BackingBucketStatusV16::Expired,
        "the final account-local provider label must normalize the shared bucket"
    );
    assert_eq!(
        terminal_market.source_backing_buckets[1].impaired_liened_backing_num,
        0
    );
    assert_eq!(
        terminal_market.source_credit[1].impaired_liened_backing_num,
        0
    );
}

#[derive(Clone, Copy, Debug)]
enum SourceCapacityFillOrder {
    Forward,
    Reverse,
}

impl SourceCapacityFillOrder {
    const ALL: [Self; 2] = [Self::Forward, Self::Reverse];

    fn assets(self) -> Vec<u16> {
        let mut assets: Vec<_> =
            (0..percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS).collect();
        if matches!(self, Self::Reverse) {
            assets.reverse();
        }
        assets
    }
}

fn assert_capacity_attempt_rolls_back<T, E>(
    label: &str,
    result: &Result<T, E>,
    env: &V16CuEnv,
    owner: Pubkey,
    counterparty: Pubkey,
    market_before: &Account,
    owner_before: &Account,
    counterparty_before: &Account,
    vault_before: u64,
) {
    assert!(result.is_err(), "{label} unexpectedly escaped the lock");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        *market_before,
        "{label} mutated market state on rejection"
    );
    assert_eq!(
        env.svm.get_account(&owner).unwrap(),
        *owner_before,
        "{label} mutated owner state on rejection"
    );
    assert_eq!(
        env.svm.get_account(&counterparty).unwrap(),
        *counterparty_before,
        "{label} mutated counterparty state on rejection"
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault_before,
        "{label} moved SPL custody on rejection"
    );
}

fn run_source_capacity_admission_order(order: SourceCapacityFillOrder) {
    const ACTIVE_CAP: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const HISTORICAL_ASSETS: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const NEW_ASSET: u16 = HISTORICAL_ASSETS;
    const LOW: u64 = 100;
    const HIGH: u64 = 101;

    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: ACTIVE_CAP,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        70,
    );
    for asset_index in ACTIVE_CAP..=NEW_ASSET {
        env.activate_asset(asset_index, u64::from(asset_index - ACTIVE_CAP + 1), LOW);
    }
    let mut slot = u64::from(NEW_ASSET - ACTIVE_CAP + 1);
    env.svm.warp_to_slot(slot);
    for asset_index in 0..=NEW_ASSET {
        env.configure_auth_mark_for_asset_as_admin(asset_index, slot, LOW);
    }

    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);

    let settle_both = |env: &mut V16CuEnv, asset_index: u16, now_slot: u64| {
        for account in [counterparty, portfolio] {
            env.crank(
                account,
                ProgInstruction::PermissionlessCrank {
                    now_slot,
                    observations: crank_observations(asset_index),
                },
            );
        }
    };

    let historical_assets = order.assets();
    for asset_index in historical_assets.iter().copied() {
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            asset_index,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            POS_SCALE as i128,
            LOW,
            0,
        );
        slot += 1;
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(asset_index, slot, HIGH);
        settle_both(&mut env, asset_index, slot);
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            asset_index,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            -(POS_SCALE as i128),
            HIGH,
            0,
        );

        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            asset_index,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            -(POS_SCALE as i128),
            HIGH,
            0,
        );
        slot += 1;
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(asset_index, slot, LOW);
        settle_both(&mut env, asset_index, slot);
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            asset_index,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            POS_SCALE as i128,
            LOW,
            0,
        );
    }

    let full = env.portfolio_state(portfolio);
    assert_eq!(
        full.source_domains
            .iter()
            .filter(|source| source.is_occupied())
            .count(),
        percolator_prog::constants::WRAPPER_MAX_BOUNDED_SOURCE_DOMAINS,
        "public historical episodes must fill the wrapper-supported sparse source table"
    );
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&full)));
    let old_asset = historical_assets[0];
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        old_asset,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        POS_SCALE as i128,
        LOW,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        old_asset,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(POS_SCALE as i128),
        LOW,
        0,
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(portfolio))),
        "risk on already-reserved source domains remains closeable"
    );

    let vault_before = env.token_amount(env.vault);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let owner_before = env.svm.get_account(&portfolio).unwrap();
    let counterparty_before = env.svm.get_account(&counterparty).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.try_trade_asset_with_cu(
        NEW_ASSET,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        POS_SCALE as i128,
        LOW,
        0,
    );
    let rejected_text = rejected
        .as_ref()
        .expect_err("unreserved new-asset risk must reject before funded admission");
    assert!(
        rejected_text.contains("Custom(9)")
            && !rejected_text.contains("ProgramFailedToComplete")
            && !rejected_text.contains("exceeded CUs"),
        "unreserved source-domain admission must fail cleanly, got {rejected_text}"
    );
    assert_capacity_attempt_rolls_back(
        "unreserved new-asset risk admission",
        &rejected,
        &env,
        portfolio,
        counterparty,
        &market_before,
        &owner_before,
        &counterparty_before,
        vault_before,
    );
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(portfolio))),
        "rejected unreserved risk must not leave a funded active leg"
    );
}

#[test]
fn v16_program_source_capacity_admission_order_matrix_rejects_unreserved_risk() {
    for order in SourceCapacityFillOrder::ALL {
        run_source_capacity_admission_order(order);
    }
}

fn run_expired_source_lien_route_matrix(now_slot: u64, hinted_first: bool) {
    const PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const OPEN_UNITS: i128 = 1_000;
    const EXPIRY_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 5_000, 500);
    env.configure_permissionless_resolve_with_cu(10_000, 1);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.top_up_backing_bucket(1, 100_000, EXPIRY_SLOT);

    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 52_501);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        OPEN_UNITS * POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(0, 1, WINNING_MARK);
    for account in [counterparty, portfolio] {
        env.crank(
            account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(env.portfolio_state(portfolio).pnl.get(), 5_000);
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        50 * POS_SCALE as i128,
        WINNING_MARK,
        0,
    );
    let source_before = env.portfolio_state(portfolio).source_domains[0];
    assert!(source_before.source_claim_counterparty_liened_num.get() > 0);
    assert!(source_before.source_lien_counterparty_backing_num.get() > 0);
    let vault_before = env.token_amount(env.vault);
    env.svm.warp_to_slot(now_slot);
    env.push_auth_mark_for_asset_as_admin(0, now_slot, WINNING_MARK);

    let mut expiry_steps = 0usize;
    while env.market_state().1.source_backing_buckets[1].status == BackingBucketStatusV16::Fresh
        && expiry_steps < 4
    {
        env.svm.expire_blockhash();
        let market_before_crank = env.svm.get_account(&env.market).unwrap();
        let portfolio_before_crank = env.svm.get_account(&portfolio).unwrap();
        let observations = if expiry_steps == 0 && hinted_first {
            crank_observations(0)
        } else {
            vec![]
        };
        let crank = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        );
        let cu = crank.expect("an honest crank must normalize lapsed backing");
        assert!(cu < 1_400_000);
        let status_after = env.market_state().1.source_backing_buckets[1].status;
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_before_crank
                || env.svm.get_account(&portfolio).unwrap() != portfolio_before_crank,
            "a successful expiry crank must mutate the liveness state: slot={now_slot}, \
             hinted_first={hinted_first}, step={expiry_steps}, status_after={status_after:?}"
        );
        expiry_steps += 1;
    }
    assert!(expiry_steps != 0 && expiry_steps <= 4);
    assert_eq!(
        env.market_state().1.source_backing_buckets[1].status,
        BackingBucketStatusV16::Impaired,
        "liened expiry must become an impaired bucket"
    );
    let impaired_source = env.portfolio_state(portfolio).source_domains[0];
    assert!(impaired_source.source_claim_counterparty_liened_num.get() > 0);
    assert!(impaired_source.source_lien_counterparty_backing_num.get() > 0);
    assert_eq!(env.token_amount(env.vault), vault_before);

    let initial_exposure = active_leg_for_asset(&env.portfolio_state(portfolio), 0)
        .basis_pos_q
        .unsigned_abs();
    assert!(initial_exposure > 0);
    env.svm.expire_blockhash();
    let reduction_cu = env
        .send(
            ProgInstruction::RebalanceReduce {
                portfolio_id: env.portfolio_id(portfolio),
                position_epoch: env.portfolio_position_epoch(portfolio),
                asset_index: 0,
                reduce_q: initial_exposure,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
        .expect("one bounded owner reduction must clear the expired-backed leg");
    assert!(reduction_cu < 1_400_000);
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(portfolio), 0),
        "the owner must reach zero exposure"
    );
    assert_eq!(env.token_amount(env.vault), vault_before);
    let exited = env.portfolio_state(portfolio);
    let remaining_capital = exited.capital.get();
    assert!(
        remaining_capital > 0,
        "the liveness witness must remain funded"
    );
    let vault_before_withdrawal = env.token_amount(env.vault);
    let (destination, withdrawal_cu) = env.withdraw_with_cu(&owner, portfolio, remaining_capital);
    assert!(withdrawal_cu < 1_400_000);
    assert_eq!(env.token_amount(destination), remaining_capital as u64);
    assert_eq!(
        vault_before_withdrawal - env.token_amount(env.vault),
        remaining_capital as u64
    );
    let after_principal = env.portfolio_state(portfolio);
    assert_eq!(after_principal.capital.get(), 0);
    assert_eq!(after_principal.pnl.get(), 5_000);
    assert!(
        after_principal.source_domains[0]
            .source_claim_liened_num
            .get()
            > 0
    );
    let market_before_conversion = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_conversion = env.svm.get_account(&portfolio).unwrap();
    let vault_before_conversion = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let conversion = env.send(
        env.convert_released_pnl_ix(portfolio, after_principal.pnl.get() as u128),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    let conversion_error = conversion
        .expect_err("an impaired source claim must not convert before public reconciliation");
    assert!(
        conversion_error.contains("Custom(19)")
            && !conversion_error.contains("ProgramFailedToComplete")
            && !conversion_error.contains("exceeded CUs"),
        "the unreconciled claim must fail stale, not exhaust compute: {conversion_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_conversion
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before_conversion
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_conversion
    );

    let market_before_progress = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_progress = env.svm.get_account(&portfolio).unwrap();
    let progress = env
        .crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: crank_observations(0),
            },
        )
        .expect("one permissionless continuation must refresh the flat impaired claim");
    assert_cu_within("flat impaired source refresh", progress, 1_400_000);
    assert!(
        env.svm.get_account(&env.market).unwrap() != market_before_progress
            || env.svm.get_account(&portfolio).unwrap() != portfolio_before_progress,
        "the successful impaired-source continuation must mutate liveness state"
    );

    let market_before_locked_conversion = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_locked_conversion = env.svm.get_account(&portfolio).unwrap();
    let vault_before_locked_conversion = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let conversion_after_progress = env.send(
        env.convert_released_pnl_ix(portfolio, after_principal.pnl.get() as u128),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    let locked_error = conversion_after_progress
        .expect_err("an impaired claim must remain locked until terminal reconciliation");
    assert!(
        locked_error.contains("Custom(21)")
            && !locked_error.contains("ProgramFailedToComplete")
            && !locked_error.contains("exceeded CUs"),
        "the refreshed impaired claim must fail locked, not exhaust compute: {locked_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_locked_conversion
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before_locked_conversion
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_locked_conversion
    );

    let resolve_slot = now_slot + 10_001;
    let resolve_cu = env.resolve_stale_permissionless_with_cu(resolve_slot);
    assert_cu_within(
        "flat impaired source permissionless resolve",
        resolve_cu,
        1_400_000,
    );
    env.svm.warp_to_slot(resolve_slot + 1);
    let (payouts, close_cu) = drain_resolved_cohort_with_cu_limit(
        &mut env,
        &[(&owner, portfolio), (&counterparty_owner, counterparty)],
        "flat impaired source terminal cohort",
        1_400_000,
    );
    assert_cu_within("flat impaired source terminal close", close_cu, 1_400_000);
    assert_eq!(
        payouts,
        vec![5_000, 995_000],
        "terminal reconciliation must pay the retained claim exactly without charging the provider bucket"
    );
    for source in env.portfolio_state(portfolio).source_domains {
        assert!(!source.is_occupied());
    }
    let terminal_market = env.market_state().1;
    assert_eq!(
        terminal_market.source_backing_buckets[1].impaired_liened_backing_num,
        0
    );
    assert_eq!(
        terminal_market.source_backing_buckets[1].consumed_liened_backing_num, 0,
        "the resolved user transfer must not consume provider backing"
    );
    assert_eq!(
        terminal_market.source_credit[1].impaired_liened_backing_num,
        0
    );
    assert_eq!(
        terminal_market.source_backing_buckets[1].status,
        BackingBucketStatusV16::Expired
    );
    for (account_owner, account) in [(&owner, portfolio), (&counterparty_owner, counterparty)] {
        let close_cu = env.close_portfolio_with_cu(account_owner, account);
        assert_cu_within("flat impaired source portfolio close", close_cu, 1_400_000);
    }
    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
}

#[test]
fn v16_program_expired_source_lien_route_matrix_preserves_bounded_owner_exit() {
    run_expired_source_lien_route_matrix(2, true);
    run_expired_source_lien_route_matrix(3, false);
}

// security.md sweep - last-principal backing withdrawal must not strand provider earnings (#22/#48):
// utilization-fee earnings are owed to the backing authority but stored beside the principal bucket.
// A provider who withdraws principal before earnings must not turn the bucket into an invalid empty
// shell with trapped earnings; the rejected attempt must roll back market, ledger, vault, and dest.
#[test]
fn v16_attack_backing_principal_withdraw_preserves_provider_earnings() {
    const PRINCIPAL: u128 = 100;
    const EARNINGS: u128 = 42;
    let mut env = V16CuEnv::new();
    env.activate_asset(1, 1, 100);
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 2, PRINCIPAL, 10_000);
    env.mutate_market(|_, group| {
        group.source_backing_buckets[2].utilization_fee_earnings = EARNINGS;
        group.vault += EARNINGS;
    });
    let funded_vault = env.market_state().1.vault as u64;
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, funded_vault);
    let (_, funded_group) = env.market_state();
    assert_eq!(
        funded_group.source_backing_buckets[2].fresh_unliened_backing_num,
        PRINCIPAL * BOUND_SCALE,
        "asset-1 backing principal is present (non-vacuous)"
    );
    assert_eq!(
        funded_group.source_backing_buckets[2].utilization_fee_earnings, EARNINGS,
        "asset-1 backing provider earnings are owed (non-vacuous)"
    );
    let asset1_market_id = funded_group.assets[1].market_id;

    let admin = env.admin.insecure_clone();
    let dest = env.token_account(admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    env.svm.expire_blockhash();
    let premature_principal = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 2,
            market_id: asset1_market_id,
            authority_epoch: 0,
            amount: PRINCIPAL,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    );
    assert!(
        premature_principal.is_err(),
        "last-principal withdrawal must reject while provider earnings remain unpaid"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected principal withdrawal leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        ledger_before,
        "rejected principal withdrawal leaves the provider ledger unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected principal withdrawal leaves the canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected principal withdrawal pays no destination tokens"
    );
    let (_, still_funded_group) = env.market_state();
    assert_eq!(
        still_funded_group.source_backing_buckets[2].fresh_unliened_backing_num,
        PRINCIPAL * BOUND_SCALE,
        "principal remains recoverable after rejected premature withdrawal"
    );
    assert_eq!(
        still_funded_group.source_backing_buckets[2].utilization_fee_earnings, EARNINGS,
        "earnings remain recoverable after rejected premature withdrawal"
    );

    env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(ledger, dest, 2, EARNINGS);
    assert_eq!(
        env.token_amount(dest),
        EARNINGS as u64,
        "backing provider recovers the accrued earnings first"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[2].utilization_fee_earnings,
        0,
        "earnings blocker is fully drained"
    );

    env.svm.expire_blockhash();
    let principal_after_earnings = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: 2,
            market_id: asset1_market_id,
            authority_epoch: 0,
            amount: PRINCIPAL,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&admin],
    );
    assert!(
        principal_after_earnings.is_ok(),
        "principal withdrawal succeeds after earnings are paid: {principal_after_earnings:?}"
    );
    assert_eq!(
        env.token_amount(dest),
        (PRINCIPAL + EARNINGS) as u64,
        "provider recovers both earnings and principal exactly once"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[2].fresh_unliened_backing_num,
        0,
        "principal blocker is fully drained after the safe order"
    );

    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let market_id = env.asset_market_id(1);
    let accepted = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_RETIRE,
            asset_index: 1,
            market_id,
            authority_epoch: 0,
            now_slot: 4,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: admin.pubkey().to_bytes(),
            insurance_operator: admin.pubkey().to_bytes(),
            backing_bucket_authority: admin.pubkey().to_bytes(),
            oracle_authority: admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        accepted.is_ok(),
        "RETIRE succeeds once provider earnings and principal are paid: {accepted:?}"
    );
    let (_, retired_group) = env.market_state();
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired,
        "asset-1 retired after backing-provider funds are paid"
    );
    assert_eq!(
        retired_group.source_backing_buckets[2].utilization_fee_earnings, 0,
        "retired slot carries no stale provider earnings"
    );
}

// security.md sweep — convert bounded by available backing (#33/#35): if a winner's positive pnl
// exceeds its source backing, ConvertReleasedPnl must release at most the AVAILABLE backing, never the
// full (partly-unbacked) pnl. Otherwise unbacked pnl would convert into withdrawable capital.
#[test]
fn v16_attack_convert_bounded_by_available_backing() {
    const BACKING: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, BACKING, 10);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    // add MORE positive pnl (80) than the backing (40).
    env.add_source_positive_pnl(p, 1, 80);
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let cap_before = env.portfolio_state(p).capital.get();
    let (_, g0) = env.market_state();
    // convert with a huge cap -> released amount must be bounded by the available backing.
    env.svm.expire_blockhash();
    env.send(
        env.convert_released_pnl_ix(p, 1_000_000_000),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    )
    .expect("convert the backed portion of released pnl");
    let converted = env.portfolio_state(p).capital.get() - cap_before;
    assert!(
        converted > 0,
        "the backing cap check must observe a real conversion"
    );
    assert!(
        converted <= BACKING,
        "convert released at most the available backing ({} <= {})",
        converted,
        BACKING
    );
    let (_, g1) = env.market_state();
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation after convert"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    let _ = g0;
}

// security.md sweep — backing bucket top-up/withdraw input gates (#33/#44): a backing top-up with an
// already-expired (or zero) expiry must reject (no dead backing injected to skew freshness accounting),
// and a withdraw can never exceed the bucket's fresh-unliened principal. Complements the watermark/lien
// permutation tests (which cover liened backing) with the plain balance + expiry gates.
#[test]
fn v16_attack_backing_bucket_topup_withdraw_input_gates() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let cur = env.svm.get_sysvar::<Clock>().slot;

    // helper: inline TopUpBackingBucket returning Result (the env helper panics on reject).
    let top_up = |env: &mut V16CuEnv, amount: u128, expiry: u64| -> Result<u64, String> {
        let source = Pubkey::new_unique();
        env.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(env.mint, admin.pubkey(), amount.max(1) as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 0,
                amount,
                expiry_slot: expiry,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };

    // (1) expiry_slot <= current_slot must reject (no already-expired backing) — for a real (amount>0) top-up.
    assert!(
        top_up(&mut env, 1_000_000, cur).is_err(),
        "expiry == now must reject"
    );
    assert!(
        top_up(&mut env, 1_000_000, 0).is_err(),
        "expiry 0 must reject"
    );
    // (2) zero amount is a benign no-op: the engine add (which holds the expiry gate) is skipped, so it
    // succeeds without injecting any backing — even with an expired expiry. Nothing enters the vault.
    assert!(
        top_up(&mut env, 0, cur + 1_000_000).is_ok(),
        "zero-amount top-up is a no-op success"
    );
    assert!(
        top_up(&mut env, 0, 0).is_ok(),
        "zero-amount top-up no-op even with expired expiry"
    );
    // nothing entered the vault on any rejected top-up or zero no-op.
    assert_eq!(
        env.market_state().1.vault,
        0,
        "no backing injected by rejected/no-op top-ups"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num,
        0,
        "no backing num"
    );

    // (3) a valid top-up succeeds; backing is Fresh & unliened.
    assert!(
        top_up(&mut env, 1_000_000, cur + 1_000_000).is_ok(),
        "valid top-up ok"
    );
    let g = env.market_state().1;
    assert_eq!(g.vault, 1_000_000, "vault holds the backing principal");
    // fresh_unliened_backing_num is in BOUND_SCALE units (amount * 1e12); just assert it's funded.
    assert!(
        g.source_backing_buckets[0].fresh_unliened_backing_num > 0,
        "backing is fresh-unliened"
    );

    // (4) withdraw beyond the fresh-unliened principal must reject.
    let dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let r_over = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: g.assets[0].market_id,
            authority_epoch: 0,
            amount: 1_000_001,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        r_over.is_err(),
        "withdraw > fresh-unliened principal must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no backing paid out on rejected over-withdraw"
    );

    // (5) withdraw exactly the principal succeeds and conserves.
    env.svm.expire_blockhash();
    env.withdraw_backing_bucket_to_admin_token_with_cu(dest, 0, 1_000_000);
    assert_eq!(
        env.token_amount(dest),
        1_000_000,
        "exactly the principal withdrawn"
    );
    let g = env.market_state().1;
    assert_eq!(
        g.source_backing_buckets[0].fresh_unliened_backing_num, 0,
        "backing drained"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — ConvertReleasedPnl cannot mint from unbacked pnl (#33/#35/#22): the existing
// caller-cap test (#?) uses FULLY-backed pnl. Here the positive pnl (100) EXCEEDS its source backing
// (residual 40). Attacker goal: convert the full 100 into withdrawable senior capital, printing 60 of
// unbacked value. Protection: only the residual-backed portion converts to capital; the phantom excess
// is cleared (it was never realizable), the account's realizable value is conserved, and the vault is
// never minted.
#[test]
fn v16_attack_convert_released_pnl_cannot_mint_from_unbacked_pnl() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000); // residual backing = 40 in domain 1
    let o = Keypair::new();
    let p = env.create_portfolio(&o);
    env.deposit(&o, p, 1_000);
    env.add_source_positive_pnl(p, 1, 100); // claim 100 -> only 40 is backed
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let pre = env.portfolio_state(p);
    let g_pre = env.market_state().1;
    let residual_pre = g_pre
        .vault
        .saturating_sub(g_pre.c_tot)
        .saturating_sub(g_pre.insurance);
    assert!(
        pre.pnl.get() > residual_pre as i128,
        "non-vacuous: pnl ({}) exceeds the backing residual ({})",
        pre.pnl.get(),
        residual_pre
    );

    // ATTACK: convert with a huge cap, trying to realize the full (partly-unbacked) pnl into capital.
    env.svm.expire_blockhash();
    let r = env.send(
        env.convert_released_pnl_ix(p, 1_000_000_000),
        vec![
            AccountMeta::new(o.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&o],
    );
    assert!(
        r.is_ok(),
        "convert should succeed (converting the backed portion): {:?}",
        r
    );

    let post = env.portfolio_state(p);
    let g = env.market_state().1;
    let minted = post.capital.get() as i128 - pre.capital.get() as i128;
    // ANTI-MINT: capital grew by AT MOST the residual that actually backed the pnl — never the full 100.
    assert!(
        minted <= residual_pre as i128,
        "capital minted ({}) must not exceed backing residual ({})",
        minted,
        residual_pre
    );
    assert_eq!(
        minted, residual_pre as i128,
        "exactly the backed portion (40) converts to capital"
    );
    assert!(
        minted < pre.pnl.get(),
        "the unbacked excess (60) is NOT converted to capital"
    );
    // realizable value conserved: capital_before + backed (== capital_after); no phantom value created.
    assert_eq!(
        post.capital.get() as i128,
        pre.capital.get() as i128 + residual_pre as i128,
        "realizable value conserved"
    );
    // no vault minting + senior conservation.
    assert_eq!(
        g.vault, g_pre.vault,
        "ConvertReleasedPnl moves no vault tokens"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — full winner extraction: convert backed pnl then withdraw (#33/#35/#44 interaction):
// a backed winner converts released junior pnl to senior capital, then withdraws. Attacker goal: extract
// MORE than (deposit + backed pnl) — print value on the way out. Protection: the conversion is bounded by
// the backing (#147) and withdraw moves only real capital, so total out == deposit + backing, no more,
// and the vault drains to exactly what the backing provider funded.
#[test]
fn v16_attack_convert_then_withdraw_extracts_exactly_backed() {
    const DEP: u128 = 1_000;
    const BACK: u128 = 40;
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACK, 10_000); // an LP backs the winner with 40
    let o = Keypair::new();
    let p = env.create_portfolio(&o);
    env.deposit(&o, p, DEP);
    env.add_source_positive_pnl(p, 1, BACK); // fully-backed 40 junior pnl
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let vault0 = env.market_state().1.vault;
    assert_eq!(
        vault0,
        DEP + BACK,
        "vault holds the deposit + the LP backing"
    );

    // (1) convert the backed junior pnl into senior capital.
    env.svm.expire_blockhash();
    let rc = env.send(
        env.convert_released_pnl_ix(p, 1_000_000_000),
        vec![
            AccountMeta::new(o.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&o],
    );
    assert!(rc.is_ok(), "convert backed pnl should succeed: {:?}", rc);
    let cap_after_convert = env.portfolio_state(p).capital.get();
    assert_eq!(
        cap_after_convert,
        DEP + BACK,
        "capital == deposit + the backed portion (exactly)"
    );

    // (2) withdraw the full converted capital (account is flat — no open position).
    env.svm.expire_blockhash();
    let dest = env.withdraw(&o, p, DEP + BACK);
    let out = env.token_amount(dest) as u128;

    // EXTRACTION BOUND: the winner pulled EXACTLY deposit + backing, never more.
    assert_eq!(
        out,
        DEP + BACK,
        "winner extracts exactly deposit + backed pnl, not a unit more"
    );
    let pf = env.portfolio_state(p);
    let g = env.market_state().1;
    assert_eq!(pf.capital.get(), 0, "winner fully withdrawn");
    assert_eq!(
        g.vault, 0,
        "vault drained to exactly the funded amount (deposit + LP backing), no residual mint"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — backing-bucket creation must reject an already-lapsed expiry. TopUpBackingBucket
// forwards expiry_slot to the engine's deposit_fresh, which requires a FUTURE expiry
// (expiry_slot > current_slot). A topup at expiry_slot 0 would mint immediately-lapsed backing
// principal (poisoning the live source-domain ledger) — it must reject, pulling no tokens; a future
// expiry is the accepted control.
#[test]
fn v16_attack_backing_topup_rejects_lapsed_expiry() {
    let mut env = V16CuEnv::new();
    let vault0 = env.token_amount(env.vault);
    let admin = env.admin.insecure_clone();
    let source = env.token_account(admin.pubkey(), 50);
    let topup = |env: &mut V16CuEnv, expiry: u64| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 1,
                amount: 50,
                expiry_slot: expiry,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };
    let r = topup(&mut env, 0);
    assert!(
        r.is_err(),
        "topup with lapsed expiry_slot=0 must reject: {r:?}"
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault0,
        "rejected lapsed-expiry topup pulled no tokens into the vault"
    );
    assert_eq!(
        env.token_amount(source),
        50,
        "source balance intact after reject"
    );
    // control: a future expiry is accepted and credits exactly the backing.
    let r_ok = topup(&mut env, 10_000);
    assert!(
        r_ok.is_ok(),
        "future-expiry backing topup must succeed: {r_ok:?}"
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault0 + 50,
        "valid topup credits exactly the backing"
    );
}

#[test]
fn v16_bpf_trade_paths_respect_source_credit_watermark_permutations() {
    for path in [
        SourceCreditWatermarkTradePath::NoCpi,
        SourceCreditWatermarkTradePath::Cpi,
    ] {
        for direction in [
            SourceCreditWatermarkDirection::PositiveSize,
            SourceCreditWatermarkDirection::NegativeSize,
        ] {
            run_source_credit_watermark_trade_case(path, direction);
        }
    }
}
