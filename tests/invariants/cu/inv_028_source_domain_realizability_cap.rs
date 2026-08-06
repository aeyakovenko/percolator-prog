//! INV-028 - Source-domain realizability cap (maximum-shape public routes).
//!
//! Normative obligation: admission must reserve enough sparse source capacity for every active
//! leg's later favorable settlement. A publicly admitted leg must never become impossible to
//! settle or close merely because historical source claims occupy the bounded source table.
//!
//! Evidence in this file (I/C/M):
//! `v16_program_source_capacity_admission_order_matrix_discovers_funded_lock` fills every source
//! slot through ordinary trades in forward and reverse asset order, admits a new live leg, moves
//! its authenticated mark, and probes keeper crank, claim conversion, unilateral reduction,
//! signed single/batch trade, and authenticated CPI exits. A finding requires every route to
//! reject with exact rollback while real capital, two active positions, and vault liquidity remain.
//! `v16_program_expired_source_lien_route_matrix_discovers_funded_residue_lock` creates a source lien by
//! ordinary risk increase, crosses its authenticated expiry at two boundaries, and judges crank
//! progress from the source-lien rank rather than the return code. It records repeated successful
//! no-op cranks, permits any genuine partial reductions, and requires every remaining owner route
//! to roll back exactly once the reduction sequence reaches a funded nonzero fixed point.
//! `v16_program_shared_expiry_prerequisite_matrix_keeps_bucket_fresh` constructs the exact
//! two-winner precursor with both a live source lien and a prospective adverse K/F delta, then
//! proves whether the lien-free close can create the impaired aggregate state required by either
//! downstream finding.
//!
//! Guarantee boundary: this is a public maximum-shape counterexample on the vulnerable engine
//! pin. It does not certify the fixed admission reservation rule.

use super::*;

