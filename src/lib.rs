//! Percolator: perpetual-markets Solana program built on Anchor v2.
//!
//! Migration in progress. The legacy native-Solana implementation is
//! preserved at `percolator-prog/` (untouched) and at
//! `percolator-prog-v2/src/percolator.rs` (in-tree reference, no
//! longer compiled — see `[lib].path` in Cargo.toml).

#![no_std]
#![allow(unexpected_cfgs)]

extern crate alloc;

use anchor_lang_v2::prelude::*;

declare_id!("Perco1ator111111111111111111111111111111111");

pub mod constants;
pub mod cpi;
pub mod errors;
pub mod guards;
pub mod instructions;
pub mod matcher_abi;
pub mod oracle;
pub mod policy;
pub mod processor;
pub mod risk_buffer;
pub mod state;
pub mod units;
pub mod zc;

use instructions::{
    AdminForceCloseAccount, CatchupAccrue, CloseAccount, CloseSlab, ConvertReleasedPnl,
    DepositCollateral, DepositFeeCredits, ForceCloseResolved, InitLp, InitMarket, InitMarketArgs,
    InitUser, LiquidateAtOracle, PushHyperpMark, ReclaimEmptyAccount, ResolveMarket,
    ResolvePermissionless, SettleAccount, TopUpInsurance, TradeNoCpi, UpdateAuthority,
    UpdateConfig, WithdrawCollateral, WithdrawInsurance, WithdrawInsuranceLimited,
};
// The `#[program]` macro looks for each Accounts struct's auto-generated
// `__client_accounts_<name>` module at `super::` (= the crate root). Our
// Accounts structs live in submodules under `instructions/`, so re-export
// each one at the crate root.
#[doc(hidden)]
pub use instructions::admin_force_close_account::__client_accounts_adminforcecloseaccount;
#[doc(hidden)]
pub use instructions::catchup_accrue::__client_accounts_catchupaccrue;
#[doc(hidden)]
pub use instructions::close_account::__client_accounts_closeaccount;
#[doc(hidden)]
pub use instructions::close_slab::__client_accounts_closeslab;
#[doc(hidden)]
pub use instructions::convert_released_pnl::__client_accounts_convertreleasedpnl;
#[doc(hidden)]
pub use instructions::deposit_collateral::__client_accounts_depositcollateral;
#[doc(hidden)]
pub use instructions::deposit_fee_credits::__client_accounts_depositfeecredits;
#[doc(hidden)]
pub use instructions::force_close_resolved::__client_accounts_forcecloseresolved;
#[doc(hidden)]
pub use instructions::init_lp::__client_accounts_initlp;
#[doc(hidden)]
pub use instructions::init_market::__client_accounts_initmarket;
#[doc(hidden)]
pub use instructions::init_user::__client_accounts_inituser;
#[doc(hidden)]
pub use instructions::liquidate_at_oracle::__client_accounts_liquidateatoracle;
#[doc(hidden)]
pub use instructions::push_hyperp_mark::__client_accounts_pushhyperpmark;
#[doc(hidden)]
pub use instructions::reclaim_empty_account::__client_accounts_reclaimemptyaccount;
#[doc(hidden)]
pub use instructions::resolve_market::__client_accounts_resolvemarket;
#[doc(hidden)]
pub use instructions::resolve_permissionless::__client_accounts_resolvepermissionless;
#[doc(hidden)]
pub use instructions::settle_account::__client_accounts_settleaccount;
#[doc(hidden)]
pub use instructions::top_up_insurance::__client_accounts_topupinsurance;
#[doc(hidden)]
pub use instructions::trade_no_cpi::__client_accounts_tradenocpi;
#[doc(hidden)]
pub use instructions::update_authority::__client_accounts_updateauthority;
#[doc(hidden)]
pub use instructions::update_config::__client_accounts_updateconfig;
#[doc(hidden)]
pub use instructions::withdraw_collateral::__client_accounts_withdrawcollateral;
#[doc(hidden)]
pub use instructions::withdraw_insurance::__client_accounts_withdrawinsurance;
#[doc(hidden)]
pub use instructions::withdraw_insurance_limited::__client_accounts_withdrawinsurancelimited;

#[cfg(not(feature = "no-entrypoint"))]
#[program]
pub mod percolator {
    use super::*;

    /// Smoke handler at discriminator 254 — kept until every legacy tag
    /// has a real handler. Wire format: `[254u8]`.
    #[discrim = 254]
    pub fn ping(_ctx: &mut Context<Ping>) -> Result<()> {
        Ok(())
    }

