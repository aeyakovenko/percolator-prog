use super::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, USER_DEPOSIT};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncarnationDiscovery {
    pub kind: PortfolioIntentKind,
    pub old_portfolio_id: u64,
    pub new_portfolio_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
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
