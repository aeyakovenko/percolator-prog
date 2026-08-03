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
//!
//! Guarantee boundary: this is a public maximum-shape counterexample on the vulnerable engine
//! pin. It does not certify the fixed admission reservation rule.

use super::*;

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
    env.crank(
        counterparty,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(NEW_ASSET),
        },
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