    /// Tag 9 — user tops up the insurance fund.
    /// See `instructions/top_up_insurance.rs`.
    #[discrim = 9]
    pub fn top_up_insurance(ctx: &mut Context<TopUpInsurance>, amount: u64) -> Result<()> {
        instructions::top_up_insurance::handler(ctx, amount)
    }

    /// Tag 13 — admin teardown of a fully-resolved + drained market.
    /// See `instructions/close_slab.rs`.
    #[discrim = 13]
    pub fn close_slab(ctx: &mut Context<CloseSlab>) -> Result<()> {
        instructions::close_slab::handler(ctx)
    }

    /// Tag 14 — admin tunes funding params + TVL/insurance cap.
    /// See `instructions/update_config.rs`.
    #[discrim = 14]
    pub fn update_config(
        ctx: &mut Context<UpdateConfig>,
        funding_horizon_slots: u64,
        funding_k_bps: u64,
        funding_max_premium_bps: i64,
        funding_max_e9_per_slot: i64,
        tvl_insurance_cap_mult: u16,
    ) -> Result<()> {
        instructions::update_config::handler(
            ctx,
            funding_horizon_slots,
            funding_k_bps,
            funding_max_premium_bps,
            funding_max_e9_per_slot,
            tvl_insurance_cap_mult,
        )
    }

    /// Tag 6 — bilateral on-chain trade (no matcher CPI).
    /// See `instructions/trade_no_cpi.rs`.
    #[discrim = 6]
    pub fn trade_no_cpi(
        ctx: &mut Context<TradeNoCpi>,
        lp_idx: u16,
        user_idx: u16,
        size: i128,
    ) -> Result<()> {
        instructions::trade_no_cpi::handler(ctx, lp_idx, user_idx, size)
    }

    /// Tag 0 — InitMarket. Admin materializes a fresh market.
    /// See `instructions/init_market.rs`.
    #[discrim = 0]
    pub fn init_market(ctx: &mut Context<InitMarket>, args: InitMarketArgs) -> Result<()> {
        instructions::init_market::handler(ctx, args)
    }

    /// Tag 1 — InitUser. Materialize a User account.
    /// See `instructions/init_user.rs`.
    #[discrim = 1]
    pub fn init_user(ctx: &mut Context<InitUser>, fee_payment: u64) -> Result<()> {
        instructions::init_user::handler(ctx, fee_payment)
    }

    /// Tag 2 — InitLP. Materialize an LP account with matcher binding.
    /// See `instructions/init_lp.rs`.
    #[discrim = 2]
    pub fn init_lp(
        ctx: &mut Context<InitLp>,
        matcher_program: [u8; 32],
        matcher_context: [u8; 32],
        fee_payment: u64,
    ) -> Result<()> {
        instructions::init_lp::handler(ctx, matcher_program, matcher_context, fee_payment)
    }

    /// Tag 3 — owner deposits collateral.
    /// See `instructions/deposit_collateral.rs`.
    #[discrim = 3]
    pub fn deposit_collateral(
        ctx: &mut Context<DepositCollateral>,
        user_idx: u16,
        amount: u64,
    ) -> Result<()> {
        instructions::deposit_collateral::handler(ctx, user_idx, amount)
    }

    /// Tag 4 — owner withdraws collateral to their SPL token ATA.
    /// See `instructions/withdraw_collateral.rs`.
    #[discrim = 4]
    pub fn withdraw_collateral(
        ctx: &mut Context<WithdrawCollateral>,
        user_idx: u16,
        amount: u64,
    ) -> Result<()> {
        instructions::withdraw_collateral::handler(ctx, user_idx, amount)
    }

    /// Tag 7 — permissionless full-close liquidation at the live price.
    /// See `instructions/liquidate_at_oracle.rs`.
    #[discrim = 7]
    pub fn liquidate_at_oracle(
        ctx: &mut Context<LiquidateAtOracle>,
        target_idx: u16,
    ) -> Result<()> {
        instructions::liquidate_at_oracle::handler(ctx, target_idx)
    }

    /// Tag 8 — owner closes their account (live or resolved markets).
    /// See `instructions/close_account.rs`.
    #[discrim = 8]
    pub fn close_account(ctx: &mut Context<CloseAccount>, user_idx: u16) -> Result<()> {
        instructions::close_account::handler(ctx, user_idx)
    }

    /// Tag 17 — Hyperp-only mark-push.
    /// See `instructions/push_hyperp_mark.rs`.
    #[discrim = 17]
    pub fn push_hyperp_mark(
        ctx: &mut Context<PushHyperpMark>,
        price_e6: u64,
        timestamp: i64,
    ) -> Result<()> {
        instructions::push_hyperp_mark::handler(ctx, price_e6, timestamp)
    }

