//! Per-instruction handlers, one file per active discriminator. Each
//! sub-module exposes `handler(ctx, ...)` and a `#[derive(Accounts)]`
//! struct named after the instruction. The `#[program]` block in
//! `crate::lib` binds each handler to its `#[discrim = N]`.

pub mod catchup_accrue;
pub mod close_account;
pub mod convert_released_pnl;
pub mod deposit_collateral;
pub mod deposit_fee_credits;
pub mod liquidate_at_oracle;
pub mod push_hyperp_mark;
pub mod reclaim_empty_account;
pub mod resolve_market;
pub mod resolve_permissionless;
pub mod settle_account;
pub mod top_up_insurance;
pub mod trade_no_cpi;
pub mod update_authority;
pub mod update_config;
pub mod withdraw_collateral;
pub mod withdraw_insurance;
pub mod withdraw_insurance_limited;

// Re-export only the `#[derive(Accounts)]` types so `crate::lib`'s
// `#[program]` block can refer to them without an `instructions::*`
// glob (which would pollute the crate root with each handler's
// `handler` fn). Bring more types in as the rest of the 28 land.
pub use catchup_accrue::CatchupAccrue;
pub use close_account::CloseAccount;
pub use convert_released_pnl::ConvertReleasedPnl;
pub use deposit_collateral::DepositCollateral;
pub use deposit_fee_credits::DepositFeeCredits;
pub use liquidate_at_oracle::LiquidateAtOracle;
pub use push_hyperp_mark::PushHyperpMark;
pub use reclaim_empty_account::ReclaimEmptyAccount;
pub use resolve_market::ResolveMarket;
pub use resolve_permissionless::ResolvePermissionless;
pub use settle_account::SettleAccount;
pub use top_up_insurance::TopUpInsurance;
pub use trade_no_cpi::TradeNoCpi;
pub use update_authority::UpdateAuthority;
pub use update_config::UpdateConfig;
pub use withdraw_collateral::WithdrawCollateral;
pub use withdraw_insurance::WithdrawInsurance;
pub use withdraw_insurance_limited::WithdrawInsuranceLimited;
