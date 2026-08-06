use super::fuzz_model::{
    Action, HintMode, Scenario, SmallMarketConfig, SubstitutionKind, TradeRoute,
};

#[allow(dead_code)]
pub fn blocker_scenarios() -> Vec<(&'static str, Scenario)> {
    vec![
        (
            "trade_mark_crank_ordering",
            scenario(
                0x11,
                vec![
                    Action::PushMark {
                        asset: 0,
                        dt: 3,
                        move_bps: 250,
                    },
                    Action::Trade {
                        route: TradeRoute::NoCpi,
                        taker: 0,
                        maker: 1,
                        asset: 0,
                        units: -1,
                        fee_bps: 10,
                        price_move_bps: 0,
                        prefer_reduce: true,
                    },
                    Action::Crank {
                        actor: 1,
                        hints: HintMode::Reversed,
                    },
                    Action::Crank {
                        actor: 0,
                        hints: HintMode::Complete,
                    },
                    Action::Trade {
                        route: TradeRoute::BatchCpi,
                        taker: 2,
                        maker: 3,
                        asset: 0,
                        units: -1,
                        fee_bps: 10,
                        price_move_bps: 0,
                        prefer_reduce: true,
                    },
                ],
            ),
        ),
        (
            "delayed_first_landing",
            scenario(
                0x22,
                vec![
                    Action::RetainTrade {
                        taker: 0,
                        maker: 1,
                        asset: 1,
                        units: 1,
                    },
                    Action::PushMark {
                        asset: 1,
                        dt: 2,
                        move_bps: -200,
                    },
                    Action::Crank {
                        actor: 2,
                        hints: HintMode::Complete,
                    },
                    Action::LandRetained,
                    Action::Crank {
                        actor: 0,
                        hints: HintMode::Complete,
                    },
                ],
            ),
        ),
        (
            "cross_market_same_shape_substitution",
            scenario(
                0x33,
                vec![
                    Action::CrossMarketSubstitution { actor: 0 },
                    Action::AccountSubstitution {
                        actor: 1,
                        kind: SubstitutionKind::ForeignDepositVault,
                    },
                    Action::AccountSubstitution {
                        actor: 2,
                        kind: SubstitutionKind::ForeignWithdrawVault,
                    },
                    Action::AccountSubstitution {
                        actor: 3,
                        kind: SubstitutionKind::ForeignCrankPortfolio,
                    },
                    Action::AccountSubstitution {
                        actor: 0,
                        kind: SubstitutionKind::MismatchedMatcherBinding,
                    },
                    Action::Trade {
                        route: TradeRoute::Cpi,
                        taker: 2,
                        maker: 3,
                        asset: 1,
                        units: 1,
                        fee_bps: 0,
                        price_move_bps: 0,
                        prefer_reduce: false,
                    },
                ],
            ),
        ),
        (
            "crank_hint_order_and_duplicate_isolation",
            scenario(
                0x44,
                vec![
                    Action::PushMark {
                        asset: 0,
                        dt: 2,
                        move_bps: 100,
                    },
                    Action::PushMark {
                        asset: 1,
                        dt: 2,
                        move_bps: -100,
                    },
                    Action::Crank {
                        actor: 0,
                        hints: HintMode::Duplicate,
                    },
                    Action::Crank {
                        actor: 0,
                        hints: HintMode::Reversed,
                    },
                    Action::Crank {
                        actor: 1,
                        hints: HintMode::Complete,
                    },
                ],
            ),
        ),
        (
            "epoch_stale_certificate_cannot_block_normal_exit",
            scenario(
                0x4a,
                vec![
                    Action::PushMark {
                        asset: 0,
                        dt: 4,
                        move_bps: 0,
                    },
                    Action::PushMark {
                        asset: 0,
                        dt: 1,
                        move_bps: 100,
                    },
                ],
            ),
        ),
        (
            "capped_funding_market_converges_before_normal_exit",
            scenario_with_config(
                0x4b,
                SmallMarketConfig {
                    max_price_move_bps_per_slot: 1,
                    max_accrual_dt_slots: 4,
                    max_abs_funding_e9_per_slot: 10_000,
                    maintenance_fee_per_slot: 0,
                },
                vec![Action::Trade {
                    route: TradeRoute::Cpi,
                    taker: 0,
                    maker: 2,
                    asset: 1,
                    units: 3,
                    fee_bps: 13,
                    price_move_bps: -22,
                    prefer_reduce: false,
                }],
            ),
        ),
        (
            "ewma_reported_price_after_catchup_preserves_exit",
            scenario_with_config(
                0x4c,
                SmallMarketConfig {
                    max_price_move_bps_per_slot: 1,
                    max_accrual_dt_slots: 4,
                    max_abs_funding_e9_per_slot: 10_000,
                    maintenance_fee_per_slot: 0,
                },
                vec![
                    Action::ConfigureEwma {
                        asset: 0,
                        halflife_slots: 1,
                        mark_min_fee: 0,
                    },
                    Action::PushMark {
                        asset: 0,
                        dt: 4,
                        move_bps: 500,
                    },
                    Action::Crank {
                        actor: 0,
                        hints: HintMode::Complete,
                    },
                    Action::Trade {
                        route: TradeRoute::NoCpi,
                        taker: 2,
                        maker: 3,
                        asset: 0,
                        units: -1,
                        fee_bps: 0,
                        price_move_bps: -500,
                        prefer_reduce: true,
                    },
                ],
            ),
        ),
        (
            "authenticated_maintenance_fee_cannot_block_exit",
            scenario_with_config(
                0x4d,
                SmallMarketConfig {
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 2,
                    max_abs_funding_e9_per_slot: 0,
                    maintenance_fee_per_slot: 10,
                },
                vec![Action::SyncMaintenanceFee { actor: 0, dt: 4 }],
            ),
        ),
        (
            "expired_retained_transaction_does_not_mutate",
            scenario(
                0x55,
                vec![
                    Action::RetainTrade {
                        taker: 0,
                        maker: 1,
                        asset: 0,
                        units: -1,
                    },
                    Action::AdvanceBlockhash,
                    Action::PushMark {
                        asset: 0,
                        dt: 1,
                        move_bps: 50,
                    },
                    Action::LandRetained,
                    Action::Crank {
                        actor: 0,
                        hints: HintMode::Complete,
                    },
                ],
            ),
        ),
    ]
}

