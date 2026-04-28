//! Tag 0 — InitMarket. Admin materializes a fresh market: validates
//! all RiskParams + market-config invariants, writes the slab header
//! / config / engine, then captures the LastRestartSlot for the
//! restart-detection gate.
//!
//! Wire format (Borsh, LE FixInt). One single `args` struct so the
//! v2 macro auto-derives a Borsh decoder. Unlike the legacy
//! all-or-nothing 66-byte extended tail (R2 in the migration plan),
//! v2 always emits every field — funding overrides become
//! `Option<T>` (`None` = use wrapper default).
//!
//!   `[0u8] [InitMarketArgs ...]`
//!
//! Accounts (strict order, matches legacy):
//!   1. admin (Signer)
//!   2. slab (Account<SlabHeader>, `#[account(zeroed)]`) — pre-allocated,
//!      disc bytes all zero. Anchor stamps the disc on entry.
//!   3. mint (UncheckedAccount)            — SPL Token mint, validated manually
//!   4. vault (UncheckedAccount)           — vault token account, must be empty
//!   5. clock (Sysvar<Clock>)
//!   6. oracle (UncheckedAccount)          — Pyth/Chainlink for non-Hyperp init
//!
//! Returns `AlreadyInitialized` semantically via Anchor's
//! `ConstraintZero` (`Custom(2004)`) when the slab's disc bytes are
//! already non-zero. The legacy `Custom(<AlreadyInitialized as u32>)`
//! is a deliberate v2 ABI change.

