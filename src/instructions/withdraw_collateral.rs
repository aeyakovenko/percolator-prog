//! Tag 4 — WithdrawCollateral. Owner-signed withdrawal of capital
//! from the engine to the user's SPL token ATA. Live markets only.
//!
//! Wire format:
//!   `[4u8] [user_idx: u16] [amount: u64]`   (11 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. user (Signer)
//!   2. slab (Account<SlabHeader>, mut)
//!   3. vault (UncheckedAccount, mut)
//!   4. user_ata (UncheckedAccount, mut)
//!   5. vault_pda (UncheckedAccount)
//!   6. token_program (UncheckedAccount)
//!   7. clock (Sysvar<Clock>)
//!   8. oracle (UncheckedAccount)

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    check_idx, compute_current_funding_rate_e9, ensure_market_accrued_to_now_with_policy,
    price_move_residual_dt, read_price_and_stamp, reject_any_target_lag, sync_account_fee,
};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct WithdrawCollateral {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: UncheckedAccount,
    /// CHECK: validated as user's SPL token ATA in the handler.
    #[account(mut)]
    pub user_ata: UncheckedAccount,
    /// CHECK: must equal the program-derived vault authority PDA.
    pub vault_pda: UncheckedAccount,
    /// CHECK: must be the SPL Token program.
    pub token_program: UncheckedAccount,
    pub clock: Sysvar<Clock>,
    /// CHECK: foreign-program oracle (Pyth/Chainlink) — decoded by `oracle::*`.
    pub oracle: UncheckedAccount,
}

pub fn handler(
    ctx: &mut Context<WithdrawCollateral>,
    user_idx: u16,
    amount: u64,
) -> Result<()> {
    cpi::verify_token_program(ctx.accounts.token_program.account())?;
    if amount == 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;
    let user_addr = *ctx.accounts.user.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let vault_view = *ctx.accounts.vault.account();
    let user_ata_view = *ctx.accounts.user_ata.account();
    let vault_pda_view = *ctx.accounts.vault_pda.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let oracle_view = *ctx.accounts.oracle.account();
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;
    let mut config = state::read_config(data);
    let mint = Address::from(config.collateral_mint);

    let derived_pda = cpi::derive_vault_authority_with_bump(
        &crate::ID,
        &slab_addr,
        config.vault_authority_bump,
    )?;
    if vault_pda_view.address() != &derived_pda {
        return Err(ProgramError::InvalidArgument.into());
    }

    cpi::verify_vault(
        &vault_view,
        &derived_pda,
        &mint,
        &Address::from(config.vault_pubkey),
    )?;
    cpi::verify_token_account(&user_ata_view, &user_addr, &mint)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Anti-retroactivity (§5.5): capture funding rate before oracle read.
    let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
    let price = {
        let is_hyperp = oracle::is_hyperp_mode(&config);
        let px = if is_hyperp {
            let eng = zc::engine_ref(data)?;
            let p_last = eng.last_oracle_price;
            let price_move_dt = price_move_residual_dt(eng, clock.slot)?;
            let cap_bps = eng.params.max_price_move_bps_per_slot;
            let oi_any = eng.oi_eff_long_q != 0 || eng.oi_eff_short_q != 0;
            oracle::get_engine_oracle_price_e6(
                p_last,
                price_move_dt,
                clock.slot,
                clock.unix_timestamp,
                &mut config,
                &oracle_view,
                cap_bps,
                oi_any,
            )?
        } else {
            read_price_and_stamp(
                &mut config,
                &oracle_view,
                clock.unix_timestamp,
                clock.slot,
                data,
            )?
        };
        state::write_config(data, &config);
        px
    };

    let engine = zc::engine_mut(data)?;
    check_idx(engine, user_idx)?;
    let owner = engine.accounts[user_idx as usize].owner;
    if !crate::policy::owner_ok(owner, user_addr.to_bytes()) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }

    if config.unit_scale != 0 && amount % config.unit_scale as u64 != 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    let (units_requested, _) = crate::units::base_to_units(amount, config.unit_scale);

    let admit_h_min = engine.params.h_min;
    let admit_h_max = engine.params.h_max;
    // accrue → sync → op.
    ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9)?;
    reject_any_target_lag(&config, engine)?;
    sync_account_fee(engine, &config, user_idx, clock.slot)?;
    let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
    engine
        .withdraw_not_atomic(
            user_idx,
            units_requested as u128,
            price,
            clock.slot,
            funding_rate_e9,
            admit_h_min,
            admit_h_max,
            admit_threshold,
        )
        .map_err(map_risk_error)?;
    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }

    let base_to_pay = crate::units::units_to_base_checked(units_requested, config.unit_scale)
        .ok_or(PercolatorError::EngineOverflow)?;

    let slab_bytes = slab_addr.to_bytes();
    let bump_arr: [u8; 1] = [config.vault_authority_bump];
    let seeds: [&[u8]; 3] = [b"vault", &slab_bytes, &bump_arr];

    cpi::withdraw(
        &token_program_view,
        &vault_view,
        &user_ata_view,
        &vault_pda_view,
        base_to_pay,
        &seeds,
    )?;
    Ok(())
}
