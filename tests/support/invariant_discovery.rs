use super::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, PRIMARY_ACTOR_COUNT, USER_DEPOSIT};
use percolator::POS_SCALE;
use serde::{Deserialize, Serialize};
use solana_sdk::{account::Account, transaction::Transaction};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PortfolioIntentKind {
    Deposit,
    Withdraw,
    Close,
    MatcherDisable,
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
}

impl PortfolioIntentKind {
    pub const ALL: [Self; 8] = [
        Self::Deposit,
        Self::Withdraw,
        Self::Close,
        Self::MatcherDisable,
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
    ];
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarketIntentKind {
    Deposit,
    MatcherEnable,
    TradeFeePolicy,
    FeeRedirectPolicy,
    MaintenanceFeePolicy,
    LiquidationFeePolicy,
    ShutdownAsset,
    ResolveMarket,
    ResolvePolicy,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetIntentKind {
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
    PushAuthMark,
    PushEwmaMark,
    ConfigureAuthMark,
    ConfigureEwmaMark,
    ConfigureHybridOracle,
    InsuranceTopUp,
    BackingTopUp,
    InsuranceWithdrawal,
    BackingFeePolicy,
    ResolveMarket,
    ResolvePolicy,
}

impl AssetIntentKind {
    pub const ALL: [Self; 15] = [
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
        Self::PushAuthMark,
        Self::PushEwmaMark,
        Self::ConfigureAuthMark,
        Self::ConfigureEwmaMark,
        Self::ConfigureHybridOracle,
        Self::InsuranceTopUp,
        Self::BackingTopUp,
        Self::InsuranceWithdrawal,
        Self::BackingFeePolicy,
        Self::ResolveMarket,
        Self::ResolvePolicy,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::TradeNoCpi => 0,
            Self::TradeCpi => 1,
            Self::BatchTradeNoCpi => 2,
            Self::BatchTradeCpi => 3,
            Self::PushAuthMark => 4,
            Self::PushEwmaMark => 5,
            Self::ConfigureAuthMark => 6,
            Self::ConfigureEwmaMark => 7,
            Self::ConfigureHybridOracle => 8,
            Self::InsuranceTopUp => 9,
            Self::BackingTopUp => 10,
            Self::InsuranceWithdrawal => 11,
            Self::BackingFeePolicy => 12,
            Self::ResolveMarket => 13,
            Self::ResolvePolicy => 14,
        }
    }

    fn uses_actor_authorities(self) -> bool {
        matches!(
            self,
            Self::InsuranceTopUp
                | Self::BackingTopUp
                | Self::InsuranceWithdrawal
                | Self::BackingFeePolicy
        )
    }
}

impl MarketIntentKind {
    pub const ALL: [Self; 9] = [
        Self::Deposit,
        Self::MatcherEnable,
        Self::TradeFeePolicy,
        Self::FeeRedirectPolicy,
        Self::MaintenanceFeePolicy,
        Self::LiquidationFeePolicy,
        Self::ShutdownAsset,
        Self::ResolveMarket,
        Self::ResolvePolicy,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::Deposit => 0,
            Self::MatcherEnable => 1,
            Self::TradeFeePolicy => 2,
            Self::FeeRedirectPolicy => 3,
            Self::MaintenanceFeePolicy => 4,
            Self::LiquidationFeePolicy => 5,
            Self::ShutdownAsset => 6,
            Self::ResolveMarket => 7,
            Self::ResolvePolicy => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncarnationDiscovery {
    pub kind: PortfolioIntentKind,
    pub old_portfolio_id: u64,
    pub new_portfolio_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketIncarnationDiscovery {
    pub kind: MarketIntentKind,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetGenerationDiscovery {
    pub kind: AssetIntentKind,
    pub old_asset_id: u64,
    pub new_asset_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
}

impl AssetGenerationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

impl MarketIncarnationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

impl IncarnationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountFingerprint {
    lamports: u64,
    owner: [u8; 32],
    executable: bool,
    rent_epoch: u64,
    data: Vec<u8>,
}

impl From<Account> for AccountFingerprint {
    fn from(account: Account) -> Self {
        Self {
            lamports: account.lamports,
            owner: account.owner.to_bytes(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
            data: account.data,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicFingerprint {
    market: Option<AccountFingerprint>,
    foreign_market: Option<AccountFingerprint>,
    backing_domain_ledger: Option<AccountFingerprint>,
    mint: Option<AccountFingerprint>,
    portfolios: Vec<Option<AccountFingerprint>>,
    token_accounts: Vec<Option<AccountFingerprint>>,
    matcher_contexts: Vec<Option<AccountFingerprint>>,
    token_supply: u128,
}

fn account_fingerprint(
    env: &V16Svm,
    key: &solana_sdk::pubkey::Pubkey,
) -> Option<AccountFingerprint> {
    env.svm.get_account(key).map(Into::into)
}

fn fingerprint(env: &V16Svm) -> EconomicFingerprint {
    let mut portfolio_keys: Vec<_> = env.actors.iter().map(|actor| actor.portfolio).collect();
    portfolio_keys.push(env.foreign_actor.portfolio);

    let mut token_keys = vec![
        env.vault,
        env.foreign_vault,
        env.provider_source_token,
        env.provider_destination_token,
        env.market_admin_destination_token,
        env.foreign_actor.source_token,
        env.foreign_actor.destination_token,
    ];
    for actor in &env.actors {
        token_keys.extend([actor.source_token, actor.destination_token]);
    }

    EconomicFingerprint {
        market: account_fingerprint(env, &env.market),
        foreign_market: account_fingerprint(env, &env.foreign_market),
        backing_domain_ledger: account_fingerprint(env, &env.backing_domain_ledger),
        mint: account_fingerprint(env, &env.mint),
        portfolios: portfolio_keys
            .iter()
            .map(|key| account_fingerprint(env, key))
            .collect(),
        token_accounts: token_keys
            .iter()
            .map(|key| account_fingerprint(env, key))
            .collect(),
        matcher_contexts: env
            .actors
            .iter()
            .map(|actor| account_fingerprint(env, &actor.matcher_context))
            .collect(),
        token_supply: env.token_supply_observed(),
    }
}

fn retained_portfolio_intent(env: &mut V16Svm, kind: PortfolioIntentKind) -> Transaction {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const AMOUNT: u128 = 1_000;
    let size_q = POS_SCALE as i128 / 4;
    match kind {
        PortfolioIntentKind::Deposit => env.build_retained_deposit(SUBJECT, AMOUNT),
        PortfolioIntentKind::Withdraw => env.build_retained_withdrawal(SUBJECT, AMOUNT),
        PortfolioIntentKind::Close => env.build_retained_close_primary_portfolio(SUBJECT),
        PortfolioIntentKind::MatcherDisable => env.build_retained_matcher_config(SUBJECT, 0),
        PortfolioIntentKind::TradeNoCpi => {
            env.build_retained_no_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, INITIAL_PRICE)
        }
        PortfolioIntentKind::TradeCpi => {
            env.build_retained_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, 0)
        }
        PortfolioIntentKind::BatchTradeNoCpi => {
            env.build_retained_batch_no_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, INITIAL_PRICE)
        }
        PortfolioIntentKind::BatchTradeCpi => {
            env.build_retained_batch_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, 0)
        }
    }
}

fn replacement_capital(kind: PortfolioIntentKind) -> u128 {
    match kind {
        PortfolioIntentKind::Close | PortfolioIntentKind::Deposit => 0,
        _ => USER_DEPOSIT,
    }
}

fn discover_one_portfolio_incarnation_replay(
    mut seed: [u8; 32],
    kind: PortfolioIntentKind,
) -> Result<IncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    seed[0] ^= 0xa3;
    seed[1] ^= kind as u8;
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let old_portfolio_id = env.primary_portfolio_id(SUBJECT);
    let retained = retained_portfolio_intent(&mut env, kind);

    let old_capital = env.primary_portfolio(SUBJECT).capital.get();
    env.withdraw_primary(SUBJECT, old_capital)
        .map_err(|error| format!("empty old portfolio: {error}"))?;
    env.close_primary_portfolio(SUBJECT)
        .map_err(|error| format!("close old portfolio: {error}"))?;
    env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
        .map_err(|error| format!("fund replacement portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(SUBJECT)
        .map_err(|error| format!("initialize replacement portfolio: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(SUBJECT);
    if new_portfolio_id <= old_portfolio_id {
        return Err(format!(
            "portfolio incarnation did not advance: {old_portfolio_id} -> {new_portfolio_id}"
        ));
    }

    let replacement_capital = replacement_capital(kind);
    if replacement_capital != 0 {
        env.deposit_primary(SUBJECT, replacement_capital)
            .map_err(|error| format!("fund replacement portfolio: {error}"))?;
    }
    if kind == PortfolioIntentKind::MatcherDisable {
        env.set_matcher_config(SUBJECT, 1)
            .map_err(|error| format!("establish replacement matcher policy: {error}"))?;
    }

    let before = fingerprint(&env);
    let result = env.land_retained(retained);
    let after = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} incarnation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale transaction succeeded without an observable state delta"
                ));
            }
            Ok(IncarnationDiscovery {
                kind,
                old_portfolio_id,
                new_portfolio_id,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale transaction did not roll back exactly"
                ));
            }
            Ok(IncarnationDiscovery {
                kind,
                old_portfolio_id,
                new_portfolio_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
            })
        }
    }
}

pub fn discover_portfolio_incarnation_replays(
    seed: [u8; 32],
) -> Result<Vec<IncarnationDiscovery>, String> {
    PortfolioIntentKind::ALL
        .into_iter()
        .map(|kind| discover_one_portfolio_incarnation_replay(seed, kind))
        .collect()
}

fn retained_market_intent(env: &mut V16Svm, kind: MarketIntentKind) -> Transaction {
    const SUBJECT: usize = 0;
    match kind {
        MarketIntentKind::Deposit => env.build_retained_deposit(SUBJECT, 1_000),
        MarketIntentKind::MatcherEnable => env.build_retained_matcher_config(SUBJECT, 1),
        MarketIntentKind::TradeFeePolicy => env.build_retained_trade_fee_policy(10_000),
        MarketIntentKind::FeeRedirectPolicy => env.build_retained_fee_redirect_policy(10_000),
        MarketIntentKind::MaintenanceFeePolicy => env.build_retained_maintenance_fee_policy(10_000),
        MarketIntentKind::LiquidationFeePolicy => env.build_retained_liquidation_fee_policy(10_000),
        MarketIntentKind::ShutdownAsset => env.build_retained_shutdown_asset(0, 12),
        MarketIntentKind::ResolveMarket => env.build_retained_resolve_market(),
        MarketIntentKind::ResolvePolicy => env.build_retained_permissionless_resolve_policy(17, 29),
    }
}

fn publicly_recreate_market(
    env: &mut V16Svm,
    config: MarketConfig,
    reinit_slot: u64,
) -> Result<(), String> {
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let capital = env.primary_portfolio(actor).capital.get();
        env.withdraw_primary(actor, capital)
            .map_err(|error| format!("empty old-market portfolio {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("close old-market portfolio {actor}: {error}"))?;
    }
    env.resolve_market()
        .map_err(|error| format!("resolve old market: {error}"))?;
    env.close_primary_slab()
        .map_err(|error| format!("close old market: {error}"))?;
    env.warp_to_slot(reinit_slot);
    env.fund_closed_primary_market()
        .map_err(|error| format!("fund replacement market: {error}"))?;
    env.recreate_primary_vault()
        .map_err(|error| format!("recreate replacement vault: {error}"))?;
    env.reinitialize_primary_market(config)
        .map_err(|error| format!("initialize replacement market: {error}"))?;
    Ok(())
}

fn discover_one_market_incarnation_replay(
    mut seed: [u8; 32],
    kind: MarketIntentKind,
) -> Result<MarketIncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    const REINIT_SLOT: u64 = 10;
    seed[0] ^= 0xb7;
    seed[1] ^= kind.discriminator();
    let config = MarketConfig {
        initial_price: INITIAL_PRICE,
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
        maintenance_fee_per_slot: 1,
        actor_deposits: [1; PRIMARY_ACTOR_COUNT],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    if kind == MarketIntentKind::ShutdownAsset {
        env.configure_permissionless_resolve(1_000_000, 1)
            .map_err(|error| format!("configure old-market shutdown policy: {error}"))?;
    }
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained = retained_market_intent(&mut env, kind);

    publicly_recreate_market(&mut env, config, REINIT_SLOT)?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    if matches!(
        kind,
        MarketIntentKind::Deposit | MarketIntentKind::MatcherEnable
    ) {
        env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
            .map_err(|error| format!("fund replacement portfolio: {error}"))?;
        env.reinitialize_primary_portfolio(SUBJECT)
            .map_err(|error| format!("initialize replacement portfolio: {error}"))?;
    }
    if kind == MarketIntentKind::ShutdownAsset {
        env.configure_permissionless_resolve(1_000_000, 1)
            .map_err(|error| format!("configure replacement shutdown policy: {error}"))?;
        env.warp_to_slot(12);
    }

    let before = fingerprint(&env);
    let result = env.land_retained(retained);
    let after = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} market-incarnation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale market transaction succeeded without an observable state delta"
                ));
            }
            Ok(MarketIncarnationDiscovery {
                kind,
                old_market_id,
                new_market_id,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale market transaction did not roll back exactly"
                ));
            }
            Ok(MarketIncarnationDiscovery {
                kind,
                old_market_id,
                new_market_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
            })
        }
    }
}