#[test]
fn v16_program_shared_expiry_prerequisite_matrix_keeps_bucket_fresh() {
    const Q: i128 = 1_000 * POS_SCALE as i128;
    const PRICE: u64 = 100;
    const UP_PRICE: u64 = 105;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 5_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 0, PRICE);
    env.top_up_backing_bucket(1, 100_000, 3);

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
    for portfolio in [target_peer, target, trigger] {
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
    let prospective_loser = env.portfolio_state(trigger_peer);
    assert_eq!(prospective_loser.pnl.get(), 0);
    assert!(
        env.market_state().1.assets[0].k_short < active_leg_for_asset(&prospective_loser, 0).k_snap,
        "the opposing loser must retain an adverse prospective K/F delta"
    );
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
    env.resolve();
    let trigger_destination = env.token_account(trigger_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(trigger_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(trigger, false),
            AccountMeta::new(trigger_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("the lien-free winner must commit the foreign expiry");
    let impaired_market = env.market_state().1;
    assert_eq!(
        impaired_market.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh,
        "the pinned predecessor unexpectedly created PR300's impaired prerequisite"
    );
    assert_eq!(
        impaired_market.source_backing_buckets[1].impaired_liened_backing_num,
        0
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(target), 0));
    assert!(env.portfolio_state(target).capital.get() != 0);
}

#[derive(Clone, Copy, Debug)]
enum SourceCapacityFillOrder {
    Forward,
    Reverse,
}

impl SourceCapacityFillOrder {
    const ALL: [Self; 2] = [Self::Forward, Self::Reverse];

    fn assets(self) -> Vec<u16> {
        let mut assets: Vec<_> = (0..16).collect();
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
    const HISTORICAL_ASSETS: u16 = 16;
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

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_context, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &counterparty_owner, counterparty);

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
        percolator::PORTFOLIO_SOURCE_DOMAIN_CAP,
        "public historical episodes must fill the sparse source table"
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
        NEW_ASSET,
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
    env.push_auth_mark_for_asset_as_admin(NEW_ASSET, slot, HIGH);
    env.crank_steps_after_market_catchup(
        counterparty,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(NEW_ASSET),
        },
        1,
    );

    let trapped = env.portfolio_state(portfolio);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&trapped)),
        2,
        "the vulnerable admission must leave both funded legs active"
    );
    assert_ne!(trapped.capital.get(), 0);
    let vault_before = env.token_amount(env.vault);
    assert!(u128::from(vault_before) >= trapped.capital.get());

    macro_rules! assert_blocked {
        ($label:literal, $attempt:expr) => {{
            env.svm.expire_blockhash();
            let market_before = env.svm.get_account(&env.market).unwrap();
            let owner_before = env.svm.get_account(&portfolio).unwrap();
            let counterparty_before = env.svm.get_account(&counterparty).unwrap();
            let result = $attempt;
            assert_capacity_attempt_rolls_back(
                $label,
                &result,
                &env,
                portfolio,
                counterparty,
                &market_before,
                &owner_before,
                &counterparty_before,
                vault_before,
            );
        }};
    }

    assert_blocked!(
        "permissionless crank",
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(NEW_ASSET),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
    );
    assert_blocked!(
        "released-PnL conversion",
        env.send(
            ProgInstruction::ConvertReleasedPnl { amount: 1 },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
    );
    assert_blocked!(
        "single signed new-leg close",
        env.try_trade_asset_with_cu(
            NEW_ASSET,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            -(POS_SCALE as i128),
            HIGH,
            0,
        )
    );
    assert_blocked!(
        "batch signed new-leg close",
        env.send(
            ProgInstruction::BatchTradeNoCpi {
                legs: vec![BatchTradeLeg {
                    asset_index: NEW_ASSET,
                    size_q: -(POS_SCALE as i128),
                    exec_price: HIGH,
                    fee_bps: 0,
                }],
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(counterparty_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(counterparty, false),
            ],
            &[&owner, &counterparty_owner],
        )
    );
    assert_blocked!(
        "authenticated CPI new-leg close",
        env.try_trade_cpi_with_cu_on_asset(
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            matcher_program,
            matcher_context,
            matcher_delegate,
            NEW_ASSET,
            -(POS_SCALE as i128),
            0,
        )
    );
    assert_blocked!(
        "single signed old-leg close",
        env.try_trade_asset_with_cu(
            old_asset,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            -(POS_SCALE as i128),
            LOW,
            0,
        )
    );
    assert_blocked!(
        "unilateral reduction",
        env.send(
            ProgInstruction::RebalanceReduce {
                asset_index: NEW_ASSET,
                reduce_q: POS_SCALE,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
    );
}

#[test]
fn v16_program_source_capacity_admission_order_matrix_discovers_funded_lock() {
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

    let mut reconciled = false;
    let mut successful_noops = 0usize;
    for step in 0..=3 {
        env.svm.expire_blockhash();
        let market_before_crank = env.svm.get_account(&env.market).unwrap();
        let portfolio_before_crank = env.svm.get_account(&portfolio).unwrap();
        let observations = if step == 0 && hinted_first {
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
        if let Ok(cu) = crank {
            assert!(cu < 1_400_000);
            if env.svm.get_account(&env.market).unwrap() == market_before_crank
                && env.svm.get_account(&portfolio).unwrap() == portfolio_before_crank
            {
                successful_noops += 1;
            }
        } else {
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_crank
            );
            assert_eq!(
                env.svm.get_account(&portfolio).unwrap(),
                portfolio_before_crank
            );
        }
        let source = env.portfolio_state(portfolio).source_domains[0];
        if source.source_claim_counterparty_liened_num.get() == 0
            && source.source_lien_counterparty_backing_num.get() == 0
        {
            assert!(source.source_claim_impaired_num.get() > 0);
            reconciled = true;
            break;
        }
    }
    assert!(
        !reconciled,
        "the vulnerable pin unexpectedly reconciled the lien"
    );
    assert!(
        successful_noops > 0,
        "the matrix must expose at least one false-success crank"
    );
    assert_eq!(env.token_amount(env.vault), vault_before);

    let initial_exposure = active_leg_for_asset(&env.portfolio_state(portfolio), 0)
        .basis_pos_q
        .unsigned_abs();
    let mut reduction_steps = 0usize;
    let mut terminal_error = None;
    while has_active_leg_for_asset(&env.portfolio_state(portfolio), 0) {
        let remaining = active_leg_for_asset(&env.portfolio_state(portfolio), 0)
            .basis_pos_q
            .unsigned_abs();
        let reduce_q = remaining.min(POS_SCALE);
        env.svm.expire_blockhash();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let portfolio_before = env.svm.get_account(&portfolio).unwrap();
        let reduction = env.send(
            ProgInstruction::RebalanceReduce {
                asset_index: 0,
                reduce_q,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        );
        match reduction {
            Ok(cu) => {
                assert!(cu < 1_400_000);
                reduction_steps += 1;
                assert!(reduction_steps <= 1_100);
            }
            Err(error) => {
                assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
                assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
                assert_eq!(env.token_amount(env.vault), vault_before);
                terminal_error = Some(error);
                break;
            }
        }
    }
    assert!(
        terminal_error.is_some(),
        "tiny reductions unexpectedly reached zero"
    );
    let trapped = env.portfolio_state(portfolio);
    let remaining_exposure = active_leg_for_asset(&trapped, 0).basis_pos_q.unsigned_abs();
    assert!(remaining_exposure > 0 && remaining_exposure <= initial_exposure);
    assert!(trapped.capital.get() > 0);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (matcher_context, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &counterparty_owner, counterparty);

    macro_rules! assert_exit_locked {
        ($label:literal, $attempt:expr) => {{
            env.svm.expire_blockhash();
            let market_before = env.svm.get_account(&env.market).unwrap();
            let portfolio_before = env.svm.get_account(&portfolio).unwrap();
            let counterparty_before = env.svm.get_account(&counterparty).unwrap();
            let matcher_before = env.svm.get_account(&matcher_context).unwrap();
            let result = $attempt;
            assert!(
                result.is_err(),
                "{} unexpectedly cleared the residue",
                $label
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
            assert_eq!(
                env.svm.get_account(&counterparty).unwrap(),
                counterparty_before
            );
            assert_eq!(
                env.svm.get_account(&matcher_context).unwrap(),
                matcher_before
            );
            assert_eq!(env.token_amount(env.vault), vault_before);
        }};
    }
    assert_exit_locked!(
        "signed bilateral reduction",
        env.try_trade_asset_with_cu(
            0,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            -(POS_SCALE as i128),
            WINNING_MARK,
            0,
        )
    );
    assert_exit_locked!(
        "signed batch reduction",
        env.send(
            ProgInstruction::BatchTradeNoCpi {
                legs: vec![BatchTradeLeg {
                    asset_index: 0,
                    size_q: -(POS_SCALE as i128),
                    exec_price: WINNING_MARK,
                    fee_bps: 0,
                }],
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(counterparty_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(counterparty, false),
            ],
            &[&owner, &counterparty_owner],
        )
    );
    assert_exit_locked!(
        "authenticated CPI reduction",
        env.try_trade_cpi_with_cu_on_asset(
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            matcher_program,
            matcher_context,
            matcher_delegate,
            0,
            -(POS_SCALE as i128),
            0,
        )
    );
    assert_exit_locked!(
        "released PnL conversion",
        env.send(
            ProgInstruction::ConvertReleasedPnl { amount: 1 },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
    );
}

#[test]
fn v16_program_expired_source_lien_route_matrix_discovers_funded_residue_lock() {
    run_expired_source_lien_route_matrix(2, true);
}
