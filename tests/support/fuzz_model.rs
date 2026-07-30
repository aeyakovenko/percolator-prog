use super::v16_svm::{
    MarketConfig, TxSuccess, V16Svm, ASSET_COUNT, EXIT_MAKER_INDEX, PRIMARY_ACTOR_COUNT,
    TX_CU_LIMIT, USER_COUNT,
};
use percolator::{BackingBucketStatusV16, MarketModeV16, PortfolioLegV16, POS_SCALE};
use percolator_prog::{
    constants::{ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3, ORACLE_MODE_EWMA_MARK},
    ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint},
};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use solana_sdk::transaction::Transaction;
use std::collections::VecDeque;

const MIN_LIVENESS_DRAIN_LIMIT: usize = 256;
const MAX_LIVENESS_DRAIN_LIMIT: usize = 100_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeRoute {
    NoCpi,
    Cpi,
    BatchNoCpi,
    BatchCpi,
}

impl TradeRoute {
    fn index(self) -> usize {
        match self {
            Self::NoCpi => 0,
            Self::Cpi => 1,
            Self::BatchNoCpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum HintMode {
    Complete,
    Reversed,
    Empty,
    Duplicate,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubstitutionKind {
    ForeignTradePortfolio,
    ForeignDepositVault,
    ForeignWithdrawVault,
    ForeignCrankPortfolio,
    MismatchedMatcherBinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownBlocker {
    LiveLapsedSourceBacking,
    OmittedRescueAccrualLiquidation,
    PostExpiryBackingFee,
    TradeRetryReplay,
    AssetGenerationTradeReplay,
    CpiCallerFeeSiphon,
    CpiBackingFeeSiphon,
    CompositeOracleRounding,
    RoundedFundingOmission,
}

impl KnownBlocker {
    pub const COUNT: usize = 9;

    pub const fn index(self) -> usize {
        match self {
            Self::LiveLapsedSourceBacking => 0,
            Self::OmittedRescueAccrualLiquidation => 1,
            Self::PostExpiryBackingFee => 2,
            Self::TradeRetryReplay => 3,
            Self::AssetGenerationTradeReplay => 4,
            Self::CpiCallerFeeSiphon => 5,
            Self::CpiBackingFeeSiphon => 6,
            Self::CompositeOracleRounding => 7,
            Self::RoundedFundingOmission => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PostExpiryBackingCase {
    pub fee_bps: u16,
    pub expiry_offset: u8,
    pub mark_move_bps: u16,
    pub increase_divisor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostExpiryBackingReproduction {
    pub blocker: KnownBlocker,
    pub victim_capital_loss: u128,
    pub provider_earnings: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OmittedRescueReproduction {
    pub blocker: KnownBlocker,
    pub omitted_position_before_q: u128,
    pub omitted_position_after_q: u128,
    pub omitted_insurance_delta: u128,
    pub complete_position_after_q: u128,
    pub complete_liquidation_deficit: u128,
    pub complete_insurance_delta: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeRetryReplayReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub victim_extra_loss: u64,
    pub attacker_extra_payout: u64,
    pub control_total_payout: u128,
    pub replay_total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub attacker_payout: u64,
    pub total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpiCallerFeeReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub attacker_profit: u64,
    pub lp_loss: u64,
    pub withdrawn_insurance: u128,
    pub total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpiBackingFeeReproduction {
    pub blocker: KnownBlocker,
    pub lp_capital_loss: u128,
    pub provider_earnings: u128,
    pub extracted_tokens: u64,
    pub attacker_capital_delta: i128,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompositeRoundingCase {
    Pr329LargeMove,
    Pr381MicroMove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeRoundingReproduction {
    pub blocker: KnownBlocker,
    pub case: CompositeRoundingCase,
    pub exact_mark: u64,
    pub rounded_target: u64,
    pub rounded_mark: u64,
    pub victim_capital_loss: u128,
    pub oi_reduction_q: u128,
    pub cranker_reward: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundedFundingOmissionReproduction {
    pub blocker: KnownBlocker,
    pub control_f_long_num: i128,
    pub control_f_short_num: i128,
    pub attack_f_long_num: i128,
    pub attack_f_short_num: i128,
    pub victim_payout_loss: u64,
    pub attacker_payout_gain: u64,
}

impl SubstitutionKind {
    const ALL: [Self; 5] = [
        Self::ForeignTradePortfolio,
        Self::ForeignDepositVault,
        Self::ForeignWithdrawVault,
        Self::ForeignCrankPortfolio,
        Self::MismatchedMatcherBinding,
    ];

    fn index(self) -> usize {
        match self {
            Self::ForeignTradePortfolio => 0,
            Self::ForeignDepositVault => 1,
            Self::ForeignWithdrawVault => 2,
            Self::ForeignCrankPortfolio => 3,
            Self::MismatchedMatcherBinding => 4,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Trade {
        route: TradeRoute,
        taker: u8,
        maker: u8,
        asset: u8,
        units: i8,
        fee_bps: u16,
        price_move_bps: i16,
        prefer_reduce: bool,
    },
    ConfigureEwma {
        asset: u8,
        halflife_slots: u16,
        mark_min_fee: u16,
    },
    PushMark {
        asset: u8,
        dt: u8,
        move_bps: i16,
    },
    Crank {
        actor: u8,
        hints: HintMode,
    },
    Withdraw {
        actor: u8,
        amount: u16,
    },
    SyncMaintenanceFee {
        actor: u8,
        dt: u8,
    },
    CrossMarketSubstitution {
        actor: u8,
    },
    AccountSubstitution {
        actor: u8,
        kind: SubstitutionKind,
    },
    RetainTrade {
        taker: u8,
        maker: u8,
        asset: u8,
        units: i8,
    },
    LandRetained,
    AdvanceBlockhash,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmallMarketConfig {
    pub max_price_move_bps_per_slot: u64,
    pub max_accrual_dt_slots: u64,
    pub max_abs_funding_e9_per_slot: u64,
    pub maintenance_fee_per_slot: u128,
}

impl Default for SmallMarketConfig {
    fn default() -> Self {
        let config = MarketConfig::default();
        Self {
            max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
            max_accrual_dt_slots: config.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
            maintenance_fee_per_slot: config.maintenance_fee_per_slot,
        }
    }
}

impl From<SmallMarketConfig> for MarketConfig {
    fn from(config: SmallMarketConfig) -> Self {
        Self {
            max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
            max_accrual_dt_slots: config.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: config.max_accrual_dt_slots,
            maintenance_fee_per_slot: config.maintenance_fee_per_slot,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scenario {
    pub seed: [u8; 32],
    pub config: SmallMarketConfig,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub loaded_program_hash: [u8; 32],
    pub route_success: [u64; 4],
    pub route_reject: [u64; 4],
    pub crank_progress: u64,
    pub mark_updates: u64,
    pub oracle_reconfigs: u64,
    pub maintenance_syncs: u64,
    pub withdrawals: u64,
    pub substitution_rejections: [u64; 5],
    pub retained_landed: u64,
    pub retained_rejected: u64,
    pub user_positions_closed: u64,
    pub liquidation_steps: u64,
    pub liquidated_abs_q: u128,
    pub known_blocker_hits: [u64; KnownBlocker::COUNT],
    pub known_blocker_exit_locks: [u64; KnownBlocker::COUNT],
    pub max_cu: u64,
}

impl Coverage {
    pub fn assert_pull_request_non_vacuity(&self) -> Result<(), String> {
        if self.loaded_program_hash == [0; 32] {
            return Err("production SBF artifact hash was not recorded".into());
        }
        for (index, successes) in self.route_success.iter().copied().enumerate() {
            if successes == 0 {
                return Err(format!(
                    "trade route {index} had no successful public execution"
                ));
            }
        }
        if self.crank_progress == 0 {
            return Err("sole public crank never demonstrated rank-decreasing progress".into());
        }
        for (index, rejections) in self.substitution_rejections.iter().copied().enumerate() {
            if rejections == 0 {
                return Err(format!(
                    "public account-substitution boundary {index} was never rejected"
                ));
            }
        }
        if self.user_positions_closed == 0
            && self.known_blocker_exit_locks.iter().all(|hits| *hits == 0)
        {
            return Err("normal-user exit campaign closed no live position".into());
        }
        if self.liquidation_steps == 0 || self.liquidated_abs_q == 0 {
            return Err(
                "public crank never reduced a currently certified liquidatable position".into(),
            );
        }
        if self.max_cu > TX_CU_LIMIT {
            return Err(format!(
                "successful public instruction exceeded CU limit: {}",
                self.max_cu
            ));
        }
        Ok(())
    }

    fn observe_success(&mut self, route: Option<TradeRoute>, success: &TxSuccess) {
        if let Some(route) = route {
            self.route_success[route.index()] += 1;
        }
        self.max_cu = self.max_cu.max(success.compute_units);
    }

    fn merge(&mut self, other: Self) {
        for index in 0..self.route_success.len() {
            self.route_success[index] += other.route_success[index];
            self.route_reject[index] += other.route_reject[index];
        }
        for index in 0..self.substitution_rejections.len() {
            self.substitution_rejections[index] += other.substitution_rejections[index];
        }
        self.crank_progress += other.crank_progress;
        self.mark_updates += other.mark_updates;
        self.oracle_reconfigs += other.oracle_reconfigs;
        self.maintenance_syncs += other.maintenance_syncs;
        self.withdrawals += other.withdrawals;
        self.retained_landed += other.retained_landed;
        self.retained_rejected += other.retained_rejected;
        self.user_positions_closed += other.user_positions_closed;
        self.liquidation_steps += other.liquidation_steps;
        self.liquidated_abs_q += other.liquidated_abs_q;
        for (target, value) in self
            .known_blocker_hits
            .iter_mut()
            .zip(other.known_blocker_hits)
        {
            *target += value;
        }
        for (target, value) in self
            .known_blocker_exit_locks
            .iter_mut()
            .zip(other.known_blocker_exit_locks)
        {
            *target += value;
        }
        self.max_cu = self.max_cu.max(other.max_cu);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProgressRank {
    market_mark_lag: u128,
    market_locks: u128,
    b_work: u128,
    stale_legs: u128,
    health_work: u128,
}

impl ProgressRank {
    fn actionable(self) -> bool {
        self != Self::default()
    }

    fn account_actionable(self) -> bool {
        self.market_locks != 0 || self.b_work != 0 || self.stale_legs != 0 || self.health_work != 0
    }

    fn key(self) -> (u128, u128, u128, u128, u128) {
        (
            self.market_mark_lag,
            self.market_locks,
            self.b_work,
            self.stale_legs,
            self.health_work,
        )
    }
}

#[derive(Clone)]
struct Snapshot {
    primary_market: Vec<u8>,
    foreign_market: Vec<u8>,
    primary_portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_domain_ledger: Vec<u8>,
    token_accounts: Vec<Vec<u8>>,
    matcher_contexts: Vec<Vec<u8>>,
}

struct RetainedTrade {
    transaction: Transaction,
    taker: usize,
    maker: usize,
    legs: Vec<(usize, i128)>,
}

enum CrankFailure {
    Rejected(String),
    Invariant(String),
}

impl CrankFailure {
    fn into_message(self) -> String {
        match self {
            Self::Rejected(message) | Self::Invariant(message) => message,
        }
    }
}

pub struct ScenarioRunner {
    env: V16Svm,
    positions: [[i128; ASSET_COUNT]; PRIMARY_ACTOR_COUNT],
    protocol_positions: [i128; ASSET_COUNT],
    liveness_limit: usize,
    retained: VecDeque<RetainedTrade>,
    pub coverage: Coverage,
}

impl ScenarioRunner {
    pub fn new(scenario: &Scenario) -> Result<Self, String> {
        let mut out = Self::new_unprefixed(scenario)?;
        out.run_required_prefix()?;
        Ok(out)
    }

    fn new_unprefixed(scenario: &Scenario) -> Result<Self, String> {
        let env = V16Svm::new(scenario.seed, scenario.config.into());
        let loaded_program_hash = env.loaded_program_hash.to_bytes();
        let out = Self {
            env,
            positions: [[0; ASSET_COUNT]; PRIMARY_ACTOR_COUNT],
            protocol_positions: [0; ASSET_COUNT],
            liveness_limit: scenario_liveness_limit(scenario)?,
            retained: VecDeque::new(),
            coverage: Coverage {
                loaded_program_hash,
                ..Coverage::default()
            },
        };
        out.assert_global_invariants()?;
        Ok(out)
    }

    pub fn run_safety_prefix(&mut self, actions: &[Action]) -> Result<(), String> {
        for (index, action) in actions.iter().enumerate() {
            self.apply_action(action)
                .map_err(|error| format!("action {index} {action:?}: {error}"))?;
            self.assert_global_invariants()
                .map_err(|error| format!("after action {index} {action:?}: {error}"))?;
        }
        Ok(())
    }

    pub fn run_permissionless_progress_campaign(&mut self) -> Result<(), String> {
        self.drain_cranks(self.liveness_limit)?;
        self.assert_global_invariants()
    }

    pub fn quarantine_known_progress_blocker(&mut self, error: &str) -> Result<bool, String> {
        let (_, group) = self.env.primary_market_state();
        let lapsed_live_backing = group.mode == MarketModeV16::Live
            && group.source_backing_buckets.iter().any(|bucket| {
                bucket.status == BackingBucketStatusV16::Fresh
                    && bucket.expiry_slot <= group.current_slot
                    && (bucket.fresh_unliened_backing_num != 0
                        || bucket.valid_liened_backing_num != 0
                        || bucket.consumed_liened_backing_num != 0
                        || bucket.impaired_liened_backing_num != 0)
            });
        let stale_rejection =
            error.contains("Custom(19)") || error.contains("custom program error: 0x13");
        if !lapsed_live_backing || !stale_rejection {
            return Ok(false);
        }

        let before = self.snapshot();
        let retry = self.drain_one_progress_step(None);
        self.assert_snapshot_unchanged(&before)?;
        let retry_error = match retry {
            Ok(()) => {
                return Err(
                    "candidate PR 204 blocker progressed on an identical public retry".into(),
                )
            }
            Err(error) => error,
        };
        if !(retry_error.contains("Custom(19)")
            || retry_error.contains("custom program error: 0x13"))
        {
            return Err(format!(
                "candidate PR 204 blocker changed rejection on identical retry: {retry_error}"
            ));
        }
        self.coverage.known_blocker_hits[KnownBlocker::LiveLapsedSourceBacking.index()] += 1;
        Ok(true)
    }

    pub fn run_direct_user_exit_campaign(&mut self) -> Result<(), String> {
        for user in 0..USER_COUNT {
            for asset in 0..ASSET_COUNT {
                if self.positions[user][asset] == 0 {
                    continue;
                }
                if !self.try_normal_exit(user, asset)? {
                    self.run_permissionless_progress_campaign()
                        .map_err(|error| {
                            format!(
                            "normal exit needed public progress, but crank could not converge: \
                             {error}"
                        )
                        })?;
                }
                if !self.try_normal_exit(user, asset)? {
                    let size = self.positions[user][asset];
                    let counterparty = self.normal_exit_counterparty(user, asset, size);
                    let legs = vec![(asset, -size)];
                    return Err(format!(
                        "all public trade routes rejected normal exit for user {user} asset \
                         {asset}; {}",
                        self.trade_diagnostics(user, counterparty, &legs)?
                    ));
                }
                self.coverage.user_positions_closed += 1;
            }
        }
        self.assert_global_invariants()?;
        for actor in 0..PRIMARY_ACTOR_COUNT {
            for asset in 0..ASSET_COUNT {
                if self.positions[actor][asset] != 0 {
                    return Err(format!(
                        "exit campaign stranded actor {actor} asset {asset} position {}",
                        self.positions[actor][asset]
                    ));
                }
            }
        }

        for actor in 0..PRIMARY_ACTOR_COUNT {
            let capital = self.env.primary_portfolio(actor).capital.get();
            if capital == 0 {
                continue;
            }
            let before = self.snapshot();
            let destination_before = self
                .env
                .token_amount(self.env.actors[actor].destination_token);
            let success = self
                .env
                .withdraw_primary(actor, capital)
                .map_err(|error| format!("actor {actor} cannot withdraw {capital}: {error}"))?;
            self.coverage.withdrawals += 1;
            self.coverage.observe_success(None, &success);
            self.assert_portfolio_frame(&before, &[actor])?;
            let destination_after = self
                .env
                .token_amount(self.env.actors[actor].destination_token);
            if destination_after as u128 - destination_before as u128 != capital {
                return Err(format!(
                    "actor {actor} withdrawal destination delta did not equal authorized debit"
                ));
            }
            if self.env.primary_portfolio(actor).capital.get() != 0 {
                return Err(format!(
                    "actor {actor} retained capital after full withdrawal"
                ));
            }
            self.assert_global_invariants()?;
        }
        let foreign_capital = self.env.foreign_market_state().1.c_tot;
        if foreign_capital != 0 {
            let success = self
                .env
                .withdraw_foreign(foreign_capital)
                .map_err(|error| format!("foreign user cannot withdraw: {error}"))?;
            self.coverage.withdrawals += 1;
            self.coverage.observe_success(None, &success);
        }
        self.assert_global_invariants()
    }

    fn try_normal_exit(&mut self, user: usize, asset: usize) -> Result<bool, String> {
        let size = self.positions[user][asset];
        if size == 0 {
            return Ok(true);
        }
        let preferred = match (user * ASSET_COUNT + asset) % 4 {
            0 => TradeRoute::NoCpi,
            1 => TradeRoute::Cpi,
            2 => TradeRoute::BatchNoCpi,
            _ => TradeRoute::BatchCpi,
        };
        let counterparty = self.normal_exit_counterparty(user, asset, size);
        let legs = vec![(asset, -size)];
        for route in [
            preferred,
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ] {
            if self.execute_trade(route, user, counterparty, legs.clone(), 0, 0, false)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn normal_exit_counterparty(&self, user: usize, asset: usize, size: i128) -> usize {
        (0..PRIMARY_ACTOR_COUNT)
            .filter(|candidate| *candidate != user)
            .find(|candidate| {
                let candidate_size = self.positions[*candidate][asset];
                candidate_size != 0
                    && candidate_size.signum() == -size.signum()
                    && candidate_size.unsigned_abs() >= size.unsigned_abs()
            })
            .unwrap_or(EXIT_MAKER_INDEX)
    }

    fn run_required_prefix(&mut self) -> Result<(), String> {
        let q = POS_SCALE as i128;
        self.execute_trade(TradeRoute::NoCpi, 0, 1, vec![(0, q)], 0, 0, true)?;
        self.execute_trade(TradeRoute::Cpi, 2, 3, vec![(0, q)], 0, 0, true)?;
        self.execute_trade(
            TradeRoute::BatchNoCpi,
            0,
            1,
            vec![(0, q / 2), (1, -(q / 2)), (2, q / 4)],
            0,
            0,
            true,
        )?;
        self.execute_trade(
            TradeRoute::BatchCpi,
            2,
            3,
            vec![(0, q / 2), (1, -(q / 2)), (2, q / 4)],
            0,
            0,
            true,
        )?;

        for kind in SubstitutionKind::ALL {
            self.execute_account_substitution(0, kind)?;
        }

        let next_slot = self.env.current_slot() + 1;
        self.env.warp_to_slot(next_slot);
        let (_, group) = self.env.primary_market_state();
        let required_move_bps = group.config.max_price_move_bps_per_slot.clamp(1, 10);
        let required_mark = super::v16_svm::INITIAL_PRICE
            .checked_add(
                super::v16_svm::INITIAL_PRICE
                    .checked_mul(required_move_bps)
                    .ok_or("required mark multiplication overflow")?
                    / 10_000,
            )
            .ok_or("required mark overflow")?;
        let mark_success = self
            .env
            .push_auth_mark(0, next_slot, required_mark)
            .map_err(|error| format!("required mark update failed: {error}"))?;
        self.coverage.mark_updates += 1;
        self.coverage.observe_success(None, &mark_success);
        self.drain_actor(0, self.liveness_limit)?;
        self.assert_global_invariants()
    }

    fn apply_action(&mut self, action: &Action) -> Result<(), String> {
        match *action {
            Action::Trade {
                route,
                taker,
                maker,
                asset,
                units,
                fee_bps,
                price_move_bps,
                prefer_reduce,
            } => {
                let taker = taker as usize % USER_COUNT;
                let mut maker = maker as usize % USER_COUNT;
                if maker == taker {
                    maker = (maker + 1) % USER_COUNT;
                }
                let asset = asset as usize % ASSET_COUNT;
                let existing = self.positions[taker][asset];
                let unit_size = (units as i128).clamp(-3, 3) * POS_SCALE as i128 / 4;
                let size = if prefer_reduce && existing != 0 {
                    -existing
                } else if unit_size == 0 {
                    POS_SCALE as i128 / 4
                } else {
                    unit_size
                };
                let legs = if matches!(route, TradeRoute::BatchNoCpi | TradeRoute::BatchCpi) {
                    let other = (asset + 1) % ASSET_COUNT;
                    let other_size = if prefer_reduce && self.positions[taker][other] != 0 {
                        -self.positions[taker][other]
                    } else {
                        -size
                    };
                    vec![(asset, size), (other, other_size)]
                } else {
                    vec![(asset, size)]
                };
                self.execute_trade(
                    route,
                    taker,
                    maker,
                    legs,
                    fee_bps as u64,
                    price_move_bps,
                    false,
                )
                .map(|_| ())
            }
            Action::ConfigureEwma {
                asset,
                halflife_slots,
                mark_min_fee,
            } => {
                let asset = asset as usize % ASSET_COUNT;
                let before = self.snapshot();
                let mark = self.env.primary_profile(asset).mark_ewma_e6;
                match self.env.configure_ewma_mark(
                    asset as u16,
                    self.env.current_slot(),
                    mark,
                    u64::from(halflife_slots.max(1)),
                    u64::from(mark_min_fee),
                ) {
                    Ok(success) => {
                        self.coverage.oracle_reconfigs += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::PushMark {
                asset,
                dt,
                move_bps,
            } => {
                let asset = asset as usize % ASSET_COUNT;
                let next_slot = self.env.current_slot() + (dt.max(1) as u64).min(4);
                self.env.warp_to_slot(next_slot);
                let old_mark = self.env.primary_profile(asset).mark_ewma_e6;
                let move_bps = move_bps.clamp(-500, 500) as i128;
                let numerator = (old_mark as i128)
                    .checked_mul(10_000 + move_bps)
                    .ok_or("mark multiplication overflow")?;
                let new_mark = u64::try_from((numerator / 10_000).max(1))
                    .map_err(|_| "mark conversion overflow")?;
                let before = self.snapshot();
                let result = if self.env.primary_profile(asset).oracle_mode == ORACLE_MODE_EWMA_MARK
                {
                    self.env.push_ewma_mark(asset as u16, next_slot, new_mark)
                } else {
                    self.env.push_auth_mark(asset as u16, next_slot, new_mark)
                };
                match result {
                    Ok(success) => {
                        self.coverage.mark_updates += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::Crank { actor, hints } => {
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                self.execute_crank(actor, hints, matches!(hints, HintMode::Complete))
                    .map_err(CrankFailure::into_message)
            }
            Action::Withdraw { actor, amount } => {
                let actor = actor as usize % USER_COUNT;
                let amount = amount as u128;
                let before = self.snapshot();
                let capital_before = self.env.primary_portfolio(actor).capital.get();
                let destination_before = self
                    .env
                    .token_amount(self.env.actors[actor].destination_token);
                match self.env.withdraw_primary(actor, amount) {
                    Ok(success) => {
                        self.coverage.withdrawals += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[actor])?;
                        let capital_after = self.env.primary_portfolio(actor).capital.get();
                        let destination_after = self
                            .env
                            .token_amount(self.env.actors[actor].destination_token);
                        if capital_before.checked_sub(amount) != Some(capital_after)
                            || destination_after as u128 != destination_before as u128 + amount
                        {
                            return Err(
                                "withdrawal debit/credit did not match owner authorization".into(),
                            );
                        }
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::SyncMaintenanceFee { actor, dt } => {
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                let next_slot = self.env.current_slot() + u64::from(dt.clamp(1, 4));
                self.env.warp_to_slot(next_slot);
                let before = self.snapshot();
                match self.env.sync_maintenance_fee(actor, next_slot) {
                    Ok(success) => {
                        self.coverage.maintenance_syncs += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[actor])?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::CrossMarketSubstitution { actor } => self.execute_account_substitution(
                actor as usize % USER_COUNT,
                SubstitutionKind::ForeignTradePortfolio,
            ),
            Action::AccountSubstitution { actor, kind } => {
                self.execute_account_substitution(actor as usize % USER_COUNT, kind)
            }
            Action::RetainTrade {
                taker,
                maker,
                asset,
                units,
            } => {
                let taker = taker as usize % USER_COUNT;
                let mut maker = maker as usize % USER_COUNT;
                if maker == taker {
                    maker = (maker + 1) % USER_COUNT;
                }
                let asset = asset as usize % ASSET_COUNT;
                let mut size = (units as i128).clamp(-2, 2) * POS_SCALE as i128 / 4;
                if size == 0 {
                    size = POS_SCALE as i128 / 4;
                }
                let price = self.env.primary_market_state().1.assets[asset].effective_price;
                let transaction =
                    self.env
                        .build_retained_no_cpi_trade(taker, maker, asset as u16, size, price);
                self.retained.push_back(RetainedTrade {
                    transaction,
                    taker,
                    maker,
                    legs: vec![(asset, size)],
                });
                Ok(())
            }
            Action::LandRetained => {
                let Some(retained) = self.retained.pop_front() else {
                    return Ok(());
                };
                let before = self.snapshot();
                match self.env.land_retained(retained.transaction) {
                    Ok(success) => {
                        self.coverage.retained_landed += 1;
                        self.coverage
                            .observe_success(Some(TradeRoute::NoCpi), &success);
                        self.record_trade(retained.taker, retained.maker, &retained.legs)?;
                        self.assert_portfolio_frame(&before, &[retained.taker, retained.maker])?;
                    }
                    Err(_) => {
                        self.coverage.retained_rejected += 1;
                        self.assert_snapshot_unchanged(&before)?;
                    }
                }
                Ok(())
            }
            Action::AdvanceBlockhash => {
                self.env.expire_blockhash();
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_trade(
        &mut self,
        route: TradeRoute,
        taker: usize,
        maker: usize,
        legs: Vec<(usize, i128)>,
        fee_bps: u64,
        price_move_bps: i16,
        must_succeed: bool,
    ) -> Result<bool, String> {
        if legs.is_empty() {
            return Ok(false);
        }
        let before = self.snapshot();
        let market = self.env.primary_market_state().1;
        let result = match route {
            TradeRoute::NoCpi => {
                let (asset, size) = legs[0];
                self.env.trade_no_cpi(
                    taker,
                    maker,
                    asset as u16,
                    size,
                    moved_price(market.assets[asset].effective_price, price_move_bps)?,
                    fee_bps,
                )
            }
            TradeRoute::Cpi => {
                let (asset, size) = legs[0];
                self.env
                    .trade_cpi(taker, maker, asset as u16, size, fee_bps, 0)
            }
            TradeRoute::BatchNoCpi => {
                let encoded = legs
                    .iter()
                    .map(|(asset, size)| {
                        Ok(BatchTradeLeg {
                            asset_index: *asset as u16,
                            size_q: *size,
                            exec_price: moved_price(
                                market.assets[*asset].effective_price,
                                price_move_bps,
                            )?,
                            fee_bps,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                self.env.batch_trade_no_cpi(taker, maker, encoded)
            }
            TradeRoute::BatchCpi => {
                let encoded = legs
                    .iter()
                    .map(|(asset, size)| BatchTradeCpiLeg {
                        asset_index: *asset as u16,
                        size_q: *size,
                        fee_bps,
                        limit_price: 0,
                    })
                    .collect();
                self.env.batch_trade_cpi(taker, maker, encoded)
            }
        };
        match result {
            Ok(success) => {
                self.coverage.observe_success(Some(route), &success);
                self.record_trade(taker, maker, &legs)?;
                self.assert_portfolio_frame(&before, &[taker, maker])?;
                Ok(true)
            }
            Err(error) => {
                self.coverage.route_reject[route.index()] += 1;
                self.assert_snapshot_unchanged(&before)?;
                if must_succeed {
                    Err(format!(
                        "valid risk-reducing {route:?} failed for taker {taker}, maker {maker}: \
                         {error}; {}",
                        self.trade_diagnostics(taker, maker, &legs)?
                    ))
                } else {
                    Ok(false)
                }
            }
        }
    }

    fn execute_account_substitution(
        &mut self,
        actor: usize,
        kind: SubstitutionKind,
    ) -> Result<(), String> {
        let before = self.snapshot();
        let result = match kind {
            SubstitutionKind::ForeignTradePortfolio => self
                .env
                .cross_market_trade_substitution(actor, POS_SCALE as i128 / 4),
            SubstitutionKind::ForeignDepositVault => {
                self.env.cross_market_deposit_vault_substitution(actor, 1)
            }
            SubstitutionKind::ForeignWithdrawVault => {
                self.env.cross_market_withdraw_vault_substitution(actor, 1)
            }
            SubstitutionKind::ForeignCrankPortfolio => self
                .env
                .cross_market_crank_portfolio_substitution(self.env.current_slot()),
            SubstitutionKind::MismatchedMatcherBinding => {
                let maker = (actor + 1) % USER_COUNT;
                let substituted_binding = (maker + 1) % USER_COUNT;
                self.env
                    .cpi_matcher_binding_substitution(actor, maker, substituted_binding)
            }
        };
        match result {
            Ok(_) => Err(format!(
                "public account substitution {kind:?} unexpectedly succeeded"
            )),
            Err(_) => {
                self.coverage.substitution_rejections[kind.index()] += 1;
                self.assert_snapshot_unchanged(&before)
            }
        }
    }

    fn record_trade(
        &mut self,
        taker: usize,
        maker: usize,
        legs: &[(usize, i128)],
    ) -> Result<(), String> {
        for &(asset, size) in legs {
            self.positions[taker][asset] = self.positions[taker][asset]
                .checked_add(size)
                .ok_or("taker ghost position overflow")?;
            self.positions[maker][asset] = self.positions[maker][asset]
                .checked_sub(size)
                .ok_or("maker ghost position overflow")?;
        }
        self.assert_positions_match()
    }

    fn execute_crank(
        &mut self,
        actor: usize,
        hints: HintMode,
        require_progress: bool,
    ) -> Result<(), CrankFailure> {
        let before = self.snapshot();
        let rank_before = self.progress_rank(actor).map_err(CrankFailure::Invariant)?;
        let liquidation_authorized = self.current_liquidation_authorization(actor);
        let observations = match hints {
            HintMode::Complete => self
                .selected_observation(actor)
                .map_err(CrankFailure::Invariant)?,
            HintMode::Reversed => all_observations().into_iter().rev().collect(),
            HintMode::Empty => vec![],
            HintMode::Duplicate => vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        };
        match self.env.crank(actor, self.env.current_slot(), observations) {
            Ok(success) => {
                self.coverage.observe_success(None, &success);
                self.assert_portfolio_frame(&before, &[actor])
                    .map_err(CrankFailure::Invariant)?;
                self.record_permissionless_position_changes(actor, liquidation_authorized)
                    .map_err(CrankFailure::Invariant)?;
                let rank_after = self.progress_rank(actor).map_err(CrankFailure::Invariant)?;
                if require_progress
                    && rank_before.actionable()
                    && rank_after.key() >= rank_before.key()
                {
                    return Err(CrankFailure::Invariant(format!(
                        "sole public crank succeeded without rank decrease: {rank_before:?} -> {rank_after:?}"
                    )));
                }
                if rank_after.key() < rank_before.key() {
                    self.coverage.crank_progress += 1;
                }
                Ok(())
            }
            Err(error) => {
                self.assert_snapshot_unchanged(&before)
                    .map_err(CrankFailure::Invariant)?;
                if require_progress && rank_before.actionable() {
                    Err(CrankFailure::Rejected(format!(
                        "sole public crank rejected actionable rank {rank_before:?}: {error}"
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn selected_observation(&self, actor: usize) -> Result<Vec<CrankObservationHint>, String> {
        let (_, group) = self.env.primary_market_state();
        let rank = self.progress_rank(actor)?;
        if rank.b_work != 0 || rank.market_locks != 0 {
            return Ok(vec![]);
        }
        let active_asset = decoded_legs(&self.env.primary_portfolio(actor))
            .into_iter()
            .find(|leg| leg.active)
            .map(|leg| leg.asset_index as usize);
        let pending_asset =
            (0..ASSET_COUNT).find(|asset| group.assets[*asset].slot_last < self.env.current_slot());
        let asset = if rank.stale_legs != 0 || rank.health_work != 0 {
            active_asset
        } else {
            pending_asset.or(active_asset)
        }
        .unwrap_or(0);
        if asset >= ASSET_COUNT {
            return Err(format!(
                "selected crank observation references out-of-world asset {asset}"
            ));
        }
        Ok(vec![CrankObservationHint {
            asset_index: asset as u16,
            oracle_accounts: self.env.primary_profile(asset).oracle_leg_count,
        }])
    }

    fn current_liquidation_authorization(&self, actor: usize) -> bool {
        let account = self.env.primary_portfolio(actor);
        let (_, group) = self.env.primary_market_state();
        let Ok(cert) = account.health_cert.try_to_runtime() else {
            return false;
        };
        cert.valid
            && cert.certified_liq_deficit != 0
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == account.active_bitmap.map(|word| word.get())
    }

    fn record_permissionless_position_changes(
        &mut self,
        actor: usize,
        liquidation_authorized: bool,
    ) -> Result<(), String> {
        let observed = observed_positions(&self.env.primary_portfolio(actor))?;
        for (asset, new) in observed.into_iter().enumerate() {
            let old = self.positions[actor][asset];
            if old == new {
                continue;
            }
            if !liquidation_authorized {
                return Err(format!(
                    "permissionless crank changed actor {actor} asset {asset} position \
                     without a current liquidation certificate: {old} -> {new}"
                ));
            }
            let same_side_or_flat = (old > 0 && new >= 0) || (old < 0 && new <= 0);
            if old == 0 || !same_side_or_flat || new.unsigned_abs() >= old.unsigned_abs() {
                return Err(format!(
                    "liquidation did not strictly reduce same-side risk for actor {actor} \
                     asset {asset}: {old} -> {new}"
                ));
            }
            let delta = new
                .checked_sub(old)
                .ok_or("liquidation position delta overflow")?;
            self.protocol_positions[asset] = self.protocol_positions[asset]
                .checked_sub(delta)
                .ok_or("protocol liquidation position overflow")?;
            self.positions[actor][asset] = new;
            let reduced = old.unsigned_abs() - new.unsigned_abs();
            self.coverage.liquidation_steps += 1;
            self.coverage.liquidated_abs_q = self
                .coverage
                .liquidated_abs_q
                .checked_add(reduced)
                .ok_or("liquidation coverage overflow")?;
        }
        Ok(())
    }

    fn drain_actor(&mut self, actor: usize, limit: usize) -> Result<(), String> {
        for _ in 0..limit {
            self.advance_liveness_clock_if_needed()?;
            if !self.progress_rank(actor)?.actionable() {
                return Ok(());
            }
            self.drain_one_progress_step(Some(actor))?;
        }
        let rank = self.progress_rank(actor)?;
        if rank.actionable() {
            return Err(format!(
                "actor {actor} did not converge in {limit} cranks; rank {rank:?}"
            ));
        }
        Ok(())
    }

    fn drain_cranks(&mut self, limit: usize) -> Result<(), String> {
        for _ in 0..limit {
            self.advance_liveness_clock_if_needed()?;
            if !(0..PRIMARY_ACTOR_COUNT)
                .map(|actor| self.progress_rank(actor))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(ProgressRank::actionable)
            {
                return Ok(());
            }
            self.drain_one_progress_step(None)?;
        }
        Err(format!(
            "permissionless drain exceeded deterministic bound {limit}"
        ))
    }

    fn drain_one_progress_step(&mut self, preferred: Option<usize>) -> Result<(), String> {
        let mut candidates = Vec::with_capacity(PRIMARY_ACTOR_COUNT);
        if let Some(actor) = preferred {
            candidates.push(actor);
        }
        candidates.extend((0..PRIMARY_ACTOR_COUNT).filter(|actor| Some(*actor) != preferred));

        let ranks = candidates
            .iter()
            .map(|actor| Ok((*actor, self.progress_rank(*actor)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let has_account_work = ranks.iter().any(|(_, rank)| rank.account_actionable());
        let mut failures = Vec::new();
        for (actor, before) in ranks {
            if !before.actionable() || (has_account_work && !before.account_actionable()) {
                continue;
            }
            match self.execute_crank(actor, HintMode::Complete, true) {
                Ok(()) => return Ok(()),
                Err(CrankFailure::Rejected(error)) => {
                    failures.push(format!("actor {actor} rank {before:?}: {error}"))
                }
                Err(CrankFailure::Invariant(error)) => return Err(error),
            }
        }
        Err(format!(
            "all independently actionable public crank candidates rejected or failed to progress: \
             {}; {}",
            failures.join(" | "),
            self.liveness_diagnostics()
        ))
    }

    fn liveness_diagnostics(&self) -> String {
        let (_, group) = self.env.primary_market_state();
        let assets: Vec<_> = group
            .assets
            .iter()
            .take(ASSET_COUNT)
            .enumerate()
            .map(|(index, asset)| {
                (
                    index,
                    asset.slot_last,
                    asset.effective_price,
                    asset.raw_oracle_target_price,
                    asset.k_long,
                    asset.k_short,
                    asset.f_long_num,
                    asset.f_short_num,
                )
            })
            .collect();
        let accounts: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| {
                let account = self.env.primary_portfolio(actor);
                (
                    actor,
                    account.health_cert.try_to_runtime().ok(),
                    decoded_legs(&account),
                )
            })
            .collect();
        format!(
            "liveness_state={{clock:{}, market_current:{}, market_slot:{}, epochs:[{},{},{},{}], \
             locks:[{},{},{}], assets:{assets:?}, accounts:{accounts:?}}}",
            self.env.current_slot(),
            group.current_slot,
            group.slot_last,
            group.oracle_epoch,
            group.funding_epoch,
            group.risk_epoch,
            group.asset_set_epoch,
            group.bankruptcy_hlock_active,
            group.threshold_stress_active,
            group.loss_stale_active,
        )
    }

    fn advance_liveness_clock_if_needed(&mut self) -> Result<(), String> {
        let has_account_work = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| self.progress_rank(actor))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(ProgressRank::account_actionable);
        if has_account_work {
            return Ok(());
        }
        let (_, group) = self.env.primary_market_state();
        let needs_time = group.assets.iter().take(ASSET_COUNT).any(|asset| {
            asset.raw_oracle_target_price != asset.effective_price
                && self.env.current_slot() <= asset.slot_last
        });
        if needs_time {
            self.env
                .warp_to_slot(self.env.current_slot() + group.config.max_accrual_dt_slots.max(1));
        }
        Ok(())
    }

    fn progress_rank(&self, actor: usize) -> Result<ProgressRank, String> {
        let account = self.env.primary_portfolio(actor);
        let (_, group) = self.env.primary_market_state();
        let mut market_mark_lag = 0u128;
        for asset in 0..ASSET_COUNT {
            let profile = self.env.primary_profile(asset);
            let engine_asset = &group.assets[asset];
            market_mark_lag = market_mark_lag
                .checked_add(
                    profile
                        .mark_ewma_last_slot
                        .saturating_sub(engine_asset.slot_last) as u128,
                )
                .ok_or("mark-lag rank overflow")?;
            market_mark_lag = market_mark_lag
                .checked_add(
                    self.env
                        .current_slot()
                        .saturating_sub(engine_asset.slot_last) as u128,
                )
                .ok_or("per-asset accrual-lag rank overflow")?;
            let engine_gap = engine_asset
                .raw_oracle_target_price
                .abs_diff(engine_asset.effective_price);
            let authenticated_gap = profile.mark_ewma_e6.abs_diff(engine_asset.effective_price);
            market_mark_lag = market_mark_lag
                .checked_add(engine_gap.max(authenticated_gap) as u128)
                .ok_or("mark-price rank overflow")?;
        }
        let mut b_work = 0u128;
        let mut stale_legs = 0u128;
        let mut active = 0u128;
        for leg in decoded_legs(&account) {
            if !leg.active {
                continue;
            }
            active += 1;
            b_work = b_work
                .checked_add(leg.b_rem)
                .ok_or("B-work rank overflow")?;
            if leg.b_stale {
                b_work = b_work.checked_add(1).ok_or("B-work flag overflow")?;
            }
            if leg.stale {
                stale_legs += 1;
            }
        }
        if account.b_stale_state != 0 {
            b_work = b_work
                .checked_add(1)
                .ok_or("portfolio B-stale rank overflow")?;
        }
        if account.stale_state != 0 {
            stale_legs = stale_legs
                .checked_add(1)
                .ok_or("portfolio stale rank overflow")?;
        }
        let cert = &account.health_cert;
        let cert_epoch_mismatch = cert.cert_oracle_epoch.get() != group.oracle_epoch
            || cert.cert_funding_epoch.get() != group.funding_epoch
            || cert.cert_risk_epoch.get() != group.risk_epoch
            || cert.cert_asset_set_epoch.get() != group.asset_set_epoch
            || cert.active_bitmap_at_cert != account.active_bitmap;
        let active_abs_q = decoded_legs(&account)
            .into_iter()
            .filter(|leg| leg.active)
            .try_fold(0u128, |sum, leg| {
                sum.checked_add(leg.basis_pos_q.unsigned_abs())
                    .ok_or("active-position rank overflow")
            })?;
        let health_work = if active == 0 {
            0
        } else if cert.valid == 0 || cert_epoch_mismatch {
            active_abs_q
                .checked_add(1u128 << 120)
                .ok_or("invalid-health rank overflow")?
        } else if cert.certified_liq_deficit.get() != 0 {
            active_abs_q
        } else {
            0
        };
        Ok(ProgressRank {
            market_mark_lag,
            market_locks: u128::from(group.bankruptcy_hlock_active)
                + u128::from(group.threshold_stress_active),
            b_work,
            stale_legs,
            health_work,
        })
    }

    fn assert_global_invariants(&self) -> Result<(), String> {
        if self.env.token_supply_observed() != self.env.initial_token_supply {
            return Err(format!(
                "SPL custody changed total supply: observed {}, expected {}",
                self.env.token_supply_observed(),
                self.env.initial_token_supply
            ));
        }
        let (_, primary) = self.env.primary_market_state();
        let (_, foreign) = self.env.foreign_market_state();
        if primary.vault != self.env.token_amount(self.env.vault) as u128 {
            return Err("primary engine vault diverged from SPL vault".into());
        }
        if foreign.vault != self.env.token_amount(self.env.foreign_vault) as u128 {
            return Err("foreign engine vault diverged from SPL vault".into());
        }
        let primary_portfolios: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| self.env.primary_portfolio(actor))
            .collect();
        let primary_capital: u128 = primary_portfolios
            .iter()
            .map(|portfolio| portfolio.capital.get())
            .sum();
        if primary_capital != primary.c_tot {
            return Err(format!(
                "primary c_tot {} != independent portfolio sum {}",
                primary.c_tot, primary_capital
            ));
        }
        let foreign_capital = self.env.foreign_portfolio().capital.get();
        if foreign_capital != foreign.c_tot {
            return Err(format!(
                "foreign c_tot {} != independent portfolio sum {}",
                foreign.c_tot, foreign_capital
            ));
        }
        let primary_senior = primary
            .c_tot
            .checked_add(primary.insurance)
            .ok_or("primary senior-value overflow")?;
        if primary.vault < primary_senior {
            return Err(format!(
                "unbacked primary value: vault {}, capital {}, insurance {}",
                primary.vault, primary.c_tot, primary.insurance
            ));
        }
        let foreign_senior = foreign
            .c_tot
            .checked_add(foreign.insurance)
            .ok_or("foreign senior-value overflow")?;
        if foreign.vault < foreign_senior {
            return Err("unbacked foreign value".into());
        }
        self.assert_positions_match()
    }

    fn assert_positions_match(&self) -> Result<(), String> {
        let mut observed = [[0i128; ASSET_COUNT]; PRIMARY_ACTOR_COUNT];
        for (actor, row) in observed.iter_mut().enumerate() {
            for leg in decoded_legs(&self.env.primary_portfolio(actor)) {
                if leg.active {
                    let asset = leg.asset_index as usize;
                    if asset >= ASSET_COUNT {
                        return Err(format!("actor {actor} has out-of-world asset {asset}"));
                    }
                    row[asset] = row[asset]
                        .checked_add(leg.basis_pos_q)
                        .ok_or("observed position overflow")?;
                }
            }
        }
        if observed != self.positions {
            return Err(format!(
                "public position deltas diverged from ghost model\nobserved={observed:?}\nghost={:?}",
                self.positions
            ));
        }
        for asset in 0..ASSET_COUNT {
            let user_net: i128 = observed.iter().map(|positions| positions[asset]).sum();
            let net = user_net
                .checked_add(self.protocol_positions[asset])
                .ok_or("user/protocol position sum overflow")?;
            if net != 0 {
                return Err(format!(
                    "asset {asset} position attribution diverged: users={user_net}, \
                     protocol={}, net={net}",
                    self.protocol_positions[asset]
                ));
            }
        }
        Ok(())
    }

    fn trade_diagnostics(
        &self,
        taker: usize,
        maker: usize,
        legs: &[(usize, i128)],
    ) -> Result<String, String> {
        let (_, group) = self.env.primary_market_state();
        let taker_account = self.env.primary_portfolio(taker);
        let maker_account = self.env.primary_portfolio(maker);
        let assets: Vec<_> = legs
            .iter()
            .map(|(asset, _)| {
                let state = &group.assets[*asset];
                (
                    *asset,
                    state.lifecycle,
                    state.mode_long,
                    state.mode_short,
                    state.raw_oracle_target_price,
                    state.effective_price,
                    state.slot_last,
                )
            })
            .collect();
        Ok(format!(
            "market={{mode:{:?}, recovery:{:?}, slot:{}, current:{}, oracle_epoch:{}, \
             funding_epoch:{}, risk_epoch:{}, locks:[{},{},{}]}} assets={assets:?} \
             taker={{rank:{:?}, stale:{}, b_stale:{}, rebalance_lock:{}, liquidation_lock:{}, \
             cert:{:?}}} maker={{rank:{:?}, stale:{}, b_stale:{}, rebalance_lock:{}, \
             liquidation_lock:{}, cert:{:?}}}",
            group.mode,
            group.recovery_reason,
            group.slot_last,
            group.current_slot,
            group.oracle_epoch,
            group.funding_epoch,
            group.risk_epoch,
            group.bankruptcy_hlock_active,
            group.threshold_stress_active,
            group.loss_stale_active,
            self.progress_rank(taker)?,
            taker_account.stale_state,
            taker_account.b_stale_state,
            taker_account.rebalance_lock,
            taker_account.liquidation_lock,
            taker_account.health_cert.try_to_runtime().ok(),
            self.progress_rank(maker)?,
            maker_account.stale_state,
            maker_account.b_stale_state,
            maker_account.rebalance_lock,
            maker_account.liquidation_lock,
            maker_account.health_cert.try_to_runtime().ok(),
        ))
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            primary_market: self.env.market_data(false),
            foreign_market: self.env.market_data(true),
            primary_portfolios: self.env.all_primary_portfolio_data(),
            foreign_portfolio: self.env.foreign_portfolio_data(),
            backing_domain_ledger: self.env.backing_domain_ledger_data(),
            token_accounts: self.env.all_token_account_data(),
            matcher_contexts: self.env.all_matcher_context_data(),
        }
    }

    fn assert_portfolio_frame(
        &self,
        before: &Snapshot,
        mutable_actors: &[usize],
    ) -> Result<(), String> {
        for actor in 0..PRIMARY_ACTOR_COUNT {
            if mutable_actors.contains(&actor) {
                continue;
            }
            let after = self.env.primary_portfolio_data(actor);
            if before.primary_portfolios[actor] != after {
                return Err(format!(
                    "action mutated unrelated primary portfolio {actor}"
                ));
            }
        }
        if before.foreign_portfolio != self.env.foreign_portfolio_data() {
            return Err("primary action mutated foreign portfolio".into());
        }
        if before.foreign_market != self.env.market_data(true) {
            return Err("primary action mutated foreign market".into());
        }
        Ok(())
    }

    fn assert_snapshot_unchanged(&self, before: &Snapshot) -> Result<(), String> {
        if before.primary_market != self.env.market_data(false)
            || before.foreign_market != self.env.market_data(true)
            || before.primary_portfolios != self.env.all_primary_portfolio_data()
            || before.foreign_portfolio != self.env.foreign_portfolio_data()
            || before.backing_domain_ledger != self.env.backing_domain_ledger_data()
            || before.token_accounts != self.env.all_token_account_data()
            || before.matcher_contexts != self.env.all_matcher_context_data()
        {
            return Err("rejected substituted transaction changed economic state".into());
        }
        Ok(())
    }
}

pub fn run_scenario(scenario: &Scenario) -> Result<Coverage, String> {
    let mut progress_runner = ScenarioRunner::new(scenario)?;
    progress_runner.run_safety_prefix(&scenario.actions)?;
    if let Err(error) = progress_runner.run_permissionless_progress_campaign() {
        if !progress_runner.quarantine_known_progress_blocker(&error)? {
            return Err(error);
        }
    }

    let mut exit_runner = ScenarioRunner::new(scenario)?;
    exit_runner.run_safety_prefix(&scenario.actions)?;
    if let Err(error) = exit_runner.run_direct_user_exit_campaign() {
        let is_failed_exit = error.starts_with("normal exit needed public progress");
        if !is_failed_exit || !exit_runner.quarantine_known_progress_blocker(&error)? {
            return Err(error);
        }
        exit_runner.coverage.known_blocker_exit_locks
            [KnownBlocker::LiveLapsedSourceBacking.index()] += 1;
    }
    progress_runner.coverage.merge(exit_runner.coverage);

    let liquidation_coverage = run_liquidation_exit_probe(scenario.seed)?;
    progress_runner.coverage.merge(liquidation_coverage);
    progress_runner.coverage.assert_pull_request_non_vacuity()?;
    Ok(progress_runner.coverage)
}

fn scenario_liveness_limit(scenario: &Scenario) -> Result<usize, String> {
    let cap_bps_per_step = scenario
        .config
        .max_price_move_bps_per_slot
        .checked_mul(scenario.config.max_accrual_dt_slots.max(1))
        .ok_or("liveness cap multiplication overflow")?
        .max(1);
    let mut mark_steps = div_ceil_u64(100, cap_bps_per_step);
    let mut authenticated_dt = 1u64;
    for action in &scenario.actions {
        match action {
            Action::PushMark { dt, move_bps, .. } => {
                mark_steps = mark_steps
                    .checked_add(div_ceil_u64(
                        u64::from(move_bps.unsigned_abs().min(500)),
                        cap_bps_per_step,
                    ))
                    .ok_or("liveness mark-step bound overflow")?;
                authenticated_dt = authenticated_dt
                    .checked_add(u64::from((*dt).clamp(1, 4)))
                    .ok_or("liveness clock bound overflow")?;
            }
            Action::SyncMaintenanceFee { dt, .. } => {
                authenticated_dt = authenticated_dt
                    .checked_add(u64::from((*dt).clamp(1, 4)))
                    .ok_or("liveness fee-clock bound overflow")?;
            }
            _ => {}
        }
    }
    let slot_steps = div_ceil_u64(
        authenticated_dt,
        scenario.config.max_accrual_dt_slots.max(1),
    )
    .checked_mul(ASSET_COUNT as u64)
    .ok_or("liveness slot-step bound overflow")?;
    let account_steps = (PRIMARY_ACTOR_COUNT as u64)
        .checked_mul(ASSET_COUNT as u64)
        .and_then(|value| value.checked_mul(16))
        .ok_or("liveness account-step bound overflow")?;
    let derived = mark_steps
        .checked_mul(2)
        .and_then(|value| value.checked_add(slot_steps))
        .and_then(|value| value.checked_add(account_steps))
        .and_then(|value| value.checked_add(64))
        .ok_or("liveness total bound overflow")?;
    let derived =
        usize::try_from(derived).map_err(|_| "liveness bound does not fit usize".to_string())?;
    let limit = derived.max(MIN_LIVENESS_DRAIN_LIMIT);
    if limit > MAX_LIVENESS_DRAIN_LIMIT {
        return Err(format!(
            "derived liveness bound {limit} exceeds harness guard {MAX_LIVENESS_DRAIN_LIMIT}"
        ));
    }
    Ok(limit)
}

fn div_ceil_u64(value: u64, divisor: u64) -> u64 {
    value.div_ceil(divisor)
}

fn run_liquidation_exit_probe(mut seed: [u8; 32]) -> Result<Coverage, String> {
    seed[0] ^= 0xa5;
    let scenario = Scenario {
        seed,
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 0,
        },
        actions: vec![],
    };
    let mut runner = ScenarioRunner::new_unprefixed(&scenario)?;
    let open_q = 80i128
        .checked_mul(POS_SCALE as i128)
        .ok_or("liquidation probe size overflow")?;
    runner.execute_trade(
        TradeRoute::NoCpi,
        0,
        EXIT_MAKER_INDEX,
        vec![(0, -open_q)],
        0,
        0,
        true,
    )?;

    let mark_slot = runner.env.current_slot() + 1;
    runner.env.warp_to_slot(mark_slot);
    let adverse_mark = super::v16_svm::INITIAL_PRICE
        .checked_mul(2)
        .ok_or("liquidation mark overflow")?;
    let mark = runner
        .env
        .push_auth_mark(0, mark_slot, adverse_mark)
        .map_err(|error| format!("liquidation probe mark rejected: {error}"))?;
    runner.coverage.mark_updates += 1;
    runner.coverage.observe_success(None, &mark);
    runner.drain_actor(0, runner.liveness_limit)?;
    if runner.coverage.liquidation_steps == 0 {
        return Err("adverse-price probe reached no public liquidation step".into());
    }

    let remaining = runner.positions[0][0];
    if remaining != 0 {
        runner.execute_trade(
            TradeRoute::NoCpi,
            0,
            EXIT_MAKER_INDEX,
            vec![(0, -remaining)],
            0,
            0,
            true,
        )?;
        runner.coverage.user_positions_closed += 1;
    }
    runner.drain_actor(0, runner.liveness_limit)?;
    if runner.positions[0] != [0; ASSET_COUNT] {
        return Err(format!(
            "liquidated user retained positions after public risk reduction: {:?}",
            runner.positions[0]
        ));
    }

    let capital = runner.env.primary_portfolio(0).capital.get();
    let destination_before = runner
        .env
        .token_amount(runner.env.actors[0].destination_token);
    let withdrawal = runner
        .env
        .withdraw_primary(0, capital)
        .map_err(|error| format!("liquidated user cannot withdraw remaining capital: {error}"))?;
    runner.coverage.withdrawals += 1;
    runner.coverage.observe_success(None, &withdrawal);
    let destination_after = runner
        .env
        .token_amount(runner.env.actors[0].destination_token);
    if destination_after as u128 != destination_before as u128 + capital {
        return Err("liquidated user's owner withdrawal was not credited exactly".into());
    }
    runner.assert_global_invariants()?;
    Ok(runner.coverage)
}

pub fn reproduce_post_expiry_backing_fee(
    mut seed: [u8; 32],
    case: PostExpiryBackingCase,
) -> Result<PostExpiryBackingReproduction, String> {
    seed[0] ^= 0x67;
    let price = 100u64;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 5_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                52_501,
                1_000_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let fee_bps = case.fee_bps.clamp(1, 10_000);
    let expiry_offset = u64::from(case.expiry_offset.clamp(2, 8));
    let mark_move_bps = u64::from(case.mark_move_bps.clamp(100, 1_000));
    let increase_divisor = i128::from(case.increase_divisor.clamp(10, 100));
    let domain = 1u16;
    let bucket_amount = 100_000u128;
    let open_q = 1_000 * POS_SCALE as i128;
    let increase_q = open_q
        .checked_div(increase_divisor)
        .ok_or("post-expiry increase divisor is zero")?;

    env.configure_auth_mark(false, 0, env.current_slot(), price)
        .map_err(|error| format!("configure low-price AuthMark: {error}"))?;
    env.update_backing_fee_policy(domain, fee_bps, 0)
        .map_err(|error| format!("configure backing fee policy: {error}"))?;
    let expiry_slot = env
        .current_slot()
        .checked_add(expiry_offset)
        .ok_or("post-expiry slot overflow")?;
    env.top_up_backing_bucket(domain, bucket_amount, expiry_slot)
        .map_err(|error| format!("top up backing bucket: {error}"))?;
    env.trade_no_cpi(0, 1, 0, open_q, price, 0)
        .map_err(|error| format!("open source-backed position: {error}"))?;

    let mark_slot = env
        .current_slot()
        .checked_add(1)
        .ok_or("post-expiry mark slot overflow")?;
    env.warp_to_slot(mark_slot);
    let winning_mark = price
        .checked_mul(
            10_000u64
                .checked_add(mark_move_bps)
                .ok_or("post-expiry mark bps overflow")?,
        )
        .and_then(|value| value.checked_div(10_000))
        .ok_or("post-expiry winning mark overflow")?;
    env.push_auth_mark(0, mark_slot, winning_mark)
        .map_err(|error| format!("push winning mark: {error}"))?;
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    env.crank(1, mark_slot, observation())
        .map_err(|error| format!("refresh counterparty at winning mark: {error}"))?;
    env.crank(0, mark_slot, observation())
        .map_err(|error| format!("refresh trader at winning mark: {error}"))?;

    let (_, before_group) = env.primary_market_state();
    let before_bucket = before_group.source_backing_buckets[domain as usize];
    if before_bucket.status != BackingBucketStatusV16::Fresh
        || before_bucket.expiry_slot != expiry_slot
    {
        return Err("backing bucket was not Fresh immediately before retained trade".into());
    }
    let capital_before = env.primary_portfolio(0).capital.get();
    let provider_before = env.token_amount(env.provider_destination_token);
    let supply_before = env.token_supply_observed();
    let retained = env.build_retained_no_cpi_trade(0, 1, 0, increase_q, winning_mark);

    env.warp_to_slot(
        expiry_slot
            .checked_add(1)
            .ok_or("post-expiry landing slot overflow")?,
    );
    if env.current_slot() <= expiry_slot {
        return Err("authenticated Clock did not pass backing expiry".into());
    }
    env.land_retained(retained)
        .map_err(|error| format!("retained post-expiry trade no longer lands: {error}"))?;

    let (_, after_group) = env.primary_market_state();
    if after_group.current_slot >= env.current_slot() {
        return Err(
            "post-expiry reproducer did not preserve the authenticated-Clock/engine-slot gap"
                .into(),
        );
    }
    let after_bucket = after_group.source_backing_buckets[domain as usize];
    let provider_earnings = after_bucket
        .utilization_fee_earnings
        .checked_sub(before_bucket.utilization_fee_earnings)
        .ok_or("post-expiry provider earnings decreased")?;
    let capital_after = env.primary_portfolio(0).capital.get();
    let victim_capital_loss = capital_before
        .checked_sub(capital_after)
        .ok_or("post-expiry trade increased victim capital")?;
    if provider_earnings == 0 || victim_capital_loss == 0 {
        return Err(format!(
            "post-expiry trade created no extractable loss: capital {capital_before}->{capital_after}, earnings {}->{}",
            before_bucket.utilization_fee_earnings, after_bucket.utilization_fee_earnings
        ));
    }

    env.withdraw_backing_bucket_earnings(domain, provider_earnings)
        .map_err(|error| format!("withdraw post-expiry provider earnings: {error}"))?;
    let provider_after = env.token_amount(env.provider_destination_token);
    let extracted_tokens = provider_after
        .checked_sub(provider_before)
        .ok_or("provider destination token balance decreased")?;
    if u128::from(extracted_tokens) != provider_earnings {
        return Err(format!(
            "provider could not extract exact post-expiry earnings: ledger {provider_earnings}, SPL {extracted_tokens}"
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err("post-expiry extraction changed total SPL supply".into());
    }
    Ok(PostExpiryBackingReproduction {
        blocker: KnownBlocker::PostExpiryBackingFee,
        victim_capital_loss,
        provider_earnings,
        extracted_tokens,
    })
}

pub fn reproduce_omitted_rescue_liquidation(
    mut seed: [u8; 32],
) -> Result<OmittedRescueReproduction, String> {
    seed[0] ^= 0x22;
    let (mut omitted, position_before_q, insurance_before) = build_omitted_rescue_world(seed)?;
    omitted
        .crank(0, 3, Vec::new())
        .map_err(|error| format!("omitted-observation stale refresh: {error}"))?;
    let stale = omitted.primary_portfolio(0);
    let stale_cert = stale
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("decode omitted-observation certificate: {error:?}"))?;
    if stale_cert.certified_liq_deficit == 0 {
        return Err("omitted later-leg funding did not create a liquidatable certificate".into());
    }
    let omitted_position_before_q = position_abs_for_asset(&stale, 1)?;
    omitted
        .crank(0, 3, Vec::new())
        .map_err(|error| format!("PR 220 liquidation no longer lands: {error}"))?;
    let omitted_position_after_q = position_abs_for_asset(&omitted.primary_portfolio(0), 1)?;
    let omitted_insurance_after = omitted.primary_market_state().1.insurance;
    let omitted_insurance_delta = omitted_insurance_after
        .checked_sub(insurance_before)
        .ok_or("omitted-observation liquidation decreased insurance")?;
    if omitted_position_after_q >= omitted_position_before_q || omitted_insurance_delta == 0 {
        return Err(format!(
            "omitted observation did not transfer liquidation value: position {omitted_position_before_q}->{omitted_position_after_q}, insurance {insurance_before}->{omitted_insurance_after}"
        ));
    }

    let (mut complete, complete_position_before_q, complete_insurance_before) =
        build_omitted_rescue_world(seed)?;
    let rescue_oracle_accounts = complete.primary_profile(0).oracle_leg_count;
    complete
        .crank(
            2,
            3,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: rescue_oracle_accounts,
            }],
        )
        .map_err(|error| format!("complete-world rescue observation: {error}"))?;
    if complete.primary_market_state().1.assets[0].f_long_num == 0 {
        return Err("complete-world rescue observation booked no long funding".into());
    }
    complete
        .crank(1, 3, Vec::new())
        .map_err(|error| format!("complete-world counterparty refresh: {error}"))?;
    complete
        .crank(0, 3, Vec::new())
        .map_err(|error| format!("complete-world user refresh: {error}"))?;
    let complete_account = complete.primary_portfolio(0);
    let complete_position_after_q = position_abs_for_asset(&complete_account, 1)?;
    let complete_cert = complete_account
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("decode complete-world certificate: {error:?}"))?;
    let complete_insurance_after = complete.primary_market_state().1.insurance;
    let complete_insurance_delta = complete_insurance_after
        .checked_sub(complete_insurance_before)
        .ok_or("complete-world insurance decreased")?;
    if complete_position_before_q != position_before_q
        || complete_position_after_q != complete_position_before_q
        || complete_cert.certified_liq_deficit != 0
        || complete_insurance_delta != 0
    {
        return Err(format!(
            "complete observation did not preserve the healthy control: position {complete_position_before_q}->{complete_position_after_q}, deficit {}, insurance delta {complete_insurance_delta}",
            complete_cert.certified_liq_deficit
        ));
    }

    Ok(OmittedRescueReproduction {
        blocker: KnownBlocker::OmittedRescueAccrualLiquidation,
        omitted_position_before_q,
        omitted_position_after_q,
        omitted_insurance_delta,
        complete_position_after_q,
        complete_liquidation_deficit: complete_cert.certified_liq_deficit,
        complete_insurance_delta,
    })
}

pub fn reproduce_trade_retry_replay(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<TradeRetryReplayReproduction, String> {
    seed[0] ^= 0x43;
    let control = run_trade_retry_world(seed, route, false)?;
    let replay = run_trade_retry_world(seed, route, true)?;
    let victim_extra_loss = control
        .0
        .checked_sub(replay.0)
        .ok_or("trade retry replay increased victim payout")?;
    let attacker_extra_payout = replay
        .1
        .checked_sub(control.1)
        .ok_or("trade retry replay decreased attacker payout")?;
    if victim_extra_loss == 0 || victim_extra_loss != attacker_extra_payout {
        return Err(format!(
            "{route:?} retry variants did not transfer equal extractable value: victim {}/{}; attacker {}/{}",
            control.0, replay.0, control.1, replay.1
        ));
    }
    if control.2 != replay.2 {
        return Err(format!(
            "{route:?} retry replay changed total withdrawn value: control {}, replay {}",
            control.2, replay.2
        ));
    }
    Ok(TradeRetryReplayReproduction {
        blocker: KnownBlocker::TradeRetryReplay,
        route,
        victim_extra_loss,
        attacker_extra_payout,
        control_total_payout: control.2,
        replay_total_payout: replay.2,
    })
}

pub fn reproduce_asset_generation_trade_replay(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<AssetGenerationReplayReproduction, String> {
    seed[0] ^= 0x31;
    let control = run_asset_generation_trade_world(seed, route, false)?;
    let replay = run_asset_generation_trade_world(seed, route, true)?;
    if control.2 != replay.2 || control.3 != replay.3 {
        return Err(format!(
            "{route:?} lifecycle controls used different generations: control {:?}, replay {:?}",
            (control.2, control.3),
            (replay.2, replay.3)
        ));
    }
    let victim_loss = control
        .0
        .checked_sub(replay.0)
        .ok_or("asset-generation replay increased victim payout")?;
    let attacker_gain = replay
        .1
        .checked_sub(control.1)
        .ok_or("asset-generation replay decreased attacker payout")?;
    if victim_loss == 0 || victim_loss != attacker_gain {
        return Err(format!(
            "{route:?} stale generation did not transfer extractable value: victim {}/{}; attacker {}/{}",
            control.0, replay.0, control.1, replay.1
        ));
    }
    let total_payout = u128::from(replay.0) + u128::from(replay.1);
    if total_payout != u128::from(control.0) + u128::from(control.1) {
        return Err(format!(
            "{route:?} stale-generation replay changed total payout"
        ));
    }
    Ok(AssetGenerationReplayReproduction {
        blocker: KnownBlocker::AssetGenerationTradeReplay,
        route,
        old_market_id: replay.2,
        new_market_id: replay.3,
        victim_loss,
        attacker_payout: replay.1,
        total_payout,
    })
}

pub fn reproduce_cpi_caller_fee_siphon(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<CpiCallerFeeReproduction, String> {
    if !matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        return Err(format!("{route:?} is not an unsigned-LP CPI route"));
    }
    seed[0] ^= 0x24;
    const DEPOSIT: u128 = 1_000_000;
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const CALLER_FEE_BPS: u64 = 10_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("{route:?} configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("{route:?} retire asset before attacker creation: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(0, ASSET, 3, PRICE, 0, 1)
        .map_err(|error| format!("{route:?} attacker asset activation: {error}"))?;

    for size_q in [SIZE_Q, -SIZE_Q] {
        match route {
            TradeRoute::Cpi => env
                .trade_cpi(0, 1, ASSET, size_q, CALLER_FEE_BPS, 0)
                .map_err(|error| format!("single CPI caller-fee leg {size_q}: {error}"))?,
            TradeRoute::BatchCpi => env
                .batch_trade_cpi(
                    0,
                    1,
                    vec![BatchTradeCpiLeg {
                        asset_index: ASSET,
                        size_q,
                        fee_bps: CALLER_FEE_BPS,
                        limit_price: 0,
                    }],
                )
                .map_err(|error| format!("batch CPI caller-fee leg {size_q}: {error}"))?,
            _ => unreachable!(),
        };
    }
    let attacker_capital = env.primary_portfolio(0).capital.get();
    let lp_capital = env.primary_portfolio(1).capital.get();
    let (_, group) = env.primary_market_state();
    if group.assets[ASSET as usize].oi_eff_long_q != 0
        || group.assets[ASSET as usize].oi_eff_short_q != 0
    {
        return Err(format!(
            "{route:?} caller-fee round trip did not close flat"
        ));
    }
    let withdrawn_insurance = group.insurance_domain_budget[2]
        .checked_add(group.insurance_domain_budget[3])
        .ok_or("CPI caller-fee insurance budget overflow")?;
    if withdrawn_insurance == 0 {
        return Err(format!(
            "{route:?} caller fee created no attacker-withdrawable budget"
        ));
    }
    env.withdraw_insurance_asset(0, ASSET, withdrawn_insurance)
        .map_err(|error| format!("{route:?} withdraw caller-fee insurance: {error}"))?;
    env.withdraw_primary(0, attacker_capital)
        .map_err(|error| format!("{route:?} attacker capital withdrawal: {error}"))?;
    env.withdraw_primary(1, lp_capital)
        .map_err(|error| format!("{route:?} LP capital withdrawal: {error}"))?;
    let attacker_payout = env.token_amount(env.actors[0].destination_token);
    let lp_payout = env.token_amount(env.actors[1].destination_token);
    let attacker_profit = attacker_payout
        .checked_sub(DEPOSIT as u64)
        .ok_or("CPI caller fee did not leave attacker above principal")?;
    let lp_loss = (DEPOSIT as u64)
        .checked_sub(lp_payout)
        .ok_or("CPI caller fee left unsigned LP above principal")?;
    if attacker_profit == 0 || attacker_profit != lp_loss {
        return Err(format!(
            "{route:?} caller-fee siphon was not value-neutral between attacker and LP: profit {attacker_profit}, loss {lp_loss}"
        ));
    }
    let total_payout = u128::from(attacker_payout) + u128::from(lp_payout);
    if total_payout != DEPOSIT * 2 {
        return Err(format!(
            "{route:?} caller-fee siphon changed total payout to {total_payout}"
        ));
    }
    Ok(CpiCallerFeeReproduction {
        blocker: KnownBlocker::CpiCallerFeeSiphon,
        route,
        attacker_profit,
        lp_loss,
        withdrawn_insurance,
        total_payout,
    })
}

pub fn reproduce_cpi_backing_fee_siphon(
    mut seed: [u8; 32],
) -> Result<CpiBackingFeeReproduction, String> {
    seed[0] ^= 0x23;
    const PRICE: u64 = 100;
    const LP_DEPOSIT: u128 = 3_190;
    const ATTACKER_DEPOSIT: u128 = 10_000;
    const WINNING_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const LOSING_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const WINNING_DOMAIN: u16 = 3;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 30,
            actor_deposits: [
                ATTACKER_DEPOSIT,
                ATTACKER_DEPOSIT,
                LP_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("configure permissionless init fee: {error}"))?;
    env.configure_auth_mark(false, 0, 1, PRICE)
        .map_err(|error| format!("configure base AuthMark: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(1, 2)
        .map_err(|error| format!("retire attacker asset slot: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(0, 1, 3, PRICE, 0, 1)
        .map_err(|error| format!("activate attacker asset: {error}"))?;
    env.configure_auth_mark_for_actor(0, 1, 3, PRICE)
        .map_err(|error| format!("configure attacker AuthMark: {error}"))?;
    env.top_up_backing_bucket_for_actor(0, WINNING_DOMAIN, 5_000, 100)
        .map_err(|error| format!("fund attacker backing domain: {error}"))?;

    env.trade_cpi(1, 2, 1, -WINNING_SIZE_Q, 0, 0)
        .map_err(|error| format!("open LP winning leg: {error}"))?;
    env.trade_cpi(1, 2, 0, -LOSING_SIZE_Q, 0, 0)
        .map_err(|error| format!("open LP losing leg: {error}"))?;

    env.warp_to_slot(4);
    env.push_auth_mark_for_actor(0, 1, 4, PRICE)
        .map_err(|error| format!("prime attacker asset: {error}"))?;
    env.push_auth_mark(0, 4, PRICE)
        .map_err(|error| format!("prime base asset: {error}"))?;
    for (actor, asset) in [(1, 1), (2, 1), (1, 0), (2, 0)] {
        crank_adapter_steps(&mut env, actor, 4, asset, 4)?;
    }
    env.sync_maintenance_fee(2, 4)
        .map_err(|error| format!("sync LP maintenance fee: {error}"))?;
    if env.primary_portfolio(2).capital.get() != 3_100 {
        return Err(format!(
            "LP maintenance setup reached capital {}, expected 3100",
            env.primary_portfolio(2).capital.get()
        ));
    }

    env.warp_to_slot(5);
    env.push_auth_mark_for_actor(0, 1, 5, 105)
        .map_err(|error| format!("push LP winning mark: {error}"))?;
    env.push_auth_mark(0, 5, 95)
        .map_err(|error| format!("push LP losing mark: {error}"))?;
    for (actor, asset) in [(1, 1), (2, 1), (1, 0)] {
        crank_adapter_steps(&mut env, actor, 5, asset, 4)?;
    }
    if env.primary_portfolio(2).pnl.get() != 1_000 {
        return Err(format!(
            "LP source-PnL setup reached {}, expected 1000",
            env.primary_portfolio(2).pnl.get()
        ));
    }

    env.update_backing_fee_policy_for_actor(0, WINNING_DOMAIN, 5_000, 0)
        .map_err(|error| format!("install post-consent backing fee: {error}"))?;
    let lp_before = env.primary_portfolio(2).capital.get();
    let attacker_before = env.primary_portfolio(0).capital.get();
    let provider_before = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let provider_destination_before = env.token_amount(env.actors[0].destination_token);
    let supply_before = env.token_supply_observed();

    env.warp_to_slot(6);
    env.trade_cpi(0, 2, 0, -INCREASE_Q, 0, 0)
        .map_err(|error| format!("fee-bearing CPI increase: {error}"))?;
    let lp_after = env.primary_portfolio(2).capital.get();
    let provider_after = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let provider_earnings = provider_after
        .checked_sub(provider_before)
        .ok_or("CPI backing provider earnings decreased")?;
    let lp_capital_loss = lp_before
        .checked_sub(lp_after)
        .ok_or("CPI backing fee increased LP capital")?;
    if provider_earnings == 0 || lp_capital_loss != provider_earnings {
        return Err(format!(
            "CPI backing fee did not transfer LP capital to provider: LP {lp_before}->{lp_after}, earnings {provider_before}->{provider_after}"
        ));
    }
    env.trade_cpi(0, 2, 0, INCREASE_Q, 0, 0)
        .map_err(|error| format!("reverse fee-bearing CPI increase: {error}"))?;
    let attacker_after = env.primary_portfolio(0).capital.get();
    let attacker_capital_delta = i128::try_from(attacker_after)
        .and_then(|after| i128::try_from(attacker_before).map(|before| after - before))
        .map_err(|_| "attacker capital does not fit i128")?;
    if attacker_capital_delta != 0 || observed_positions(&env.primary_portfolio(0))?[0] != 0 {
        return Err(format!(
            "CPI backing-fee attacker did not return flat: capital delta {attacker_capital_delta}"
        ));
    }

    env.withdraw_backing_bucket_earnings_for_actor(0, WINNING_DOMAIN, provider_earnings)
        .map_err(|error| format!("withdraw LP-funded provider earnings: {error}"))?;
    let provider_destination_after = env.token_amount(env.actors[0].destination_token);
    let extracted_tokens = provider_destination_after
        .checked_sub(provider_destination_before)
        .ok_or("provider destination balance decreased")?;
    if u128::from(extracted_tokens) != provider_earnings {
        return Err(format!(
            "CPI backing fee ledger/SPL extraction mismatch: {provider_earnings} vs {extracted_tokens}"
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err("CPI backing-fee siphon changed total SPL supply".into());
    }
    Ok(CpiBackingFeeReproduction {
        blocker: KnownBlocker::CpiBackingFeeSiphon,
        lp_capital_loss,
        provider_earnings,
        extracted_tokens,
        attacker_capital_delta,
    })
}

pub fn reproduce_composite_oracle_rounding(
    mut seed: [u8; 32],
    case: CompositeRoundingCase,
) -> Result<CompositeRoundingReproduction, String> {
    seed[0] ^= match case {
        CompositeRoundingCase::Pr329LargeMove => 0x29,
        CompositeRoundingCase::Pr381MicroMove => 0x81,
    };
    let (
        exact_mark,
        initial_prices,
        fresh_prices,
        victim_deposit,
        counterparty_deposit,
        cranker_deposit,
        size_units,
        catch_up_steps,
        catch_up_dt,
        soft_stale_slots,
    ) = match case {
        CompositeRoundingCase::Pr329LargeMove => (
            1_500_000u64,
            [3i64, 1_000_000, 2],
            [3i64, 2_000_000, 1],
            540_000u128,
            540_000u128,
            1_000u128,
            1u128,
            12usize,
            20u64,
            1_000u64,
        ),
        CompositeRoundingCase::Pr381MicroMove => (
            1_002_000u64,
            [501i64, 1_000_000, 500],
            [501i64, 500_000_000, 1],
            50_100_000u128,
            50_100_000u128,
            1u128,
            1_000u128,
            1usize,
            1u64,
            3u64,
        ),
    };
    let exact_composite = |prices: [i64; 3]| -> Result<u128, String> {
        let p0 = u128::try_from(prices[0]).map_err(|_| "negative composite leg 0")?;
        let p1 = u128::try_from(prices[1]).map_err(|_| "negative composite leg 1")?;
        let p2 = u128::try_from(prices[2]).map_err(|_| "negative composite leg 2")?;
        p0.checked_mul(1_000_000_000_000)
            .and_then(|value| value.checked_div(p1.checked_mul(p2)?))
            .ok_or("exact composite arithmetic failed".into())
    };
    if exact_composite(initial_prices)? != u128::from(exact_mark)
        || exact_composite(fresh_prices)? != u128::from(exact_mark)
    {
        return Err(format!(
            "{case:?} fixture does not preserve exact mark {exact_mark}"
        ));
    }

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: exact_mark,
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 20,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 10_000_000,
            actor_deposits: [
                victim_deposit,
                counterparty_deposit,
                cranker_deposit,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("{case:?} configure cranker share: {error}"))?;
    env.set_clock(1, 100);
    let feeds = [[0xe1u8; 32], [0xe2u8; 32], [0xe3u8; 32]];
    let initial_oracles: Vec<_> = initial_prices
        .iter()
        .enumerate()
        .map(|(index, price)| env.set_pyth_price(&feeds[index], *price, -6, 0, 100))
        .collect();
    env.configure_hybrid_oracle(
        0,
        1,
        100,
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
        feeds,
        &initial_oracles,
        soft_stale_slots,
        0,
    )
    .map_err(|error| format!("{case:?} configure exact composite: {error}"))?;
    if env.primary_market_state().0.oracle_target_price_e6 != exact_mark {
        return Err(format!(
            "{case:?} initial composite target is {}, expected {exact_mark}",
            env.primary_market_state().0.oracle_target_price_e6
        ));
    }

    let size_q = size_units
        .checked_mul(POS_SCALE)
        .and_then(|value| i128::try_from(value).ok())
        .ok_or("composite victim size overflow")?;
    env.trade_no_cpi(0, 1, 0, size_q, exact_mark, 0)
        .map_err(|error| format!("{case:?} open victim long: {error}"))?;
    let victim_capital_before = env.primary_portfolio(0).capital.get();
    let oi_before = env.primary_market_state().1.assets[0].oi_eff_long_q;
    let cranker_capital_before = env.primary_portfolio(2).capital.get();
    let supply_before = env.token_supply_observed();

    let fresh_oracles: Vec<_> = fresh_prices
        .iter()
        .enumerate()
        .map(|(index, price)| env.set_pyth_price(&feeds[index], *price, -6, 0, 101))
        .collect();
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 3,
        }]
    };
    let mut slot = 1u64;
    for _ in 0..catch_up_steps {
        slot = slot
            .checked_add(catch_up_dt)
            .ok_or("composite catch-up slot overflow")?;
        env.set_clock(slot, 101);
        env.crank_with_oracles(2, slot, observations(), &fresh_oracles)
            .map_err(|error| format!("{case:?} composite catch-up at slot {slot}: {error}"))?;
    }
    env.crank_with_oracles(0, slot, observations(), &fresh_oracles)
        .map_err(|error| format!("{case:?} refresh victim at rounded mark: {error}"))?;
    let victim_cert = env
        .primary_portfolio(0)
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("{case:?} decode victim certificate: {error:?}"))?;
    if victim_cert.certified_liq_deficit == 0 {
        return Err(format!(
            "{case:?} rounded composite did not falsely certify liquidation"
        ));
    }

    env.crank_with_reward(2, 0, slot, observations(), &fresh_oracles)
        .map_err(|error| format!("{case:?} false-price liquidation no longer lands: {error}"))?;
    let (wrapper_after, group_after) = env.primary_market_state();
    let victim_capital_after = env.primary_portfolio(0).capital.get();
    let oi_after = group_after.assets[0].oi_eff_long_q;
    let cranker_capital_after = env.primary_portfolio(2).capital.get();
    let victim_capital_loss = victim_capital_before
        .checked_sub(victim_capital_after)
        .ok_or("composite liquidation increased victim capital")?;
    let oi_reduction_q = oi_before
        .checked_sub(oi_after)
        .ok_or("composite liquidation increased victim OI")?;
    let cranker_reward = cranker_capital_after
        .checked_sub(cranker_capital_before)
        .ok_or("composite liquidation decreased cranker capital")?;
    if victim_capital_loss == 0 || oi_reduction_q == 0 || cranker_reward == 0 {
        return Err(format!(
            "{case:?} false composite was not economically committed: victim loss {victim_capital_loss}, OI reduction {oi_reduction_q}, reward {cranker_reward}"
        ));
    }
    env.withdraw_primary(2, cranker_reward)
        .map_err(|error| format!("{case:?} withdraw false-liquidation reward: {error}"))?;
    let extracted_tokens = env.token_amount(env.actors[2].destination_token);
    if u128::from(extracted_tokens) != cranker_reward {
        return Err(format!(
            "{case:?} cranker reward/SPL mismatch: {cranker_reward} vs {extracted_tokens}"
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{case:?} false liquidation changed total SPL supply"
        ));
    }
    if wrapper_after.oracle_target_price_e6 == exact_mark
        || group_after.assets[0].effective_price == exact_mark
    {
        return Err(format!(
            "{case:?} liquidation occurred without the expected rounded target/mark divergence"
        ));
    }
    Ok(CompositeRoundingReproduction {
        blocker: KnownBlocker::CompositeOracleRounding,
        case,
        exact_mark,
        rounded_target: wrapper_after.oracle_target_price_e6,
        rounded_mark: group_after.assets[0].effective_price,
        victim_capital_loss,
        oi_reduction_q,
        cranker_reward,
        extracted_tokens,
    })
}

pub fn reproduce_rounded_funding_omission(
    mut seed: [u8; 32],
) -> Result<RoundedFundingOmissionReproduction, String> {
    seed[0] ^= 0x53;
    let control = run_rounded_funding_world(seed, false)?;
    let attack = run_rounded_funding_world(seed, true)?;
    if !attack.0 {
        return Err("PR 253 no-observation crank no longer lands".into());
    }
    let victim_payout_loss = control
        .3
        .checked_sub(attack.3)
        .ok_or("rounded-funding omission increased victim payout")?;
    let attacker_payout_gain = attack
        .4
        .checked_sub(control.4)
        .ok_or("rounded-funding omission decreased short payout")?;
    if victim_payout_loss == 0 || victim_payout_loss != attacker_payout_gain {
        return Err(format!(
            "rounded-funding omission did not transfer equal SPL value: victim {}/{}; short {}/{}",
            control.3, attack.3, control.4, attack.4
        ));
    }
    if control.1 <= 0
        || control.2 >= 0
        || attack.1 != 0
        || attack.2 != 0
        || u128::from(control.3) + u128::from(control.4)
            != u128::from(attack.3) + u128::from(attack.4)
    {
        return Err(format!(
            "rounded-funding indexes/payouts do not match the omission class: control={control:?}, attack={attack:?}"
        ));
    }
    Ok(RoundedFundingOmissionReproduction {
        blocker: KnownBlocker::RoundedFundingOmission,
        control_f_long_num: control.1,
        control_f_short_num: control.2,
        attack_f_long_num: attack.1,
        attack_f_short_num: attack.2,
        victim_payout_loss,
        attacker_payout_gain,
    })
}

fn run_rounded_funding_world(
    seed: [u8; 32],
    omit_selected_observation: bool,
) -> Result<(bool, i128, i128, u64, u64), String> {
    const PRICE: u64 = 100;
    const MARK: u64 = 99;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.trade_no_cpi(0, 1, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open rounded-funding victim pair: {error}"))?;
    env.trade_no_cpi(2, 3, 1, POS_SCALE as i128, PRICE, 0)
        .map_err(|error| format!("open rounded-funding epoch pair: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, MARK)
        .map_err(|error| format!("stage rounded funding mark: {error}"))?;
    let asset0_observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    env.crank(0, 2, asset0_observation.clone())
        .map_err(|error| format!("prime rounded funding checkpoint: {error}"))?;
    let (_, primed) = env.primary_market_state();
    if primed.assets[0].effective_price != PRICE
        || primed.assets[0].slot_last != 2
        || primed.assets[0].f_long_num != 0
    {
        return Err("rounded-funding prime state is not price-stationary and unfunded".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, MARK)
        .map_err(|error| format!("stage unrelated epoch mark: {error}"))?;
    env.crank(
        2,
        3,
        vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: env.primary_profile(1).oracle_leg_count,
        }],
    )
    .map_err(|error| format!("advance unrelated market epoch: {error}"))?;
    let refresh = env.crank(
        0,
        3,
        if omit_selected_observation {
            Vec::new()
        } else {
            asset0_observation
        },
    );
    let missing_observation_landed = refresh.is_ok();
    if !omit_selected_observation {
        refresh.map_err(|error| format!("observed rounded-funding refresh: {error}"))?;
    } else if let Err(error) = refresh {
        return Err(format!(
            "omitted rounded-funding observation rejected: {error}"
        ));
    }
    let (_, funded) = env.primary_market_state();

    env.crank(1, 3, Vec::new())
        .map_err(|error| format!("settle rounded-funding short: {error}"))?;
    env.trade_no_cpi(0, 1, 0, -SIZE_Q, PRICE, 0)
        .map_err(|error| format!("close rounded-funding victim pair: {error}"))?;
    env.trade_no_cpi(2, 3, 1, -(POS_SCALE as i128), PRICE, 0)
        .map_err(|error| format!("close rounded-funding epoch pair: {error}"))?;
    if env.primary_portfolio(0).pnl.get() > 0 {
        env.convert_released_pnl(0, u128::MAX)
            .map_err(|error| format!("convert victim funding PnL: {error}"))?;
    }
    let victim_capital = env.primary_portfolio(0).capital.get();
    let short_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("withdraw rounded-funding victim: {error}"))?;
    env.withdraw_primary(1, short_capital)
        .map_err(|error| format!("withdraw rounded-funding short: {error}"))?;
    Ok((
        missing_observation_landed,
        funded.assets[0].f_long_num,
        funded.assets[0].f_short_num,
        env.token_amount(env.actors[0].destination_token),
        env.token_amount(env.actors[1].destination_token),
    ))
}

fn crank_adapter_steps(
    env: &mut V16Svm,
    actor: usize,
    now_slot: u64,
    asset_index: u16,
    attempts: usize,
) -> Result<(), String> {
    let oracle_accounts = env.primary_profile(asset_index as usize).oracle_leg_count;
    let observations = vec![CrankObservationHint {
        asset_index,
        oracle_accounts,
    }];
    let mut progressed = false;
    for _ in 0..attempts {
        match env.crank(actor, now_slot, observations.clone()) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => {
                return Err(format!(
                    "actor {actor} asset {asset_index} crank failed before progress: {error}"
                ));
            }
        }
    }
    if !progressed {
        return Err(format!(
            "actor {actor} asset {asset_index} crank made no progress"
        ));
    }
    Ok(())
}

fn run_asset_generation_trade_world(
    seed: [u8; 32],
    route: TradeRoute,
    land_replay: bool,
) -> Result<(u64, u64, u64, u64), String> {
    const ASSET: u16 = 1;
    const OLD_PRICE: u64 = 100;
    const NEW_PRICE: u64 = 250;
    const ADVERSE_PRICE: u64 = 200;
    const SIZE_Q: i128 = 1_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            actor_deposits: [
                1_000_000,
                1_000_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, ASSET, 1, OLD_PRICE)
        .map_err(|error| format!("{route:?} configure old generation: {error}"))?;
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("{route:?} configure permissionless init fee: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained = build_retained_trade(&mut env, route, 0, 1, ASSET, SIZE_Q, OLD_PRICE, 0);

    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("{route:?} retire old asset generation: {error}"))?;
    env.warp_to_slot(4);
    env.activate_permissionless_asset(2, ASSET, 4, NEW_PRICE, 1)
        .map_err(|error| format!("{route:?} activate replacement generation: {error}"))?;
    env.configure_auth_mark(false, ASSET, 4, NEW_PRICE)
        .map_err(|error| format!("{route:?} configure replacement AuthMark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "{route:?} asset slot reuse did not change market ID {old_market_id}"
        ));
    }
    if land_replay {
        env.land_retained(retained).map_err(|error| {
            format!("{route:?} stale-generation trade no longer lands: {error}")
        })?;
        let victim_position = observed_positions(&env.primary_portfolio(0))?[ASSET as usize];
        if victim_position != SIZE_Q {
            return Err(format!(
                "{route:?} stale trade created position {victim_position}, expected {SIZE_Q}"
            ));
        }
    }

    env.warp_to_slot(5);
    env.push_auth_mark(ASSET, 5, ADVERSE_PRICE)
        .map_err(|error| format!("{route:?} push replacement adverse mark: {error}"))?;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    env.crank(1, 5, observation())
        .map_err(|error| format!("{route:?} refresh replacement attacker: {error}"))?;
    env.crank(0, 5, observation())
        .map_err(|error| format!("{route:?} refresh replacement victim: {error}"))?;
    if land_replay {
        let victim_position = observed_positions(&env.primary_portfolio(0))?[ASSET as usize];
        env.trade_no_cpi(0, 1, ASSET, -victim_position, ADVERSE_PRICE, 0)
            .map_err(|error| format!("{route:?} close stale-generation exposure: {error}"))?;
        env.convert_released_pnl(1, u128::MAX)
            .map_err(|error| format!("{route:?} convert stale-generation profit: {error}"))?;
    }
    let victim_capital = env.primary_portfolio(0).capital.get();
    let attacker_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("{route:?} replacement victim withdrawal: {error}"))?;
    env.withdraw_primary(1, attacker_capital)
        .map_err(|error| format!("{route:?} replacement attacker withdrawal: {error}"))?;
    Ok((
        env.token_amount(env.actors[0].destination_token),
        env.token_amount(env.actors[1].destination_token),
        old_market_id,
        new_market_id,
    ))
}

fn run_trade_retry_world(
    seed: [u8; 32],
    route: TradeRoute,
    land_retry: bool,
) -> Result<(u64, u64, u128), String> {
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 1,
            actor_deposits: [
                4_000_000,
                4_000_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let size_q = -(POS_SCALE as i128);
    let original = build_retained_trade(
        &mut env,
        route,
        0,
        1,
        0,
        size_q,
        super::v16_svm::INITIAL_PRICE,
        super::v16_svm::INITIAL_PRICE,
    );
    let retry = build_retained_trade(
        &mut env,
        route,
        0,
        1,
        0,
        size_q,
        super::v16_svm::INITIAL_PRICE,
        super::v16_svm::INITIAL_PRICE,
    );
    env.land_retained(original)
        .map_err(|error| format!("{route:?} original intent: {error}"))?;
    if land_retry {
        env.land_retained(retry)
            .map_err(|error| format!("{route:?} retry variant no longer lands: {error}"))?;
    }

    let expected_abs_q = (if land_retry { 2 } else { 1 }) * POS_SCALE;
    let victim_position = observed_positions(&env.primary_portfolio(0))?[0];
    if victim_position >= 0 || victim_position.unsigned_abs() != expected_abs_q {
        return Err(format!(
            "{route:?} retry setup produced unexpected victim position {victim_position}, expected -{expected_abs_q}"
        ));
    }

    env.warp_to_slot(2);
    let close_price = super::v16_svm::INITIAL_PRICE
        .checked_mul(11)
        .and_then(|value| value.checked_div(10))
        .ok_or("trade retry close-price overflow")?;
    env.push_auth_mark(0, 2, close_price)
        .map_err(|error| format!("{route:?} push adverse mark: {error}"))?;
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    env.crank(1, 2, observation())
        .map_err(|error| format!("{route:?} refresh attacker: {error}"))?;
    env.crank(0, 2, observation())
        .map_err(|error| format!("{route:?} refresh victim: {error}"))?;
    let victim_position = observed_positions(&env.primary_portfolio(0))?[0];
    env.trade_no_cpi(0, 1, 0, -victim_position, close_price, 0)
        .map_err(|error| format!("{route:?} close replayed exposure: {error}"))?;
    if observed_positions(&env.primary_portfolio(0))?[0] != 0
        || observed_positions(&env.primary_portfolio(1))?[0] != 0
    {
        return Err(format!("{route:?} replay world did not close flat"));
    }

    env.convert_released_pnl(1, u128::MAX)
        .map_err(|error| format!("{route:?} convert attacker released PnL: {error}"))?;
    let victim_capital = env.primary_portfolio(0).capital.get();
    let attacker_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("{route:?} victim withdrawal: {error}"))?;
    env.withdraw_primary(1, attacker_capital)
        .map_err(|error| format!("{route:?} attacker withdrawal: {error}"))?;
    let victim_payout = env.token_amount(env.actors[0].destination_token);
    let attacker_payout = env.token_amount(env.actors[1].destination_token);
    Ok((
        victim_payout,
        attacker_payout,
        u128::from(victim_payout) + u128::from(attacker_payout),
    ))
}

fn build_retained_trade(
    env: &mut V16Svm,
    route: TradeRoute,
    taker: usize,
    maker: usize,
    asset_index: u16,
    size_q: i128,
    exec_price: u64,
    limit_price: u64,
) -> Transaction {
    match route {
        TradeRoute::NoCpi => {
            env.build_retained_no_cpi_trade(taker, maker, asset_index, size_q, exec_price)
        }
        TradeRoute::Cpi => {
            env.build_retained_cpi_trade(taker, maker, asset_index, size_q, limit_price)
        }
        TradeRoute::BatchNoCpi => {
            env.build_retained_batch_no_cpi_trade(taker, maker, asset_index, size_q, exec_price)
        }
        TradeRoute::BatchCpi => {
            env.build_retained_batch_cpi_trade(taker, maker, asset_index, size_q, limit_price)
        }
    }
}

fn build_omitted_rescue_world(seed: [u8; 32]) -> Result<(V16Svm, u128, u128), String> {
    const ADVERSE_PRICE: u64 = 1_000_000;
    const ADVERSE_TARGET: u64 = 997_600;
    const RESCUE_PRICE: u64 = 100;
    const RESCUE_MARK: u64 = 99;
    const ADVERSE_SIZE_Q: i128 = 50 * POS_SCALE as i128;
    const RESCUE_SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                3_115_000,
                50_000_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, 0, 1, RESCUE_PRICE)
        .map_err(|error| format!("configure rescue AuthMark: {error}"))?;
    env.configure_auth_mark(false, 1, 1, ADVERSE_PRICE)
        .map_err(|error| format!("configure adverse AuthMark: {error}"))?;
    env.top_up_backing_bucket(1, 200_000, 10)
        .map_err(|error| format!("top up rescue backing: {error}"))?;
    env.trade_no_cpi(0, 1, 1, ADVERSE_SIZE_Q, ADVERSE_PRICE, 0)
        .map_err(|error| format!("open adverse first leg: {error}"))?;
    env.trade_no_cpi(0, 1, 0, RESCUE_SIZE_Q, RESCUE_PRICE, 0)
        .map_err(|error| format!("open rescue later leg: {error}"))?;
    let opened = env.primary_portfolio(0);
    let active_assets: Vec<_> = decoded_legs(&opened)
        .into_iter()
        .filter(|leg| leg.active)
        .map(|leg| leg.asset_index)
        .collect();
    if active_assets != [1, 0] {
        return Err(format!(
            "rescue setup did not preserve first/later leg ordering: {active_assets:?}"
        ));
    }
    let position_before_q = position_abs_for_asset(&opened, 1)?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, RESCUE_MARK)
        .map_err(|error| format!("stage rounded rescue mark: {error}"))?;
    let rescue_oracle_accounts = env.primary_profile(0).oracle_leg_count;
    env.crank(
        2,
        2,
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: rescue_oracle_accounts,
        }],
    )
    .map_err(|error| format!("prime rounded rescue mark: {error}"))?;
    let (_, primed) = env.primary_market_state();
    if primed.assets[0].effective_price != RESCUE_PRICE
        || primed.assets[0].slot_last != 2
        || primed.assets[0].f_long_num != 0
    {
        return Err("rescue leg did not reach the rounded, zero-funding prime state".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, ADVERSE_TARGET)
        .map_err(|error| format!("stage adverse mark: {error}"))?;
    let adverse_oracle_accounts = env.primary_profile(1).oracle_leg_count;
    env.crank(
        2,
        3,
        vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: adverse_oracle_accounts,
        }],
    )
    .map_err(|error| format!("commit adverse mark: {error}"))?;
    let (_, stale_group) = env.primary_market_state();
    if stale_group.assets[0].effective_price != RESCUE_PRICE || stale_group.assets[0].slot_last != 2
    {
        return Err("adverse prefix accidentally consumed the later rescue leg".into());
    }
    let insurance_before = stale_group.insurance;
    Ok((env, position_before_q, insurance_before))
}

fn position_abs_for_asset(
    account: &percolator_prog::state::PortfolioAccountV16,
    asset_index: usize,
) -> Result<u128, String> {
    decoded_legs(account)
        .into_iter()
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q.unsigned_abs())
        .ok_or_else(|| format!("portfolio has no active asset {asset_index}"))
}

#[allow(dead_code)]
pub fn post_expiry_backing_case_strategy(
) -> impl Strategy<Value = ([u8; 32], PostExpiryBackingCase)> {
    (
        any::<[u8; 32]>(),
        Just(5_000u16),
        prop::sample::select(vec![2u8, 3, 5, 8]),
        Just(500u16),
        Just(20u8),
    )
        .prop_map(
            |(seed, fee_bps, expiry_offset, mark_move_bps, increase_divisor)| {
                (
                    seed,
                    PostExpiryBackingCase {
                        fee_bps,
                        expiry_offset,
                        mark_move_bps,
                        increase_divisor,
                    },
                )
            },
        )
}

#[allow(dead_code)]
pub fn omitted_rescue_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn trade_retry_replay_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn asset_generation_replay_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn cpi_caller_fee_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![TradeRoute::Cpi, TradeRoute::BatchCpi]),
    )
}

#[allow(dead_code)]
pub fn cpi_backing_fee_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn composite_rounding_strategy() -> impl Strategy<Value = ([u8; 32], CompositeRoundingCase)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            CompositeRoundingCase::Pr329LargeMove,
            CompositeRoundingCase::Pr381MicroMove,
        ]),
    )
}

#[allow(dead_code)]
pub fn rounded_funding_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn scenario_strategy(max_actions: usize) -> impl Strategy<Value = Scenario> {
    (
        any::<[u8; 32]>(),
        small_market_config_strategy(),
        prop::collection::vec(action_strategy(), 1..=max_actions),
    )
        .prop_map(|(seed, config, actions)| Scenario {
            seed,
            config,
            actions,
        })
}

fn small_market_config_strategy() -> impl Strategy<Value = SmallMarketConfig> {
    prop::sample::select(vec![
        SmallMarketConfig::default(),
        SmallMarketConfig {
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 4,
            max_abs_funding_e9_per_slot: 10_000,
            maintenance_fee_per_slot: 0,
        },
        SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
        SmallMarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 10,
        },
    ])
}

#[allow(dead_code)]
fn action_strategy() -> impl Strategy<Value = Action> {
    let route = prop_oneof![
        Just(TradeRoute::NoCpi),
        Just(TradeRoute::Cpi),
        Just(TradeRoute::BatchNoCpi),
        Just(TradeRoute::BatchCpi),
    ];
    prop_oneof![
        12 => (
            route,
            any::<u8>(),
            any::<u8>(),
            any::<u8>(),
            -3i8..=3,
            0u16..=25,
            -1_000i16..=1_000,
            any::<bool>(),
        )
            .prop_map(
                |(
                    route,
                    taker,
                    maker,
                    asset,
                    units,
                    fee_bps,
                    price_move_bps,
                    prefer_reduce,
                )| {
                    Action::Trade {
                        route,
                        taker,
                        maker,
                        asset,
                        units,
                        fee_bps,
                        price_move_bps,
                        prefer_reduce,
                    }
                },
            ),
        2 => (any::<u8>(), 1u16..=2_000, 0u16..=100).prop_map(
            |(asset, halflife_slots, mark_min_fee)| Action::ConfigureEwma {
                asset,
                halflife_slots,
                mark_min_fee,
            },
        ),
        5 => (any::<u8>(), 1u8..=4, -500i16..=500).prop_map(
            |(asset, dt, move_bps)| Action::PushMark {
                asset,
                dt,
                move_bps,
            },
        ),
        6 => (
            any::<u8>(),
            prop_oneof![
                5 => Just(HintMode::Complete),
                2 => Just(HintMode::Reversed),
                1 => Just(HintMode::Empty),
                1 => Just(HintMode::Duplicate),
            ],
        )
            .prop_map(|(actor, hints)| Action::Crank { actor, hints }),
        2 => (any::<u8>(), 0u16..=500)
            .prop_map(|(actor, amount)| Action::Withdraw { actor, amount }),
        2 => (any::<u8>(), 1u8..=4)
            .prop_map(|(actor, dt)| Action::SyncMaintenanceFee { actor, dt }),
        2 => any::<u8>().prop_map(|actor| Action::CrossMarketSubstitution { actor }),
        2 => (
            any::<u8>(),
            prop::sample::select(SubstitutionKind::ALL.to_vec()),
        )
            .prop_map(|(actor, kind)| Action::AccountSubstitution { actor, kind }),
        3 => (any::<u8>(), any::<u8>(), any::<u8>(), -2i8..=2).prop_map(
            |(taker, maker, asset, units)| Action::RetainTrade {
                taker,
                maker,
                asset,
                units,
            },
        ),
        2 => Just(Action::LandRetained),
        1 => Just(Action::AdvanceBlockhash),
    ]
}

fn moved_price(price: u64, move_bps: i16) -> Result<u64, String> {
    let numerator = (price as i128)
        .checked_mul(10_000 + i128::from(move_bps.clamp(-9_999, 10_000)))
        .ok_or("reported-price multiplication overflow")?;
    u64::try_from((numerator / 10_000).max(1))
        .map_err(|_| "reported-price conversion overflow".into())
}

fn all_observations() -> Vec<CrankObservationHint> {
    (0..ASSET_COUNT)
        .map(|asset_index| CrankObservationHint {
            asset_index: asset_index as u16,
            oracle_accounts: 0,
        })
        .collect()
}

fn decoded_legs(account: &percolator_prog::state::PortfolioAccountV16) -> Vec<PortfolioLegV16> {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .collect()
}

fn observed_positions(
    account: &percolator_prog::state::PortfolioAccountV16,
) -> Result<[i128; ASSET_COUNT], String> {
    let mut out = [0i128; ASSET_COUNT];
    for leg in decoded_legs(account) {
        if !leg.active {
            continue;
        }
        let asset = leg.asset_index as usize;
        if asset >= ASSET_COUNT {
            return Err(format!("portfolio has out-of-world asset {asset}"));
        }
        out[asset] = out[asset]
            .checked_add(leg.basis_pos_q)
            .ok_or("observed position overflow")?;
    }
    Ok(out)
}