pub fn discover_market_incarnation_replays(
    seed: [u8; 32],
) -> Result<Vec<MarketIncarnationDiscovery>, String> {
    MarketIntentKind::ALL
        .into_iter()
        .map(|kind| discover_one_market_incarnation_replay(seed, kind))
        .collect()
}

fn configure_old_asset_intent(
    env: &mut V16Svm,
    kind: AssetIntentKind,
    asset_index: u16,
    authority_actor: usize,
) -> Result<Option<solana_sdk::pubkey::Pubkey>, String> {
    match kind {
        AssetIntentKind::PushAuthMark => env
            .configure_auth_mark(false, asset_index, 1, INITIAL_PRICE)
            .map(|_| None)
            .map_err(|error| format!("configure old AuthMark: {error}")),
        AssetIntentKind::PushEwmaMark => env
            .configure_ewma_mark(asset_index, 1, INITIAL_PRICE, 1, 0)
            .map(|_| None)
            .map_err(|error| format!("configure old EwmaMark: {error}")),
        AssetIntentKind::InsuranceTopUp => env
            .update_asset_authority_from_admin(
                asset_index,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                authority_actor,
            )
            .map(|_| None)
            .map_err(|error| format!("install old insurance authority: {error}")),
        AssetIntentKind::InsuranceWithdrawal => {
            for authority_kind in [
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
            ] {
                env.update_asset_authority_from_admin(asset_index, authority_kind, authority_actor)
                    .map_err(|error| format!("install old insurance role: {error}"))?;
            }
            Ok(None)
        }
        AssetIntentKind::BackingTopUp | AssetIntentKind::BackingFeePolicy => env
            .update_asset_authority_from_admin(
                asset_index,
                percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
                authority_actor,
            )
            .map(|_| None)
            .map_err(|error| format!("install old backing authority: {error}")),
        AssetIntentKind::ConfigureHybridOracle => {
            env.set_clock(1, 100);
            let feed = [0x5au8; 32];
            Ok(Some(env.set_pyth_price(
                &feed,
                INITIAL_PRICE as i64,
                -6,
                0,
                101,
            )))
        }
        _ => Ok(None),
    }
}