    /// Tag 19 — admin resolves a live market (Ordinary or Degenerate).
    /// See `instructions/resolve_market.rs`.
    #[discrim = 19]
    pub fn resolve_market(ctx: &mut Context<ResolveMarket>, mode: u8) -> Result<()> {
        instructions::resolve_market::handler(ctx, mode)
    }

    /// Tag 20 — unbounded insurance withdrawal (resolved markets only).
    /// See `instructions/withdraw_insurance.rs`.
    #[discrim = 20]
    pub fn withdraw_insurance(ctx: &mut Context<WithdrawInsurance>) -> Result<()> {
        instructions::withdraw_insurance::handler(ctx)
    }

    /// Tag 21 — admin force-close on resolved markets.
    /// See `instructions/admin_force_close_account.rs`.
    #[discrim = 21]
    pub fn admin_force_close_account(
        ctx: &mut Context<AdminForceCloseAccount>,
        user_idx: u16,
    ) -> Result<()> {
        instructions::admin_force_close_account::handler(ctx, user_idx)
    }

    /// Tag 23 — bounded live insurance withdrawal.
    /// See `instructions/withdraw_insurance_limited.rs`.
    #[discrim = 23]
    pub fn withdraw_insurance_limited(
        ctx: &mut Context<WithdrawInsuranceLimited>,
        amount: u64,
    ) -> Result<()> {
        instructions::withdraw_insurance_limited::handler(ctx, amount)
    }

    /// Tag 29 — permissionless degenerate resolve after the hard-timeout
    /// staleness window matures. Payload-less.
    /// See `instructions/resolve_permissionless.rs`.
    #[discrim = 29]
    pub fn resolve_permissionless(ctx: &mut Context<ResolvePermissionless>) -> Result<()> {
        instructions::resolve_permissionless::handler(ctx)
    }

    /// Tag 25 — permissionless flat-account reclaim.
    /// See `instructions/reclaim_empty_account.rs`.
    #[discrim = 25]
    pub fn reclaim_empty_account(
        ctx: &mut Context<ReclaimEmptyAccount>,
        user_idx: u16,
    ) -> Result<()> {
        instructions::reclaim_empty_account::handler(ctx, user_idx)
    }

    /// Tag 26 — permissionless single-account settlement.
    /// See `instructions/settle_account.rs`.
    #[discrim = 26]
    pub fn settle_account(ctx: &mut Context<SettleAccount>, user_idx: u16) -> Result<()> {
        instructions::settle_account::handler(ctx, user_idx)
    }

    /// Tag 27 — direct fee-debt repayment (§10.3.1).
    /// See `instructions/deposit_fee_credits.rs`.
    #[discrim = 27]
    pub fn deposit_fee_credits(
        ctx: &mut Context<DepositFeeCredits>,
        user_idx: u16,
        amount: u64,
    ) -> Result<()> {
        instructions::deposit_fee_credits::handler(ctx, user_idx, amount)
    }

    /// Tag 28 — voluntary in-engine PnL conversion (no SPL CPI).
    /// See `instructions/convert_released_pnl.rs`.
    #[discrim = 28]
    pub fn convert_released_pnl(
        ctx: &mut Context<ConvertReleasedPnl>,
        user_idx: u16,
        amount: u64,
    ) -> Result<()> {
        instructions::convert_released_pnl::handler(ctx, user_idx, amount)
    }

    /// Tag 30 — permissionless force-close on resolved markets after
    /// `force_close_delay_slots` cooldown.
    /// See `instructions/force_close_resolved.rs`.
    #[discrim = 30]
    pub fn force_close_resolved(
        ctx: &mut Context<ForceCloseResolved>,
        user_idx: u16,
    ) -> Result<()> {
        instructions::force_close_resolved::handler(ctx, user_idx)
    }

    /// Tag 31 — permissionless market-clock catchup. Payload-less.
    /// See `instructions/catchup_accrue.rs`.
    #[discrim = 31]
    pub fn catchup_accrue(ctx: &mut Context<CatchupAccrue>) -> Result<()> {
        instructions::catchup_accrue::handler(ctx)
    }

    /// Tag 32 — rotate or burn one of four scoped authority pubkeys.
    /// See `instructions/update_authority.rs` for wire format + semantics.
    #[discrim = 32]
    pub fn update_authority(
        ctx: &mut Context<UpdateAuthority>,
        kind: u8,
        new_pubkey: [u8; 32],
    ) -> Result<()> {
        instructions::update_authority::handler(ctx, kind, new_pubkey)
    }
}

#[derive(Accounts)]
pub struct Ping {
    pub payer: Signer,
}
