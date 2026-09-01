//! INV-055 - State-indexed admission.
//!
//! This bounded matrix reaches each lifecycle state through public wrapper
//! instructions, then submits fully valid funded user operations. It avoids a
//! vacuous "everything rejected" result by requiring every operation to land in
//! each state where it is admissible and to produce its exact economic delta.
//! Forbidden cells must reject with byte-exact program/SPL/lamport rollback.
//!
//! Covered states are market `Live` with asset `Active`, `DrainOnly`, and
//! `Recovery`, plus market `Resolved`. Covered operations are a fresh matched
//! open, a bilateral exact reduction, unilateral `RebalanceReduce`, Recovery
//! `ForfeitRecoveryLeg`, deposit, withdraw, and `CloseResolved`. The owner-exit
//! cells require strict exposure reduction with no SPL movement, so they cannot
//! pass as accepted no-ops. Reset-side, retirement/reactivation, and the
//! remaining public instruction classes remain outside this bounded matrix.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{AssetLifecycleV16, MarketModeV16, POS_SCALE};
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifecycleState {
    Active,
    DrainOnly,
    Recovery,
    Resolved,
}

impl LifecycleState {
    const ALL: [Self; 4] = [
        Self::Active,
        Self::DrainOnly,
        Self::Recovery,
        Self::Resolved,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UserOperation {
    Open,
    Reduce,
    RebalanceReduce,
    ForfeitRecovery,
    Deposit,
    Withdraw,
    CloseResolved,
}

impl UserOperation {
    const ALL: [Self; 7] = [
        Self::Open,
        Self::Reduce,
        Self::RebalanceReduce,
        Self::ForfeitRecovery,
        Self::Deposit,
        Self::Withdraw,
        Self::CloseResolved,
    ];

    fn allowed(self, state: LifecycleState) -> bool {
        match self {
            Self::Open => state == LifecycleState::Active,
            Self::Reduce => state != LifecycleState::Resolved,
            Self::RebalanceReduce => {
                matches!(state, LifecycleState::Active | LifecycleState::DrainOnly)
            }
            Self::ForfeitRecovery => state == LifecycleState::Recovery,
            Self::Deposit | Self::Withdraw => state != LifecycleState::Resolved,
            Self::CloseResolved => state == LifecycleState::Resolved,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    tokens: Vec<(Pubkey, Vec<u8>)>,
    lamports: Vec<(Pubkey, u64)>,
}

fn snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        tokens: env.all_token_account_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn prepare_world(
    state: LifecycleState,
    operation: UserOperation,
) -> Result<(V16Svm, MarketConfig), String> {
    let config = MarketConfig::default();
    let mut env = V16Svm::new([state as u8; 32], config);
    if matches!(
        operation,
        UserOperation::Reduce | UserOperation::RebalanceReduce | UserOperation::ForfeitRecovery
    ) {
        env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, config.initial_price, 0)
            .map_err(|error| format!("prepare matched position: {error}"))?;
    }

    match state {
        LifecycleState::Active => {}
        LifecycleState::DrainOnly => {
            env.drain_only_asset(0, 0)
                .map_err(|error| format!("enter DrainOnly: {error}"))?;
        }
        LifecycleState::Recovery => {
            env.configure_permissionless_resolve(1_000, 100)
                .map_err(|error| format!("configure public Recovery: {error}"))?;
            env.shutdown_asset(0, 1)
                .map_err(|error| format!("enter Recovery: {error}"))?;
        }
        LifecycleState::Resolved => {
            env.resolve_market()
                .map_err(|error| format!("enter Resolved: {error}"))?;
        }
    }

    let group = env.primary_market_state().1;
    match state {
        LifecycleState::Active => {
            if group.mode != MarketModeV16::Live
                || group.assets[0].lifecycle != AssetLifecycleV16::Active
            {
                return Err("public setup missed Live/Active".to_string());
            }
        }
        LifecycleState::DrainOnly => {
            if group.mode != MarketModeV16::Live
                || group.assets[0].lifecycle != AssetLifecycleV16::DrainOnly
            {
                return Err("public setup missed Live/DrainOnly".to_string());
            }
        }
        LifecycleState::Recovery => {
            if group.mode != MarketModeV16::Live
                || group.assets[0].lifecycle != AssetLifecycleV16::Recovery
            {
                return Err("public setup missed Live/Recovery".to_string());
            }
        }
        LifecycleState::Resolved => {
            if group.mode != MarketModeV16::Resolved {
                return Err("public setup missed Resolved".to_string());
            }
        }
    }
    Ok((env, config))
}

fn actor_asset_exposure(env: &V16Svm, actor: usize, asset_index: u32) -> u128 {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .filter(|leg| leg.active != 0 && leg.asset_index.get() == asset_index)
        .map(|leg| leg.basis_pos_q.get().unsigned_abs())
        .sum()
}

fn exercise_cell(state: LifecycleState, operation: UserOperation) -> Result<(), String> {
    let (mut env, config) = prepare_world(state, operation)?;
    let before = snapshot(&env);
    let capital_before = env.primary_portfolio(2).capital.get();
    let destination_before = env.token_amount(env.actors[2].destination_token);
    let source_before = env.token_amount(env.actors[2].source_token);
    let vault_before = env.token_amount(env.vault);
    let oi_before = env.primary_market_state().1.assets[0].oi_eff_long_q;
    let exit_exposure_before = actor_asset_exposure(&env, 0, 0);
    let exit_capital_before = env.primary_portfolio(0).capital.get();
    let exit_pnl_before = env.primary_portfolio(0).pnl.get();

    let result = match operation {
        UserOperation::Open => {
            env.trade_no_cpi(2, 3, 0, POS_SCALE as i128, config.initial_price, 0)
        }
        UserOperation::Reduce => {
            env.trade_no_cpi(0, 1, 0, -(POS_SCALE as i128), config.initial_price, 0)
        }
        UserOperation::RebalanceReduce => env.rebalance_reduce(0, 0, POS_SCALE),
        UserOperation::ForfeitRecovery => env.forfeit_recovery_leg(0, 0, u128::MAX),
        UserOperation::Deposit => env.deposit_primary(2, 1),
        UserOperation::Withdraw => env.withdraw_primary(2, 1),
        UserOperation::CloseResolved => env.close_resolved_primary(2),
    };

    if !operation.allowed(state) {
        if result.is_ok() {
            return Err(format!("forbidden {operation:?} landed in {state:?}"));
        }
        if snapshot(&env) != before {
            return Err(format!(
                "rejected {operation:?} in {state:?} did not roll back exactly"
            ));
        }
        return Ok(());
    }
    result.map_err(|error| format!("allowed {operation:?} rejected in {state:?}: {error}"))?;

    let group = env.primary_market_state().1;
    match operation {
        UserOperation::Open => {
            if oi_before != 0 || group.assets[0].oi_eff_long_q != POS_SCALE {
                return Err(format!(
                    "Live open produced wrong OI: {oi_before}->{}",
                    group.assets[0].oi_eff_long_q
                ));
            }
        }
        UserOperation::Reduce => {
            if oi_before != POS_SCALE || group.assets[0].oi_eff_long_q != 0 {
                return Err(format!(
                    "{state:?} reduction produced wrong OI: {oi_before}->{}",
                    group.assets[0].oi_eff_long_q
                ));
            }
        }
        UserOperation::RebalanceReduce | UserOperation::ForfeitRecovery => {
            let exit_exposure_after = actor_asset_exposure(&env, 0, 0);
            if exit_exposure_before != POS_SCALE || exit_exposure_after >= exit_exposure_before {
                return Err(format!(
                    "{state:?}/{operation:?} did not strictly reduce exposure: \
                     {exit_exposure_before}->{exit_exposure_after}"
                ));
            }
            let asset = env.primary_market_state().1.assets[0];
            if asset.oi_eff_long_q > oi_before || asset.oi_eff_short_q > oi_before {
                return Err(format!(
                    "{state:?}/{operation:?} increased matched OI: long={}, short={}, before={oi_before}",
                    asset.oi_eff_long_q, asset.oi_eff_short_q
                ));
            }
            if env.primary_portfolio(0).capital.get() != exit_capital_before
                || env.primary_portfolio(0).pnl.get() != exit_pnl_before
                || env.all_token_account_data() != before.tokens
            {
                return Err(format!(
                    "{state:?}/{operation:?} moved value in a zero-PnL owner-exit cell"
                ));
            }
        }
        UserOperation::Deposit => {
            if env.primary_portfolio(2).capital.get() != capital_before + 1
                || env.token_amount(env.actors[2].source_token) + 1 != source_before
                || env.token_amount(env.vault) != vault_before + 1
            {
                return Err(format!(
                    "{state:?} deposit did not transfer and account exactly"
                ));
            }
        }
        UserOperation::Withdraw => {
            if env.primary_portfolio(2).capital.get() + 1 != capital_before
                || env.token_amount(env.actors[2].destination_token) != destination_before + 1
                || env.token_amount(env.vault) + 1 != vault_before
            {
                return Err(format!(
                    "{state:?} withdraw did not transfer and account exactly"
                ));
            }
        }
        UserOperation::CloseResolved => {
            let expected =
                u64::try_from(config.actor_deposits[2]).expect("fixture deposit fits SPL amount");
            if env.primary_portfolio(2).capital.get() != 0
                || env.token_amount(env.actors[2].destination_token)
                    != destination_before + expected
                || env.token_amount(env.vault) + expected != vault_before
            {
                return Err(
                    "resolved close did not pay the exact flat-account entitlement".to_string(),
                );
            }
        }
    }
    if env.market_data(true) != before.foreign_market
        || env.foreign_portfolio_data() != before.foreign_portfolio
    {
        return Err(format!(
            "{operation:?} in {state:?} escaped its market scope"
        ));
    }
    Ok(())
}

#[test]
fn v16_program_user_operation_lifecycle_admission_matrix() {
    for state in LifecycleState::ALL {
        for operation in UserOperation::ALL {
            exercise_cell(state, operation)
                .unwrap_or_else(|error| panic!("{state:?}/{operation:?}: {error}"));
        }
    }
}