fn retained_asset_intent(
    env: &mut V16Svm,
    kind: AssetIntentKind,
    asset_index: u16,
    authority_actor: usize,
    oracle_account: Option<solana_sdk::pubkey::Pubkey>,
) -> Transaction {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const AMOUNT: u128 = 1_000;
    let size_q = POS_SCALE as i128 / 4;
    let stale_price = INITIAL_PRICE / 2;
    let domain = asset_index * 2;
    match kind {
        AssetIntentKind::TradeNoCpi => env.build_retained_no_cpi_trade(
            SUBJECT,
            COUNTERPARTY,
            asset_index,
            size_q,
            INITIAL_PRICE,
        ),
        AssetIntentKind::TradeCpi => {
            env.build_retained_cpi_trade(SUBJECT, COUNTERPARTY, asset_index, size_q, 0)
        }
        AssetIntentKind::BatchTradeNoCpi => env.build_retained_batch_no_cpi_trade(
            SUBJECT,
            COUNTERPARTY,
            asset_index,
            size_q,
            INITIAL_PRICE,
        ),
        AssetIntentKind::BatchTradeCpi => {
            env.build_retained_batch_cpi_trade(SUBJECT, COUNTERPARTY, asset_index, size_q, 0)
        }
        AssetIntentKind::PushAuthMark => env.build_retained_auth_mark(asset_index, stale_price),
        AssetIntentKind::PushEwmaMark => env.build_retained_ewma_mark(asset_index, stale_price),
        AssetIntentKind::ConfigureAuthMark => {
            env.build_retained_auth_config(asset_index, stale_price)
        }
        AssetIntentKind::ConfigureEwmaMark => {
            env.build_retained_ewma_config(asset_index, stale_price, 1, 0)
        }
        AssetIntentKind::ConfigureHybridOracle => {
            let feed = [0x5au8; 32];
            env.build_retained_hybrid_oracle_config(
                asset_index,
                5,
                101,
                0,
                [feed, [0; 32], [0; 32]],
                &[oracle_account.expect("hybrid oracle fixture")],
                1,
                0,
            )
        }
        AssetIntentKind::InsuranceTopUp => {
            env.build_retained_insurance_domain_top_up_for_actor(authority_actor, domain, AMOUNT)
        }
        AssetIntentKind::BackingTopUp => {
            env.build_retained_backing_bucket_top_up_for_actor(authority_actor, domain, AMOUNT, 100)
        }
        AssetIntentKind::InsuranceWithdrawal => {
            env.build_retained_insurance_withdrawal_for_actor(authority_actor, asset_index, AMOUNT)
        }
        AssetIntentKind::BackingFeePolicy => {
            env.build_retained_backing_fee_policy_for_actor(authority_actor, domain, 100, 5_000)
        }
        AssetIntentKind::ResolveMarket => env.build_retained_resolve_market(),
        AssetIntentKind::ResolvePolicy => env.build_retained_permissionless_resolve_policy(17, 29),
    }
}