use crate::constants::{
    DEFAULT_FUNDING_HORIZON_SLOTS, DEFAULT_FUNDING_K_BPS, DEFAULT_FUNDING_MAX_E9_PER_SLOT,
    DEFAULT_FUNDING_MAX_PREMIUM_BPS, DEFAULT_MARK_EWMA_HALFLIFE_SLOTS, MAGIC,
    MAX_FORCE_CLOSE_DELAY_SLOTS, MAX_ORACLE_STALENESS_SECS,
    INSURANCE_WITHDRAW_DEPOSITS_ONLY_FLAG, INSURANCE_WITHDRAW_MAX_BPS_MASK,
    MAX_ABS_FUNDING_E9_PER_SLOT, MAX_ACCRUAL_DT_SLOTS, MIN_FUNDING_LIFETIME_SLOTS,
    MIN_CONF_FILTER_BPS, MAX_CONF_FILTER_BPS,
};
use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::state::{self, MarketConfig, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use percolator::{RiskParams, U128};
use solana_program_error::ProgramError;

#[derive(wincode::SchemaWrite, wincode::SchemaRead, Clone)]
pub struct RiskParamsArgs {
    pub h_min: u64,
    pub maintenance_margin_bps: u64,
    pub initial_margin_bps: u64,
    pub trading_fee_bps: u64,
    pub max_accounts: u64,
    pub new_account_fee: u128,
    pub h_max: u64,
    pub liquidation_fee_bps: u64,
    pub liquidation_fee_cap: u128,
    pub resolve_price_deviation_bps: u64,
    pub min_liquidation_abs: u128,
    pub min_nonzero_mm_req: u128,
    pub min_nonzero_im_req: u128,
    pub max_price_move_bps_per_slot: u64,
}

#[derive(wincode::SchemaWrite, wincode::SchemaRead, Clone)]
pub struct InitMarketArgs {
    pub admin: [u8; 32],
    pub collateral_mint: [u8; 32],
    pub index_feed_id: [u8; 32],
    pub max_staleness_secs: u64,
    pub conf_filter_bps: u16,
    pub invert: u8,
    pub unit_scale: u32,
    pub initial_mark_price_e6: u64,
    pub maintenance_fee_per_slot: u128,
    pub risk_params: RiskParamsArgs,
    /// `top bit` is the deposits-only mode flag (legacy
    /// `INSURANCE_WITHDRAW_DEPOSITS_ONLY_FLAG`); the lower 15 bits
    /// are the bps cap (0..=10_000).
    pub insurance_withdraw_max_bps: u16,
    pub insurance_withdraw_cooldown_slots: u64,
    pub permissionless_resolve_stale_slots: u64,
    /// `None` = wrapper default (`DEFAULT_FUNDING_HORIZON_SLOTS`).
    pub funding_horizon_slots: Option<u64>,
    pub funding_k_bps: Option<u64>,
    pub funding_max_premium_bps: Option<i64>,
    pub funding_max_e9_per_slot: Option<i64>,
    pub mark_min_fee: u64,
    pub force_close_delay_slots: u64,
}

#[derive(Accounts)]
pub struct InitMarket {
    pub admin: Signer,
    /// `zeroed`: pre-allocated slab whose disc bytes are all zero.
    /// Anchor stamps the disc on entry; the handler then writes
    /// MAGIC + header + config + engine.
    #[account(zeroed)]
    pub slab: Account<SlabHeader>,
    /// CHECK: SPL Token mint, validated by `pinocchio_token::Mint::from_account_view`.
    pub mint: UncheckedAccount,
    /// CHECK: vault token account, validated by `cpi::verify_vault_empty`.
    #[account(mut)]
    pub vault: UncheckedAccount,
    pub clock: Sysvar<Clock>,
    /// CHECK: Pyth / Chainlink oracle for non-Hyperp init; validated by `oracle::*`.
    pub oracle: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<InitMarket>, args: InitMarketArgs) -> Result<()> {
    let InitMarketArgs {
        admin,
        collateral_mint,
        index_feed_id,
        max_staleness_secs,
        conf_filter_bps,
        invert,
        unit_scale,
        initial_mark_price_e6,
        maintenance_fee_per_slot,
        risk_params: rp_args,
        insurance_withdraw_max_bps: iwm_raw,
        insurance_withdraw_cooldown_slots,
        permissionless_resolve_stale_slots,
        funding_horizon_slots: custom_funding_horizon,
        funding_k_bps: custom_funding_k,
        funding_max_premium_bps: custom_max_premium,
        funding_max_e9_per_slot: custom_max_per_slot,
        mark_min_fee,
        force_close_delay_slots,
    } = args;

    let admin_addr = *ctx.accounts.admin.address();
    let mint_addr = *ctx.accounts.mint.address();
    let vault_addr = *ctx.accounts.vault.account().address();
    let mint_view = *ctx.accounts.mint.account();
    let vault_view = *ctx.accounts.vault.account();
    let oracle_view = *ctx.accounts.oracle.account();
    let slab_addr = *ctx.accounts.slab.account().address();
    let clock = *ctx.accounts.clock;

    // Sanity: instruction data must agree with the signer + mint accounts.
    if admin != admin_addr.to_bytes() {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if collateral_mint != mint_addr.to_bytes() {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    // SPL Token Mint shape: owner + LEN + initialized.
    if !mint_view.owned_by(&pinocchio_token::ID) {
        return Err(ProgramError::IllegalOwner.into());
    }
    {
        // pinocchio_token::Mint::from_account_view does length + owner;
        // the existence of a successful borrow + the LEN match is enough
        // to confirm the mint is a real SPL mint.
        let _mint = pinocchio_token::state::Mint::from_account_view(&mint_view)
            .map_err(|_| ProgramError::from(PercolatorError::InvalidMint))?;
    }

    // Boolean encoded as u8.
    if invert > 1 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if conf_filter_bps < MIN_CONF_FILTER_BPS || conf_filter_bps > MAX_CONF_FILTER_BPS {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if !crate::policy::init_market_scale_ok(unit_scale) {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    // RiskParams envelope checks (cheap fast-path before engine init).
    if rp_args.initial_margin_bps == 0 || rp_args.maintenance_margin_bps == 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if rp_args.initial_margin_bps > 10_000 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if rp_args.initial_margin_bps < rp_args.maintenance_margin_bps {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if rp_args.h_min == 0 || rp_args.h_max < rp_args.h_min {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    // Insurance-withdraw flag + bps unpack.
    let insurance_withdraw_deposits_only =
        (iwm_raw & INSURANCE_WITHDRAW_DEPOSITS_ONLY_FLAG) != 0;
    let insurance_withdraw_max_bps = iwm_raw & INSURANCE_WITHDRAW_MAX_BPS_MASK;
    if insurance_withdraw_max_bps > 10_000 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if insurance_withdraw_max_bps > 0 && insurance_withdraw_cooldown_slots == 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    if max_staleness_secs == 0 || max_staleness_secs > MAX_ORACLE_STALENESS_SECS {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    let is_hyperp = index_feed_id == [0u8; 32];
    if is_hyperp && initial_mark_price_e6 == 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    // Normalize Hyperp initial mark to engine-space (invert + scale).
    let initial_mark_price_e6 = if is_hyperp {
        let p = crate::policy::to_engine_price(initial_mark_price_e6, invert, unit_scale)
            .ok_or(PercolatorError::OracleInvalid)?;
        if p > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
        }
        p
    } else {
        initial_mark_price_e6
    };

    if rp_args.new_account_fee > percolator::MAX_VAULT_TVL {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if unit_scale > 0 && rp_args.new_account_fee % (unit_scale as u128) != 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    // Lifecycle invariants.
    if permissionless_resolve_stale_slots > 0 && force_close_delay_slots == 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if force_close_delay_slots > MAX_FORCE_CLOSE_DELAY_SLOTS {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if permissionless_resolve_stale_slots > 0 && rp_args.h_max > permissionless_resolve_stale_slots
    {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if permissionless_resolve_stale_slots > 0
        && permissionless_resolve_stale_slots > MAX_ACCRUAL_DT_SLOTS
    {
        return Err(PercolatorError::InvalidConfigParam.into());
    }
    // Non-Hyperp markets MUST opt into permissionless resolve, else
    // an admin burn would brick lifecycle.
    if !is_hyperp && permissionless_resolve_stale_slots == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    if let Some(h) = custom_funding_horizon {
        if h == 0 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }
    }
    if let Some(k) = custom_funding_k {
        if k > 100_000 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }
    }
    if let Some(mp) = custom_max_premium {
        if mp < 0 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }
    }
    if let Some(ms) = custom_max_per_slot {
        if ms < 0 || (ms as i128) > MAX_ABS_FUNDING_E9_PER_SLOT as i128 {
            return Err(PercolatorError::InvalidConfigParam.into());
        }
    }
    if (mark_min_fee as u128) > percolator::MAX_PROTOCOL_FEE_ABS {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    // F2: Hyperp permissionless-resolve liveness spoof defense.
    if is_hyperp && permissionless_resolve_stale_slots > 0 && mark_min_fee == 0 {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    // Anti-spam: at least one of the two account-cost dials must be on.
    if rp_args.new_account_fee == 0 && maintenance_fee_per_slot == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    // Build the engine's `RiskParams` from the wire form. Wrapper-
    // immutable fields are stamped with the deployment constants.
    let risk_params = RiskParams {
        maintenance_margin_bps: rp_args.maintenance_margin_bps,
        initial_margin_bps: rp_args.initial_margin_bps,
        trading_fee_bps: rp_args.trading_fee_bps,
        max_accounts: rp_args.max_accounts,
        liquidation_fee_bps: rp_args.liquidation_fee_bps,
        liquidation_fee_cap: U128::new(rp_args.liquidation_fee_cap),
        min_liquidation_abs: U128::new(rp_args.min_liquidation_abs),
        min_nonzero_mm_req: rp_args.min_nonzero_mm_req,
        min_nonzero_im_req: rp_args.min_nonzero_im_req,
        h_min: rp_args.h_min,
        h_max: rp_args.h_max,
        resolve_price_deviation_bps: rp_args.resolve_price_deviation_bps,
        max_accrual_dt_slots: MAX_ACCRUAL_DT_SLOTS,
        max_abs_funding_e9_per_slot: MAX_ABS_FUNDING_E9_PER_SLOT,
        max_active_positions_per_side: rp_args.max_accounts,
        min_funding_lifetime_slots: MIN_FUNDING_LIFETIME_SLOTS,
        max_price_move_bps_per_slot: rp_args.max_price_move_bps_per_slot,
    };

    // Engine RiskParams envelope checks (mirrors legacy prevalidation).
    if (risk_params.max_accounts as usize) > percolator::MAX_ACCOUNTS
        || risk_params.max_accounts == 0
    {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.maintenance_margin_bps > risk_params.initial_margin_bps
        || risk_params.initial_margin_bps > 10_000
    {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.trading_fee_bps > 10_000 || risk_params.liquidation_fee_bps > 10_000 {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.min_nonzero_mm_req == 0
        || risk_params.min_nonzero_mm_req >= risk_params.min_nonzero_im_req
        || risk_params.min_nonzero_im_req > percolator::MAX_VAULT_TVL
    {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.min_liquidation_abs.get() > risk_params.liquidation_fee_cap.get()
        || risk_params.liquidation_fee_cap.get() > percolator::MAX_PROTOCOL_FEE_ABS
    {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.h_min == 0 || risk_params.h_max < risk_params.h_min {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.resolve_price_deviation_bps > percolator::MAX_RESOLVE_PRICE_DEVIATION_BPS {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if risk_params.max_price_move_bps_per_slot == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    // Vault PDA derivation + emptiness check. We use `find_program_address`
    // (not the cached-bump variant) since this is genesis — there's no
    // stored bump yet.
    let (auth, bump) = cpi::derive_vault_authority(&crate::ID, &slab_addr);
    cpi::verify_vault_empty(&vault_view, &auth, &mint_addr, &vault_addr)?;

    // For non-Hyperp markets read a fresh oracle now and use it as
    // `init_price`; for Hyperp seed at the admin-chosen mark.
    let (init_price, init_publish_time) = if is_hyperp {
        (initial_mark_price_e6, 0i64)
    } else {
        let (fresh, publish_time) = oracle::read_engine_price_e6(
            &oracle_view,
            &index_feed_id,
            clock.unix_timestamp,
            max_staleness_secs,
            conf_filter_bps,
            invert,
            unit_scale,
        )?;
        if fresh == 0 || fresh > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
        }
        (fresh, publish_time)
    };

    // Capture LastRestartSlot for the cluster-restart freeze gate.
    let init_restart_slot = {
        use solana_sysvar::last_restart_slot::LastRestartSlot;
        use solana_sysvar::Sysvar as SolanaSysvar;
        SolanaSysvar::get()
            .map(|lrs: LastRestartSlot| lrs.last_restart_slot)
            .unwrap_or(0)
    };

    // Now switch into byte-window mode for the engine + body writes.
    slab_shape_guard(&ctx.accounts.slab)?;
    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;

    // Anchor's `zeroed` constraint guaranteed the disc was all-zero
    // before stamping. The body's zero-init comes from
    // `system::create_account` (runtime-guaranteed to zero new
    // allocations) and the program-owner check that prevents foreign
    // writes. `engine.init_in_place` then fully overwrites the engine
    // region; the config + header writes below cover their regions.
    // The risk buffer + generation table are read-as-zero on a fresh
    // slab and need no explicit init. A defensive byte-by-byte re-zero
    // of the entire 1.5 MB body cost ~1.5 M CU and was the migration's
    // sole budget killer for InitMarket.
    let engine = zc::engine_mut(data)?;
    engine
        .init_in_place(risk_params, clock.slot, init_price)
        .map_err(map_risk_error)?;

    let config = MarketConfig {
        collateral_mint: mint_addr.to_bytes(),
        vault_pubkey: vault_addr.to_bytes(),
        index_feed_id,
        max_staleness_secs,
        conf_filter_bps,
        vault_authority_bump: bump,
        invert,
        unit_scale,
        funding_horizon_slots: custom_funding_horizon.unwrap_or(DEFAULT_FUNDING_HORIZON_SLOTS),
        funding_k_bps: custom_funding_k.unwrap_or(DEFAULT_FUNDING_K_BPS),
        funding_max_premium_bps: custom_max_premium.unwrap_or(DEFAULT_FUNDING_MAX_PREMIUM_BPS),
        funding_max_e9_per_slot: custom_max_per_slot.unwrap_or(DEFAULT_FUNDING_MAX_E9_PER_SLOT),
        hyperp_authority: if is_hyperp {
            admin_addr.to_bytes()
        } else {
            [0u8; 32]
        },
        hyperp_mark_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
        last_oracle_publish_time: init_publish_time,
        last_effective_price_e6: if is_hyperp {
            initial_mark_price_e6
        } else {
            init_price
        },
        insurance_withdraw_max_bps,
        tvl_insurance_cap_mult: 0,
        insurance_withdraw_deposits_only: insurance_withdraw_deposits_only as u8,
        _iw_padding: [0u8; 3],
        insurance_withdraw_cooldown_slots,
        oracle_target_price_e6: init_price,
        oracle_target_publish_time: init_publish_time,
        last_hyperp_index_slot: if is_hyperp { clock.slot } else { 0 },
        last_mark_push_slot: if is_hyperp { clock.slot as u128 } else { 0 },
        last_insurance_withdraw_slot: 0,
        insurance_withdraw_deposit_remaining: 0,
        mark_ewma_e6: if is_hyperp { initial_mark_price_e6 } else { 0 },
        mark_ewma_last_slot: if is_hyperp { clock.slot } else { 0 },
        mark_ewma_halflife_slots: DEFAULT_MARK_EWMA_HALFLIFE_SLOTS,
        init_restart_slot,
        permissionless_resolve_stale_slots,
        last_good_oracle_slot: clock.slot,
        maintenance_fee_per_slot,
        fee_sweep_cursor_word: 0,
        fee_sweep_cursor_bit: 0,
        mark_min_fee,
        force_close_delay_slots,
        new_account_fee: rp_args.new_account_fee,
    };
    state::write_config(data, &config);

    let new_header = SlabHeader {
        magic: MAGIC,
        version: 0,
        bump,
        _padding: [0; 3],
        admin: admin_addr.to_bytes(),
        _reserved: [0; 24],
        // Default scoped authorities = creator. Operators carve them
        // off via UpdateAuthority { kind: AUTHORITY_INSURANCE / _OPERATOR }.
        insurance_authority: admin_addr.to_bytes(),
        insurance_operator: admin_addr.to_bytes(),
    };
    state::write_header(data, &new_header);
    state::write_req_nonce(data, 0);
    state::set_oracle_initialized(data);
    Ok(())
}