#[allow(dead_code)]
pub fn fixed_blocker_scenarios() -> Vec<(&'static str, Scenario)> {
    vec![(
        "pr204_live_lapsed_source_backing_auto_crank",
        Scenario {
            seed: [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 254, 166, 229, 105, 89,
                246, 226, 191, 41, 163, 225, 108, 86,
            ],
            config: SmallMarketConfig {
                max_price_move_bps_per_slot: 1,
                max_accrual_dt_slots: 4,
                max_abs_funding_e9_per_slot: 10_000,
                maintenance_fee_per_slot: 0,
            },
            actions: vec![
                Action::Trade {
                    route: TradeRoute::BatchNoCpi,
                    taker: 142,
                    maker: 204,
                    asset: 57,
                    units: -3,
                    fee_bps: 17,
                    price_move_bps: -636,
                    prefer_reduce: false,
                },
                Action::PushMark {
                    asset: 152,
                    dt: 1,
                    move_bps: 138,
                },
                Action::Crank {
                    actor: 24,
                    hints: HintMode::Complete,
                },
            ],
        },
    )]
}

#[allow(dead_code)]
fn scenario(seed_byte: u8, actions: Vec<Action>) -> Scenario {
    scenario_with_config(seed_byte, SmallMarketConfig::default(), actions)
}

#[allow(dead_code)]
fn scenario_with_config(
    seed_byte: u8,
    config: SmallMarketConfig,
    actions: Vec<Action>,
) -> Scenario {
    Scenario {
        seed: [seed_byte; 32],
        config,
        actions,
    }
}