fn configure_replacement_asset(
    env: &mut V16Svm,
    kind: AssetIntentKind,
    asset_index: u16,
    authority_actor: usize,
) -> Result<(), String> {
    match kind {
        AssetIntentKind::TradeNoCpi
        | AssetIntentKind::TradeCpi
        | AssetIntentKind::BatchTradeNoCpi
        | AssetIntentKind::BatchTradeCpi
        | AssetIntentKind::PushAuthMark
        | AssetIntentKind::ConfigureAuthMark
        | AssetIntentKind::ConfigureEwmaMark
        | AssetIntentKind::ConfigureHybridOracle => env
            .configure_auth_mark(false, asset_index, 4, INITIAL_PRICE)
            .map(|_| ())
            .map_err(|error| format!("configure replacement AuthMark: {error}")),
        AssetIntentKind::PushEwmaMark => env
            .configure_ewma_mark(asset_index, 4, INITIAL_PRICE, 1, 0)
            .map(|_| ())
            .map_err(|error| format!("configure replacement EwmaMark: {error}")),
        AssetIntentKind::InsuranceWithdrawal => env
            .top_up_insurance_domain_for_actor(authority_actor, asset_index * 2, 1_000)
            .map(|_| ())
            .map_err(|error| format!("fund replacement insurance reserve: {error}")),
        _ => Ok(()),
    }
}

fn discover_one_asset_generation_replay(
    mut seed: [u8; 32],
    kind: AssetIntentKind,
) -> Result<AssetGenerationDiscovery, String> {
    const ASSET: u16 = 1;
    const AUTHORITY_ACTOR: usize = 2;
    const ACTIVATION_PAYER: usize = 3;
    seed[0] ^= 0xc9;
    seed[1] ^= kind.discriminator();
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let oracle_account = configure_old_asset_intent(&mut env, kind, ASSET, AUTHORITY_ACTOR)?;
    if kind == AssetIntentKind::InsuranceWithdrawal {
        env.top_up_insurance_domain_for_actor(AUTHORITY_ACTOR, ASSET * 2, 1_000)
            .map_err(|error| format!("fund old insurance reserve: {error}"))?;
    }
    let old_asset_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained = retained_asset_intent(&mut env, kind, ASSET, AUTHORITY_ACTOR, oracle_account);
    if kind == AssetIntentKind::InsuranceWithdrawal {
        env.withdraw_insurance_asset(AUTHORITY_ACTOR, ASSET, 1_000)
            .map_err(|error| format!("clear old insurance reserve: {error}"))?;
    }

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("configure permissionless asset activation: {error}"))?;
    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("retire old asset: {error}"))?;
    env.warp_to_slot(4);
    if kind.uses_actor_authorities() {
        env.activate_permissionless_asset_with_actor_authorities(
            ACTIVATION_PAYER,
            ASSET,
            4,
            INITIAL_PRICE,
            AUTHORITY_ACTOR,
            AUTHORITY_ACTOR,
            AUTHORITY_ACTOR,
            AUTHORITY_ACTOR,
            1,
        )
        .map_err(|error| format!("activate actor-authority replacement asset: {error}"))?;
    } else {
        env.activate_permissionless_asset(ACTIVATION_PAYER, ASSET, 4, INITIAL_PRICE, 1)
            .map_err(|error| format!("activate replacement asset: {error}"))?;
    }
    let new_asset_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_asset_id <= old_asset_id {
        return Err(format!(
            "asset generation did not advance: {old_asset_id} -> {new_asset_id}"
        ));
    }
    configure_replacement_asset(&mut env, kind, ASSET, AUTHORITY_ACTOR)?;
    if matches!(
        kind,
        AssetIntentKind::PushAuthMark
            | AssetIntentKind::PushEwmaMark
            | AssetIntentKind::ConfigureAuthMark
            | AssetIntentKind::ConfigureEwmaMark
            | AssetIntentKind::ConfigureHybridOracle
    ) {
        env.warp_to_slot(5);
    }
    if kind == AssetIntentKind::ConfigureHybridOracle {
        env.set_clock(5, 101);
    }

    let before = fingerprint(&env);
    let result = env.land_retained(retained);
    let after = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} asset-generation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale asset transaction succeeded without an observable state delta"
                ));
            }
            Ok(AssetGenerationDiscovery {
                kind,
                old_asset_id,
                new_asset_id,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale asset transaction did not roll back exactly"
                ));
            }
            Ok(AssetGenerationDiscovery {
                kind,
                old_asset_id,
                new_asset_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
            })
        }
    }
}

pub fn discover_asset_generation_replays(
    seed: [u8; 32],
) -> Result<Vec<AssetGenerationDiscovery>, String> {
    let trace = std::env::var_os("PERCOLATOR_DISCOVERY_TRACE").is_some();
    let mut discoveries = Vec::with_capacity(AssetIntentKind::ALL.len());
    for kind in AssetIntentKind::ALL {
        if trace {
            eprintln!("asset-generation probe start: {kind:?}");
        }
        let discovery = discover_one_asset_generation_replay(seed, kind).map_err(|error| {
            if trace {
                eprintln!("asset-generation probe error: {kind:?}: {error}");
            }
            error
        })?;
        if trace {
            eprintln!(
                "asset-generation probe finish: {kind:?} violation={}",
                discovery.is_violation()
            );
        }
        discoveries.push(discovery);
    }
    Ok(discoveries)
}
