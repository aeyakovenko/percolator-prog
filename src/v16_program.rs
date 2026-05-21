//! Percolator v16 Solana wrapper.
//!
//! v16 is account-local: a market-group account stores `MarketGroupV16`, and
//! each trader/LP is an independently supplied `PortfolioAccountV16`. The
//! wrapper deliberately does not recreate the legacy global account slab.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use percolator::{
    AssetLifecycleV16, AssetStateV16, BackingBucketStatusV16, BackingBucketV16,
    CloseProgressLedgerV16, HealthCertV16, InsuranceCreditReservationV16, MarketGroupV16,
    MarketModeV16, PermissionlessCrankActionV16, PermissionlessCrankRequestV16,
    PermissionlessRecoveryReasonV16, PortfolioAccountV16, PortfolioLegV16, ProvenanceHeaderV16,
    RebalanceRequestV16, ResolvedCloseOutcomeV16, ResolvedPayoutLedgerV16,
    ResolvedPayoutReceiptV16, SideModeV16, SideV16, SourceCreditStateV16, TradeOutcomeV16,
    TradeRequestV16, V16Config, V16Error, V16Result, BOUND_SCALE, V16_DOMAIN_COUNT,
    V16_MAX_MARKET_SLOTS_N, V16_MAX_PORTFOLIO_ASSETS_N,
};
use solana_program::{
    account_info::AccountInfo,
    clock::Clock,
    declare_id,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction as SolInstruction},
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    sysvar::Sysvar,
};

declare_id!("Perco1ator111111111111111111111111111111111");

pub mod constants {
    use core::mem::size_of;
    use percolator::{
        EngineAssetSlotV16Account, MarketGroupV16HeaderAccount, PortfolioAccountV16Account,
        V16_MAX_MARKET_SLOTS_N,
    };

    pub const MAGIC: u64 = 0x5045_5243_5631_3600; // "PERCV16\0"
    pub const VERSION: u16 = 16;
    pub const KIND_MARKET: u8 = 1;
    pub const KIND_PORTFOLIO: u8 = 2;

    pub const HEADER_LEN: usize = 16;
    pub const WRAPPER_CONFIG_LEN: usize = 544;
    pub const ASSET_ORACLE_PROFILE_LEN: usize = 232;
    pub const MARKET_GROUP_LEN: usize = size_of::<MarketGroupV16HeaderAccount>();
    pub const MARKET_ASSET_SLOT_LEN: usize =
        size_of::<EngineAssetSlotV16Account>() + ASSET_ORACLE_PROFILE_LEN;
    pub const PORTFOLIO_STATE_LEN: usize = size_of::<PortfolioAccountV16Account>();
    // Runtime engine layout guards for the unsafe heap constructors below.
    // These are deliberately literal values for the pinned engine revision. If
    // an engine bump changes MarketGroupV16 or PortfolioAccountV16, compilation
    // must fail until new_market_group_boxed / new_portfolio_boxed and the
    // wire materializers are audited field-for-field.
    #[cfg(not(target_os = "solana"))]
    pub const MARKET_GROUP_RUNTIME_LEN: usize = 86_672;
    #[cfg(target_os = "solana")]
    pub const MARKET_GROUP_RUNTIME_LEN: usize = 82_544;
    #[cfg(not(target_os = "solana"))]
    pub const MARKET_GROUP_RUNTIME_ALIGN: usize = 16;
    #[cfg(target_os = "solana")]
    pub const MARKET_GROUP_RUNTIME_ALIGN: usize = 8;
    #[cfg(not(target_os = "solana"))]
    pub const MARKET_GROUP_RUNTIME_MODE_OFF: usize = 86_516;
    #[cfg(target_os = "solana")]
    pub const MARKET_GROUP_RUNTIME_MODE_OFF: usize = 82_396;
    #[cfg(not(target_os = "solana"))]
    pub const MARKET_GROUP_RUNTIME_RESOLVED_LEDGER_OFF: usize = 86_576;
    #[cfg(target_os = "solana")]
    pub const MARKET_GROUP_RUNTIME_RESOLVED_LEDGER_OFF: usize = 82_448;
    #[cfg(not(target_os = "solana"))]
    pub const PORTFOLIO_RUNTIME_LEN: usize = 22_960;
    #[cfg(target_os = "solana")]
    pub const PORTFOLIO_RUNTIME_LEN: usize = 22_664;
    #[cfg(not(target_os = "solana"))]
    pub const PORTFOLIO_RUNTIME_ALIGN: usize = 16;
    #[cfg(target_os = "solana")]
    pub const PORTFOLIO_RUNTIME_ALIGN: usize = 8;
    #[cfg(not(target_os = "solana"))]
    pub const PORTFOLIO_RUNTIME_LAST_FEE_SLOT_OFF: usize = 19_680;
    #[cfg(target_os = "solana")]
    pub const PORTFOLIO_RUNTIME_LAST_FEE_SLOT_OFF: usize = 19_672;
    #[cfg(not(target_os = "solana"))]
    pub const PORTFOLIO_RUNTIME_RESOLVED_RECEIPT_OFF: usize = 22_864;
    #[cfg(target_os = "solana")]
    pub const PORTFOLIO_RUNTIME_RESOLVED_RECEIPT_OFF: usize = 22_584;
    pub const MARKET_GROUP_OFF: usize = HEADER_LEN + WRAPPER_CONFIG_LEN;
    pub const MIN_MARKET_ACCOUNT_LEN: usize = MARKET_GROUP_OFF + MARKET_GROUP_LEN;
    pub const DEFAULT_MARKET_SLOT_CAPACITY: usize = V16_MAX_MARKET_SLOTS_N;
    pub const MARKET_ACCOUNT_LEN: usize =
        MARKET_GROUP_OFF + MARKET_GROUP_LEN + DEFAULT_MARKET_SLOT_CAPACITY * MARKET_ASSET_SLOT_LEN;
    pub const PORTFOLIO_ACCOUNT_LEN: usize = HEADER_LEN + PORTFOLIO_STATE_LEN;
    pub const MAX_MATCHER_TAIL_ACCOUNTS: usize = 32;
    pub const MATCHER_ABI_VERSION: u32 = 3;
    pub const MATCHER_CONTEXT_MIN_LEN: usize = 64;
    pub const ORACLE_LEG_CAP: usize = 3;
    pub const ORACLE_MODE_MANUAL: u8 = 0;
    pub const ORACLE_MODE_HYBRID_AFTER_HOURS: u8 = 1;
    pub const ORACLE_MODE_HYPERP: u8 = 2;
    pub const ORACLE_LEG_FLAG_DIVIDE_LEG2: u8 = 1 << 0;
    pub const ORACLE_LEG_FLAG_DIVIDE_LEG3: u8 = 1 << 1;
    pub const ORACLE_LEG_FLAGS_MASK: u8 = ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3;
    pub const SWITCHBOARD_RESULT_SCALE: u128 = 1_000_000_000_000;
    pub const DEFAULT_MARK_EWMA_HALFLIFE_SLOTS: u64 = 600;
    pub const MAX_DYNAMIC_TRADE_FEE_BPS: u64 = 10_000;
    pub const MIN_INSURANCE_WITHDRAW_FLOOR_UNITS: u128 = 10;
    pub const MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS: u64 = 6_480_000;
    pub const MAX_FORCE_CLOSE_DELAY_SLOTS: u64 = 10_000_000;
    // v16 exposes up to 64 market slots, but one portfolio may only carry the
    // largest active-leg count that fits the audited stale-trade and crank CU
    // envelope. Additional markets remain usable through separate portfolios.
    pub const WRAPPER_MAX_PORTFOLIO_ASSETS: u16 = 14;
}

const _: () = {
    assert!(core::mem::size_of::<MarketGroupV16>() == constants::MARKET_GROUP_RUNTIME_LEN);
    assert!(core::mem::align_of::<MarketGroupV16>() == constants::MARKET_GROUP_RUNTIME_ALIGN);
    assert!(
        core::mem::offset_of!(MarketGroupV16, mode) == constants::MARKET_GROUP_RUNTIME_MODE_OFF
    );
    assert!(
        core::mem::offset_of!(MarketGroupV16, resolved_payout_ledger)
            == constants::MARKET_GROUP_RUNTIME_RESOLVED_LEDGER_OFF
    );
    assert!(core::mem::size_of::<PortfolioAccountV16>() == constants::PORTFOLIO_RUNTIME_LEN);
    assert!(core::mem::align_of::<PortfolioAccountV16>() == constants::PORTFOLIO_RUNTIME_ALIGN);
    assert!(
        core::mem::offset_of!(PortfolioAccountV16, last_fee_slot)
            == constants::PORTFOLIO_RUNTIME_LAST_FEE_SLOT_OFF
    );
    assert!(
        core::mem::offset_of!(PortfolioAccountV16, resolved_payout_receipt)
            == constants::PORTFOLIO_RUNTIME_RESOLVED_RECEIPT_OFF
    );
};

pub mod error {
    use percolator::V16Error;
    use solana_program::program_error::ProgramError;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum PercolatorError {
        InvalidMagic,
        InvalidVersion,
        AlreadyInitialized,
        NotInitialized,
        InvalidAccountKind,
        InvalidAccountLen,
        ExpectedSigner,
        ExpectedWritable,
        Unauthorized,
        InvalidInstruction,
        InvalidMint,
        InvalidTokenAccount,
        InvalidVaultAccount,
        InvalidTokenProgram,
        EngineInvalidConfig,
        EngineArithmeticOverflow,
        EngineProvenanceMismatch,
        EngineHiddenLeg,
        EngineInvalidLeg,
        EngineStale,
        EngineBStale,
        EngineLockActive,
        EngineNonProgress,
        EngineRecoveryRequired,
        EngineCounterOverflow,
        EngineCounterUnderflow,
        OracleInvalid,
        OracleStale,
        OracleConfTooWide,
        InvalidOracleKey,
    }

    impl From<PercolatorError> for ProgramError {
        fn from(value: PercolatorError) -> Self {
            ProgramError::Custom(value as u32)
        }
    }

    pub fn map_v16_error(err: V16Error) -> ProgramError {
        let mapped = match err {
            V16Error::InvalidConfig => PercolatorError::EngineInvalidConfig,
            V16Error::ArithmeticOverflow => PercolatorError::EngineArithmeticOverflow,
            V16Error::ProvenanceMismatch => PercolatorError::EngineProvenanceMismatch,
            V16Error::HiddenLeg => PercolatorError::EngineHiddenLeg,
            V16Error::InvalidLeg => PercolatorError::EngineInvalidLeg,
            V16Error::Stale => PercolatorError::EngineStale,
            V16Error::BStale => PercolatorError::EngineBStale,
            V16Error::LockActive => PercolatorError::EngineLockActive,
            V16Error::NonProgress => PercolatorError::EngineNonProgress,
            V16Error::RecoveryRequired => PercolatorError::EngineRecoveryRequired,
            V16Error::CounterOverflow => PercolatorError::EngineCounterOverflow,
            V16Error::CounterUnderflow => PercolatorError::EngineCounterUnderflow,
        };
        mapped.into()
    }
}

pub mod state {
    use crate::{
        constants::{
            ASSET_ORACLE_PROFILE_LEN, HEADER_LEN, KIND_MARKET, KIND_PORTFOLIO, MAGIC,
            MARKET_GROUP_LEN, MARKET_GROUP_OFF, MIN_MARKET_ACCOUNT_LEN, ORACLE_LEG_CAP,
            ORACLE_LEG_FLAGS_MASK, ORACLE_MODE_HYBRID_AFTER_HOURS, ORACLE_MODE_HYPERP,
            ORACLE_MODE_MANUAL, PORTFOLIO_ACCOUNT_LEN, VERSION, WRAPPER_CONFIG_LEN,
        },
        error::PercolatorError,
    };
    use alloc::boxed::Box;
    use core::ptr::addr_of_mut;
    use percolator::{
        AssetStateV16, AssetStateV16Account, BackingBucketV16, BackingBucketV16Account,
        EngineAssetSlotV16Account, HealthCertV16Account, InsuranceCreditReservationV16,
        InsuranceCreditReservationV16Account, MarketGroupV16, MarketGroupV16HeaderAccount,
        MarketModeV16, PortfolioAccountV16, PortfolioAccountV16Account, PortfolioLegV16Account,
        ProvenanceHeaderV16, ProvenanceHeaderV16Account, ResolvedPayoutLedgerV16Account,
        ResolvedPayoutReceiptV16Account, SourceCreditStateV16, SourceCreditStateV16Account,
        V16ConfigAccount, V16Error, V16OptionalRecoveryReasonAccount, V16PodI128, V16PodU128,
        V16PodU64, V16_DOMAIN_COUNT, V16_MAX_MARKET_SLOTS_N, V16_MAX_PORTFOLIO_ASSETS_N,
    };
    use solana_program::program_error::ProgramError;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
    pub struct WrapperConfigV16 {
        pub admin: [u8; 32],
        pub collateral_mint: [u8; 32],
        pub maintenance_fee_per_slot: u128,
        pub trade_fee_base_bps: u64,
        pub permissionless_resolve_stale_slots: u64,
        pub force_close_delay_slots: u64,
        pub last_good_oracle_slot: u64,
        pub insurance_authority: [u8; 32],
        pub insurance_operator: [u8; 32],
        pub backing_bucket_authority: [u8; 32],
        pub asset_authority: [u8; 32],
        pub hyperp_mark_authority: [u8; 32],
        pub insurance_withdraw_deposit_remaining: u128,
        pub insurance_withdraw_max_bps: u16,
        pub liquidation_cranker_fee_share_bps: u16,
        pub maintenance_cranker_fee_share_bps: u16,
        pub backing_trade_fee_bps: u16,
        pub unit_scale: u32,
        pub conf_filter_bps: u16,
        pub insurance_withdraw_deposits_only: u8,
        pub oracle_mode: u8,
        pub oracle_leg_count: u8,
        pub oracle_leg_flags: u8,
        pub invert: u8,
        pub _padding0: u8,
        pub _padding1: [u8; 4],
        pub insurance_withdraw_cooldown_slots: u64,
        pub last_insurance_withdraw_slot: u64,
        pub max_staleness_secs: u64,
        pub hybrid_soft_stale_slots: u64,
        pub mark_ewma_e6: u64,
        pub mark_ewma_last_slot: u64,
        pub mark_ewma_halflife_slots: u64,
        pub mark_min_fee: u64,
        pub oracle_target_price_e6: u64,
        pub oracle_target_publish_time: i64,
        pub oracle_leg_feeds: [[u8; 32]; ORACLE_LEG_CAP],
        pub oracle_leg_prices_e6: [u64; ORACLE_LEG_CAP],
        pub oracle_leg_publish_times: [i64; ORACLE_LEG_CAP],
        pub _padding_tail: [u8; 8],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
    pub struct AssetOracleProfileV16 {
        pub oracle_mode: u8,
        pub oracle_leg_count: u8,
        pub oracle_leg_flags: u8,
        pub invert: u8,
        pub unit_scale: u32,
        pub conf_filter_bps: u16,
        pub _padding0: [u8; 6],
        pub max_staleness_secs: u64,
        pub hybrid_soft_stale_slots: u64,
        pub mark_ewma_e6: u64,
        pub mark_ewma_last_slot: u64,
        pub mark_ewma_halflife_slots: u64,
        pub mark_min_fee: u64,
        pub oracle_target_price_e6: u64,
        pub oracle_target_publish_time: i64,
        pub last_good_oracle_slot: u64,
        pub oracle_leg_feeds: [[u8; 32]; ORACLE_LEG_CAP],
        pub oracle_leg_prices_e6: [u64; ORACLE_LEG_CAP],
        pub oracle_leg_publish_times: [i64; ORACLE_LEG_CAP],
    }

    #[inline]
    fn read_u16(data: &[u8], off: usize) -> Result<u16, ProgramError> {
        let bytes: [u8; 2] = data
            .get(off..off + 2)
            .ok_or(PercolatorError::InvalidAccountLen)?
            .try_into()
            .unwrap();
        Ok(u16::from_le_bytes(bytes))
    }

    #[inline]
    fn read_u64(data: &[u8], off: usize) -> Result<u64, ProgramError> {
        let bytes: [u8; 8] = data
            .get(off..off + 8)
            .ok_or(PercolatorError::InvalidAccountLen)?
            .try_into()
            .unwrap();
        Ok(u64::from_le_bytes(bytes))
    }

    #[inline]
    fn write_header(data: &mut [u8], kind: u8) -> Result<(), ProgramError> {
        if data.len() < HEADER_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        data[0..8].copy_from_slice(&MAGIC.to_le_bytes());
        data[8..10].copy_from_slice(&VERSION.to_le_bytes());
        data[10] = kind;
        for b in data[11..HEADER_LEN].iter_mut() {
            *b = 0;
        }
        Ok(())
    }

    #[inline]
    fn check_header(data: &[u8], kind: u8) -> Result<(), ProgramError> {
        if data.len() < HEADER_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        if read_u64(data, 0)? != MAGIC {
            return Err(PercolatorError::NotInitialized.into());
        }
        if read_u16(data, 8)? != VERSION {
            return Err(PercolatorError::InvalidVersion.into());
        }
        if data[10] != kind {
            return Err(PercolatorError::InvalidAccountKind.into());
        }
        Ok(())
    }

    #[inline]
    pub fn is_initialized(data: &[u8]) -> bool {
        data.len() >= HEADER_LEN && read_u64(data, 0).ok() == Some(MAGIC)
    }

    #[inline]
    fn map_account_wire_error(_: V16Error) -> ProgramError {
        ProgramError::InvalidAccountData
    }

    #[inline]
    fn read_wrapper_config_from_bytes(data: &[u8]) -> Result<WrapperConfigV16, ProgramError> {
        let bytes = data
            .get(HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN)
            .ok_or(PercolatorError::InvalidAccountLen)?;
        let config = bytemuck::pod_read_unaligned(bytes);
        validate_wrapper_config(&config)?;
        Ok(config)
    }

    fn read_wrapper_config_boxed_from_bytes(
        data: &[u8],
    ) -> Result<Box<WrapperConfigV16>, ProgramError> {
        let bytes = data
            .get(HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN)
            .ok_or(PercolatorError::InvalidAccountLen)?;
        let mut boxed = Box::<WrapperConfigV16>::new_uninit();
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                boxed.as_mut_ptr() as *mut u8,
                WRAPPER_CONFIG_LEN,
            );
            let boxed = boxed.assume_init();
            validate_wrapper_config(boxed.as_ref())?;
            Ok(boxed)
        }
    }

    #[inline]
    fn validate_wrapper_config(config: &WrapperConfigV16) -> Result<(), ProgramError> {
        if !insurance_withdraw_policy_shape_ok(
            config.insurance_withdraw_max_bps,
            config.insurance_withdraw_deposits_only,
            config.insurance_withdraw_cooldown_slots,
        ) || config.liquidation_cranker_fee_share_bps > 10_000
            || config.maintenance_cranker_fee_share_bps > 10_000
            || config.backing_trade_fee_bps > 10_000
            || config.conf_filter_bps > 10_000
            || config.invert > 1
            || config._padding0 != 0
            || config._padding1 != [0u8; 4]
            || config._padding_tail != [0u8; 8]
            || config.oracle_leg_count as usize > ORACLE_LEG_CAP
            || (config.oracle_leg_flags & !ORACLE_LEG_FLAGS_MASK) != 0
        {
            return Err(ProgramError::InvalidAccountData);
        }

        match config.oracle_mode {
            ORACLE_MODE_MANUAL => {
                if config.oracle_leg_count != 0 || config.oracle_leg_flags != 0 {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            ORACLE_MODE_HYBRID_AFTER_HOURS => {
                if config.oracle_leg_count == 0
                    || config.max_staleness_secs == 0
                    || config.hybrid_soft_stale_slots == 0
                    || !valid_engine_oracle_price(config.mark_ewma_e6)
                    || !valid_engine_oracle_price(config.oracle_target_price_e6)
                    || !crate::oracle_v16::oracle_leg_config_ok(
                        config.oracle_leg_count,
                        config.oracle_leg_flags,
                        &config.oracle_leg_feeds,
                    )
                {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            ORACLE_MODE_HYPERP => {
                if config.oracle_leg_count != 0
                    || config.oracle_leg_flags != 0
                    || config.invert != 0
                    || config.unit_scale != 0
                    || config.conf_filter_bps != 0
                    || config.max_staleness_secs != 0
                    || config.hybrid_soft_stale_slots != 0
                    || !valid_engine_oracle_price(config.mark_ewma_e6)
                    || !valid_engine_oracle_price(config.oracle_target_price_e6)
                    || config.mark_ewma_halflife_slots == 0
                    || config.oracle_leg_feeds.iter().any(|f| *f != [0u8; 32])
                    || config.oracle_leg_prices_e6.iter().any(|p| *p != 0)
                    || config.oracle_leg_publish_times.iter().any(|t| *t != 0)
                {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }

        Ok(())
    }

    #[inline]
    pub(super) fn insurance_withdraw_policy_shape_ok(
        max_bps: u16,
        deposits_only: u8,
        cooldown_slots: u64,
    ) -> bool {
        if max_bps > 10_000 || deposits_only > 1 {
            return false;
        }
        if max_bps == 0 || deposits_only != 0 {
            return true;
        }
        max_bps < 10_000 && cooldown_slots != 0
    }

    #[inline]
    fn valid_engine_oracle_price(price: u64) -> bool {
        price != 0 && price <= percolator::MAX_ORACLE_PRICE
    }

    #[inline]
    pub fn validate_asset_oracle_profile(
        profile: &AssetOracleProfileV16,
    ) -> Result<(), ProgramError> {
        if profile.conf_filter_bps > 10_000
            || profile.invert > 1
            || profile._padding0 != [0u8; 6]
            || profile.oracle_leg_count as usize > ORACLE_LEG_CAP
            || (profile.oracle_leg_flags & !ORACLE_LEG_FLAGS_MASK) != 0
        {
            return Err(ProgramError::InvalidAccountData);
        }

        match profile.oracle_mode {
            ORACLE_MODE_MANUAL => {
                if profile.oracle_leg_count != 0
                    || profile.oracle_leg_flags != 0
                    || profile.oracle_leg_feeds.iter().any(|f| *f != [0u8; 32])
                    || profile.oracle_leg_prices_e6.iter().any(|p| *p != 0)
                    || profile.oracle_leg_publish_times.iter().any(|t| *t != 0)
                {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            ORACLE_MODE_HYBRID_AFTER_HOURS => {
                if profile.oracle_leg_count == 0
                    || profile.max_staleness_secs == 0
                    || profile.hybrid_soft_stale_slots == 0
                    || !valid_engine_oracle_price(profile.mark_ewma_e6)
                    || !valid_engine_oracle_price(profile.oracle_target_price_e6)
                    || profile.mark_ewma_halflife_slots == 0
                    || !crate::oracle_v16::oracle_leg_config_ok(
                        profile.oracle_leg_count,
                        profile.oracle_leg_flags,
                        &profile.oracle_leg_feeds,
                    )
                {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            ORACLE_MODE_HYPERP => {
                if profile.oracle_leg_count != 0
                    || profile.oracle_leg_flags != 0
                    || profile.invert != 0
                    || profile.unit_scale != 0
                    || profile.conf_filter_bps != 0
                    || profile.max_staleness_secs != 0
                    || profile.hybrid_soft_stale_slots != 0
                    || !valid_engine_oracle_price(profile.mark_ewma_e6)
                    || !valid_engine_oracle_price(profile.oracle_target_price_e6)
                    || profile.mark_ewma_halflife_slots == 0
                    || profile.oracle_leg_feeds.iter().any(|f| *f != [0u8; 32])
                    || profile.oracle_leg_prices_e6.iter().any(|p| *p != 0)
                    || profile.oracle_leg_publish_times.iter().any(|t| *t != 0)
                {
                    return Err(ProgramError::InvalidAccountData);
                }
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }

        Ok(())
    }

    #[inline]
    pub fn manual_asset_oracle_profile(initial_price: u64, slot: u64) -> AssetOracleProfileV16 {
        AssetOracleProfileV16 {
            oracle_mode: ORACLE_MODE_MANUAL,
            oracle_leg_count: 0,
            oracle_leg_flags: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 0,
            _padding0: [0u8; 6],
            max_staleness_secs: 0,
            hybrid_soft_stale_slots: 0,
            mark_ewma_e6: initial_price,
            mark_ewma_last_slot: slot,
            mark_ewma_halflife_slots: crate::constants::DEFAULT_MARK_EWMA_HALFLIFE_SLOTS,
            mark_min_fee: 0,
            oracle_target_price_e6: initial_price,
            oracle_target_publish_time: 0,
            last_good_oracle_slot: slot,
            oracle_leg_feeds: [[0u8; 32]; ORACLE_LEG_CAP],
            oracle_leg_prices_e6: [0u64; ORACLE_LEG_CAP],
            oracle_leg_publish_times: [0i64; ORACLE_LEG_CAP],
        }
    }

    pub fn asset_oracle_profile_from_config(config: &WrapperConfigV16) -> AssetOracleProfileV16 {
        AssetOracleProfileV16 {
            oracle_mode: config.oracle_mode,
            oracle_leg_count: config.oracle_leg_count,
            oracle_leg_flags: config.oracle_leg_flags,
            invert: config.invert,
            unit_scale: config.unit_scale,
            conf_filter_bps: config.conf_filter_bps,
            _padding0: [0u8; 6],
            max_staleness_secs: config.max_staleness_secs,
            hybrid_soft_stale_slots: config.hybrid_soft_stale_slots,
            mark_ewma_e6: config.mark_ewma_e6,
            mark_ewma_last_slot: config.mark_ewma_last_slot,
            mark_ewma_halflife_slots: config.mark_ewma_halflife_slots,
            mark_min_fee: config.mark_min_fee,
            oracle_target_price_e6: config.oracle_target_price_e6,
            oracle_target_publish_time: config.oracle_target_publish_time,
            last_good_oracle_slot: config.last_good_oracle_slot,
            oracle_leg_feeds: config.oracle_leg_feeds,
            oracle_leg_prices_e6: config.oracle_leg_prices_e6,
            oracle_leg_publish_times: config.oracle_leg_publish_times,
        }
    }

    #[inline]
    fn write_wrapper_config_to_bytes(
        data: &mut [u8],
        config: &WrapperConfigV16,
    ) -> Result<(), ProgramError> {
        validate_wrapper_config(config)?;
        data.get_mut(HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN)
            .ok_or(PercolatorError::InvalidAccountLen)?
            .copy_from_slice(bytemuck::bytes_of(config));
        Ok(())
    }

    #[inline]
    pub fn market_account_len_for_capacity(capacity: usize) -> Result<usize, ProgramError> {
        let dynamic_len = MarketGroupV16HeaderAccount::dynamic_market_group_account_len(
            capacity,
            ASSET_ORACLE_PROFILE_LEN,
        )
        .map_err(map_account_wire_error)?;
        MARKET_GROUP_OFF
            .checked_add(dynamic_len)
            .ok_or(PercolatorError::InvalidAccountLen.into())
    }

    #[inline]
    pub fn market_slot_capacity(data: &[u8]) -> Result<usize, ProgramError> {
        if data.len() < MARKET_GROUP_OFF + MARKET_GROUP_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        MarketGroupV16HeaderAccount::dynamic_asset_slot_capacity_from_account_len(
            data.len() - MARKET_GROUP_OFF,
            ASSET_ORACLE_PROFILE_LEN,
        )
        .map_err(map_account_wire_error)
    }

    #[inline]
    fn validate_market_dynamic_len(data: &[u8]) -> Result<usize, ProgramError> {
        let capacity = market_slot_capacity(data)?;
        MarketGroupV16HeaderAccount::validate_dynamic_market_group_account_len(
            data.len() - MARKET_GROUP_OFF,
            capacity,
            ASSET_ORACLE_PROFILE_LEN,
        )
        .map_err(map_account_wire_error)?;
        Ok(capacity)
    }

    #[inline]
    fn dynamic_slot_offset(asset_index: usize) -> Result<usize, ProgramError> {
        Ok(MARKET_GROUP_OFF
            + MarketGroupV16HeaderAccount::dynamic_asset_slot_offset(
                asset_index,
                ASSET_ORACLE_PROFILE_LEN,
            )
            .map_err(map_account_wire_error)?)
    }

    #[inline]
    fn asset_slot_range(asset_index: usize) -> Result<core::ops::Range<usize>, ProgramError> {
        let start = dynamic_slot_offset(asset_index)?;
        Ok(start..start + core::mem::size_of::<EngineAssetSlotV16Account>())
    }

    #[inline]
    fn asset_oracle_profile_range(
        data: &[u8],
        asset_index: usize,
    ) -> Result<core::ops::Range<usize>, ProgramError> {
        let capacity = market_slot_capacity(data)?;
        if asset_index >= capacity {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let slot_range = asset_slot_range(asset_index)?;
        let start = slot_range.end;
        Ok(start..start + ASSET_ORACLE_PROFILE_LEN)
    }

    pub fn read_asset_oracle_profile(
        data: &[u8],
        asset_index: usize,
    ) -> Result<AssetOracleProfileV16, ProgramError> {
        check_header(data, KIND_MARKET)?;
        let range = asset_oracle_profile_range(data, asset_index)?;
        let bytes = data.get(range).ok_or(PercolatorError::InvalidAccountLen)?;
        let profile: AssetOracleProfileV16 = bytemuck::pod_read_unaligned(bytes);
        validate_asset_oracle_profile(&profile)?;
        Ok(profile)
    }

    pub fn read_market_config_mode_and_capacity(
        data: &[u8],
    ) -> Result<(WrapperConfigV16, MarketModeV16, usize, usize), ProgramError> {
        check_header(data, KIND_MARKET)?;
        validate_market_dynamic_len(data)?;
        let config = read_wrapper_config_from_bytes(data)?;
        let header = market_header(data)?;
        let engine_config = header
            .config
            .try_to_runtime()
            .map_err(map_account_wire_error)?;
        Ok((
            config,
            decode_market_mode(header.mode)?,
            engine_config.max_market_slots as usize,
            header.asset_slot_capacity.get() as usize,
        ))
    }

    pub fn write_asset_oracle_profile(
        data: &mut [u8],
        asset_index: usize,
        profile: &AssetOracleProfileV16,
    ) -> Result<(), ProgramError> {
        check_header(data, KIND_MARKET)?;
        validate_asset_oracle_profile(profile)?;
        let range = asset_oracle_profile_range(data, asset_index)?;
        data.get_mut(range)
            .ok_or(PercolatorError::InvalidAccountLen)?
            .copy_from_slice(bytemuck::bytes_of(profile));
        Ok(())
    }

    pub fn activate_dynamic_asset_slot(
        data: &mut [u8],
        asset_index: usize,
        now_slot: u64,
        initial_price: u64,
    ) -> Result<AssetOracleProfileV16, ProgramError> {
        check_header(data, KIND_MARKET)?;
        let capacity = validate_market_dynamic_len(data)?;
        if asset_index >= capacity || asset_index > u32::MAX as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let mut header = *market_header(data)?;
        let engine_config = header
            .config
            .try_to_runtime()
            .map_err(map_account_wire_error)?;
        let old_n = engine_config.max_market_slots as usize;
        if asset_index != old_n {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let new_n = asset_index
            .checked_add(1)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        let mut slot = *asset_slot_wire(data, asset_index)?;
        header
            .grow_asset_slot_capacity_not_atomic(capacity as u32, new_n as u32)
            .map_err(map_account_wire_error)?;
        header
            .activate_empty_asset_slot_not_atomic(
                asset_index as u32,
                &mut slot,
                initial_price,
                now_slot,
            )
            .map_err(map_account_wire_error)?;
        *market_header_mut(data)? = header;
        *asset_slot_wire_mut(data, asset_index)? = slot;
        Ok(manual_asset_oracle_profile(initial_price, now_slot))
    }

    fn init_asset_oracle_profiles(
        data: &mut [u8],
        profile: &AssetOracleProfileV16,
    ) -> Result<(), ProgramError> {
        validate_asset_oracle_profile(profile)?;
        let bytes = bytemuck::bytes_of(profile);
        let capacity = market_slot_capacity(data)?;
        let mut i = 0usize;
        while i < capacity {
            let range = asset_oracle_profile_range(data, i)?;
            data.get_mut(range)
                .ok_or(PercolatorError::InvalidAccountLen)?
                .copy_from_slice(bytes);
            i += 1;
        }
        Ok(())
    }

    #[inline]
    fn market_header(data: &[u8]) -> Result<&MarketGroupV16HeaderAccount, ProgramError> {
        let bytes = data
            .get(
                MARKET_GROUP_OFF
                    ..MARKET_GROUP_OFF + core::mem::size_of::<MarketGroupV16HeaderAccount>(),
            )
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn market_header_mut(
        data: &mut [u8],
    ) -> Result<&mut MarketGroupV16HeaderAccount, ProgramError> {
        let bytes = data
            .get_mut(
                MARKET_GROUP_OFF
                    ..MARKET_GROUP_OFF + core::mem::size_of::<MarketGroupV16HeaderAccount>(),
            )
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes_mut(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn asset_slot_wire(
        data: &[u8],
        asset_index: usize,
    ) -> Result<&EngineAssetSlotV16Account, ProgramError> {
        let range = asset_slot_range(asset_index)?;
        let bytes = data.get(range).ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn asset_slot_wire_mut(
        data: &mut [u8],
        asset_index: usize,
    ) -> Result<&mut EngineAssetSlotV16Account, ProgramError> {
        let range = asset_slot_range(asset_index)?;
        let bytes = data
            .get_mut(range)
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes_mut(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn portfolio_wire(data: &[u8]) -> Result<&PortfolioAccountV16Account, ProgramError> {
        let bytes = data
            .get(HEADER_LEN..HEADER_LEN + core::mem::size_of::<PortfolioAccountV16Account>())
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn portfolio_wire_mut(
        data: &mut [u8],
    ) -> Result<&mut PortfolioAccountV16Account, ProgramError> {
        let bytes = data
            .get_mut(HEADER_LEN..HEADER_LEN + core::mem::size_of::<PortfolioAccountV16Account>())
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes_mut(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn encode_bool(v: bool) -> u8 {
        v as u8
    }

    #[inline]
    fn decode_bool(v: u8) -> Result<bool, ProgramError> {
        match v {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    #[inline]
    fn encode_market_mode(mode: MarketModeV16) -> u8 {
        match mode {
            MarketModeV16::Live => 0,
            MarketModeV16::Resolved => 1,
            MarketModeV16::Recovery => 2,
        }
    }

    #[inline]
    fn decode_market_mode(v: u8) -> Result<MarketModeV16, ProgramError> {
        match v {
            0 => Ok(MarketModeV16::Live),
            1 => Ok(MarketModeV16::Resolved),
            2 => Ok(MarketModeV16::Recovery),
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    #[inline]
    fn validate_non_min_i128(v: i128) -> Result<(), ProgramError> {
        if v == i128::MIN {
            Err(ProgramError::InvalidAccountData)
        } else {
            Ok(())
        }
    }

    #[inline]
    fn validate_fee_credits(v: i128) -> Result<(), ProgramError> {
        if v > 0 || v == i128::MIN {
            Err(ProgramError::InvalidAccountData)
        } else {
            Ok(())
        }
    }

    // The engine owns the Pod wire structs and discriminator validation. The
    // wrapper still materializes top-level accounts field-by-field so SBF never
    // places an entire MarketGroupV16/PortfolioAccountV16 temporary on stack.
    fn market_from_wire_boxed(data: &[u8]) -> Result<Box<MarketGroupV16>, ProgramError> {
        let capacity = validate_market_dynamic_len(data)?;
        let wire = market_header(data)?;
        let mut boxed = Box::<MarketGroupV16>::new_uninit();
        let ptr = boxed.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).market_group_id).write(wire.market_group_id);
            let engine_config = wire
                .config
                .try_to_runtime()
                .map_err(map_account_wire_error)?;
            // The persisted account may have more dynamic slots than the
            // fixed runtime window. Full trade/crank APIs still materialize
            // MarketGroupV16, so they are intentionally fail-closed until the
            // engine exposes dynamic operation APIs over selected slots.
            if capacity < engine_config.max_market_slots as usize
                || engine_config.max_market_slots as usize > V16_MAX_MARKET_SLOTS_N
            {
                return Err(ProgramError::InvalidAccountData);
            }
            if wire.asset_slot_capacity.get() as usize != capacity {
                return Err(ProgramError::InvalidAccountData);
            }
            let n = engine_config.max_market_slots as usize;
            addr_of_mut!((*ptr).config).write(engine_config);
            addr_of_mut!((*ptr).vault).write(wire.vault.get());
            addr_of_mut!((*ptr).insurance).write(wire.insurance.get());
            addr_of_mut!((*ptr).c_tot).write(wire.c_tot.get());
            addr_of_mut!((*ptr).pnl_pos_tot).write(wire.pnl_pos_tot.get());
            addr_of_mut!((*ptr).pnl_pos_bound_tot_num).write(wire.pnl_pos_bound_tot_num.get());
            addr_of_mut!((*ptr).pnl_pos_bound_tot).write(wire.pnl_pos_bound_tot.get());
            addr_of_mut!((*ptr).pnl_matured_pos_tot).write(wire.pnl_matured_pos_tot.get());
            let mut i = 0;
            while i < n {
                let slot = *asset_slot_wire(data, i)?;
                let long_domain = i * 2;
                let short_domain = long_domain + 1;
                addr_of_mut!((*ptr).assets[i]).write(
                    slot.asset
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                addr_of_mut!((*ptr).insurance_domain_budget[long_domain])
                    .write(slot.insurance_domain_budget_long.get());
                addr_of_mut!((*ptr).insurance_domain_budget[short_domain])
                    .write(slot.insurance_domain_budget_short.get());
                addr_of_mut!((*ptr).insurance_domain_spent[long_domain])
                    .write(slot.insurance_domain_spent_long.get());
                addr_of_mut!((*ptr).insurance_domain_spent[short_domain])
                    .write(slot.insurance_domain_spent_short.get());
                addr_of_mut!((*ptr).pending_domain_loss_barriers[long_domain])
                    .write(slot.pending_domain_loss_barrier_long.get());
                addr_of_mut!((*ptr).pending_domain_loss_barriers[short_domain])
                    .write(slot.pending_domain_loss_barrier_short.get());
                addr_of_mut!((*ptr).source_credit[long_domain]).write(
                    slot.source_credit_long
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                addr_of_mut!((*ptr).source_credit[short_domain]).write(
                    slot.source_credit_short
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                addr_of_mut!((*ptr).source_backing_buckets[long_domain]).write(
                    slot.backing_long
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                addr_of_mut!((*ptr).source_backing_buckets[short_domain]).write(
                    slot.backing_short
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                addr_of_mut!((*ptr).insurance_credit_reservations[long_domain]).write(
                    slot.insurance_reservation_long
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                addr_of_mut!((*ptr).insurance_credit_reservations[short_domain]).write(
                    slot.insurance_reservation_short
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
                i += 1;
            }
            while i < V16_MAX_MARKET_SLOTS_N {
                let mut asset = AssetStateV16::default();
                asset.lifecycle = percolator::AssetLifecycleV16::Disabled;
                asset.market_id = 0;
                addr_of_mut!((*ptr).assets[i]).write(asset);
                let long_domain = i * 2;
                let short_domain = long_domain + 1;
                addr_of_mut!((*ptr).insurance_domain_budget[long_domain])
                    .write(percolator::MAX_VAULT_TVL);
                addr_of_mut!((*ptr).insurance_domain_budget[short_domain])
                    .write(percolator::MAX_VAULT_TVL);
                addr_of_mut!((*ptr).insurance_domain_spent[long_domain]).write(0);
                addr_of_mut!((*ptr).insurance_domain_spent[short_domain]).write(0);
                addr_of_mut!((*ptr).pending_domain_loss_barriers[long_domain]).write(0);
                addr_of_mut!((*ptr).pending_domain_loss_barriers[short_domain]).write(0);
                addr_of_mut!((*ptr).source_credit[long_domain]).write(SourceCreditStateV16::EMPTY);
                addr_of_mut!((*ptr).source_credit[short_domain]).write(SourceCreditStateV16::EMPTY);
                addr_of_mut!((*ptr).source_backing_buckets[long_domain])
                    .write(BackingBucketV16::EMPTY);
                addr_of_mut!((*ptr).source_backing_buckets[short_domain])
                    .write(BackingBucketV16::EMPTY);
                addr_of_mut!((*ptr).insurance_credit_reservations[long_domain])
                    .write(InsuranceCreditReservationV16::EMPTY);
                addr_of_mut!((*ptr).insurance_credit_reservations[short_domain])
                    .write(InsuranceCreditReservationV16::EMPTY);
                i += 1;
            }
            addr_of_mut!((*ptr).materialized_portfolio_count)
                .write(wire.materialized_portfolio_count.get());
            addr_of_mut!((*ptr).stale_certificate_count).write(wire.stale_certificate_count.get());
            addr_of_mut!((*ptr).b_stale_account_count).write(wire.b_stale_account_count.get());
            addr_of_mut!((*ptr).negative_pnl_account_count)
                .write(wire.negative_pnl_account_count.get());
            addr_of_mut!((*ptr).risk_epoch).write(wire.risk_epoch.get());
            addr_of_mut!((*ptr).asset_set_epoch).write(wire.asset_set_epoch.get());
            addr_of_mut!((*ptr).asset_activation_count).write(wire.asset_activation_count.get());
            addr_of_mut!((*ptr).last_asset_activation_slot)
                .write(wire.last_asset_activation_slot.get());
            addr_of_mut!((*ptr).next_market_id).write(wire.next_market_id.get());
            addr_of_mut!((*ptr).oracle_epoch).write(wire.oracle_epoch.get());
            addr_of_mut!((*ptr).funding_epoch).write(wire.funding_epoch.get());
            addr_of_mut!((*ptr).slot_last).write(wire.slot_last.get());
            addr_of_mut!((*ptr).current_slot).write(wire.current_slot.get());
            addr_of_mut!((*ptr).bankruptcy_hlock_active)
                .write(decode_bool(wire.bankruptcy_hlock_active)?);
            addr_of_mut!((*ptr).threshold_stress_active)
                .write(decode_bool(wire.threshold_stress_active)?);
            addr_of_mut!((*ptr).loss_stale_active).write(decode_bool(wire.loss_stale_active)?);
            addr_of_mut!((*ptr).recovery_reason).write(
                wire.recovery_reason
                    .try_to_runtime()
                    .map_err(map_account_wire_error)?,
            );
            addr_of_mut!((*ptr).mode).write(decode_market_mode(wire.mode)?);
            addr_of_mut!((*ptr).resolved_slot).write(wire.resolved_slot.get());
            addr_of_mut!((*ptr).payout_snapshot).write(wire.payout_snapshot.get());
            addr_of_mut!((*ptr).payout_snapshot_pnl_pos_tot)
                .write(wire.payout_snapshot_pnl_pos_tot.get());
            addr_of_mut!((*ptr).payout_snapshot_captured)
                .write(decode_bool(wire.payout_snapshot_captured)?);
            addr_of_mut!((*ptr).resolved_payout_ledger).write(
                wire.resolved_payout_ledger
                    .try_to_runtime()
                    .map_err(map_account_wire_error)?,
            );
            let group = boxed.assume_init();
            group
                .assert_public_invariants()
                .map_err(map_account_wire_error)?;
            Ok(group)
        }
    }

    fn write_market_wire(data: &mut [u8], group: &MarketGroupV16) -> Result<(), ProgramError> {
        let capacity = validate_market_dynamic_len(data)?;
        if capacity < group.config.max_market_slots as usize
            || group.config.max_market_slots as usize > V16_MAX_MARKET_SLOTS_N
        {
            return Err(ProgramError::InvalidAccountData);
        }
        {
            let wire = market_header_mut(data)?;
            wire.market_group_id = group.market_group_id;
            wire.config = V16ConfigAccount::from_runtime(&group.config);
            wire.asset_slot_capacity = percolator::V16PodU32::new(capacity as u32);
            wire.vault = V16PodU128::new(group.vault);
            wire.insurance = V16PodU128::new(group.insurance);
            wire.c_tot = V16PodU128::new(group.c_tot);
            wire.pnl_pos_tot = V16PodU128::new(group.pnl_pos_tot);
            wire.pnl_pos_bound_tot_num = V16PodU128::new(group.pnl_pos_bound_tot_num);
            wire.pnl_pos_bound_tot = V16PodU128::new(group.pnl_pos_bound_tot);
            wire.pnl_matured_pos_tot = V16PodU128::new(group.pnl_matured_pos_tot);
            wire.materialized_portfolio_count = V16PodU64::new(group.materialized_portfolio_count);
            wire.stale_certificate_count = V16PodU64::new(group.stale_certificate_count);
            wire.b_stale_account_count = V16PodU64::new(group.b_stale_account_count);
            wire.negative_pnl_account_count = V16PodU64::new(group.negative_pnl_account_count);
            wire.risk_epoch = V16PodU64::new(group.risk_epoch);
            wire.asset_set_epoch = V16PodU64::new(group.asset_set_epoch);
            wire.asset_activation_count = V16PodU64::new(group.asset_activation_count);
            wire.last_asset_activation_slot = V16PodU64::new(group.last_asset_activation_slot);
            wire.next_market_id = V16PodU64::new(group.next_market_id);
            wire.oracle_epoch = V16PodU64::new(group.oracle_epoch);
            wire.funding_epoch = V16PodU64::new(group.funding_epoch);
            wire.slot_last = V16PodU64::new(group.slot_last);
            wire.current_slot = V16PodU64::new(group.current_slot);
        }
        let mut i = 0;
        let n = group.config.max_market_slots as usize;
        while i < n {
            let long_domain = i * 2;
            let short_domain = long_domain + 1;
            *asset_slot_wire_mut(data, i)? = EngineAssetSlotV16Account {
                asset: AssetStateV16Account::from_runtime(&group.assets[i]),
                insurance_domain_budget_long: V16PodU128::new(
                    group.insurance_domain_budget[long_domain],
                ),
                insurance_domain_budget_short: V16PodU128::new(
                    group.insurance_domain_budget[short_domain],
                ),
                insurance_domain_spent_long: V16PodU128::new(
                    group.insurance_domain_spent[long_domain],
                ),
                insurance_domain_spent_short: V16PodU128::new(
                    group.insurance_domain_spent[short_domain],
                ),
                pending_domain_loss_barrier_long: V16PodU64::new(
                    group.pending_domain_loss_barriers[long_domain],
                ),
                pending_domain_loss_barrier_short: V16PodU64::new(
                    group.pending_domain_loss_barriers[short_domain],
                ),
                source_credit_long: SourceCreditStateV16Account::from_runtime(
                    &group.source_credit[long_domain],
                ),
                source_credit_short: SourceCreditStateV16Account::from_runtime(
                    &group.source_credit[short_domain],
                ),
                backing_long: BackingBucketV16Account::from_runtime(
                    &group.source_backing_buckets[long_domain],
                ),
                backing_short: BackingBucketV16Account::from_runtime(
                    &group.source_backing_buckets[short_domain],
                ),
                insurance_reservation_long: InsuranceCreditReservationV16Account::from_runtime(
                    &group.insurance_credit_reservations[long_domain],
                ),
                insurance_reservation_short: InsuranceCreditReservationV16Account::from_runtime(
                    &group.insurance_credit_reservations[short_domain],
                ),
            };
            i += 1;
        }
        let wire = market_header_mut(data)?;
        wire.bankruptcy_hlock_active = encode_bool(group.bankruptcy_hlock_active);
        wire.threshold_stress_active = encode_bool(group.threshold_stress_active);
        wire.loss_stale_active = encode_bool(group.loss_stale_active);
        wire.recovery_reason =
            V16OptionalRecoveryReasonAccount::from_runtime(group.recovery_reason);
        wire.mode = encode_market_mode(group.mode);
        wire.resolved_slot = V16PodU64::new(group.resolved_slot);
        wire.payout_snapshot = V16PodU128::new(group.payout_snapshot);
        wire.payout_snapshot_pnl_pos_tot = V16PodU128::new(group.payout_snapshot_pnl_pos_tot);
        wire.payout_snapshot_captured = encode_bool(group.payout_snapshot_captured);
        wire.resolved_payout_ledger =
            ResolvedPayoutLedgerV16Account::from_runtime(&group.resolved_payout_ledger);
        Ok(())
    }

    fn portfolio_from_wire_boxed(
        wire: &PortfolioAccountV16Account,
    ) -> Result<Box<PortfolioAccountV16>, ProgramError> {
        let provenance_header = wire
            .provenance_header
            .try_to_runtime()
            .map_err(map_account_wire_error)?;
        if provenance_header.owner != wire.owner {
            return Err(ProgramError::InvalidAccountData);
        }
        let pnl = wire.pnl.get();
        let fee_credits = wire.fee_credits.get();
        validate_non_min_i128(pnl)?;
        validate_fee_credits(fee_credits)?;
        let capital = wire.capital.get();
        let reserved_pnl = wire.reserved_pnl.get();
        if reserved_pnl > pnl.max(0) as u128 {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut boxed = Box::<PortfolioAccountV16>::new_uninit();
        let ptr = boxed.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).provenance_header).write(provenance_header);
            addr_of_mut!((*ptr).owner).write(wire.owner);
            addr_of_mut!((*ptr).capital).write(capital);
            addr_of_mut!((*ptr).pnl).write(pnl);
            addr_of_mut!((*ptr).reserved_pnl).write(reserved_pnl);
            for d in 0..V16_DOMAIN_COUNT {
                addr_of_mut!((*ptr).source_claim_market_id[d])
                    .write(wire.source_claim_market_id[d].get());
                addr_of_mut!((*ptr).source_claim_bound_num[d])
                    .write(wire.source_claim_bound_num[d].get());
                addr_of_mut!((*ptr).source_claim_liened_num[d])
                    .write(wire.source_claim_liened_num[d].get());
                addr_of_mut!((*ptr).source_claim_counterparty_liened_num[d])
                    .write(wire.source_claim_counterparty_liened_num[d].get());
                addr_of_mut!((*ptr).source_claim_insurance_liened_num[d])
                    .write(wire.source_claim_insurance_liened_num[d].get());
                addr_of_mut!((*ptr).source_lien_effective_reserved[d])
                    .write(wire.source_lien_effective_reserved[d].get());
                addr_of_mut!((*ptr).source_lien_counterparty_backing_num[d])
                    .write(wire.source_lien_counterparty_backing_num[d].get());
                addr_of_mut!((*ptr).source_lien_insurance_backing_num[d])
                    .write(wire.source_lien_insurance_backing_num[d].get());
                addr_of_mut!((*ptr).source_claim_impaired_num[d])
                    .write(wire.source_claim_impaired_num[d].get());
                addr_of_mut!((*ptr).source_lien_impaired_effective_reserved[d])
                    .write(wire.source_lien_impaired_effective_reserved[d].get());
            }
            addr_of_mut!((*ptr).fee_credits).write(fee_credits);
            addr_of_mut!((*ptr).cancel_deposit_escrow).write(wire.cancel_deposit_escrow.get());
            addr_of_mut!((*ptr).last_fee_slot).write(wire.last_fee_slot.get());
            addr_of_mut!((*ptr).active_bitmap).write(wire.active_bitmap.map(|v| v.get()));
            for i in 0..V16_MAX_PORTFOLIO_ASSETS_N {
                addr_of_mut!((*ptr).legs[i]).write(
                    wire.legs[i]
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
            }
            addr_of_mut!((*ptr).health_cert).write(
                wire.health_cert
                    .try_to_runtime()
                    .map_err(map_account_wire_error)?,
            );
            addr_of_mut!((*ptr).stale_state).write(decode_bool(wire.stale_state)?);
            addr_of_mut!((*ptr).b_stale_state).write(decode_bool(wire.b_stale_state)?);
            addr_of_mut!((*ptr).rebalance_lock).write(decode_bool(wire.rebalance_lock)?);
            addr_of_mut!((*ptr).liquidation_lock).write(decode_bool(wire.liquidation_lock)?);
            addr_of_mut!((*ptr).close_progress).write(
                wire.close_progress
                    .try_to_runtime()
                    .map_err(map_account_wire_error)?,
            );
            addr_of_mut!((*ptr).resolved_payout_receipt).write(
                wire.resolved_payout_receipt
                    .try_to_runtime()
                    .map_err(map_account_wire_error)?,
            );
            Ok(boxed.assume_init())
        }
    }

    fn write_portfolio_wire(wire: &mut PortfolioAccountV16Account, account: &PortfolioAccountV16) {
        wire.provenance_header =
            ProvenanceHeaderV16Account::from_runtime(&account.provenance_header);
        wire.owner = account.owner;
        wire.capital = V16PodU128::new(account.capital);
        wire.pnl = V16PodI128::new(account.pnl);
        wire.reserved_pnl = V16PodU128::new(account.reserved_pnl);
        for d in 0..V16_DOMAIN_COUNT {
            wire.source_claim_market_id[d] = V16PodU64::new(account.source_claim_market_id[d]);
            wire.source_claim_bound_num[d] = V16PodU128::new(account.source_claim_bound_num[d]);
            wire.source_claim_liened_num[d] = V16PodU128::new(account.source_claim_liened_num[d]);
            wire.source_claim_counterparty_liened_num[d] =
                V16PodU128::new(account.source_claim_counterparty_liened_num[d]);
            wire.source_claim_insurance_liened_num[d] =
                V16PodU128::new(account.source_claim_insurance_liened_num[d]);
            wire.source_lien_effective_reserved[d] =
                V16PodU128::new(account.source_lien_effective_reserved[d]);
            wire.source_lien_counterparty_backing_num[d] =
                V16PodU128::new(account.source_lien_counterparty_backing_num[d]);
            wire.source_lien_insurance_backing_num[d] =
                V16PodU128::new(account.source_lien_insurance_backing_num[d]);
            wire.source_claim_impaired_num[d] =
                V16PodU128::new(account.source_claim_impaired_num[d]);
            wire.source_lien_impaired_effective_reserved[d] =
                V16PodU128::new(account.source_lien_impaired_effective_reserved[d]);
        }
        wire.fee_credits = V16PodI128::new(account.fee_credits);
        wire.cancel_deposit_escrow = V16PodU128::new(account.cancel_deposit_escrow);
        wire.last_fee_slot = V16PodU64::new(account.last_fee_slot);
        wire.active_bitmap = account.active_bitmap.map(V16PodU64::new);
        for i in 0..V16_MAX_PORTFOLIO_ASSETS_N {
            wire.legs[i] = PortfolioLegV16Account::from_runtime(&account.legs[i]);
        }
        wire.health_cert = HealthCertV16Account::from_runtime(&account.health_cert);
        wire.stale_state = encode_bool(account.stale_state);
        wire.b_stale_state = encode_bool(account.b_stale_state);
        wire.rebalance_lock = encode_bool(account.rebalance_lock);
        wire.liquidation_lock = encode_bool(account.liquidation_lock);
        wire.close_progress =
            percolator::CloseProgressLedgerV16Account::from_runtime(&account.close_progress);
        wire.resolved_payout_receipt =
            ResolvedPayoutReceiptV16Account::from_runtime(&account.resolved_payout_receipt);
    }

    pub fn init_market_account(
        data: &mut [u8],
        config: &WrapperConfigV16,
        group: &MarketGroupV16,
    ) -> Result<(), ProgramError> {
        if data.len() < MIN_MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        if is_initialized(data) {
            return Err(PercolatorError::AlreadyInitialized.into());
        }
        for b in data.iter_mut() {
            *b = 0;
        }
        write_header(data, KIND_MARKET)?;
        write_wrapper_config_to_bytes(data, config)?;
        let base_profile = manual_asset_oracle_profile(
            config.oracle_target_price_e6,
            config.last_good_oracle_slot,
        );
        init_asset_oracle_profiles(data, &base_profile)?;
        write_market_wire(data, group)?;
        Ok(())
    }

    #[cfg(not(target_os = "solana"))]
    pub fn read_market(data: &[u8]) -> Result<(WrapperConfigV16, MarketGroupV16), ProgramError> {
        if data.len() < MIN_MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_wrapper_config_from_bytes(data)?;
        Ok((config, *market_from_wire_boxed(data)?))
    }

    pub fn read_market_boxed(
        data: &[u8],
    ) -> Result<(Box<WrapperConfigV16>, Box<MarketGroupV16>), ProgramError> {
        if data.len() < MIN_MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_wrapper_config_boxed_from_bytes(data)?;
        Ok((config, market_from_wire_boxed(data)?))
    }

    pub fn read_market_trade_preflight(
        data: &[u8],
        asset_index: usize,
    ) -> Result<(WrapperConfigV16, MarketModeV16, u64, u64, u64), ProgramError> {
        if data.len() < MIN_MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_wrapper_config_from_bytes(data)?;
        let wire = market_header(data)?;
        let engine_config = wire
            .config
            .try_to_runtime()
            .map_err(map_account_wire_error)?;
        if asset_index >= engine_config.max_market_slots as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let slot = asset_slot_wire(data, asset_index)?;
        Ok((
            config,
            decode_market_mode(wire.mode)?,
            wire.current_slot.get(),
            slot.asset.effective_price.get(),
            engine_config.max_trading_fee_bps,
        ))
    }

    pub fn write_market(
        data: &mut [u8],
        config: &WrapperConfigV16,
        group: &MarketGroupV16,
    ) -> Result<(), ProgramError> {
        if data.len() < MIN_MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        write_wrapper_config_to_bytes(data, config)?;
        if config.oracle_mode != ORACLE_MODE_MANUAL {
            let base_profile = asset_oracle_profile_from_config(config);
            write_asset_oracle_profile(data, 0, &base_profile)?;
        }
        write_market_wire(data, group)?;
        Ok(())
    }

    pub fn init_portfolio_account(
        data: &mut [u8],
        account: &PortfolioAccountV16,
    ) -> Result<(), ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        if is_initialized(data) {
            return Err(PercolatorError::AlreadyInitialized.into());
        }
        for b in data.iter_mut() {
            *b = 0;
        }
        write_header(data, KIND_PORTFOLIO)?;
        write_portfolio_wire(portfolio_wire_mut(data)?, account);
        Ok(())
    }

    #[cfg(not(target_os = "solana"))]
    pub fn read_portfolio(data: &[u8]) -> Result<PortfolioAccountV16, ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        Ok(*portfolio_from_wire_boxed(portfolio_wire(data)?)?)
    }

    pub fn read_portfolio_boxed(data: &[u8]) -> Result<Box<PortfolioAccountV16>, ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        portfolio_from_wire_boxed(portfolio_wire(data)?)
    }

    pub fn read_portfolio_owner_preflight(
        data: &[u8],
    ) -> Result<(ProvenanceHeaderV16, [u8; 32]), ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        let wire = portfolio_wire(data)?;
        let header = wire
            .provenance_header
            .try_to_runtime()
            .map_err(map_account_wire_error)?;
        if header.owner != wire.owner {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok((header, wire.owner))
    }

    pub fn write_portfolio(
        data: &mut [u8],
        account: &PortfolioAccountV16,
    ) -> Result<(), ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        write_portfolio_wire(portfolio_wire_mut(data)?, account);
        Ok(())
    }

    pub const fn alignment_note() -> usize {
        1
    }

    pub const fn wrapper_config_len_for_test() -> usize {
        WRAPPER_CONFIG_LEN
    }
}

pub mod ix {
    use alloc::vec::Vec;
    use solana_program::program_error::ProgramError;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Instruction {
        InitMarket {
            max_portfolio_assets: u16,
            h_min: u64,
            h_max: u64,
            initial_price: u64,
            min_nonzero_mm_req: u128,
            min_nonzero_im_req: u128,
            maintenance_margin_bps: u64,
            initial_margin_bps: u64,
            max_trading_fee_bps: u64,
            trade_fee_base_bps: u64,
            liquidation_fee_bps: u64,
            liquidation_fee_cap: u128,
            min_liquidation_abs: u128,
            max_price_move_bps_per_slot: u64,
            max_accrual_dt_slots: u64,
            max_abs_funding_e9_per_slot: u64,
            min_funding_lifetime_slots: u64,
            max_account_b_settlement_chunks: u64,
            max_bankrupt_close_chunks: u64,
            max_bankrupt_close_lifetime_slots: u64,
            public_b_chunk_atoms: u128,
            maintenance_fee_per_slot: u128,
        },
        InitPortfolio,
        Deposit {
            amount: u128,
        },
        Withdraw {
            amount: u128,
        },
        PermissionlessCrank {
            action: u8,
            asset_index: u16,
            now_slot: u64,
            effective_price: u64,
            funding_rate_e9: i128,
            close_q: u128,
            fee_bps: u64,
            recovery_reason: u8,
        },
        TradeNoCpi {
            asset_index: u16,
            size_q: i128,
            exec_price: u64,
            fee_bps: u64,
        },
        TradeCpi {
            asset_index: u16,
            size_q: i128,
            fee_bps: u64,
            limit_price: u64,
        },
        ClosePortfolio,
        TopUpInsurance {
            amount: u128,
        },
        CloseSlab,
        ResolveMarket,
        WithdrawInsuranceLimited {
            amount: u128,
        },
        TopUpBackingBucket {
            domain: u8,
            amount: u128,
            expiry_slot: u64,
        },
        WithdrawBackingBucket {
            domain: u8,
            amount: u128,
        },
        ConvertReleasedPnl {
            amount: u128,
        },
        CloseResolved {
            fee_rate_per_slot: u128,
        },
        UpdateAuthority {
            kind: u8,
            new_pubkey: [u8; 32],
        },
        UpdateInsurancePolicy {
            max_bps: u16,
            deposits_only: u8,
            cooldown_slots: u64,
        },
        UpdateLiquidationFeePolicy {
            cranker_share_bps: u16,
        },
        UpdateMaintenanceFeePolicy {
            cranker_share_bps: u16,
        },
        UpdateBackingFeePolicy {
            fee_bps: u16,
        },
        ConfigurePermissionlessResolve {
            stale_slots: u64,
            force_close_delay_slots: u64,
        },
        ResolveStalePermissionless {
            now_slot: u64,
        },
        ConfigureHybridOracle {
            asset_index: u16,
            now_slot: u64,
            now_unix_ts: i64,
            oracle_leg_count: u8,
            oracle_leg_flags: u8,
            max_staleness_secs: u64,
            hybrid_soft_stale_slots: u64,
            mark_ewma_halflife_slots: u64,
            mark_min_fee: u64,
            invert: u8,
            unit_scale: u32,
            conf_filter_bps: u16,
            oracle_leg_feeds: [[u8; 32]; 3],
        },
        ConfigureHyperpMark {
            asset_index: u16,
            now_slot: u64,
            initial_mark_e6: u64,
            mark_ewma_halflife_slots: u64,
            mark_min_fee: u64,
        },
        PushHyperpMark {
            asset_index: u16,
            now_slot: u64,
            mark_e6: u64,
        },
        UpdateAssetLifecycle {
            action: u8,
            asset_index: u16,
            now_slot: u64,
            initial_price: u64,
        },
        WithdrawInsurance {
            amount: u128,
        },
        CureAndCancelClose {
            optional_deposit: u128,
        },
        ForfeitRecoveryLeg {
            asset_index: u16,
            b_delta_budget: u128,
        },
        RebalanceReduce {
            asset_index: u16,
            reduce_q: u128,
        },
        FinalizeResetSide {
            asset_index: u16,
            side: u8,
        },
        ClaimResolvedPayoutTopup,
        RefineResolvedUnreceiptedBound {
            decrease_num: u128,
        },
        SyncMaintenanceFee {
            now_slot: u64,
        },
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input
                .split_first()
                .ok_or(ProgramError::InvalidInstructionData)?;
            let ix = match tag {
                0 => Self::InitMarket {
                    max_portfolio_assets: read_u16(&mut rest)?,
                    h_min: read_u64(&mut rest)?,
                    h_max: read_u64(&mut rest)?,
                    initial_price: read_u64(&mut rest)?,
                    min_nonzero_mm_req: read_u128(&mut rest)?,
                    min_nonzero_im_req: read_u128(&mut rest)?,
                    maintenance_margin_bps: read_u64(&mut rest)?,
                    initial_margin_bps: read_u64(&mut rest)?,
                    max_trading_fee_bps: read_u64(&mut rest)?,
                    trade_fee_base_bps: read_u64(&mut rest)?,
                    liquidation_fee_bps: read_u64(&mut rest)?,
                    liquidation_fee_cap: read_u128(&mut rest)?,
                    min_liquidation_abs: read_u128(&mut rest)?,
                    max_price_move_bps_per_slot: read_u64(&mut rest)?,
                    max_accrual_dt_slots: read_u64(&mut rest)?,
                    max_abs_funding_e9_per_slot: read_u64(&mut rest)?,
                    min_funding_lifetime_slots: read_u64(&mut rest)?,
                    max_account_b_settlement_chunks: read_u64(&mut rest)?,
                    max_bankrupt_close_chunks: read_u64(&mut rest)?,
                    max_bankrupt_close_lifetime_slots: read_u64(&mut rest)?,
                    public_b_chunk_atoms: read_u128(&mut rest)?,
                    maintenance_fee_per_slot: read_u128(&mut rest)?,
                },
                1 => Self::InitPortfolio,
                3 => Self::Deposit {
                    amount: read_u128(&mut rest)?,
                },
                4 => Self::Withdraw {
                    amount: read_u128(&mut rest)?,
                },
                5 => Self::PermissionlessCrank {
                    action: read_u8(&mut rest)?,
                    asset_index: read_u16(&mut rest)?,
                    now_slot: read_u64(&mut rest)?,
                    effective_price: read_u64(&mut rest)?,
                    funding_rate_e9: read_i128(&mut rest)?,
                    close_q: read_u128(&mut rest)?,
                    fee_bps: read_u64(&mut rest)?,
                    recovery_reason: read_u8(&mut rest)?,
                },
                6 => Self::TradeNoCpi {
                    asset_index: read_u16(&mut rest)?,
                    size_q: read_i128(&mut rest)?,
                    exec_price: read_u64(&mut rest)?,
                    fee_bps: read_u64(&mut rest)?,
                },
                10 => Self::TradeCpi {
                    asset_index: read_u16(&mut rest)?,
                    size_q: read_i128(&mut rest)?,
                    fee_bps: read_u64(&mut rest)?,
                    limit_price: read_u64(&mut rest)?,
                },
                8 => Self::ClosePortfolio,
                9 => Self::TopUpInsurance {
                    amount: read_u128(&mut rest)?,
                },
                13 => Self::CloseSlab,
                19 => Self::ResolveMarket,
                23 => Self::WithdrawInsuranceLimited {
                    amount: read_u128(&mut rest)?,
                },
                24 => Self::TopUpBackingBucket {
                    domain: read_u8(&mut rest)?,
                    amount: read_u128(&mut rest)?,
                    expiry_slot: read_u64(&mut rest)?,
                },
                50 => Self::WithdrawBackingBucket {
                    domain: read_u8(&mut rest)?,
                    amount: read_u128(&mut rest)?,
                },
                28 => Self::ConvertReleasedPnl {
                    amount: read_u128(&mut rest)?,
                },
                30 => Self::CloseResolved {
                    fee_rate_per_slot: read_u128(&mut rest)?,
                },
                32 => Self::UpdateAuthority {
                    kind: read_u8(&mut rest)?,
                    new_pubkey: read_bytes32(&mut rest)?,
                },
                33 => Self::UpdateInsurancePolicy {
                    max_bps: read_u16(&mut rest)?,
                    deposits_only: read_u8(&mut rest)?,
                    cooldown_slots: read_u64(&mut rest)?,
                },
                37 => Self::UpdateLiquidationFeePolicy {
                    cranker_share_bps: read_u16(&mut rest)?,
                },
                49 => Self::UpdateMaintenanceFeePolicy {
                    cranker_share_bps: read_u16(&mut rest)?,
                },
                51 => Self::UpdateBackingFeePolicy {
                    fee_bps: read_u16(&mut rest)?,
                },
                38 => Self::ConfigurePermissionlessResolve {
                    stale_slots: read_u64(&mut rest)?,
                    force_close_delay_slots: read_u64(&mut rest)?,
                },
                39 => Self::ResolveStalePermissionless {
                    now_slot: read_u64(&mut rest)?,
                },
                34 => Self::ConfigureHybridOracle {
                    asset_index: read_u16(&mut rest)?,
                    now_slot: read_u64(&mut rest)?,
                    now_unix_ts: read_i64(&mut rest)?,
                    oracle_leg_count: read_u8(&mut rest)?,
                    oracle_leg_flags: read_u8(&mut rest)?,
                    max_staleness_secs: read_u64(&mut rest)?,
                    hybrid_soft_stale_slots: read_u64(&mut rest)?,
                    mark_ewma_halflife_slots: read_u64(&mut rest)?,
                    mark_min_fee: read_u64(&mut rest)?,
                    invert: read_u8(&mut rest)?,
                    unit_scale: read_u32(&mut rest)?,
                    conf_filter_bps: read_u16(&mut rest)?,
                    oracle_leg_feeds: [
                        read_bytes32(&mut rest)?,
                        read_bytes32(&mut rest)?,
                        read_bytes32(&mut rest)?,
                    ],
                },
                35 => Self::ConfigureHyperpMark {
                    asset_index: read_u16(&mut rest)?,
                    now_slot: read_u64(&mut rest)?,
                    initial_mark_e6: read_u64(&mut rest)?,
                    mark_ewma_halflife_slots: read_u64(&mut rest)?,
                    mark_min_fee: read_u64(&mut rest)?,
                },
                36 => Self::PushHyperpMark {
                    asset_index: read_u16(&mut rest)?,
                    now_slot: read_u64(&mut rest)?,
                    mark_e6: read_u64(&mut rest)?,
                },
                40 => Self::UpdateAssetLifecycle {
                    action: read_u8(&mut rest)?,
                    asset_index: read_u16(&mut rest)?,
                    now_slot: read_u64(&mut rest)?,
                    initial_price: read_u64(&mut rest)?,
                },
                41 => Self::WithdrawInsurance {
                    amount: read_u128(&mut rest)?,
                },
                42 => Self::CureAndCancelClose {
                    optional_deposit: read_u128(&mut rest)?,
                },
                43 => Self::ForfeitRecoveryLeg {
                    asset_index: read_u16(&mut rest)?,
                    b_delta_budget: read_u128(&mut rest)?,
                },
                44 => Self::RebalanceReduce {
                    asset_index: read_u16(&mut rest)?,
                    reduce_q: read_u128(&mut rest)?,
                },
                45 => Self::FinalizeResetSide {
                    asset_index: read_u16(&mut rest)?,
                    side: read_u8(&mut rest)?,
                },
                46 => Self::ClaimResolvedPayoutTopup,
                47 => Self::RefineResolvedUnreceiptedBound {
                    decrease_num: read_u128(&mut rest)?,
                },
                48 => Self::SyncMaintenanceFee {
                    now_slot: read_u64(&mut rest)?,
                },
                _ => return Err(ProgramError::InvalidInstructionData),
            };
            if !rest.is_empty() {
                return Err(ProgramError::InvalidInstructionData);
            }
            Ok(ix)
        }

        pub fn encode(&self) -> Vec<u8> {
            let mut out = Vec::new();
            match *self {
                Self::InitMarket {
                    max_portfolio_assets,
                    h_min,
                    h_max,
                    initial_price,
                    min_nonzero_mm_req,
                    min_nonzero_im_req,
                    maintenance_margin_bps,
                    initial_margin_bps,
                    max_trading_fee_bps,
                    trade_fee_base_bps,
                    liquidation_fee_bps,
                    liquidation_fee_cap,
                    min_liquidation_abs,
                    max_price_move_bps_per_slot,
                    max_accrual_dt_slots,
                    max_abs_funding_e9_per_slot,
                    min_funding_lifetime_slots,
                    max_account_b_settlement_chunks,
                    max_bankrupt_close_chunks,
                    max_bankrupt_close_lifetime_slots,
                    public_b_chunk_atoms,
                    maintenance_fee_per_slot,
                } => {
                    out.push(0);
                    push_u16(&mut out, max_portfolio_assets);
                    push_u64(&mut out, h_min);
                    push_u64(&mut out, h_max);
                    push_u64(&mut out, initial_price);
                    push_u128(&mut out, min_nonzero_mm_req);
                    push_u128(&mut out, min_nonzero_im_req);
                    push_u64(&mut out, maintenance_margin_bps);
                    push_u64(&mut out, initial_margin_bps);
                    push_u64(&mut out, max_trading_fee_bps);
                    push_u64(&mut out, trade_fee_base_bps);
                    push_u64(&mut out, liquidation_fee_bps);
                    push_u128(&mut out, liquidation_fee_cap);
                    push_u128(&mut out, min_liquidation_abs);
                    push_u64(&mut out, max_price_move_bps_per_slot);
                    push_u64(&mut out, max_accrual_dt_slots);
                    push_u64(&mut out, max_abs_funding_e9_per_slot);
                    push_u64(&mut out, min_funding_lifetime_slots);
                    push_u64(&mut out, max_account_b_settlement_chunks);
                    push_u64(&mut out, max_bankrupt_close_chunks);
                    push_u64(&mut out, max_bankrupt_close_lifetime_slots);
                    push_u128(&mut out, public_b_chunk_atoms);
                    push_u128(&mut out, maintenance_fee_per_slot);
                }
                Self::InitPortfolio => out.push(1),
                Self::Deposit { amount } => {
                    out.push(3);
                    push_u128(&mut out, amount);
                }
                Self::Withdraw { amount } => {
                    out.push(4);
                    push_u128(&mut out, amount);
                }
                Self::PermissionlessCrank {
                    action,
                    asset_index,
                    now_slot,
                    effective_price,
                    funding_rate_e9,
                    close_q,
                    fee_bps,
                    recovery_reason,
                } => {
                    out.push(5);
                    out.push(action);
                    push_u16(&mut out, asset_index);
                    push_u64(&mut out, now_slot);
                    push_u64(&mut out, effective_price);
                    push_i128(&mut out, funding_rate_e9);
                    push_u128(&mut out, close_q);
                    push_u64(&mut out, fee_bps);
                    out.push(recovery_reason);
                }
                Self::TradeNoCpi {
                    asset_index,
                    size_q,
                    exec_price,
                    fee_bps,
                } => {
                    out.push(6);
                    push_u16(&mut out, asset_index);
                    push_i128(&mut out, size_q);
                    push_u64(&mut out, exec_price);
                    push_u64(&mut out, fee_bps);
                }
                Self::TradeCpi {
                    asset_index,
                    size_q,
                    fee_bps,
                    limit_price,
                } => {
                    out.push(10);
                    push_u16(&mut out, asset_index);
                    push_i128(&mut out, size_q);
                    push_u64(&mut out, fee_bps);
                    push_u64(&mut out, limit_price);
                }
                Self::ClosePortfolio => out.push(8),
                Self::TopUpInsurance { amount } => {
                    out.push(9);
                    push_u128(&mut out, amount);
                }
                Self::CloseSlab => out.push(13),
                Self::ResolveMarket => out.push(19),
                Self::WithdrawInsuranceLimited { amount } => {
                    out.push(23);
                    push_u128(&mut out, amount);
                }
                Self::TopUpBackingBucket {
                    domain,
                    amount,
                    expiry_slot,
                } => {
                    out.push(24);
                    out.push(domain);
                    push_u128(&mut out, amount);
                    push_u64(&mut out, expiry_slot);
                }
                Self::WithdrawBackingBucket { domain, amount } => {
                    out.push(50);
                    out.push(domain);
                    push_u128(&mut out, amount);
                }
                Self::ConvertReleasedPnl { amount } => {
                    out.push(28);
                    push_u128(&mut out, amount);
                }
                Self::CloseResolved { fee_rate_per_slot } => {
                    out.push(30);
                    push_u128(&mut out, fee_rate_per_slot);
                }
                Self::UpdateAuthority { kind, new_pubkey } => {
                    out.push(32);
                    out.push(kind);
                    out.extend_from_slice(&new_pubkey);
                }
                Self::UpdateInsurancePolicy {
                    max_bps,
                    deposits_only,
                    cooldown_slots,
                } => {
                    out.push(33);
                    push_u16(&mut out, max_bps);
                    out.push(deposits_only);
                    push_u64(&mut out, cooldown_slots);
                }
                Self::UpdateLiquidationFeePolicy { cranker_share_bps } => {
                    out.push(37);
                    push_u16(&mut out, cranker_share_bps);
                }
                Self::UpdateMaintenanceFeePolicy { cranker_share_bps } => {
                    out.push(49);
                    push_u16(&mut out, cranker_share_bps);
                }
                Self::UpdateBackingFeePolicy { fee_bps } => {
                    out.push(51);
                    push_u16(&mut out, fee_bps);
                }
                Self::ConfigurePermissionlessResolve {
                    stale_slots,
                    force_close_delay_slots,
                } => {
                    out.push(38);
                    push_u64(&mut out, stale_slots);
                    push_u64(&mut out, force_close_delay_slots);
                }
                Self::ResolveStalePermissionless { now_slot } => {
                    out.push(39);
                    push_u64(&mut out, now_slot);
                }
                Self::ConfigureHybridOracle {
                    asset_index,
                    now_slot,
                    now_unix_ts,
                    oracle_leg_count,
                    oracle_leg_flags,
                    max_staleness_secs,
                    hybrid_soft_stale_slots,
                    mark_ewma_halflife_slots,
                    mark_min_fee,
                    invert,
                    unit_scale,
                    conf_filter_bps,
                    oracle_leg_feeds,
                } => {
                    out.push(34);
                    push_u16(&mut out, asset_index);
                    push_u64(&mut out, now_slot);
                    push_i64(&mut out, now_unix_ts);
                    out.push(oracle_leg_count);
                    out.push(oracle_leg_flags);
                    push_u64(&mut out, max_staleness_secs);
                    push_u64(&mut out, hybrid_soft_stale_slots);
                    push_u64(&mut out, mark_ewma_halflife_slots);
                    push_u64(&mut out, mark_min_fee);
                    out.push(invert);
                    push_u32(&mut out, unit_scale);
                    push_u16(&mut out, conf_filter_bps);
                    for feed in oracle_leg_feeds {
                        out.extend_from_slice(&feed);
                    }
                }
                Self::ConfigureHyperpMark {
                    asset_index,
                    now_slot,
                    initial_mark_e6,
                    mark_ewma_halflife_slots,
                    mark_min_fee,
                } => {
                    out.push(35);
                    push_u16(&mut out, asset_index);
                    push_u64(&mut out, now_slot);
                    push_u64(&mut out, initial_mark_e6);
                    push_u64(&mut out, mark_ewma_halflife_slots);
                    push_u64(&mut out, mark_min_fee);
                }
                Self::PushHyperpMark {
                    asset_index,
                    now_slot,
                    mark_e6,
                } => {
                    out.push(36);
                    push_u16(&mut out, asset_index);
                    push_u64(&mut out, now_slot);
                    push_u64(&mut out, mark_e6);
                }
                Self::UpdateAssetLifecycle {
                    action,
                    asset_index,
                    now_slot,
                    initial_price,
                } => {
                    out.push(40);
                    out.push(action);
                    push_u16(&mut out, asset_index);
                    push_u64(&mut out, now_slot);
                    push_u64(&mut out, initial_price);
                }
                Self::WithdrawInsurance { amount } => {
                    out.push(41);
                    push_u128(&mut out, amount);
                }
                Self::CureAndCancelClose { optional_deposit } => {
                    out.push(42);
                    push_u128(&mut out, optional_deposit);
                }
                Self::ForfeitRecoveryLeg {
                    asset_index,
                    b_delta_budget,
                } => {
                    out.push(43);
                    push_u16(&mut out, asset_index);
                    push_u128(&mut out, b_delta_budget);
                }
                Self::RebalanceReduce {
                    asset_index,
                    reduce_q,
                } => {
                    out.push(44);
                    push_u16(&mut out, asset_index);
                    push_u128(&mut out, reduce_q);
                }
                Self::FinalizeResetSide { asset_index, side } => {
                    out.push(45);
                    push_u16(&mut out, asset_index);
                    out.push(side);
                }
                Self::ClaimResolvedPayoutTopup => out.push(46),
                Self::RefineResolvedUnreceiptedBound { decrease_num } => {
                    out.push(47);
                    push_u128(&mut out, decrease_num);
                }
                Self::SyncMaintenanceFee { now_slot } => {
                    out.push(48);
                    push_u64(&mut out, now_slot);
                }
            }
            out
        }
    }

    fn read_u8(input: &mut &[u8]) -> Result<u8, ProgramError> {
        let (&v, rest) = input
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;
        *input = rest;
        Ok(v)
    }

    fn read_u64(input: &mut &[u8]) -> Result<u64, ProgramError> {
        if input.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u16(input: &mut &[u8]) -> Result<u16, ProgramError> {
        if input.len() < 2 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(2);
        *input = rest;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u32(input: &mut &[u8]) -> Result<u32, ProgramError> {
        if input.len() < 4 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(4);
        *input = rest;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u128(input: &mut &[u8]) -> Result<u128, ProgramError> {
        if input.len() < 16 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(u128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i128(input: &mut &[u8]) -> Result<i128, ProgramError> {
        if input.len() < 16 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(i128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i64(input: &mut &[u8]) -> Result<i64, ProgramError> {
        if input.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_bytes32(input: &mut &[u8]) -> Result<[u8; 32], ProgramError> {
        if input.len() < 32 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(bytes.try_into().unwrap())
    }

    fn push_u64(out: &mut Vec<u8>, v: u64) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u128(out: &mut Vec<u8>, v: u128) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_i128(out: &mut Vec<u8>, v: i128) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_i64(out: &mut Vec<u8>, v: i64) {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

pub mod matcher_abi {
    use crate::constants::MATCHER_ABI_VERSION;
    use solana_program::program_error::ProgramError;

    pub const FLAG_VALID: u32 = 1;
    pub const FLAG_PARTIAL_OK: u32 = 2;
    pub const FLAG_REJECTED: u32 = 4;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MatcherReturn {
        pub abi_version: u32,
        pub flags: u32,
        pub exec_price_e6: u64,
        pub exec_size: i128,
        pub req_id: u64,
        pub lp_account_id: u64,
        pub oracle_price_e6: u64,
        pub asset_index: u64,
    }

    pub fn read_matcher_return(ctx: &[u8]) -> Result<MatcherReturn, ProgramError> {
        if ctx.len() < 64 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(MatcherReturn {
            abi_version: u32::from_le_bytes(ctx[0..4].try_into().unwrap()),
            flags: u32::from_le_bytes(ctx[4..8].try_into().unwrap()),
            exec_price_e6: u64::from_le_bytes(ctx[8..16].try_into().unwrap()),
            exec_size: i128::from_le_bytes(ctx[16..32].try_into().unwrap()),
            req_id: u64::from_le_bytes(ctx[32..40].try_into().unwrap()),
            lp_account_id: u64::from_le_bytes(ctx[40..48].try_into().unwrap()),
            oracle_price_e6: u64::from_le_bytes(ctx[48..56].try_into().unwrap()),
            asset_index: u64::from_le_bytes(ctx[56..64].try_into().unwrap()),
        })
    }

    pub fn validate_matcher_return(
        ret: &MatcherReturn,
        lp_account_id: u64,
        asset_index: u16,
        oracle_price_e6: u64,
        req_size: i128,
        req_id: u64,
    ) -> Result<(), ProgramError> {
        if ret.abi_version != MATCHER_ABI_VERSION {
            return Err(ProgramError::InvalidAccountData);
        }
        const KNOWN_FLAGS: u32 = FLAG_VALID | FLAG_PARTIAL_OK | FLAG_REJECTED;
        if (ret.flags & !KNOWN_FLAGS) != 0
            || (ret.flags & FLAG_VALID) == 0
            || (ret.flags & FLAG_REJECTED) != 0
        {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.lp_account_id != lp_account_id
            || ret.oracle_price_e6 != oracle_price_e6
            || ret.asset_index != asset_index as u64
            || ret.req_id != req_id
            || ret.exec_price_e6 == 0
        {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.exec_size == 0 {
            if (ret.flags & FLAG_PARTIAL_OK) == 0 || ret.exec_price_e6 != oracle_price_e6 {
                return Err(ProgramError::InvalidAccountData);
            }
            return Ok(());
        }
        if ret.exec_size == i128::MIN || req_size == i128::MIN || req_size == 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.exec_size.signum() != req_size.signum() {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.exec_size.unsigned_abs() > req_size.unsigned_abs() {
            return Err(ProgramError::InvalidAccountData);
        }
        if ret.exec_size.unsigned_abs() < req_size.unsigned_abs()
            && (ret.flags & FLAG_PARTIAL_OK) == 0
        {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}

pub mod oracle_v16 {
    use crate::{
        constants::{
            ORACLE_LEG_CAP, ORACLE_LEG_FLAGS_MASK, ORACLE_LEG_FLAG_DIVIDE_LEG2,
            ORACLE_LEG_FLAG_DIVIDE_LEG3, ORACLE_MODE_HYBRID_AFTER_HOURS, ORACLE_MODE_HYPERP,
            ORACLE_MODE_MANUAL, SWITCHBOARD_RESULT_SCALE,
        },
        error::PercolatorError,
        state::{AssetOracleProfileV16, WrapperConfigV16},
    };
    use borsh::BorshDeserialize;
    use pythnet_sdk::messages::PriceFeedMessage;
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

    pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x0c, 0xb7, 0xfa, 0xbb, 0x52, 0xf7, 0xa6, 0x48, 0xbb, 0x5b, 0x31, 0x7d, 0x9a, 0x01, 0x8b,
        0x90, 0x57, 0xcb, 0x02, 0x47, 0x74, 0xfa, 0xfe, 0x01, 0xe6, 0xc4, 0xdf, 0x98, 0xcc, 0x38,
        0x58, 0x81,
    ]);
    pub const SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID: Pubkey =
        solana_program::pubkey!("SBondMDrcV3K4kxZR1HNVT7osZxAHVHgYXL5Ze1oMUv");
    pub const SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID: Pubkey =
        solana_program::pubkey!("Aio4gaXjXzJNVLtzwtNVmSqGKpANtXhybbkhtAC94ji2");
    pub const CHAINLINK_STORE_PROGRAM_ID: Pubkey =
        solana_program::pubkey!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");
    const PRICE_UPDATE_V2_MIN_LEN: usize = 134;
    const OFF_VERIFICATION_LEVEL: usize = 40;
    const OFF_PRICE_FEED_MESSAGE: usize = 41;
    const PYTH_PRICE_UPDATE_V2_DISCRIMINATOR: [u8; 8] =
        [0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd];
    const PYTH_VERIFICATION_FULL_TAG: u8 = 1;
    const MAX_EXPO_ABS: i32 = 18;
    const SWITCHBOARD_PULL_FEED_DISCRIMINATOR: [u8; 8] = [196, 27, 108, 196, 10, 215, 219, 40];
    const SWITCHBOARD_PULL_FEED_MIN_LEN: usize = 3_208;
    const SB_OFF_FEED_HASH: usize = 8 + 2_112;
    const SB_OFF_MIN_SAMPLE_SIZE: usize = 8 + 2_207;
    const SB_OFF_LAST_UPDATE_TIMESTAMP: usize = 8 + 2_208;
    const SB_OFF_RESULT_VALUE: usize = 8 + 2_256;
    const SB_OFF_RESULT_STD_DEV: usize = 8 + 2_272;
    const SB_OFF_RESULT_NUM_SAMPLES: usize = 8 + 2_352;
    const SB_OFF_RESULT_SLOT: usize = 8 + 2_360;
    const CHAINLINK_TRANSMISSIONS_DISCRIMINATOR: [u8; 8] = [96, 179, 69, 66, 128, 129, 73, 117];
    const CHAINLINK_HEADER_SIZE: usize = 192;
    const CHAINLINK_FEED_MIN_LEN: usize = 8 + CHAINLINK_HEADER_SIZE + 48;
    const CL_OFF_VERSION: usize = 8;
    const CL_OFF_DECIMALS: usize = 138;
    const CL_OFF_LATEST_ROUND_ID: usize = 143;
    const CL_OFF_LIVE_LENGTH: usize = 148;
    const CL_OFF_TRANSMISSION: usize = 8 + CHAINLINK_HEADER_SIZE;
    const CL_TRANS_OFF_SLOT: usize = 0;
    const CL_TRANS_OFF_TIMESTAMP: usize = 8;
    const CL_TRANS_OFF_ANSWER: usize = 16;

    pub fn is_hybrid(config: &WrapperConfigV16) -> bool {
        config.oracle_mode == ORACLE_MODE_HYBRID_AFTER_HOURS
    }

    pub fn is_hyperp(config: &WrapperConfigV16) -> bool {
        config.oracle_mode == ORACLE_MODE_HYPERP
    }

    pub fn profile_is_hybrid(profile: &AssetOracleProfileV16) -> bool {
        profile.oracle_mode == ORACLE_MODE_HYBRID_AFTER_HOURS
    }

    pub fn profile_is_hyperp(profile: &AssetOracleProfileV16) -> bool {
        profile.oracle_mode == ORACLE_MODE_HYPERP
    }

    pub fn profile_is_price_managed(profile: &AssetOracleProfileV16) -> bool {
        profile_is_hybrid(profile) || profile_is_hyperp(profile)
    }

    pub fn hybrid_soft_stale_matured(config: &WrapperConfigV16, now_slot: u64) -> bool {
        is_hybrid(config)
            && config.hybrid_soft_stale_slots != 0
            && now_slot.saturating_sub(config.last_good_oracle_slot)
                > config.hybrid_soft_stale_slots
    }

    pub fn profile_hybrid_soft_stale_matured(
        profile: &AssetOracleProfileV16,
        now_slot: u64,
    ) -> bool {
        profile_is_hybrid(profile)
            && profile.hybrid_soft_stale_slots != 0
            && now_slot.saturating_sub(profile.last_good_oracle_slot)
                > profile.hybrid_soft_stale_slots
    }

    pub fn hard_stale_matured(config: &WrapperConfigV16, now_slot: u64) -> bool {
        is_hybrid(config) && permissionless_stale_matured(config, now_slot)
    }

    pub fn permissionless_stale_matured(config: &WrapperConfigV16, now_slot: u64) -> bool {
        config.permissionless_resolve_stale_slots != 0
            && now_slot.saturating_sub(config.last_good_oracle_slot)
                >= config.permissionless_resolve_stale_slots
    }

    pub fn oracle_leg_config_ok(count: u8, flags: u8, feeds: &[[u8; 32]; ORACLE_LEG_CAP]) -> bool {
        if flags & !ORACLE_LEG_FLAGS_MASK != 0 {
            return false;
        }
        if count == 0 {
            return flags == 0 && feeds.iter().all(|f| *f == [0u8; 32]);
        }
        if count > ORACLE_LEG_CAP as u8 || feeds[0] == [0u8; 32] {
            return false;
        }
        if count == 1 {
            return flags == 0 && feeds[1] == [0u8; 32] && feeds[2] == [0u8; 32];
        }
        if feeds[1] == [0u8; 32] || feeds[1] == feeds[0] {
            return false;
        }
        if count == 2 {
            return (flags & ORACLE_LEG_FLAG_DIVIDE_LEG3) == 0 && feeds[2] == [0u8; 32];
        }
        feeds[2] != [0u8; 32] && feeds[2] != feeds[0] && feeds[2] != feeds[1]
    }

    fn leg_divides(config: &WrapperConfigV16, idx: usize) -> bool {
        match idx {
            1 => (config.oracle_leg_flags & ORACLE_LEG_FLAG_DIVIDE_LEG2) != 0,
            2 => (config.oracle_leg_flags & ORACLE_LEG_FLAG_DIVIDE_LEG3) != 0,
            _ => false,
        }
    }

    fn profile_leg_divides(profile: &AssetOracleProfileV16, idx: usize) -> bool {
        match idx {
            1 => (profile.oracle_leg_flags & ORACLE_LEG_FLAG_DIVIDE_LEG2) != 0,
            2 => (profile.oracle_leg_flags & ORACLE_LEG_FLAG_DIVIDE_LEG3) != 0,
            _ => false,
        }
    }

    pub fn read_pyth_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
    ) -> Result<(u64, i64), ProgramError> {
        if *price_ai.owner != PYTH_RECEIVER_PROGRAM_ID {
            return Err(ProgramError::IllegalOwner);
        }
        let data = price_ai.try_borrow_data()?;
        if data.len() < PRICE_UPDATE_V2_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if data[..8] != PYTH_PRICE_UPDATE_V2_DISCRIMINATOR {
            return Err(PercolatorError::OracleInvalid.into());
        }
        if data[OFF_VERIFICATION_LEVEL] != PYTH_VERIFICATION_FULL_TAG {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let msg = <PriceFeedMessage as BorshDeserialize>::deserialize(
            &mut &data[OFF_PRICE_FEED_MESSAGE..],
        )
        .map_err(|_| PercolatorError::OracleInvalid)?;
        if &msg.feed_id != expected_feed_id {
            return Err(PercolatorError::InvalidOracleKey.into());
        }
        if msg.price <= 0 || msg.exponent < -MAX_EXPO_ABS || msg.exponent > MAX_EXPO_ABS {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let age = now_unix_ts.saturating_sub(msg.publish_time);
        if age < 0 || age as u64 > max_staleness_secs {
            return Err(PercolatorError::OracleStale.into());
        }
        let price_u = msg.price as u128;
        if conf_bps != 0 && (msg.conf as u128).saturating_mul(10_000) > price_u * conf_bps as u128 {
            return Err(PercolatorError::OracleConfTooWide.into());
        }
        let scale = msg.exponent + 6;
        let out = if scale >= 0 {
            price_u
                .checked_mul(10u128.pow(scale as u32))
                .ok_or(PercolatorError::EngineArithmeticOverflow)?
        } else {
            price_u / 10u128.pow((-scale) as u32)
        };
        if out == 0 || out > percolator::MAX_ORACLE_PRICE as u128 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        Ok((out as u64, msg.publish_time))
    }

    #[inline]
    fn read_u32_le(data: &[u8], off: usize) -> Result<u32, ProgramError> {
        let bytes: [u8; 4] = data
            .get(off..off + 4)
            .ok_or(ProgramError::InvalidAccountData)?
            .try_into()
            .unwrap();
        Ok(u32::from_le_bytes(bytes))
    }

    #[inline]
    fn read_u64_le(data: &[u8], off: usize) -> Result<u64, ProgramError> {
        let bytes: [u8; 8] = data
            .get(off..off + 8)
            .ok_or(ProgramError::InvalidAccountData)?
            .try_into()
            .unwrap();
        Ok(u64::from_le_bytes(bytes))
    }

    #[inline]
    fn read_i64_le(data: &[u8], off: usize) -> Result<i64, ProgramError> {
        let bytes: [u8; 8] = data
            .get(off..off + 8)
            .ok_or(ProgramError::InvalidAccountData)?
            .try_into()
            .unwrap();
        Ok(i64::from_le_bytes(bytes))
    }

    #[inline]
    fn read_i128_le(data: &[u8], off: usize) -> Result<i128, ProgramError> {
        let bytes: [u8; 16] = data
            .get(off..off + 16)
            .ok_or(ProgramError::InvalidAccountData)?
            .try_into()
            .unwrap();
        Ok(i128::from_le_bytes(bytes))
    }

    fn scale_decimal_to_e6(mantissa: i128, scale: u32) -> Result<u64, ProgramError> {
        if mantissa <= 0 || scale > MAX_EXPO_ABS as u32 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let mantissa = mantissa as u128;
        let out = if scale >= 6 {
            mantissa / 10u128.pow(scale - 6)
        } else {
            mantissa
                .checked_mul(10u128.pow(6 - scale))
                .ok_or(PercolatorError::EngineArithmeticOverflow)?
        };
        if out == 0 || out > percolator::MAX_ORACLE_PRICE as u128 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        Ok(out as u64)
    }

    pub fn read_switchboard_price_e6(
        price_ai: &AccountInfo,
        expected_feed_key: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
    ) -> Result<(u64, i64), ProgramError> {
        if *price_ai.owner != SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID
            && *price_ai.owner != SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID
        {
            return Err(ProgramError::IllegalOwner);
        }
        if price_ai.key.to_bytes() != *expected_feed_key {
            return Err(PercolatorError::InvalidOracleKey.into());
        }
        let data = price_ai.try_borrow_data()?;
        if data.len() < SWITCHBOARD_PULL_FEED_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if data[..8] != SWITCHBOARD_PULL_FEED_DISCRIMINATOR {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let feed_hash: [u8; 32] = data[SB_OFF_FEED_HASH..SB_OFF_FEED_HASH + 32]
            .try_into()
            .unwrap();
        let min_sample_size = data[SB_OFF_MIN_SAMPLE_SIZE];
        let publish_time = read_i64_le(&data, SB_OFF_LAST_UPDATE_TIMESTAMP)?;
        let value = read_i128_le(&data, SB_OFF_RESULT_VALUE)?;
        let std_dev = read_i128_le(&data, SB_OFF_RESULT_STD_DEV)?;
        let num_samples = data[SB_OFF_RESULT_NUM_SAMPLES];
        let result_slot = read_u64_le(&data, SB_OFF_RESULT_SLOT)?;
        if feed_hash == [0u8; 32]
            || min_sample_size == 0
            || num_samples < min_sample_size
            || result_slot == 0
            || publish_time <= 0
            || value <= 0
            || std_dev < 0
        {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let age = now_unix_ts.saturating_sub(publish_time);
        if age < 0 || age as u64 > max_staleness_secs {
            return Err(PercolatorError::OracleStale.into());
        }
        let value_u = value as u128;
        if conf_bps != 0 && (std_dev as u128).saturating_mul(10_000) > value_u * conf_bps as u128 {
            return Err(PercolatorError::OracleConfTooWide.into());
        }
        let out = value_u / SWITCHBOARD_RESULT_SCALE;
        if out == 0 || out > percolator::MAX_ORACLE_PRICE as u128 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        Ok((out as u64, publish_time))
    }

    pub fn read_chainlink_price_e6(
        price_ai: &AccountInfo,
        expected_feed_key: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
    ) -> Result<(u64, i64), ProgramError> {
        if *price_ai.owner != CHAINLINK_STORE_PROGRAM_ID {
            return Err(ProgramError::IllegalOwner);
        }
        if price_ai.key.to_bytes() != *expected_feed_key {
            return Err(PercolatorError::InvalidOracleKey.into());
        }
        let data = price_ai.try_borrow_data()?;
        if data.len() < CHAINLINK_FEED_MIN_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if data[..8] != CHAINLINK_TRANSMISSIONS_DISCRIMINATOR {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let version = data[CL_OFF_VERSION];
        let decimals = data[CL_OFF_DECIMALS];
        let latest_round_id = read_u32_le(&data, CL_OFF_LATEST_ROUND_ID)?;
        let live_length = read_u32_le(&data, CL_OFF_LIVE_LENGTH)?;
        let tx = CL_OFF_TRANSMISSION;
        let result_slot = read_u64_le(&data, tx + CL_TRANS_OFF_SLOT)?;
        let publish_time = read_u32_le(&data, tx + CL_TRANS_OFF_TIMESTAMP)? as i64;
        let answer = read_i128_le(&data, tx + CL_TRANS_OFF_ANSWER)?;
        if version == 0
            || latest_round_id == 0
            || live_length != 1
            || result_slot == 0
            || publish_time <= 0
        {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let age = now_unix_ts.saturating_sub(publish_time);
        if age < 0 || age as u64 > max_staleness_secs {
            return Err(PercolatorError::OracleStale.into());
        }
        scale_decimal_to_e6(answer, decimals as u32).map(|p| (p, publish_time))
    }

    pub fn read_oracle_price_e6(
        price_ai: &AccountInfo,
        expected_feed_id: &[u8; 32],
        now_unix_ts: i64,
        max_staleness_secs: u64,
        conf_bps: u16,
    ) -> Result<(u64, i64), ProgramError> {
        if *price_ai.owner == PYTH_RECEIVER_PROGRAM_ID {
            read_pyth_price_e6(
                price_ai,
                expected_feed_id,
                now_unix_ts,
                max_staleness_secs,
                conf_bps,
            )
        } else if *price_ai.owner == SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID
            || *price_ai.owner == SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID
        {
            read_switchboard_price_e6(
                price_ai,
                expected_feed_id,
                now_unix_ts,
                max_staleness_secs,
                conf_bps,
            )
        } else if *price_ai.owner == CHAINLINK_STORE_PROGRAM_ID {
            read_chainlink_price_e6(price_ai, expected_feed_id, now_unix_ts, max_staleness_secs)
        } else {
            Err(ProgramError::IllegalOwner)
        }
    }

    fn apply_transform(raw_price: u64, invert: u8, unit_scale: u32) -> Result<u64, ProgramError> {
        let mut price = raw_price;
        if invert != 0 {
            price = (1_000_000_000_000u128 / price as u128)
                .try_into()
                .map_err(|_| PercolatorError::OracleInvalid)?;
        }
        if unit_scale > 1 {
            price /= unit_scale as u64;
        }
        if price == 0 || price > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
        }
        Ok(price)
    }

    fn compose(acc_e6: u64, leg_e6: u64, divide: bool) -> Result<u64, ProgramError> {
        if leg_e6 == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let next = if divide {
            (acc_e6 as u128)
                .checked_mul(1_000_000)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?
                / leg_e6 as u128
        } else {
            (acc_e6 as u128)
                .checked_mul(leg_e6 as u128)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?
                / 1_000_000
        };
        if next == 0 || next > percolator::MAX_ORACLE_PRICE as u128 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        Ok(next as u64)
    }

    pub fn read_external_price_e6(
        config: &mut WrapperConfigV16,
        oracle_accounts: &[AccountInfo],
        now_unix_ts: i64,
    ) -> Result<(u64, i64, bool), ProgramError> {
        if config.oracle_mode == ORACLE_MODE_MANUAL {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let count = config.oracle_leg_count as usize;
        if count == 0 || count > ORACLE_LEG_CAP || oracle_accounts.len() < count {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        if !oracle_leg_config_ok(
            config.oracle_leg_count,
            config.oracle_leg_flags,
            &config.oracle_leg_feeds,
        ) {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        let mut acc = 0u64;
        let mut advanced = false;
        let mut max_publish_time = i64::MIN;
        let mut i = 0usize;
        while i < count {
            let (price, publish_time) = read_oracle_price_e6(
                &oracle_accounts[i],
                &config.oracle_leg_feeds[i],
                now_unix_ts,
                config.max_staleness_secs,
                config.conf_filter_bps,
            )?;
            let prev_time = config.oracle_leg_publish_times[i];
            let prev_price = config.oracle_leg_prices_e6[i];
            if prev_time != 0 {
                if publish_time < prev_time {
                    return Err(PercolatorError::OracleStale.into());
                }
                if publish_time == prev_time && prev_price != 0 && price != prev_price {
                    return Err(PercolatorError::OracleInvalid.into());
                }
            }
            if publish_time > prev_time {
                config.oracle_leg_publish_times[i] = publish_time;
                config.oracle_leg_prices_e6[i] = price;
                advanced = true;
            }
            max_publish_time = core::cmp::max(max_publish_time, publish_time);
            acc = if i == 0 {
                price
            } else {
                compose(acc, price, leg_divides(config, i))?
            };
            i += 1;
        }
        Ok((
            apply_transform(acc, config.invert, config.unit_scale)?,
            max_publish_time,
            advanced,
        ))
    }

    pub fn read_external_price_e6_profile(
        profile: &mut AssetOracleProfileV16,
        oracle_accounts: &[AccountInfo],
        now_unix_ts: i64,
    ) -> Result<(u64, i64, bool), ProgramError> {
        if profile.oracle_mode == ORACLE_MODE_MANUAL {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let count = profile.oracle_leg_count as usize;
        if count == 0 || count > ORACLE_LEG_CAP || oracle_accounts.len() < count {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        if !oracle_leg_config_ok(
            profile.oracle_leg_count,
            profile.oracle_leg_flags,
            &profile.oracle_leg_feeds,
        ) {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        let mut acc = 0u64;
        let mut advanced = false;
        let mut max_publish_time = i64::MIN;
        let mut i = 0usize;
        while i < count {
            let (price, publish_time) = read_oracle_price_e6(
                &oracle_accounts[i],
                &profile.oracle_leg_feeds[i],
                now_unix_ts,
                profile.max_staleness_secs,
                profile.conf_filter_bps,
            )?;
            let prev_time = profile.oracle_leg_publish_times[i];
            let prev_price = profile.oracle_leg_prices_e6[i];
            if prev_time != 0 {
                if publish_time < prev_time {
                    return Err(PercolatorError::OracleStale.into());
                }
                if publish_time == prev_time && prev_price != 0 && price != prev_price {
                    return Err(PercolatorError::OracleInvalid.into());
                }
            }
            if publish_time > prev_time {
                profile.oracle_leg_publish_times[i] = publish_time;
                profile.oracle_leg_prices_e6[i] = price;
                advanced = true;
            }
            max_publish_time = core::cmp::max(max_publish_time, publish_time);
            acc = if i == 0 {
                price
            } else {
                compose(acc, price, profile_leg_divides(profile, i))?
            };
            i += 1;
        }
        Ok((
            apply_transform(acc, profile.invert, profile.unit_scale)?,
            max_publish_time,
            advanced,
        ))
    }

    pub fn clamp_toward_engine_dt(p_last: u64, target: u64, cap_bps: u64, dt_slots: u64) -> u64 {
        if p_last == 0 || target == 0 {
            return target;
        }
        if cap_bps == 0 || dt_slots == 0 {
            return p_last;
        }
        let max_delta = (p_last as u128)
            .saturating_mul(cap_bps as u128)
            .saturating_mul(dt_slots as u128)
            / 10_000;
        let max_delta = core::cmp::min(max_delta, u64::MAX as u128) as u64;
        if target > p_last {
            core::cmp::min(target, p_last.saturating_add(max_delta))
        } else {
            core::cmp::max(target, p_last.saturating_sub(max_delta))
        }
    }

    pub fn effective_price_from_target(
        anchor: u64,
        target: u64,
        max_change_bps: u64,
        dt_slots: u64,
        exposed: bool,
    ) -> u64 {
        if exposed {
            clamp_toward_engine_dt(anchor, target, max_change_bps, dt_slots)
        } else {
            target
        }
    }
}

pub mod policy_v16 {
    use crate::constants::MAX_DYNAMIC_TRADE_FEE_BPS;

    pub fn price_move_bps_ceil(old: u64, new: u64) -> Option<u64> {
        if old == 0 || old == new {
            return Some(0);
        }
        let diff = old.abs_diff(new) as u128;
        let den = old as u128;
        let bps = diff.checked_mul(10_000)?.checked_add(den.checked_sub(1)?)? / den;
        u64::try_from(bps).ok()
    }

    fn two_sided_trade_fee_paid_cap(notional: u128, fee_bps: u64) -> Option<u64> {
        if notional == 0 || fee_bps == 0 {
            return Some(0);
        }
        let one_side = notional.checked_mul(fee_bps as u128)?.checked_add(9_999)? / 10_000;
        u64::try_from(one_side.checked_mul(2)?).ok()
    }

    fn ceil_div_u128(num: u128, den: u128) -> Option<u128> {
        if den == 0 {
            return None;
        }
        Some(num.checked_add(den.checked_sub(1)?)? / den)
    }

    fn ewma_effective_alpha_bps(alpha_bps: u128, fee_paid: u64, mark_min_fee: u64) -> u128 {
        if mark_min_fee == 0 || fee_paid >= mark_min_fee {
            alpha_bps
        } else {
            alpha_bps.saturating_mul(fee_paid as u128) / mark_min_fee as u128
        }
    }

    pub fn ewma_update(
        old: u64,
        price: u64,
        halflife_slots: u64,
        last_slot: u64,
        now_slot: u64,
        fee_paid: u64,
        mark_min_fee: u64,
    ) -> u64 {
        if old == 0 {
            if mark_min_fee > 0 && fee_paid < mark_min_fee {
                return 0;
            }
            return price;
        }
        let dt = now_slot.saturating_sub(last_slot);
        if dt == 0 {
            return old;
        }
        if halflife_slots == 0 {
            return price;
        }
        if fee_paid == 0 && mark_min_fee > 0 {
            return old;
        }
        let alpha_bps = (10_000u128 * dt as u128) / (dt as u128 + halflife_slots as u128);
        let alpha_bps = ewma_effective_alpha_bps(alpha_bps, fee_paid, mark_min_fee);
        let old128 = old as u128;
        let price128 = price as u128;
        let out = if price >= old {
            old128 + ((price128 - old128) * alpha_bps / 10_000)
        } else {
            old128 - ((old128 - price128) * alpha_bps / 10_000)
        };
        core::cmp::min(out, u64::MAX as u128) as u64
    }

    pub fn dynamic_fee_bps_with_externality_floor(
        base_fee_bps: u64,
        old_mark_e6: u64,
        clamped_exec_e6: u64,
        halflife_slots: u64,
        last_mark_slot: u64,
        now_slot: u64,
        trade_notional: u128,
        mark_externality_notional: u128,
        mark_min_fee: u64,
        min_externality_bps: u64,
    ) -> Option<u64> {
        if base_fee_bps > MAX_DYNAMIC_TRADE_FEE_BPS {
            return None;
        }
        let mut fee_bps = base_fee_bps;
        let mut i = 0;
        while i < 64 {
            let fee_paid = two_sided_trade_fee_paid_cap(trade_notional, fee_bps)?;
            let next_mark = ewma_update(
                old_mark_e6,
                clamped_exec_e6,
                halflife_slots,
                last_mark_slot,
                now_slot,
                fee_paid,
                mark_min_fee,
            );
            let mark_move_bps = price_move_bps_ceil(old_mark_e6, next_mark)?;
            let charged_move_bps = core::cmp::max(mark_move_bps, min_externality_bps);
            let base_paid = two_sided_trade_fee_paid_cap(trade_notional, base_fee_bps)? as u128;
            let mark_fee = ceil_div_u128(
                mark_externality_notional.checked_mul(charged_move_bps as u128)?,
                10_000,
            )?;
            let required = base_paid.checked_add(mark_fee)?;
            let denom = trade_notional.checked_mul(2)?;
            let needed = ceil_div_u128(required.checked_mul(10_000)?, denom)?;
            let needed = u64::try_from(needed).ok()?;
            if needed > MAX_DYNAMIC_TRADE_FEE_BPS {
                return None;
            }
            if needed <= fee_bps {
                return Some(fee_bps);
            }
            fee_bps = needed;
            i += 1;
        }
        None
    }
}

pub mod processor {
    use super::*;
    use crate::{
        error::{map_v16_error, PercolatorError},
        ix::Instruction,
        state::{self, WrapperConfigV16},
    };

    pub const AUTHORITY_ADMIN: u8 = 0;
    pub const AUTHORITY_HYPERP_MARK: u8 = 1;
    pub const AUTHORITY_INSURANCE: u8 = 2;
    pub const AUTHORITY_BACKING_BUCKET: u8 = 3;
    pub const AUTHORITY_INSURANCE_OPERATOR: u8 = 4;
    pub const AUTHORITY_ASSET: u8 = 5;

    pub const ASSET_ACTION_ACTIVATE: u8 = 0;
    pub const ASSET_ACTION_DRAIN_ONLY: u8 = 1;
    pub const ASSET_ACTION_RETIRE: u8 = 2;

    fn authenticated_slot_or_fallback(fallback_slot: u64) -> u64 {
        Clock::get().map(|c| c.slot).unwrap_or(fallback_slot)
    }

    fn authenticated_market_slot_or_fallback(group: &MarketGroupV16) -> u64 {
        Clock::get().map(|c| c.slot).unwrap_or(group.current_slot)
    }

    fn decode_side(value: u8) -> Result<SideV16, ProgramError> {
        match value {
            0 => Ok(SideV16::Long),
            1 => Ok(SideV16::Short),
            _ => Err(PercolatorError::InvalidInstruction.into()),
        }
    }

    fn decode_recovery_reason(value: u8) -> Result<PermissionlessRecoveryReasonV16, ProgramError> {
        match value {
            0 => Ok(PermissionlessRecoveryReasonV16::BelowProgressFloor),
            1 => Ok(PermissionlessRecoveryReasonV16::BlockedSegmentHeadroomOrRepresentability),
            2 => Ok(PermissionlessRecoveryReasonV16::AccountBSettlementCannotProgress),
            3 => Ok(PermissionlessRecoveryReasonV16::BIndexHeadroomExhausted),
            4 => Ok(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress),
            5 => Ok(PermissionlessRecoveryReasonV16::ExplicitLossOrDustAuditOverflow),
            6 => {
                Ok(PermissionlessRecoveryReasonV16::OracleOrTargetUnavailableByAuthenticatedPolicy)
            }
            7 => Ok(PermissionlessRecoveryReasonV16::CounterOrEpochOverflowDeclaredRecovery),
            _ => Err(PercolatorError::InvalidInstruction.into()),
        }
    }

    fn permissionless_resolve_matured_now(cfg: &WrapperConfigV16, group: &MarketGroupV16) -> bool {
        oracle_v16::permissionless_stale_matured(cfg, authenticated_market_slot_or_fallback(group))
    }

    fn permissionless_resolve_matured_for_profile(
        cfg: &WrapperConfigV16,
        profile: &state::AssetOracleProfileV16,
        group: &MarketGroupV16,
    ) -> bool {
        cfg.permissionless_resolve_stale_slots != 0
            && profile.last_good_oracle_slot != 0
            && authenticated_market_slot_or_fallback(group)
                .saturating_sub(profile.last_good_oracle_slot)
                >= cfg.permissionless_resolve_stale_slots
    }

    fn reject_permissionless_resolve_matured_live(
        cfg: &WrapperConfigV16,
        group: &MarketGroupV16,
    ) -> ProgramResult {
        if group.mode == MarketModeV16::Live && permissionless_resolve_matured_now(cfg, group) {
            return Err(PercolatorError::OracleStale.into());
        }
        Ok(())
    }

    fn reject_permissionless_resolve_matured_live_for_profile(
        cfg: &WrapperConfigV16,
        profile: &state::AssetOracleProfileV16,
        group: &MarketGroupV16,
    ) -> ProgramResult {
        if !oracle_v16::profile_is_price_managed(profile) {
            return reject_permissionless_resolve_matured_live(cfg, group);
        }
        if group.mode == MarketModeV16::Live
            && permissionless_resolve_matured_for_profile(cfg, profile, group)
        {
            return Err(PercolatorError::OracleStale.into());
        }
        Ok(())
    }

    fn read_oracle_profile_for_asset(
        market_data: &[u8],
        cfg: &WrapperConfigV16,
        asset_index: usize,
    ) -> Result<state::AssetOracleProfileV16, ProgramError> {
        if asset_index == 0 {
            Ok(state::asset_oracle_profile_from_config(cfg))
        } else {
            state::read_asset_oracle_profile(market_data, asset_index)
        }
    }

    fn write_oracle_profile_if_separate(
        market_data: &mut [u8],
        asset_index: usize,
        profile: &state::AssetOracleProfileV16,
    ) -> ProgramResult {
        if asset_index != 0 && oracle_v16::profile_is_price_managed(profile) {
            state::write_asset_oracle_profile(market_data, asset_index, profile)?;
        }
        Ok(())
    }

    fn mirror_manual_profile_to_base_config(
        cfg: &mut WrapperConfigV16,
        profile: &state::AssetOracleProfileV16,
        refresh_liveness: bool,
    ) {
        cfg.oracle_mode = constants::ORACLE_MODE_MANUAL;
        cfg.oracle_leg_count = 0;
        cfg.oracle_leg_flags = 0;
        cfg.invert = 0;
        cfg.unit_scale = 0;
        cfg.conf_filter_bps = 0;
        cfg.max_staleness_secs = 0;
        cfg.hybrid_soft_stale_slots = 0;
        cfg.mark_ewma_e6 = profile.mark_ewma_e6;
        cfg.mark_ewma_last_slot = profile.mark_ewma_last_slot;
        cfg.mark_ewma_halflife_slots = profile.mark_ewma_halflife_slots;
        cfg.mark_min_fee = 0;
        cfg.oracle_target_price_e6 = profile.oracle_target_price_e6;
        cfg.oracle_target_publish_time = 0;
        if refresh_liveness {
            cfg.last_good_oracle_slot = profile.last_good_oracle_slot;
        }
        cfg.oracle_leg_feeds = [[0u8; 32]; constants::ORACLE_LEG_CAP];
        cfg.oracle_leg_prices_e6 = [0u64; constants::ORACLE_LEG_CAP];
        cfg.oracle_leg_publish_times = [0i64; constants::ORACLE_LEG_CAP];
    }

    fn require_asset_active_for_oracle_reconfiguration(
        group: &MarketGroupV16,
        asset_index: usize,
    ) -> ProgramResult {
        if group.assets[asset_index].lifecycle != AssetLifecycleV16::Active
            || asset_has_position_or_loss_state(group, asset_index)
        {
            return Err(PercolatorError::EngineLockActive.into());
        }
        Ok(())
    }

    fn require_asset_mark_pushable(group: &MarketGroupV16, asset_index: usize) -> ProgramResult {
        match group.assets[asset_index].lifecycle {
            AssetLifecycleV16::Active | AssetLifecycleV16::DrainOnly => Ok(()),
            _ => Err(PercolatorError::EngineLockActive.into()),
        }
    }

    pub fn process_instruction<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        match Instruction::decode(instruction_data)? {
            Instruction::InitMarket {
                max_portfolio_assets,
                h_min,
                h_max,
                initial_price,
                min_nonzero_mm_req,
                min_nonzero_im_req,
                maintenance_margin_bps,
                initial_margin_bps,
                max_trading_fee_bps,
                trade_fee_base_bps,
                liquidation_fee_bps,
                liquidation_fee_cap,
                min_liquidation_abs,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots,
                max_account_b_settlement_chunks,
                max_bankrupt_close_chunks,
                max_bankrupt_close_lifetime_slots,
                public_b_chunk_atoms,
                maintenance_fee_per_slot,
            } => handle_init_market(
                program_id,
                accounts,
                max_portfolio_assets,
                h_min,
                h_max,
                initial_price,
                min_nonzero_mm_req,
                min_nonzero_im_req,
                maintenance_margin_bps,
                initial_margin_bps,
                max_trading_fee_bps,
                trade_fee_base_bps,
                liquidation_fee_bps,
                liquidation_fee_cap,
                min_liquidation_abs,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots,
                max_account_b_settlement_chunks,
                max_bankrupt_close_chunks,
                max_bankrupt_close_lifetime_slots,
                public_b_chunk_atoms,
                maintenance_fee_per_slot,
            ),
            Instruction::InitPortfolio => handle_init_portfolio(program_id, accounts),
            Instruction::Deposit { amount } => handle_deposit(program_id, accounts, amount),
            Instruction::Withdraw { amount } => handle_withdraw(program_id, accounts, amount),
            Instruction::PermissionlessCrank {
                action,
                asset_index,
                now_slot,
                effective_price,
                funding_rate_e9,
                close_q,
                fee_bps,
                recovery_reason,
            } => handle_permissionless_crank(
                program_id,
                accounts,
                action,
                asset_index,
                now_slot,
                effective_price,
                funding_rate_e9,
                close_q,
                fee_bps,
                recovery_reason,
            ),
            Instruction::TradeNoCpi {
                asset_index,
                size_q,
                exec_price,
                fee_bps,
            } => handle_trade_nocpi(
                program_id,
                accounts,
                asset_index,
                size_q,
                exec_price,
                fee_bps,
            ),
            Instruction::TradeCpi {
                asset_index,
                size_q,
                fee_bps,
                limit_price,
            } => handle_trade_cpi(
                program_id,
                accounts,
                asset_index,
                size_q,
                fee_bps,
                limit_price,
            ),
            Instruction::ClosePortfolio => handle_close_portfolio(program_id, accounts),
            Instruction::TopUpInsurance { amount } => {
                handle_top_up_insurance(program_id, accounts, amount)
            }
            Instruction::CloseSlab => handle_close_slab(program_id, accounts),
            Instruction::ResolveMarket => handle_resolve_market(program_id, accounts),
            Instruction::WithdrawInsuranceLimited { amount } => {
                handle_withdraw_insurance_limited(program_id, accounts, amount)
            }
            Instruction::TopUpBackingBucket {
                domain,
                amount,
                expiry_slot,
            } => handle_top_up_backing_bucket(program_id, accounts, domain, amount, expiry_slot),
            Instruction::WithdrawBackingBucket { domain, amount } => {
                handle_withdraw_backing_bucket(program_id, accounts, domain, amount)
            }
            Instruction::ConvertReleasedPnl { amount } => {
                handle_convert_released_pnl(program_id, accounts, amount)
            }
            Instruction::CloseResolved { fee_rate_per_slot } => {
                handle_close_resolved(program_id, accounts, fee_rate_per_slot)
            }
            Instruction::UpdateAuthority { kind, new_pubkey } => {
                handle_update_authority(program_id, accounts, kind, new_pubkey)
            }
            Instruction::UpdateInsurancePolicy {
                max_bps,
                deposits_only,
                cooldown_slots,
            } => handle_update_insurance_policy(
                program_id,
                accounts,
                max_bps,
                deposits_only,
                cooldown_slots,
            ),
            Instruction::UpdateLiquidationFeePolicy { cranker_share_bps } => {
                handle_update_liquidation_fee_policy(program_id, accounts, cranker_share_bps)
            }
            Instruction::UpdateMaintenanceFeePolicy { cranker_share_bps } => {
                handle_update_maintenance_fee_policy(program_id, accounts, cranker_share_bps)
            }
            Instruction::UpdateBackingFeePolicy { fee_bps } => {
                handle_update_backing_fee_policy(program_id, accounts, fee_bps)
            }
            Instruction::ConfigurePermissionlessResolve {
                stale_slots,
                force_close_delay_slots,
            } => handle_configure_permissionless_resolve(
                program_id,
                accounts,
                stale_slots,
                force_close_delay_slots,
            ),
            Instruction::ResolveStalePermissionless { now_slot } => {
                handle_resolve_stale_permissionless(program_id, accounts, now_slot)
            }
            Instruction::ConfigureHybridOracle {
                asset_index,
                now_slot,
                now_unix_ts,
                oracle_leg_count,
                oracle_leg_flags,
                max_staleness_secs,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots,
                mark_min_fee,
                invert,
                unit_scale,
                conf_filter_bps,
                oracle_leg_feeds,
            } => handle_configure_hybrid_oracle(
                program_id,
                accounts,
                asset_index,
                now_slot,
                now_unix_ts,
                oracle_leg_count,
                oracle_leg_flags,
                max_staleness_secs,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots,
                mark_min_fee,
                invert,
                unit_scale,
                conf_filter_bps,
                oracle_leg_feeds,
            ),
            Instruction::ConfigureHyperpMark {
                asset_index,
                now_slot,
                initial_mark_e6,
                mark_ewma_halflife_slots,
                mark_min_fee,
            } => handle_configure_hyperp_mark(
                program_id,
                accounts,
                asset_index,
                now_slot,
                initial_mark_e6,
                mark_ewma_halflife_slots,
                mark_min_fee,
            ),
            Instruction::PushHyperpMark {
                asset_index,
                now_slot,
                mark_e6,
            } => handle_push_hyperp_mark(program_id, accounts, asset_index, now_slot, mark_e6),
            Instruction::UpdateAssetLifecycle {
                action,
                asset_index,
                now_slot,
                initial_price,
            } => handle_update_asset_lifecycle(
                program_id,
                accounts,
                action,
                asset_index,
                now_slot,
                initial_price,
            ),
            Instruction::WithdrawInsurance { amount } => {
                handle_withdraw_insurance(program_id, accounts, amount)
            }
            Instruction::CureAndCancelClose { optional_deposit } => {
                handle_cure_and_cancel_close(program_id, accounts, optional_deposit)
            }
            Instruction::ForfeitRecoveryLeg {
                asset_index,
                b_delta_budget,
            } => handle_forfeit_recovery_leg(program_id, accounts, asset_index, b_delta_budget),
            Instruction::RebalanceReduce {
                asset_index,
                reduce_q,
            } => handle_rebalance_reduce(program_id, accounts, asset_index, reduce_q),
            Instruction::FinalizeResetSide { asset_index, side } => {
                handle_finalize_reset_side(program_id, accounts, asset_index, side)
            }
            Instruction::ClaimResolvedPayoutTopup => {
                handle_claim_resolved_payout_topup(program_id, accounts)
            }
            Instruction::RefineResolvedUnreceiptedBound { decrease_num } => {
                handle_refine_resolved_unreceipted_bound(program_id, accounts, decrease_num)
            }
            Instruction::SyncMaintenanceFee { now_slot } => {
                handle_sync_maintenance_fee(program_id, accounts, now_slot)
            }
        }
    }

    #[inline(never)]
    fn handle_init_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        max_portfolio_assets: u16,
        h_min: u64,
        h_max: u64,
        initial_price: u64,
        min_nonzero_mm_req: u128,
        min_nonzero_im_req: u128,
        maintenance_margin_bps: u64,
        initial_margin_bps: u64,
        max_trading_fee_bps: u64,
        trade_fee_base_bps: u64,
        liquidation_fee_bps: u64,
        liquidation_fee_cap: u128,
        min_liquidation_abs: u128,
        max_price_move_bps_per_slot: u64,
        max_accrual_dt_slots: u64,
        max_abs_funding_e9_per_slot: u64,
        min_funding_lifetime_slots: u64,
        max_account_b_settlement_chunks: u64,
        max_bankrupt_close_chunks: u64,
        max_bankrupt_close_lifetime_slots: u64,
        public_b_chunk_atoms: u128,
        maintenance_fee_per_slot: u128,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let mint_ai = account(accounts, 2)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        verify_mint(mint_ai)?;
        if trade_fee_base_bps > max_trading_fee_bps
            || max_portfolio_assets == 0
            || max_portfolio_assets > constants::WRAPPER_MAX_PORTFOLIO_ASSETS
        {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        let mut cfg = V16Config::public_user_fund(max_portfolio_assets, h_min, h_max);
        cfg.min_nonzero_mm_req = min_nonzero_mm_req;
        cfg.min_nonzero_im_req = min_nonzero_im_req;
        cfg.maintenance_margin_bps = maintenance_margin_bps;
        cfg.initial_margin_bps = initial_margin_bps;
        cfg.max_trading_fee_bps = max_trading_fee_bps;
        cfg.liquidation_fee_bps = liquidation_fee_bps;
        cfg.liquidation_fee_cap = liquidation_fee_cap;
        cfg.min_liquidation_abs = min_liquidation_abs;
        cfg.max_price_move_bps_per_slot = max_price_move_bps_per_slot;
        cfg.max_accrual_dt_slots = max_accrual_dt_slots;
        cfg.max_abs_funding_e9_per_slot = max_abs_funding_e9_per_slot;
        cfg.min_funding_lifetime_slots = min_funding_lifetime_slots;
        cfg.max_account_b_settlement_chunks = max_account_b_settlement_chunks;
        cfg.max_bankrupt_close_chunks = max_bankrupt_close_chunks;
        cfg.max_bankrupt_close_lifetime_slots = max_bankrupt_close_lifetime_slots;
        cfg.public_b_chunk_atoms = public_b_chunk_atoms;
        let mut group = new_market_group_boxed(market_ai.key.to_bytes(), cfg)?;
        if initial_price == 0 || initial_price > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        group.assets[0].raw_oracle_target_price = initial_price;
        group.assets[0].effective_price = initial_price;
        group.assets[0].fund_px_last = initial_price;
        let mut i = 1;
        while i < group.config.max_market_slots as usize {
            group.assets[i].raw_oracle_target_price = initial_price;
            group.assets[i].effective_price = initial_price;
            group.assets[i].fund_px_last = initial_price;
            i += 1;
        }
        group.assert_public_invariants().map_err(map_v16_error)?;
        let wrapper = WrapperConfigV16 {
            admin: admin.key.to_bytes(),
            collateral_mint: mint_ai.key.to_bytes(),
            maintenance_fee_per_slot,
            trade_fee_base_bps,
            permissionless_resolve_stale_slots: 0,
            force_close_delay_slots: 0,
            last_good_oracle_slot: Clock::get().map(|c| c.slot).unwrap_or(0),
            insurance_authority: admin.key.to_bytes(),
            insurance_operator: admin.key.to_bytes(),
            backing_bucket_authority: admin.key.to_bytes(),
            asset_authority: admin.key.to_bytes(),
            hyperp_mark_authority: admin.key.to_bytes(),
            insurance_withdraw_deposit_remaining: 0,
            insurance_withdraw_max_bps: 0,
            liquidation_cranker_fee_share_bps: 0,
            maintenance_cranker_fee_share_bps: 0,
            backing_trade_fee_bps: 0,
            unit_scale: 0,
            conf_filter_bps: 0,
            insurance_withdraw_deposits_only: 0,
            oracle_mode: constants::ORACLE_MODE_MANUAL,
            oracle_leg_count: 0,
            oracle_leg_flags: 0,
            invert: 0,
            _padding0: 0,
            _padding1: [0u8; 4],
            insurance_withdraw_cooldown_slots: 0,
            last_insurance_withdraw_slot: 0,
            max_staleness_secs: 0,
            hybrid_soft_stale_slots: 0,
            mark_ewma_e6: initial_price,
            mark_ewma_last_slot: Clock::get().map(|c| c.slot).unwrap_or(0),
            mark_ewma_halflife_slots: constants::DEFAULT_MARK_EWMA_HALFLIFE_SLOTS,
            mark_min_fee: 0,
            oracle_target_price_e6: initial_price,
            oracle_target_publish_time: 0,
            oracle_leg_feeds: [[0u8; 32]; constants::ORACLE_LEG_CAP],
            oracle_leg_prices_e6: [0u64; constants::ORACLE_LEG_CAP],
            oracle_leg_publish_times: [0i64; constants::ORACLE_LEG_CAP],
            _padding_tail: [0u8; 8],
        };
        state::init_market_account(
            &mut market_ai.try_borrow_mut_data()?,
            &wrapper,
            group.as_ref(),
        )
    }

    #[inline(never)]
    fn handle_init_portfolio<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        expect_signer(owner)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        if state::is_initialized(&portfolio_ai.try_borrow_data()?) {
            return Err(PercolatorError::AlreadyInitialized.into());
        }
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        let account = new_portfolio_boxed(
            ProvenanceHeaderV16::new(
                market_ai.key.to_bytes(),
                portfolio_ai.key.to_bytes(),
                owner.key.to_bytes(),
            ),
            authenticated_market_slot_or_fallback(group.as_ref()),
        )?;
        group
            .create_portfolio_account(account.as_ref())
            .map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::init_portfolio_account(&mut portfolio_ai.try_borrow_mut_data()?, account.as_ref())
    }

    #[inline(never)]
    fn handle_deposit<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        let source_token = account(accounts, 3)?;
        let vault_token = account(accounts, 4)?;
        let token_program = account(accounts, 5)?;
        expect_signer(owner)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_writable(source_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        verify_token_program(token_program)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;
        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, _) = derive_vault_authority(program_id, market_ai.key);
        verify_user_token_account(source_token, owner.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(source_token, amount_u64)?;

        group
            .deposit_not_atomic(portfolio.as_mut(), amount)
            .map_err(map_v16_error)?;
        transfer_tokens(token_program, source_token, vault_token, owner, amount_u64)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    #[inline(never)]
    fn handle_withdraw<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        let dest_token = account(accounts, 3)?;
        let vault_token = account(accounts, 4)?;
        let vault_authority_ai = account(accounts, 5)?;
        let token_program = account(accounts, 6)?;
        expect_signer(owner)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_writable(dest_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        verify_token_program(token_program)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;
        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
        expect_key(vault_authority_ai, &vault_authority)?;
        verify_user_token_account(dest_token, owner.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(vault_token, amount_u64)?;

        let prices = effective_prices_boxed(group.as_ref()).map_err(map_v16_error)?;
        group
            .withdraw_not_atomic(portfolio.as_mut(), amount, &prices[..])
            .map_err(map_v16_error)?;
        let bump_arr = [bump];
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
        transfer_tokens_signed(
            token_program,
            vault_token,
            dest_token,
            vault_authority_ai,
            amount_u64,
            signer_seeds,
        )?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    #[inline(never)]
    fn handle_trade_nocpi<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> ProgramResult {
        let signer_a = account(accounts, 0)?;
        let signer_b = account(accounts, 1)?;
        let market_ai = account(accounts, 2)?;
        let account_a_ai = account(accounts, 3)?;
        let account_b_ai = account(accounts, 4)?;
        expect_signer(signer_a)?;
        expect_signer(signer_b)?;
        expect_writable(market_ai)?;
        expect_writable(account_a_ai)?;
        expect_writable(account_b_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(account_a_ai, program_id)?;
        expect_owner(account_b_ai, program_id)?;
        if account_a_ai.key == account_b_ai.key {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut oracle_profile = read_oracle_profile_for_asset(
            &market_ai.try_borrow_data()?,
            &cfg,
            asset_index as usize,
        )?;
        let mut account_a = state::read_portfolio_boxed(&account_a_ai.try_borrow_data()?)?;
        let mut account_b = state::read_portfolio_boxed(&account_b_ai.try_borrow_data()?)?;
        reject_permissionless_resolve_matured_live_for_profile(
            &cfg,
            &oracle_profile,
            group.as_ref(),
        )?;
        expect_portfolio_account_key(account_a.as_ref(), account_a_ai.key)?;
        expect_portfolio_account_key(account_b.as_ref(), account_b_ai.key)?;
        expect_portfolio_owner(account_a.as_ref(), signer_a.key)?;
        expect_portfolio_owner(account_b.as_ref(), signer_b.key)?;
        let size_abs = if size_q == i128::MIN || size_q == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        } else {
            size_q.unsigned_abs()
        };
        let prices = effective_prices_boxed(group.as_ref()).map_err(map_v16_error)?;
        let fee_bps = hybrid_trade_fee_bps(
            &cfg,
            &oracle_profile,
            group.as_ref(),
            asset_index as usize,
            size_abs,
            exec_price,
            fee_bps,
        )?;
        let req = TradeRequestV16 {
            asset_index: asset_index as usize,
            size_q: size_abs,
            exec_price,
            fee_bps,
        };
        let outcome = if size_q > 0 {
            execute_trade_svm_aware(
                group.as_mut(),
                account_a.as_mut(),
                account_b.as_mut(),
                req,
                &prices[..],
            )
            .map_err(map_v16_error)?
        } else {
            execute_trade_svm_aware(
                group.as_mut(),
                account_b.as_mut(),
                account_a.as_mut(),
                req,
                &prices[..],
            )
            .map_err(map_v16_error)?
        };
        update_hybrid_mark_after_trade(
            &mut oracle_profile,
            group.as_ref(),
            asset_index as usize,
            exec_price,
            outcome
                .fee_a
                .checked_add(outcome.fee_b)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?,
        )?;
        if asset_index == 0 && oracle_v16::profile_is_price_managed(&oracle_profile) {
            cfg.mark_ewma_e6 = oracle_profile.mark_ewma_e6;
            cfg.mark_ewma_last_slot = oracle_profile.mark_ewma_last_slot;
        }
        let mut market_data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut market_data, &cfg, group.as_ref())?;
        write_oracle_profile_if_separate(&mut market_data, asset_index as usize, &oracle_profile)?;
        state::write_portfolio(&mut account_a_ai.try_borrow_mut_data()?, account_a.as_ref())?;
        state::write_portfolio(&mut account_b_ai.try_borrow_mut_data()?, account_b.as_ref())
    }

    #[inline(never)]
    fn handle_trade_cpi<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
        limit_price: u64,
    ) -> ProgramResult {
        let signer_a = account(accounts, 0)?;
        let signer_b = account(accounts, 1)?;
        let market_ai = account(accounts, 2)?;
        let account_a_ai = account(accounts, 3)?;
        let account_b_ai = account(accounts, 4)?;
        let matcher_prog = account(accounts, 5)?;
        let matcher_ctx = account(accounts, 6)?;
        let matcher_delegate = account(accounts, 7)?;
        let tail = &accounts[8..];

        expect_signer(signer_a)?;
        expect_signer(signer_b)?;
        expect_writable(market_ai)?;
        expect_writable(account_a_ai)?;
        expect_writable(account_b_ai)?;
        expect_writable(matcher_ctx)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(account_a_ai, program_id)?;
        expect_owner(account_b_ai, program_id)?;
        if account_a_ai.key == account_b_ai.key
            || !matcher_prog.executable
            || matcher_ctx.owner != matcher_prog.key
            || matcher_ctx.data_len() < constants::MATCHER_CONTEXT_MIN_LEN
            || tail.len() > constants::MAX_MATCHER_TAIL_ACCOUNTS
        {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        for ai in tail {
            if ai.key == market_ai.key
                || ai.key == account_a_ai.key
                || ai.key == account_b_ai.key
                || ai.key == program_id
                || ai.owner == program_id
            {
                return Err(PercolatorError::InvalidInstruction.into());
            }
        }

        let (delegate, bump) = derive_matcher_delegate(
            program_id,
            market_ai.key,
            account_b_ai.key,
            matcher_prog.key,
            matcher_ctx.key,
        );
        expect_key(matcher_delegate, &delegate)?;

        let (cfg_pre, mode_pre, current_slot_pre, oracle_price, max_trading_fee_bps) =
            state::read_market_trade_preflight(
                &market_ai.try_borrow_data()?,
                asset_index as usize,
            )?;
        let (account_a_header, account_a_owner) =
            state::read_portfolio_owner_preflight(&account_a_ai.try_borrow_data()?)?;
        let (account_b_header, account_b_owner) =
            state::read_portfolio_owner_preflight(&account_b_ai.try_borrow_data()?)?;
        if mode_pre != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        let oracle_profile_pre = read_oracle_profile_for_asset(
            &market_ai.try_borrow_data()?,
            &cfg_pre,
            asset_index as usize,
        )?;
        let stale_matured = if oracle_v16::profile_is_price_managed(&oracle_profile_pre) {
            cfg_pre.permissionless_resolve_stale_slots != 0
                && authenticated_slot_or_fallback(current_slot_pre)
                    .saturating_sub(oracle_profile_pre.last_good_oracle_slot)
                    >= cfg_pre.permissionless_resolve_stale_slots
        } else {
            oracle_v16::permissionless_stale_matured(
                &cfg_pre,
                authenticated_slot_or_fallback(current_slot_pre),
            )
        };
        if stale_matured {
            return Err(PercolatorError::OracleStale.into());
        }
        let fee_floor_pre = core::cmp::max(
            core::cmp::max(fee_bps, cfg_pre.trade_fee_base_bps),
            cfg_pre.backing_trade_fee_bps as u64,
        );
        if fee_floor_pre > max_trading_fee_bps {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        if account_a_header.portfolio_account_id != account_a_ai.key.to_bytes()
            || account_b_header.portfolio_account_id != account_b_ai.key.to_bytes()
        {
            return Err(PercolatorError::EngineProvenanceMismatch.into());
        }
        if account_a_owner != signer_a.key.to_bytes() || account_b_owner != signer_b.key.to_bytes()
        {
            return Err(PercolatorError::Unauthorized.into());
        }
        if size_q == 0 || size_q == i128::MIN {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        if oracle_price == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let req_id = current_slot_pre.wrapping_add(1);
        let lp_account_id = matcher_lp_account_id(&delegate);

        invoke_matcher(
            matcher_prog,
            matcher_ctx,
            matcher_delegate,
            tail,
            req_id,
            asset_index,
            lp_account_id,
            oracle_price,
            size_q,
            &[
                b"matcher",
                market_ai.key.as_ref(),
                account_b_ai.key.as_ref(),
                matcher_prog.key.as_ref(),
                matcher_ctx.key.as_ref(),
                &[bump],
            ],
        )?;

        let ret = {
            let data = matcher_ctx.try_borrow_data()?;
            matcher_abi::read_matcher_return(&data)?
        };
        matcher_abi::validate_matcher_return(
            &ret,
            lp_account_id,
            asset_index,
            oracle_price,
            size_q,
            req_id,
        )?;
        if limit_price != 0 {
            let limit_ok = if size_q > 0 {
                ret.exec_price_e6 <= limit_price
            } else {
                ret.exec_price_e6 >= limit_price
            };
            if !limit_ok {
                return Err(PercolatorError::InvalidInstruction.into());
            }
        }
        if ret.exec_size == 0 {
            return Ok(());
        }

        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut oracle_profile = read_oracle_profile_for_asset(
            &market_ai.try_borrow_data()?,
            &cfg,
            asset_index as usize,
        )?;
        let mut account_a = state::read_portfolio_boxed(&account_a_ai.try_borrow_data()?)?;
        let mut account_b = state::read_portfolio_boxed(&account_b_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(account_a.as_ref(), account_a_ai.key)?;
        expect_portfolio_account_key(account_b.as_ref(), account_b_ai.key)?;
        expect_portfolio_owner(account_a.as_ref(), signer_a.key)?;
        expect_portfolio_owner(account_b.as_ref(), signer_b.key)?;

        let prices = effective_prices_boxed(group.as_ref()).map_err(map_v16_error)?;
        let fee_bps = hybrid_trade_fee_bps(
            &cfg,
            &oracle_profile,
            group.as_ref(),
            asset_index as usize,
            ret.exec_size.unsigned_abs(),
            ret.exec_price_e6,
            fee_bps,
        )?;
        let req = TradeRequestV16 {
            asset_index: asset_index as usize,
            size_q: ret.exec_size.unsigned_abs(),
            exec_price: ret.exec_price_e6,
            fee_bps,
        };
        let outcome = if ret.exec_size > 0 {
            execute_trade_svm_aware(
                group.as_mut(),
                account_a.as_mut(),
                account_b.as_mut(),
                req,
                &prices[..],
            )
            .map_err(map_v16_error)?
        } else {
            execute_trade_svm_aware(
                group.as_mut(),
                account_b.as_mut(),
                account_a.as_mut(),
                req,
                &prices[..],
            )
            .map_err(map_v16_error)?
        };
        update_hybrid_mark_after_trade(
            &mut oracle_profile,
            group.as_ref(),
            asset_index as usize,
            ret.exec_price_e6,
            outcome
                .fee_a
                .checked_add(outcome.fee_b)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?,
        )?;
        if asset_index == 0 && oracle_v16::profile_is_price_managed(&oracle_profile) {
            cfg.mark_ewma_e6 = oracle_profile.mark_ewma_e6;
            cfg.mark_ewma_last_slot = oracle_profile.mark_ewma_last_slot;
        }
        let mut market_data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut market_data, &cfg, group.as_ref())?;
        write_oracle_profile_if_separate(&mut market_data, asset_index as usize, &oracle_profile)?;
        state::write_portfolio(&mut account_a_ai.try_borrow_mut_data()?, account_a.as_ref())?;
        state::write_portfolio(&mut account_b_ai.try_borrow_mut_data()?, account_b.as_ref())
    }

    #[inline(never)]
    fn handle_close_portfolio<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        expect_signer(owner)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;
        group
            .close_portfolio_account(portfolio.as_ref())
            .map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        for b in portfolio_ai.try_borrow_mut_data()?.iter_mut() {
            *b = 0;
        }
        Ok(())
    }

    #[inline(never)]
    fn handle_top_up_insurance<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        let signer = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let source_token = account(accounts, 2)?;
        let vault_token = account(accounts, 3)?;
        let token_program = account(accounts, 4)?;
        expect_signer(signer)?;
        expect_writable(market_ai)?;
        expect_writable(source_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        verify_token_program(token_program)?;
        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        expect_live_authority(&cfg.insurance_authority, signer.key)?;
        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, _) = derive_vault_authority(program_id, market_ai.key);
        verify_user_token_account(source_token, signer.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(source_token, amount_u64)?;
        group.insurance = group
            .insurance
            .checked_add(amount)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        group.vault = group
            .vault
            .checked_add(amount)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        if cfg.insurance_withdraw_deposits_only != 0 {
            cfg.insurance_withdraw_deposit_remaining = cfg
                .insurance_withdraw_deposit_remaining
                .checked_add(amount)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        }
        group.assert_public_invariants().map_err(map_v16_error)?;
        transfer_tokens(token_program, source_token, vault_token, signer, amount_u64)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_top_up_backing_bucket<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        domain: u8,
        amount: u128,
        expiry_slot: u64,
    ) -> ProgramResult {
        let signer = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let source_token = account(accounts, 2)?;
        let vault_token = account(accounts, 3)?;
        let token_program = account(accounts, 4)?;
        expect_signer(signer)?;
        expect_writable(market_ai)?;
        expect_writable(source_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        verify_token_program(token_program)?;
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        expect_live_authority(&cfg.backing_bucket_authority, signer.key)?;
        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, _) = derive_vault_authority(program_id, market_ai.key);
        verify_user_token_account(source_token, signer.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(source_token, amount_u64)?;
        if amount != 0 {
            let backing_num = amount
                .checked_mul(BOUND_SCALE)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?;
            group.vault = group
                .vault
                .checked_add(amount)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?;
            // Provider receivable refill is engine-owned. This call first
            // repays consumed backing receivables, then records new fresh
            // backing; the wrapper must not duplicate that accounting.
            group
                .add_fresh_counterparty_backing_not_atomic(
                    domain as usize,
                    backing_num,
                    expiry_slot,
                )
                .map_err(map_v16_error)?;
            group.assert_public_invariants().map_err(map_v16_error)?;
        }
        transfer_tokens(token_program, source_token, vault_token, signer, amount_u64)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_withdraw_backing_bucket<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        domain: u8,
        amount: u128,
    ) -> ProgramResult {
        let authority = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let dest_token = account(accounts, 2)?;
        let vault_token = account(accounts, 3)?;
        let vault_authority_ai = account(accounts, 4)?;
        let token_program = account(accounts, 5)?;
        expect_signer(authority)?;
        expect_writable(market_ai)?;
        expect_writable(dest_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        verify_token_program(token_program)?;
        if amount == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.backing_bucket_authority, authority.key)?;
        match group.mode {
            MarketModeV16::Live => {
                reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
                if group.bankruptcy_hlock_active
                    || group.threshold_stress_active
                    || group.loss_stale_active
                    || group.recovery_reason.is_some()
                {
                    return Err(PercolatorError::EngineLockActive.into());
                }
            }
            MarketModeV16::Resolved => {
                if group.materialized_portfolio_count != 0 || group.c_tot != 0 {
                    return Err(PercolatorError::EngineLockActive.into());
                }
            }
            MarketModeV16::Recovery => return Err(PercolatorError::EngineLockActive.into()),
        }

        let domain = domain as usize;
        if domain >= V16_DOMAIN_COUNT {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        group.assert_public_invariants().map_err(map_v16_error)?;
        let backing_num = amount
            .checked_mul(BOUND_SCALE)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        let source = group.source_credit[domain];
        let bucket = group.source_backing_buckets[domain];
        if source.positive_claim_bound_num != 0
            || source.exact_positive_claim_num != 0
            || bucket.status != BackingBucketStatusV16::Fresh
            || bucket.fresh_unliened_backing_num < backing_num
            || source.fresh_reserved_backing_num < backing_num
            || amount > group.vault
        {
            return Err(PercolatorError::EngineLockActive.into());
        }

        {
            let bucket = &mut group.source_backing_buckets[domain];
            bucket.fresh_unliened_backing_num -= backing_num;
            if bucket.fresh_unliened_backing_num == 0 && bucket.valid_liened_backing_num == 0 {
                if bucket.impaired_liened_backing_num != 0 {
                    bucket.status = BackingBucketStatusV16::Impaired;
                } else if bucket.consumed_liened_backing_num != 0 {
                    bucket.status = BackingBucketStatusV16::Expired;
                } else {
                    bucket.status = BackingBucketStatusV16::Empty;
                    bucket.expiry_slot = 0;
                }
            }
        }
        group.source_credit[domain].fresh_reserved_backing_num -= backing_num;
        group.source_credit[domain].credit_rate_num = percolator::CREDIT_RATE_SCALE;
        group.source_credit[domain].credit_epoch = group.source_credit[domain]
            .credit_epoch
            .checked_add(1)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        group.risk_epoch = group
            .risk_epoch
            .checked_add(1)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        group.vault = group
            .vault
            .checked_sub(amount)
            .ok_or(PercolatorError::EngineCounterUnderflow)?;
        group
            .reservation_encumbrance_proof_for_domain(domain)
            .map_err(map_v16_error)?
            .validate()
            .map_err(map_v16_error)?;
        group.assert_public_invariants().map_err(map_v16_error)?;

        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
        expect_key(vault_authority_ai, &vault_authority)?;
        verify_user_token_account(dest_token, authority.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(vault_token, amount_u64)?;
        let bump_arr = [bump];
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
        transfer_tokens_signed(
            token_program,
            vault_token,
            dest_token,
            vault_authority_ai,
            amount_u64,
            signer_seeds,
        )?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_withdraw_insurance<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        let authority = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let dest_token = account(accounts, 2)?;
        let vault_token = account(accounts, 3)?;
        let vault_authority_ai = account(accounts, 4)?;
        let token_program = account(accounts, 5)?;
        expect_signer(authority)?;
        expect_writable(market_ai)?;
        expect_writable(dest_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        verify_token_program(token_program)?;
        if amount == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.insurance_authority, authority.key)?;
        if group.mode != MarketModeV16::Resolved
            || group.materialized_portfolio_count != 0
            || group.c_tot != 0
            || amount > group.insurance
            || amount > group.vault
        {
            return Err(PercolatorError::EngineLockActive.into());
        }

        group.insurance = group
            .insurance
            .checked_sub(amount)
            .ok_or(PercolatorError::EngineCounterUnderflow)?;
        group.vault = group
            .vault
            .checked_sub(amount)
            .ok_or(PercolatorError::EngineCounterUnderflow)?;
        group.assert_public_invariants().map_err(map_v16_error)?;

        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
        expect_key(vault_authority_ai, &vault_authority)?;
        verify_user_token_account(dest_token, authority.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(vault_token, amount_u64)?;
        let bump_arr = [bump];
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
        transfer_tokens_signed(
            token_program,
            vault_token,
            dest_token,
            vault_authority_ai,
            amount_u64,
            signer_seeds,
        )?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_close_slab<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
        let admin_dest = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let vault_token = account(accounts, 2)?;
        let vault_authority_ai = account(accounts, 3)?;
        let dest_token = account(accounts, 4)?;
        let token_program = account(accounts, 5)?;
        expect_signer(admin_dest)?;
        expect_writable(admin_dest)?;
        expect_writable(market_ai)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        verify_token_program(token_program)?;

        let (cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.admin, admin_dest.key)?;
        if group.mode != MarketModeV16::Resolved {
            return Err(PercolatorError::EngineLockActive.into());
        }
        if group.vault != 0
            || group.insurance != 0
            || group.c_tot != 0
            || group.materialized_portfolio_count != 0
        {
            return Err(PercolatorError::EngineLockActive.into());
        }

        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
        expect_key(vault_authority_ai, &vault_authority)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;

        let vault_account = spl_token::state::Account::unpack(&vault_token.try_borrow_data()?)?;
        let stranded = vault_account.amount;
        if stranded > 0 {
            verify_user_token_account(dest_token, admin_dest.key, &mint)?;
            let bump_arr = [bump];
            let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
            transfer_tokens_signed(
                token_program,
                vault_token,
                dest_token,
                vault_authority_ai,
                stranded,
                signer_seeds,
            )?;
        }

        let bump_arr = [bump];
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
        let close_ix = spl_token::instruction::close_account(
            token_program.key,
            vault_token.key,
            admin_dest.key,
            vault_authority_ai.key,
            &[],
        )?;
        invoke_signed(
            &close_ix,
            &[
                vault_token.clone(),
                admin_dest.clone(),
                vault_authority_ai.clone(),
                token_program.clone(),
            ],
            signer_seeds,
        )?;

        for b in market_ai.try_borrow_mut_data()?.iter_mut() {
            *b = 0;
        }
        let market_lamports = market_ai.lamports();
        **market_ai.lamports.borrow_mut() = 0;
        **admin_dest.lamports.borrow_mut() = admin_dest
            .lamports()
            .checked_add(market_lamports)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        Ok(())
    }

    #[inline(never)]
    fn handle_withdraw_insurance_limited<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        let operator = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let dest_token = account(accounts, 2)?;
        let vault_token = account(accounts, 3)?;
        let vault_authority_ai = account(accounts, 4)?;
        let token_program = account(accounts, 5)?;
        expect_signer(operator)?;
        expect_writable(market_ai)?;
        expect_writable(dest_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        verify_token_program(token_program)?;
        if amount == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }

        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.insurance_operator, operator.key)?;

        match group.mode {
            MarketModeV16::Live => {
                reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
                if group.bankruptcy_hlock_active
                    || group.threshold_stress_active
                    || group.loss_stale_active
                    || group.recovery_reason.is_some()
                {
                    return Err(PercolatorError::EngineLockActive.into());
                }
            }
            MarketModeV16::Resolved | MarketModeV16::Recovery => {
                return Err(PercolatorError::EngineLockActive.into())
            }
        }

        let clock_slot = Clock::get().map(|c| c.slot).unwrap_or(group.current_slot);
        if cfg.insurance_withdraw_max_bps == 0 {
            return Err(PercolatorError::EngineLockActive.into());
        }
        if cfg.last_insurance_withdraw_slot != 0
            && cfg.insurance_withdraw_cooldown_slots != 0
            && clock_slot.saturating_sub(cfg.last_insurance_withdraw_slot)
                < cfg.insurance_withdraw_cooldown_slots
        {
            return Err(PercolatorError::EngineLockActive.into());
        }
        let mut cap = group
            .insurance
            .checked_mul(cfg.insurance_withdraw_max_bps as u128)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?
            / 10_000;
        if cap == 0 && group.insurance >= constants::MIN_INSURANCE_WITHDRAW_FLOOR_UNITS {
            cap = constants::MIN_INSURANCE_WITHDRAW_FLOOR_UNITS;
        }
        if cfg.insurance_withdraw_deposits_only != 0 {
            cap = core::cmp::min(cap, cfg.insurance_withdraw_deposit_remaining);
        }
        if amount > cap || amount > group.insurance || amount > group.vault {
            return Err(PercolatorError::EngineLockActive.into());
        }
        group.insurance -= amount;
        group.vault -= amount;
        if cfg.insurance_withdraw_deposits_only != 0 {
            cfg.insurance_withdraw_deposit_remaining = cfg
                .insurance_withdraw_deposit_remaining
                .checked_sub(amount)
                .ok_or(PercolatorError::EngineCounterUnderflow)?;
        }
        cfg.last_insurance_withdraw_slot = clock_slot;
        group.assert_public_invariants().map_err(map_v16_error)?;

        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
        expect_key(vault_authority_ai, &vault_authority)?;
        verify_user_token_account(dest_token, operator.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;
        let amount_u64 = amount_to_u64(amount)?;
        require_token_balance(vault_token, amount_u64)?;
        let bump_arr = [bump];
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
        transfer_tokens_signed(
            token_program,
            vault_token,
            dest_token,
            vault_authority_ai,
            amount_u64,
            signer_seeds,
        )?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_convert_released_pnl<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        amount: u128,
    ) -> ProgramResult {
        if amount == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        with_one_portfolio(program_id, accounts, true, |group, portfolio, cfg| {
            if group.mode != MarketModeV16::Live {
                return Err(V16Error::LockActive);
            }
            if permissionless_resolve_matured_now(cfg, group) {
                return Err(V16Error::LockActive);
            }
            // The v16 engine converts the currently released residual-bounded
            // amount atomically. Preserve the wrapper caller cap by staging the
            // conversion and only committing it when the converted amount fits.
            let converted = group.convert_released_pnl_to_capital_not_atomic(portfolio)?;
            if converted == 0 || converted > amount {
                return Err(V16Error::LockActive);
            }
            Ok(())
        })
    }

    #[inline(never)]
    fn handle_cure_and_cancel_close<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        optional_deposit: u128,
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        expect_signer(owner)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;

        let amount_u64 = if optional_deposit != 0 {
            let source_token = account(accounts, 3)?;
            let vault_token = account(accounts, 4)?;
            let token_program = account(accounts, 5)?;
            expect_writable(source_token)?;
            expect_writable(vault_token)?;
            verify_token_program(token_program)?;
            let mint = Pubkey::new_from_array(cfg.collateral_mint);
            let (vault_authority, _) = derive_vault_authority(program_id, market_ai.key);
            verify_user_token_account(source_token, owner.key, &mint)?;
            verify_vault_token_account(vault_token, &vault_authority, &mint)?;
            let amount_u64 = amount_to_u64(optional_deposit)?;
            require_token_balance(source_token, amount_u64)?;
            Some((amount_u64, source_token, vault_token, token_program))
        } else {
            None
        };

        let prices = effective_prices_boxed(group.as_ref()).map_err(map_v16_error)?;
        group
            .cure_and_cancel_close_not_atomic(portfolio.as_mut(), optional_deposit, &prices[..])
            .map_err(map_v16_error)?;

        if let Some((amount_u64, source_token, vault_token, token_program)) = amount_u64 {
            transfer_tokens(token_program, source_token, vault_token, owner, amount_u64)?;
        }

        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    #[inline(never)]
    fn handle_forfeit_recovery_leg<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        b_delta_budget: u128,
    ) -> ProgramResult {
        if b_delta_budget == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        with_one_portfolio(program_id, accounts, true, |group, portfolio, _cfg| {
            group
                .forfeit_recovery_leg_not_atomic(portfolio, asset_index as usize, b_delta_budget)
                .map(|_| ())
        })
    }

    #[inline(never)]
    fn handle_rebalance_reduce<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        reduce_q: u128,
    ) -> ProgramResult {
        if reduce_q == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        with_one_portfolio(program_id, accounts, true, |group, portfolio, _cfg| {
            let prices = effective_prices_boxed(group)?;
            group
                .rebalance_reduce_position_not_atomic(
                    portfolio,
                    RebalanceRequestV16 {
                        asset_index: asset_index as usize,
                        reduce_q,
                    },
                    &prices[..],
                )
                .map(|_| ())
        })
    }

    #[inline(never)]
    fn handle_sync_maintenance_fee<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        now_slot: u64,
    ) -> ProgramResult {
        let market_ai = account(accounts, 0)?;
        let portfolio_ai = account(accounts, 1)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode == MarketModeV16::Live {
            reject_permissionless_resolve_matured_live(&cfg, group.as_ref())?;
        }
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        let authenticated_now_slot = authenticated_slot_or_fallback(now_slot);
        let mut cranker_is_same_portfolio = false;
        let mut cranker_portfolio_state = None;
        if let Some(cranker_portfolio_ai) = accounts.get(2) {
            expect_writable(cranker_portfolio_ai)?;
            expect_owner(cranker_portfolio_ai, program_id)?;
            if cranker_portfolio_ai.key == portfolio_ai.key {
                cranker_is_same_portfolio = true;
            } else {
                let cranker_portfolio =
                    state::read_portfolio_boxed(&cranker_portfolio_ai.try_borrow_data()?)?;
                expect_portfolio_account_key(cranker_portfolio.as_ref(), cranker_portfolio_ai.key)?;
                group
                    .validate_account_shape(cranker_portfolio.as_ref())
                    .map_err(map_v16_error)?;
                cranker_portfolio_state = Some((cranker_portfolio_ai, cranker_portfolio));
            }
        }

        let charged = group
            .sync_account_fee_to_slot_not_atomic(
                portfolio.as_mut(),
                authenticated_now_slot,
                cfg.maintenance_fee_per_slot,
            )
            .map_err(map_v16_error)?;
        let cranker_reward = if cranker_is_same_portfolio || cranker_portfolio_state.is_some() {
            charged
                .checked_mul(cfg.maintenance_cranker_fee_share_bps as u128)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?
                / 10_000
        } else {
            0
        };
        if cranker_reward != 0 {
            // SyncMaintenanceFee first uses the engine's loss-senior fee path,
            // which books charged maintenance into insurance. If a Percolator
            // cranker portfolio is supplied, the configured share is reclassed
            // from that newly-collected insurance into cranker capital without
            // touching SPL custody. The unsplit insurance portion is still real
            // even when the cranker portfolio is the same as the fee payer.
            group.insurance = group
                .insurance
                .checked_sub(cranker_reward)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?;
            group.c_tot = group
                .c_tot
                .checked_add(cranker_reward)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?;
            if cranker_is_same_portfolio {
                portfolio.capital = portfolio
                    .capital
                    .checked_add(cranker_reward)
                    .ok_or(PercolatorError::EngineArithmeticOverflow)?;
                portfolio.health_cert.valid = false;
            } else if let Some((_, cranker_portfolio)) = cranker_portfolio_state.as_mut() {
                cranker_portfolio.capital = cranker_portfolio
                    .capital
                    .checked_add(cranker_reward)
                    .ok_or(PercolatorError::EngineArithmeticOverflow)?;
                cranker_portfolio.health_cert.valid = false;
            }
            group.assert_public_invariants().map_err(map_v16_error)?;
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())?;
        if let Some((cranker_portfolio_ai, cranker_portfolio)) = cranker_portfolio_state.as_ref() {
            state::write_portfolio(
                &mut cranker_portfolio_ai.try_borrow_mut_data()?,
                cranker_portfolio.as_ref(),
            )?;
        }
        Ok(())
    }

    #[inline(never)]
    fn handle_resolve_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        expect_live_authority(&cfg.admin, admin.key)?;
        let slot = Clock::get().map(|c| c.slot).unwrap_or(group.current_slot);
        group
            .resolve_market_not_atomic(slot)
            .map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_update_authority<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        kind: u8,
        new_pubkey: [u8; 32],
    ) -> ProgramResult {
        let current = account(accounts, 0)?;
        let new_authority = account(accounts, 1)?;
        let market_ai = account(accounts, 2)?;
        expect_signer(current)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;

        if new_pubkey != [0u8; 32] {
            expect_signer(new_authority)?;
            if new_authority.key.to_bytes() != new_pubkey {
                return Err(PercolatorError::Unauthorized.into());
            }
        }

        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        match kind {
            AUTHORITY_ADMIN => {
                expect_live_authority(&cfg.admin, current.key)?;
                if new_pubkey == [0u8; 32]
                    && (group.mode == MarketModeV16::Live
                        && (cfg.permissionless_resolve_stale_slots == 0
                            || cfg.force_close_delay_slots == 0))
                {
                    return Err(PercolatorError::InvalidInstruction.into());
                }
                cfg.admin = new_pubkey;
            }
            AUTHORITY_HYPERP_MARK => {
                expect_live_authority(&cfg.hyperp_mark_authority, current.key)?;
                cfg.hyperp_mark_authority = new_pubkey;
            }
            AUTHORITY_INSURANCE => {
                expect_live_authority(&cfg.insurance_authority, current.key)?;
                cfg.insurance_authority = new_pubkey;
            }
            AUTHORITY_BACKING_BUCKET => {
                expect_live_authority(&cfg.backing_bucket_authority, current.key)?;
                cfg.backing_bucket_authority = new_pubkey;
            }
            AUTHORITY_ASSET => {
                expect_live_authority(&cfg.asset_authority, current.key)?;
                cfg.asset_authority = new_pubkey;
            }
            AUTHORITY_INSURANCE_OPERATOR => {
                expect_live_authority(&cfg.insurance_operator, current.key)?;
                cfg.insurance_operator = new_pubkey;
            }
            _ => return Err(PercolatorError::InvalidInstruction.into()),
        }

        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_update_asset_lifecycle<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        action: u8,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
    ) -> ProgramResult {
        let authority = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(authority)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;

        let asset_index = asset_index as usize;
        if asset_index >= V16_MAX_MARKET_SLOTS_N {
            // Header-only append path for dynamic slots past the fixed runtime
            // window. Non-append lifecycle changes still need engine dynamic
            // drain/retire APIs so the wrapper does not reimplement risk-state
            // emptiness rules.
            let (cfg, mode, max_market_slots, capacity) =
                state::read_market_config_mode_and_capacity(&market_ai.try_borrow_data()?)?;
            expect_live_authority(&cfg.asset_authority, authority.key)?;
            if mode != MarketModeV16::Live {
                return Err(PercolatorError::EngineLockActive.into());
            }
            if action != ASSET_ACTION_ACTIVATE || asset_index != max_market_slots {
                return Err(PercolatorError::InvalidInstruction.into());
            }
            if asset_index >= capacity {
                let new_len = state::market_account_len_for_capacity(asset_index + 1)?;
                market_ai.realloc(new_len, true)?;
            }
            let mut data = market_ai.try_borrow_mut_data()?;
            let profile = state::activate_dynamic_asset_slot(
                &mut data,
                asset_index,
                now_slot,
                initial_price,
            )?;
            state::write_asset_oracle_profile(&mut data, asset_index, &profile)?;
            return Ok(());
        }

        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.asset_authority, authority.key)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }

        let mut reset_profile = None;
        match action {
            ASSET_ACTION_ACTIVATE => {
                if asset_index >= group.config.max_market_slots as usize {
                    grow_configured_asset_capacity(group.as_mut(), asset_index)?;
                }
                group
                    .activate_empty_asset_not_atomic(asset_index, initial_price, now_slot)
                    .map_err(map_v16_error)?;
                let profile = state::manual_asset_oracle_profile(initial_price, now_slot);
                if asset_index == 0 {
                    mirror_manual_profile_to_base_config(&mut cfg, &profile, true);
                }
                reset_profile = Some(profile);
            }
            ASSET_ACTION_DRAIN_ONLY => {
                if now_slot != 0 || initial_price != 0 {
                    return Err(PercolatorError::InvalidInstruction.into());
                }
                group
                    .mark_asset_drain_only_not_atomic(asset_index)
                    .map_err(map_v16_error)?;
            }
            ASSET_ACTION_RETIRE => {
                if now_slot == 0 || initial_price != 0 {
                    return Err(PercolatorError::InvalidInstruction.into());
                }
                group
                    .retire_empty_asset_not_atomic(asset_index, now_slot)
                    .map_err(map_v16_error)?;
                let price = group.assets[asset_index].effective_price;
                let profile = state::manual_asset_oracle_profile(price, now_slot);
                if asset_index == 0 {
                    mirror_manual_profile_to_base_config(&mut cfg, &profile, false);
                }
                reset_profile = Some(profile);
            }
            _ => return Err(PercolatorError::InvalidInstruction.into()),
        }

        let required_capacity = group.config.max_market_slots as usize;
        let current_capacity = state::market_slot_capacity(&market_ai.try_borrow_data()?)?;
        if required_capacity > current_capacity {
            let new_len = state::market_account_len_for_capacity(required_capacity)?;
            market_ai.realloc(new_len, true)?;
        }
        let mut data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut data, &cfg, group.as_ref())?;
        if let Some(profile) = reset_profile {
            state::write_asset_oracle_profile(&mut data, asset_index, &profile)?;
        }
        Ok(())
    }

    #[inline(never)]
    fn handle_finalize_reset_side<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        side: u8,
    ) -> ProgramResult {
        let market_ai = account(accounts, 0)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        let side = decode_side(side)?;
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        group
            .finalize_ready_reset_side(asset_index as usize, side)
            .map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_refine_resolved_unreceipted_bound<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        decrease_num: u128,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if decrease_num == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.admin, admin.key)?;
        group
            .refine_resolved_unreceipted_bound_not_atomic(decrease_num)
            .map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_update_insurance_policy<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        max_bps: u16,
        deposits_only: u8,
        cooldown_slots: u64,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if !state::insurance_withdraw_policy_shape_ok(max_bps, deposits_only, cooldown_slots) {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.admin, admin.key)?;
        cfg.insurance_withdraw_max_bps = max_bps;
        cfg.insurance_withdraw_deposits_only = deposits_only;
        cfg.insurance_withdraw_cooldown_slots = cooldown_slots;
        if deposits_only == 0 {
            cfg.insurance_withdraw_deposit_remaining = 0;
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_update_liquidation_fee_policy<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        cranker_share_bps: u16,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if cranker_share_bps > 10_000 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.admin, admin.key)?;
        cfg.liquidation_cranker_fee_share_bps = cranker_share_bps;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_update_maintenance_fee_policy<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        cranker_share_bps: u16,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if cranker_share_bps > 10_000 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.admin, admin.key)?;
        cfg.maintenance_cranker_fee_share_bps = cranker_share_bps;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_update_backing_fee_policy<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        fee_bps: u16,
    ) -> ProgramResult {
        let authority = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(authority)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.backing_bucket_authority, authority.key)?;
        if fee_bps > 10_000 || fee_bps as u64 > group.config.max_trading_fee_bps {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        cfg.backing_trade_fee_bps = fee_bps;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_configure_permissionless_resolve<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        stale_slots: u64,
        force_close_delay_slots: u64,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if stale_slots == 0
            || stale_slots > constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS
            || force_close_delay_slots == 0
            || force_close_delay_slots > constants::MAX_FORCE_CLOSE_DELAY_SLOTS
        {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        expect_live_authority(&cfg.admin, admin.key)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        cfg.permissionless_resolve_stale_slots = stale_slots;
        cfg.force_close_delay_slots = force_close_delay_slots;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_resolve_stale_permissionless<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        now_slot: u64,
    ) -> ProgramResult {
        let market_ai = account(accounts, 0)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        let authenticated_slot = authenticated_slot_or_fallback(now_slot);
        if authenticated_slot < group.current_slot {
            return Err(PercolatorError::EngineStale.into());
        }
        if !oracle_v16::permissionless_stale_matured(&cfg, authenticated_slot) {
            return Err(PercolatorError::OracleStale.into());
        }
        group
            .resolve_market_not_atomic(authenticated_slot)
            .map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn handle_configure_hybrid_oracle<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        now_slot: u64,
        now_unix_ts: i64,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        max_staleness_secs: u64,
        hybrid_soft_stale_slots: u64,
        mark_ewma_halflife_slots: u64,
        mark_min_fee: u64,
        invert: u8,
        unit_scale: u32,
        conf_filter_bps: u16,
        oracle_leg_feeds: [[u8; 32]; constants::ORACLE_LEG_CAP],
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if oracle_leg_count == 0
            || oracle_leg_count as usize > constants::ORACLE_LEG_CAP
            || !oracle_v16::oracle_leg_config_ok(
                oracle_leg_count,
                oracle_leg_flags,
                &oracle_leg_feeds,
            )
            || max_staleness_secs == 0
            || hybrid_soft_stale_slots == 0
            || invert > 1
        {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let oracle_accounts = accounts
            .get(2..2 + oracle_leg_count as usize)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let asset_index_usize = asset_index as usize;
        expect_live_authority(&cfg.admin, admin.key)?;
        if asset_index_usize >= group.config.max_market_slots as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let authenticated_slot = authenticated_slot_or_fallback(now_slot);
        let authenticated_unix_ts = Clock::get()
            .map(|c| c.unix_timestamp)
            .unwrap_or(now_unix_ts);
        if authenticated_slot < group.current_slot {
            return Err(PercolatorError::EngineStale.into());
        }
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        require_asset_active_for_oracle_reconfiguration(group.as_ref(), asset_index_usize)?;
        let group_had_position_or_loss_state = group_has_position_or_loss_state(group.as_ref());

        let mut profile = state::AssetOracleProfileV16 {
            oracle_mode: constants::ORACLE_MODE_HYBRID_AFTER_HOURS,
            oracle_leg_count,
            oracle_leg_flags,
            invert,
            unit_scale,
            conf_filter_bps,
            _padding0: [0u8; 6],
            max_staleness_secs,
            hybrid_soft_stale_slots,
            mark_ewma_e6: 0,
            mark_ewma_last_slot: 0,
            mark_ewma_halflife_slots,
            mark_min_fee,
            oracle_target_price_e6: 0,
            oracle_target_publish_time: 0,
            last_good_oracle_slot: 0,
            oracle_leg_feeds,
            oracle_leg_prices_e6: [0u64; constants::ORACLE_LEG_CAP],
            oracle_leg_publish_times: [0i64; constants::ORACLE_LEG_CAP],
        };

        let (price, publish_time, advanced) = oracle_v16::read_external_price_e6_profile(
            &mut profile,
            oracle_accounts,
            authenticated_unix_ts,
        )?;
        if !advanced || price == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        profile.last_good_oracle_slot = authenticated_slot;
        profile.oracle_target_price_e6 = price;
        profile.oracle_target_publish_time = publish_time;
        profile.mark_ewma_e6 = price;
        profile.mark_ewma_last_slot = authenticated_slot;
        group.assets[asset_index_usize].raw_oracle_target_price = price;
        group.assets[asset_index_usize].effective_price = price;
        group.assets[asset_index_usize].fund_px_last = price;
        group.assets[asset_index_usize].slot_last = authenticated_slot;
        cfg.last_good_oracle_slot = core::cmp::max(cfg.last_good_oracle_slot, authenticated_slot);
        if asset_index_usize == 0 {
            cfg.oracle_mode = profile.oracle_mode;
            cfg.oracle_leg_count = profile.oracle_leg_count;
            cfg.oracle_leg_flags = profile.oracle_leg_flags;
            cfg.invert = profile.invert;
            cfg.unit_scale = profile.unit_scale;
            cfg.conf_filter_bps = profile.conf_filter_bps;
            cfg.max_staleness_secs = profile.max_staleness_secs;
            cfg.hybrid_soft_stale_slots = profile.hybrid_soft_stale_slots;
            cfg.mark_ewma_halflife_slots = profile.mark_ewma_halflife_slots;
            cfg.mark_min_fee = profile.mark_min_fee;
            cfg.oracle_leg_feeds = profile.oracle_leg_feeds;
            cfg.oracle_leg_prices_e6 = profile.oracle_leg_prices_e6;
            cfg.oracle_leg_publish_times = profile.oracle_leg_publish_times;
            cfg.oracle_target_price_e6 = profile.oracle_target_price_e6;
            cfg.oracle_target_publish_time = profile.oracle_target_publish_time;
            cfg.mark_ewma_e6 = profile.mark_ewma_e6;
            cfg.mark_ewma_last_slot = profile.mark_ewma_last_slot;
        }
        group.current_slot = authenticated_slot;
        // `slot_last` is the group-wide loss-safe fee anchor; do not make
        // unrelated exposed assets fee-current while configuring one empty slot.
        if !group_had_position_or_loss_state {
            group.slot_last = authenticated_slot;
        }
        group.assert_public_invariants().map_err(map_v16_error)?;
        let mut data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut data, &cfg, group.as_ref())?;
        state::write_asset_oracle_profile(&mut data, asset_index_usize, &profile)
    }

    #[inline(never)]
    fn handle_configure_hyperp_mark<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        now_slot: u64,
        initial_mark_e6: u64,
        mark_ewma_halflife_slots: u64,
        mark_min_fee: u64,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if initial_mark_e6 == 0
            || initial_mark_e6 > percolator::MAX_ORACLE_PRICE
            || mark_ewma_halflife_slots == 0
        {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let asset_index_usize = asset_index as usize;
        expect_live_authority(&cfg.admin, admin.key)?;
        if asset_index_usize >= group.config.max_market_slots as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let authenticated_slot = authenticated_slot_or_fallback(now_slot);
        if authenticated_slot < group.current_slot {
            return Err(PercolatorError::EngineStale.into());
        }
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        require_asset_active_for_oracle_reconfiguration(group.as_ref(), asset_index_usize)?;
        let group_had_position_or_loss_state = group_has_position_or_loss_state(group.as_ref());

        let profile = state::AssetOracleProfileV16 {
            oracle_mode: constants::ORACLE_MODE_HYPERP,
            oracle_leg_count: 0,
            oracle_leg_flags: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 0,
            _padding0: [0u8; 6],
            max_staleness_secs: 0,
            hybrid_soft_stale_slots: 0,
            mark_ewma_e6: initial_mark_e6,
            mark_ewma_last_slot: authenticated_slot,
            mark_ewma_halflife_slots,
            mark_min_fee,
            oracle_target_price_e6: initial_mark_e6,
            oracle_target_publish_time: 0,
            last_good_oracle_slot: authenticated_slot,
            oracle_leg_feeds: [[0u8; 32]; constants::ORACLE_LEG_CAP],
            oracle_leg_prices_e6: [0u64; constants::ORACLE_LEG_CAP],
            oracle_leg_publish_times: [0i64; constants::ORACLE_LEG_CAP],
        };

        group.assets[asset_index_usize].raw_oracle_target_price = initial_mark_e6;
        group.assets[asset_index_usize].effective_price = initial_mark_e6;
        group.assets[asset_index_usize].fund_px_last = initial_mark_e6;
        group.assets[asset_index_usize].slot_last = authenticated_slot;
        cfg.last_good_oracle_slot = core::cmp::max(cfg.last_good_oracle_slot, authenticated_slot);
        if asset_index_usize == 0 {
            cfg.oracle_mode = profile.oracle_mode;
            cfg.oracle_leg_count = 0;
            cfg.oracle_leg_flags = 0;
            cfg.invert = 0;
            cfg.unit_scale = 0;
            cfg.conf_filter_bps = 0;
            cfg.max_staleness_secs = 0;
            cfg.hybrid_soft_stale_slots = 0;
            cfg.mark_ewma_e6 = profile.mark_ewma_e6;
            cfg.mark_ewma_last_slot = profile.mark_ewma_last_slot;
            cfg.mark_ewma_halflife_slots = profile.mark_ewma_halflife_slots;
            cfg.mark_min_fee = profile.mark_min_fee;
            cfg.oracle_target_price_e6 = profile.oracle_target_price_e6;
            cfg.oracle_target_publish_time = 0;
            cfg.oracle_leg_feeds = [[0u8; 32]; constants::ORACLE_LEG_CAP];
            cfg.oracle_leg_prices_e6 = [0u64; constants::ORACLE_LEG_CAP];
            cfg.oracle_leg_publish_times = [0i64; constants::ORACLE_LEG_CAP];
        }
        group.current_slot = authenticated_slot;
        // `slot_last` is the group-wide loss-safe fee anchor; do not make
        // unrelated exposed assets fee-current while configuring one empty slot.
        if !group_had_position_or_loss_state {
            group.slot_last = authenticated_slot;
        }
        group.assert_public_invariants().map_err(map_v16_error)?;
        let mut data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut data, &cfg, group.as_ref())?;
        state::write_asset_oracle_profile(&mut data, asset_index_usize, &profile)
    }

    #[inline(never)]
    fn handle_push_hyperp_mark<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u16,
        now_slot: u64,
        mark_e6: u64,
    ) -> ProgramResult {
        let authority = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(authority)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        if mark_e6 == 0 || mark_e6 > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let (mut cfg, group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let asset_index_usize = asset_index as usize;
        if asset_index_usize >= group.config.max_market_slots as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let mut profile =
            state::read_asset_oracle_profile(&market_ai.try_borrow_data()?, asset_index_usize)?;
        if group.mode != MarketModeV16::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        require_asset_mark_pushable(group.as_ref(), asset_index_usize)?;
        if !oracle_v16::profile_is_hyperp(&profile) {
            return Err(PercolatorError::Unauthorized.into());
        }
        expect_live_authority(&cfg.hyperp_mark_authority, authority.key)?;
        let authenticated_slot = authenticated_slot_or_fallback(now_slot);
        if authenticated_slot < profile.mark_ewma_last_slot
            || authenticated_slot < group.current_slot
        {
            return Err(PercolatorError::EngineStale.into());
        }
        let full_weight_fee = if profile.mark_min_fee == 0 {
            0
        } else {
            profile.mark_min_fee
        };
        let next_mark = policy_v16::ewma_update(
            profile.mark_ewma_e6,
            mark_e6,
            profile.mark_ewma_halflife_slots,
            profile.mark_ewma_last_slot,
            authenticated_slot,
            full_weight_fee,
            profile.mark_min_fee,
        );
        if next_mark == 0 || next_mark > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::OracleInvalid.into());
        }
        profile.mark_ewma_e6 = next_mark;
        profile.mark_ewma_last_slot = authenticated_slot;
        profile.oracle_target_price_e6 = next_mark;
        profile.oracle_target_publish_time = 0;
        profile.last_good_oracle_slot = authenticated_slot;
        cfg.last_good_oracle_slot = core::cmp::max(cfg.last_good_oracle_slot, authenticated_slot);
        if asset_index_usize == 0 {
            cfg.mark_ewma_e6 = profile.mark_ewma_e6;
            cfg.mark_ewma_last_slot = profile.mark_ewma_last_slot;
            cfg.oracle_target_price_e6 = profile.oracle_target_price_e6;
            cfg.oracle_target_publish_time = 0;
        }
        let mut data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut data, &cfg, group.as_ref())?;
        state::write_asset_oracle_profile(&mut data, asset_index_usize, &profile)
    }

    #[inline(never)]
    fn handle_close_resolved<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        _fee_rate_per_slot: u128,
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;
        if cfg.force_close_delay_slots != 0
            && authenticated_market_slot_or_fallback(group.as_ref())
                .saturating_sub(group.resolved_slot)
                < cfg.force_close_delay_slots
        {
            expect_signer(owner)?;
        }

        let outcome = group
            .close_resolved_account_not_atomic(portfolio.as_mut(), cfg.maintenance_fee_per_slot)
            .map_err(map_v16_error)?;
        if let ResolvedCloseOutcomeV16::Closed { payout } = outcome {
            if payout != 0 {
                let dest_token = account(accounts, 3)?;
                let vault_token = account(accounts, 4)?;
                let vault_authority_ai = account(accounts, 5)?;
                let token_program = account(accounts, 6)?;
                expect_writable(dest_token)?;
                expect_writable(vault_token)?;
                verify_token_program(token_program)?;
                let mint = Pubkey::new_from_array(cfg.collateral_mint);
                let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
                expect_key(vault_authority_ai, &vault_authority)?;
                verify_user_token_account(dest_token, owner.key, &mint)?;
                verify_vault_token_account(vault_token, &vault_authority, &mint)?;
                let payout_u64 = amount_to_u64(payout)?;
                require_token_balance(vault_token, payout_u64)?;
                let bump_arr = [bump];
                let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
                transfer_tokens_signed(
                    token_program,
                    vault_token,
                    dest_token,
                    vault_authority_ai,
                    payout_u64,
                    signer_seeds,
                )?;
            }
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    #[inline(never)]
    fn handle_claim_resolved_payout_topup<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;

        let payout = group
            .claim_resolved_payout_topup_not_atomic(portfolio.as_mut())
            .map_err(map_v16_error)?;
        if payout != 0 {
            let dest_token = account(accounts, 3)?;
            let vault_token = account(accounts, 4)?;
            let vault_authority_ai = account(accounts, 5)?;
            let token_program = account(accounts, 6)?;
            expect_writable(dest_token)?;
            expect_writable(vault_token)?;
            verify_token_program(token_program)?;
            let mint = Pubkey::new_from_array(cfg.collateral_mint);
            let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
            expect_key(vault_authority_ai, &vault_authority)?;
            verify_user_token_account(dest_token, owner.key, &mint)?;
            verify_vault_token_account(vault_token, &vault_authority, &mint)?;
            let payout_u64 = amount_to_u64(payout)?;
            require_token_balance(vault_token, payout_u64)?;
            let bump_arr = [bump];
            let signer_seeds: &[&[&[u8]]] = &[&[b"vault", market_ai.key.as_ref(), &bump_arr]];
            transfer_tokens_signed(
                token_program,
                vault_token,
                dest_token,
                vault_authority_ai,
                payout_u64,
                signer_seeds,
            )?;
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn handle_permissionless_crank<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        action: u8,
        asset_index: u16,
        now_slot: u64,
        effective_price: u64,
        funding_rate_e9: i128,
        close_q: u128,
        fee_bps: u64,
        recovery_reason: u8,
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        let (mut cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        let authenticated_now_slot = authenticated_slot_or_fallback(now_slot);
        let asset_index_usize = asset_index as usize;
        if asset_index_usize >= group.config.max_market_slots as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let mut oracle_profile =
            read_oracle_profile_for_asset(&market_ai.try_borrow_data()?, &cfg, asset_index_usize)?;
        let now_unix_ts = Clock::get().map(|c| c.unix_timestamp).unwrap_or_else(|_| {
            let elapsed_slots =
                authenticated_now_slot.saturating_sub(oracle_profile.last_good_oracle_slot);
            oracle_profile
                .oracle_target_publish_time
                .saturating_add(i64::try_from(elapsed_slots).unwrap_or(i64::MAX))
        });
        let reward_enabled = action == 1 && cfg.liquidation_cranker_fee_share_bps != 0;
        let tail = accounts.get(3..).unwrap_or(&[]);
        let mut oracle_tail = tail;
        let mut cranker_portfolio_state = None;
        if reward_enabled {
            if let Some((last, rest)) = tail.split_last() {
                if last.owner == program_id {
                    expect_signer(owner)?;
                    expect_writable(last)?;
                    if last.key == portfolio_ai.key {
                        return Err(PercolatorError::InvalidInstruction.into());
                    }
                    let cranker_portfolio = state::read_portfolio_boxed(&last.try_borrow_data()?)?;
                    expect_portfolio_account_key(cranker_portfolio.as_ref(), last.key)?;
                    expect_portfolio_owner(cranker_portfolio.as_ref(), owner.key)?;
                    group
                        .validate_account_shape(cranker_portfolio.as_ref())
                        .map_err(map_v16_error)?;
                    cranker_portfolio_state = Some((last, cranker_portfolio));
                    oracle_tail = rest;
                }
            }
        }
        if funding_rate_e9 != 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        if action == 1 && fee_bps != 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        reject_permissionless_resolve_matured_live_for_profile(
            &cfg,
            &oracle_profile,
            group.as_ref(),
        )?;
        let crank_price = hybrid_effective_price_for_crank(
            &cfg,
            &mut oracle_profile,
            group.as_ref(),
            asset_index_usize,
            authenticated_now_slot,
            now_unix_ts,
            oracle_tail,
            effective_price,
        )?;
        group.assets[asset_index as usize].raw_oracle_target_price =
            if oracle_v16::profile_is_price_managed(&oracle_profile) {
                oracle_profile.oracle_target_price_e6
            } else {
                crank_price
            };
        cfg.last_good_oracle_slot = core::cmp::max(
            cfg.last_good_oracle_slot,
            oracle_profile.last_good_oracle_slot,
        );
        if asset_index_usize == 0 && oracle_v16::profile_is_price_managed(&oracle_profile) {
            cfg.oracle_mode = oracle_profile.oracle_mode;
            cfg.oracle_leg_count = oracle_profile.oracle_leg_count;
            cfg.oracle_leg_flags = oracle_profile.oracle_leg_flags;
            cfg.invert = oracle_profile.invert;
            cfg.unit_scale = oracle_profile.unit_scale;
            cfg.conf_filter_bps = oracle_profile.conf_filter_bps;
            cfg.max_staleness_secs = oracle_profile.max_staleness_secs;
            cfg.hybrid_soft_stale_slots = oracle_profile.hybrid_soft_stale_slots;
            cfg.mark_ewma_e6 = oracle_profile.mark_ewma_e6;
            cfg.mark_ewma_last_slot = oracle_profile.mark_ewma_last_slot;
            cfg.mark_ewma_halflife_slots = oracle_profile.mark_ewma_halflife_slots;
            cfg.mark_min_fee = oracle_profile.mark_min_fee;
            cfg.oracle_target_price_e6 = oracle_profile.oracle_target_price_e6;
            cfg.oracle_target_publish_time = oracle_profile.oracle_target_publish_time;
            cfg.oracle_leg_feeds = oracle_profile.oracle_leg_feeds;
            cfg.oracle_leg_prices_e6 = oracle_profile.oracle_leg_prices_e6;
            cfg.oracle_leg_publish_times = oracle_profile.oracle_leg_publish_times;
        }
        if action == 1 && cfg.liquidation_cranker_fee_share_bps != 0 {
            let insurance_before = group.insurance;
            let prices =
                effective_prices_with_boxed(group.as_ref(), asset_index as usize, crank_price)
                    .map_err(map_v16_error)?;
            let outcome = group
                .liquidate_account_not_atomic(
                    portfolio.as_mut(),
                    percolator::LiquidationRequestV16 {
                        asset_index: asset_index as usize,
                        close_q,
                        fee_bps: group.config.liquidation_fee_bps,
                    },
                    &prices[..],
                )
                .map_err(map_v16_error)?;
            group
                .accrue_asset_to_not_atomic(
                    asset_index as usize,
                    authenticated_now_slot,
                    crank_price,
                    funding_rate_e9,
                    true,
                )
                .map_err(map_v16_error)?;
            let retained_fee = core::cmp::min(
                outcome.fee_charged,
                group.insurance.saturating_sub(insurance_before),
            );
            let reward = retained_fee
                .checked_mul(cfg.liquidation_cranker_fee_share_bps as u128)
                .ok_or(PercolatorError::EngineArithmeticOverflow)?
                / 10_000;
            let reward = core::cmp::min(reward, retained_fee);
            if let Some((_, cranker_portfolio)) =
                cranker_portfolio_state.as_mut().filter(|_| reward != 0)
            {
                // The cranker reward is paid inside the Percolator ledger, not
                // by withdrawing SPL tokens from custody. The engine first
                // books the liquidation penalty into insurance; the wrapper
                // then reclassifies the configured retained-fee share from
                // insurance into the signed cranker portfolio's capital.
                group.insurance = group
                    .insurance
                    .checked_sub(reward)
                    .ok_or(PercolatorError::EngineArithmeticOverflow)?;
                group.c_tot = group
                    .c_tot
                    .checked_add(reward)
                    .ok_or(PercolatorError::EngineArithmeticOverflow)?;
                cranker_portfolio.capital = cranker_portfolio
                    .capital
                    .checked_add(reward)
                    .ok_or(PercolatorError::EngineArithmeticOverflow)?;
                cranker_portfolio.health_cert.valid = false;
                group.assert_public_invariants().map_err(map_v16_error)?;
            }
        } else {
            if action == 3 {
                // Recovery is terminal at the engine layer. Public callers may
                // drive normal crank, liquidation, and settlement progress, but
                // they may not select a recovery reason and lock the market.
                // Engine-internal recovery declarations remain available for
                // exceptional states discovered by engine-owned progress code.
                return Err(PercolatorError::InvalidInstruction.into());
            } else if recovery_reason != 0 {
                return Err(PercolatorError::InvalidInstruction.into());
            }
            crank_one_portfolio(
                group.as_mut(),
                portfolio.as_mut(),
                action,
                asset_index,
                authenticated_now_slot,
                crank_price,
                funding_rate_e9,
                close_q,
                0,
                recovery_reason,
            )
            .map_err(map_v16_error)?;
        }
        let mut market_data = market_ai.try_borrow_mut_data()?;
        state::write_market(&mut market_data, &cfg, group.as_ref())?;
        write_oracle_profile_if_separate(&mut market_data, asset_index_usize, &oracle_profile)?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())?;
        if let Some((cranker_portfolio_ai, cranker_portfolio)) = cranker_portfolio_state.as_ref() {
            state::write_portfolio(
                &mut cranker_portfolio_ai.try_borrow_mut_data()?,
                cranker_portfolio.as_ref(),
            )?;
        }
        let _ = owner;
        Ok(())
    }

    #[inline(never)]
    fn crank_one_portfolio(
        group: &mut MarketGroupV16,
        portfolio: &mut PortfolioAccountV16,
        action: u8,
        asset_index: u16,
        now_slot: u64,
        effective_price: u64,
        funding_rate_e9: i128,
        close_q: u128,
        _fee_bps: u64,
        _recovery_reason: u8,
    ) -> Result<(), V16Error> {
        let action = match action {
            0 => PermissionlessCrankActionV16::Refresh,
            1 => PermissionlessCrankActionV16::Liquidate(percolator::LiquidationRequestV16 {
                asset_index: asset_index as usize,
                close_q,
                fee_bps: group.config.liquidation_fee_bps,
            }),
            2 => PermissionlessCrankActionV16::SettleB {
                asset_index: asset_index as usize,
            },
            3 => PermissionlessCrankActionV16::Recover(
                decode_recovery_reason(_recovery_reason).map_err(|_| V16Error::InvalidConfig)?,
            ),
            _ => return Err(V16Error::InvalidConfig),
        };
        let prices = effective_prices_with_boxed(group, asset_index as usize, effective_price)?;
        group
            .permissionless_crank_not_atomic(
                portfolio,
                PermissionlessCrankRequestV16 {
                    now_slot,
                    asset_index: asset_index as usize,
                    effective_price,
                    funding_rate_e9,
                    action,
                },
                &prices[..],
            )
            .map(|_| ())
    }

    #[allow(unsafe_code)]
    #[inline(never)]
    fn alloc_raw<T>() -> Result<*mut T, ProgramError> {
        let layout = core::alloc::Layout::new::<T>();
        let raw = unsafe { alloc::alloc::alloc(layout) as *mut T };
        if raw.is_null() {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(raw)
    }

    #[allow(unsafe_code)]
    #[inline(never)]
    fn new_market_group_boxed(
        market_group_id: [u8; 32],
        config: V16Config,
    ) -> Result<alloc::boxed::Box<MarketGroupV16>, ProgramError> {
        // Keep InitMarket SBF-safe by writing the large market object directly
        // to heap memory. This mirrors `MarketGroupV16::new` field-for-field,
        // then validates through the engine's public invariant checker before
        // the bytes are persisted.
        config.validate_public_user_fund().map_err(map_v16_error)?;
        let raw = alloc_raw::<MarketGroupV16>()?;
        unsafe {
            core::ptr::addr_of_mut!((*raw).market_group_id).write(market_group_id);
            core::ptr::addr_of_mut!((*raw).config).write(config);
            core::ptr::addr_of_mut!((*raw).vault).write(0);
            core::ptr::addr_of_mut!((*raw).insurance).write(0);
            core::ptr::addr_of_mut!((*raw).c_tot).write(0);
            core::ptr::addr_of_mut!((*raw).pnl_pos_tot).write(0);
            core::ptr::addr_of_mut!((*raw).pnl_pos_bound_tot_num).write(0);
            core::ptr::addr_of_mut!((*raw).pnl_pos_bound_tot).write(0);
            core::ptr::addr_of_mut!((*raw).pnl_matured_pos_tot).write(0);
            let insurance_domain_budget =
                core::ptr::addr_of_mut!((*raw).insurance_domain_budget) as *mut u128;
            let insurance_domain_spent =
                core::ptr::addr_of_mut!((*raw).insurance_domain_spent) as *mut u128;
            let pending_domain_loss_barriers =
                core::ptr::addr_of_mut!((*raw).pending_domain_loss_barriers) as *mut u64;
            let source_credit =
                core::ptr::addr_of_mut!((*raw).source_credit) as *mut SourceCreditStateV16;
            let source_backing_buckets =
                core::ptr::addr_of_mut!((*raw).source_backing_buckets) as *mut BackingBucketV16;
            let insurance_credit_reservations =
                core::ptr::addr_of_mut!((*raw).insurance_credit_reservations)
                    as *mut InsuranceCreditReservationV16;
            let mut d = 0;
            while d < V16_DOMAIN_COUNT {
                insurance_domain_budget
                    .add(d)
                    .write(percolator::MAX_VAULT_TVL);
                insurance_domain_spent.add(d).write(0);
                pending_domain_loss_barriers.add(d).write(0);
                source_credit.add(d).write(SourceCreditStateV16::EMPTY);
                source_backing_buckets.add(d).write(BackingBucketV16::EMPTY);
                insurance_credit_reservations
                    .add(d)
                    .write(InsuranceCreditReservationV16::EMPTY);
                d += 1;
            }
            core::ptr::addr_of_mut!((*raw).materialized_portfolio_count).write(0);
            core::ptr::addr_of_mut!((*raw).stale_certificate_count).write(0);
            core::ptr::addr_of_mut!((*raw).b_stale_account_count).write(0);
            core::ptr::addr_of_mut!((*raw).negative_pnl_account_count).write(0);
            core::ptr::addr_of_mut!((*raw).risk_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).asset_set_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).asset_activation_count).write(0);
            core::ptr::addr_of_mut!((*raw).last_asset_activation_slot).write(0);
            core::ptr::addr_of_mut!((*raw).next_market_id)
                .write(config.max_market_slots as u64 + 1);
            core::ptr::addr_of_mut!((*raw).oracle_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).funding_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).slot_last).write(0);
            core::ptr::addr_of_mut!((*raw).current_slot).write(0);
            let assets = core::ptr::addr_of_mut!((*raw).assets) as *mut AssetStateV16;
            let mut i = 0;
            while i < V16_MAX_MARKET_SLOTS_N {
                let mut asset = AssetStateV16::default();
                if i < config.max_market_slots as usize {
                    asset.market_id = i as u64 + 1;
                    let long_domain = i * 2;
                    let short_domain = long_domain + 1;
                    source_backing_buckets
                        .add(long_domain)
                        .write(BackingBucketV16::empty_for_market(asset.market_id));
                    source_backing_buckets
                        .add(short_domain)
                        .write(BackingBucketV16::empty_for_market(asset.market_id));
                } else {
                    asset.lifecycle = AssetLifecycleV16::Disabled;
                    asset.market_id = 0;
                }
                assets.add(i).write(asset);
                i += 1;
            }
            core::ptr::addr_of_mut!((*raw).bankruptcy_hlock_active).write(false);
            core::ptr::addr_of_mut!((*raw).threshold_stress_active).write(false);
            core::ptr::addr_of_mut!((*raw).loss_stale_active).write(false);
            core::ptr::addr_of_mut!((*raw).recovery_reason).write(None);
            core::ptr::addr_of_mut!((*raw).mode).write(MarketModeV16::Live);
            core::ptr::addr_of_mut!((*raw).resolved_slot).write(0);
            core::ptr::addr_of_mut!((*raw).payout_snapshot).write(0);
            core::ptr::addr_of_mut!((*raw).payout_snapshot_pnl_pos_tot).write(0);
            core::ptr::addr_of_mut!((*raw).payout_snapshot_captured).write(false);
            core::ptr::addr_of_mut!((*raw).resolved_payout_ledger)
                .write(ResolvedPayoutLedgerV16::EMPTY);
            let group = alloc::boxed::Box::from_raw(raw);
            group.assert_public_invariants().map_err(map_v16_error)?;
            Ok(group)
        }
    }

    fn disabled_empty_asset() -> AssetStateV16 {
        let mut asset = AssetStateV16::default();
        asset.lifecycle = AssetLifecycleV16::Disabled;
        asset
    }

    fn grow_configured_asset_capacity(
        group: &mut MarketGroupV16,
        asset_index: usize,
    ) -> ProgramResult {
        if asset_index >= V16_MAX_MARKET_SLOTS_N {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let old_n = group.config.max_market_slots as usize;
        let new_n = asset_index
            .checked_add(1)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        if new_n <= old_n {
            return Ok(());
        }
        if new_n != old_n + 1 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let mut i = old_n;
        while i < new_n {
            group.assets[i] = disabled_empty_asset();
            i += 1;
        }
        group.config.max_market_slots =
            u32::try_from(new_n).map_err(|_| PercolatorError::InvalidInstruction)?;
        group
            .config
            .validate_public_user_fund()
            .map_err(map_v16_error)?;
        Ok(())
    }

    #[allow(unsafe_code)]
    #[inline(never)]
    fn new_portfolio_boxed(
        header: ProvenanceHeaderV16,
        last_fee_slot: u64,
    ) -> Result<alloc::boxed::Box<PortfolioAccountV16>, ProgramError> {
        // Same pattern as market init: avoid a multi-KB stack temporary in the
        // SBF entrypoint while preserving the engine's canonical empty shape.
        let raw = alloc_raw::<PortfolioAccountV16>()?;
        unsafe {
            core::ptr::addr_of_mut!((*raw).provenance_header).write(header);
            core::ptr::addr_of_mut!((*raw).owner).write(header.owner);
            core::ptr::addr_of_mut!((*raw).capital).write(0);
            core::ptr::addr_of_mut!((*raw).pnl).write(0);
            core::ptr::addr_of_mut!((*raw).reserved_pnl).write(0);
            let source_claim_market_id =
                core::ptr::addr_of_mut!((*raw).source_claim_market_id) as *mut u64;
            let source_claim_bound_num =
                core::ptr::addr_of_mut!((*raw).source_claim_bound_num) as *mut u128;
            let source_claim_liened_num =
                core::ptr::addr_of_mut!((*raw).source_claim_liened_num) as *mut u128;
            let source_claim_counterparty_liened_num =
                core::ptr::addr_of_mut!((*raw).source_claim_counterparty_liened_num) as *mut u128;
            let source_claim_insurance_liened_num =
                core::ptr::addr_of_mut!((*raw).source_claim_insurance_liened_num) as *mut u128;
            let source_lien_effective_reserved =
                core::ptr::addr_of_mut!((*raw).source_lien_effective_reserved) as *mut u128;
            let source_lien_counterparty_backing_num =
                core::ptr::addr_of_mut!((*raw).source_lien_counterparty_backing_num) as *mut u128;
            let source_lien_insurance_backing_num =
                core::ptr::addr_of_mut!((*raw).source_lien_insurance_backing_num) as *mut u128;
            let source_claim_impaired_num =
                core::ptr::addr_of_mut!((*raw).source_claim_impaired_num) as *mut u128;
            let source_lien_impaired_effective_reserved =
                core::ptr::addr_of_mut!((*raw).source_lien_impaired_effective_reserved)
                    as *mut u128;
            let mut d = 0;
            while d < V16_DOMAIN_COUNT {
                source_claim_market_id.add(d).write(0);
                source_claim_bound_num.add(d).write(0);
                source_claim_liened_num.add(d).write(0);
                source_claim_counterparty_liened_num.add(d).write(0);
                source_claim_insurance_liened_num.add(d).write(0);
                source_lien_effective_reserved.add(d).write(0);
                source_lien_counterparty_backing_num.add(d).write(0);
                source_lien_insurance_backing_num.add(d).write(0);
                source_claim_impaired_num.add(d).write(0);
                source_lien_impaired_effective_reserved.add(d).write(0);
                d += 1;
            }
            core::ptr::addr_of_mut!((*raw).fee_credits).write(0);
            core::ptr::addr_of_mut!((*raw).cancel_deposit_escrow).write(0);
            core::ptr::addr_of_mut!((*raw).last_fee_slot).write(last_fee_slot);
            core::ptr::addr_of_mut!((*raw).active_bitmap).write(percolator::active_bitmap_empty());
            let legs = core::ptr::addr_of_mut!((*raw).legs) as *mut PortfolioLegV16;
            let mut i = 0;
            while i < V16_MAX_PORTFOLIO_ASSETS_N {
                legs.add(i).write(PortfolioLegV16::EMPTY);
                i += 1;
            }
            core::ptr::addr_of_mut!((*raw).health_cert).write(HealthCertV16 {
                certified_equity: 0,
                certified_initial_req: 0,
                certified_maintenance_req: 0,
                certified_liq_deficit: 0,
                certified_worst_case_loss: 0,
                cert_oracle_epoch: 0,
                cert_funding_epoch: 0,
                cert_risk_epoch: 0,
                cert_asset_set_epoch: 0,
                active_bitmap_at_cert: percolator::active_bitmap_empty(),
                valid: false,
            });
            core::ptr::addr_of_mut!((*raw).stale_state).write(false);
            core::ptr::addr_of_mut!((*raw).b_stale_state).write(false);
            core::ptr::addr_of_mut!((*raw).rebalance_lock).write(false);
            core::ptr::addr_of_mut!((*raw).liquidation_lock).write(false);
            core::ptr::addr_of_mut!((*raw).close_progress).write(CloseProgressLedgerV16::EMPTY);
            core::ptr::addr_of_mut!((*raw).resolved_payout_receipt)
                .write(ResolvedPayoutReceiptV16::EMPTY);
            Ok(alloc::boxed::Box::from_raw(raw))
        }
    }

    fn account<'a>(
        accounts: &'a [AccountInfo<'a>],
        idx: usize,
    ) -> Result<&'a AccountInfo<'a>, ProgramError> {
        accounts.get(idx).ok_or(ProgramError::NotEnoughAccountKeys)
    }

    fn expect_signer(ai: &AccountInfo) -> Result<(), ProgramError> {
        if !ai.is_signer {
            return Err(PercolatorError::ExpectedSigner.into());
        }
        Ok(())
    }

    fn expect_writable(ai: &AccountInfo) -> Result<(), ProgramError> {
        if !ai.is_writable {
            return Err(PercolatorError::ExpectedWritable.into());
        }
        Ok(())
    }

    fn expect_owner(ai: &AccountInfo, owner: &Pubkey) -> Result<(), ProgramError> {
        if ai.owner != owner {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(())
    }

    fn expect_live_authority(expected: &[u8; 32], signer: &Pubkey) -> Result<(), ProgramError> {
        if *expected == [0u8; 32] || *expected != signer.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        Ok(())
    }

    fn expect_portfolio_owner(
        portfolio: &PortfolioAccountV16,
        owner: &Pubkey,
    ) -> Result<(), ProgramError> {
        if portfolio.owner != owner.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        Ok(())
    }

    fn expect_portfolio_account_key(
        portfolio: &PortfolioAccountV16,
        key: &Pubkey,
    ) -> Result<(), ProgramError> {
        if portfolio.provenance_header.portfolio_account_id != key.to_bytes() {
            return Err(PercolatorError::EngineProvenanceMismatch.into());
        }
        Ok(())
    }

    fn with_one_portfolio<'a, F>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        owner_must_sign: bool,
        f: F,
    ) -> ProgramResult
    where
        F: FnOnce(
            &mut MarketGroupV16,
            &mut PortfolioAccountV16,
            &WrapperConfigV16,
        ) -> Result<(), V16Error>,
    {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        if owner_must_sign {
            expect_signer(owner)?;
        }
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        if owner_must_sign {
            expect_portfolio_owner(portfolio.as_ref(), owner.key)?;
        }
        f(group.as_mut(), portfolio.as_mut(), &cfg).map_err(map_v16_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    type EffectivePricesBox = alloc::boxed::Box<[u64; V16_MAX_MARKET_SLOTS_N]>;

    #[allow(unsafe_code)]
    fn effective_prices_boxed(group: &MarketGroupV16) -> V16Result<EffectivePricesBox> {
        let layout = core::alloc::Layout::new::<[u64; V16_MAX_MARKET_SLOTS_N]>();
        let raw =
            unsafe { alloc::alloc::alloc_zeroed(layout) as *mut [u64; V16_MAX_MARKET_SLOTS_N] };
        if raw.is_null() {
            return Err(V16Error::InvalidConfig);
        }
        unsafe {
            let n = group.config.max_market_slots as usize;
            let mut j = 0;
            while j < n {
                (*raw)[j] = group.assets[j].effective_price;
                j += 1;
            }
            Ok(alloc::boxed::Box::from_raw(raw))
        }
    }

    fn group_has_global_loss_state(group: &MarketGroupV16) -> bool {
        group.pnl_pos_tot != 0
            || group.stale_certificate_count != 0
            || group.b_stale_account_count != 0
            || group.negative_pnl_account_count != 0
            || group.bankruptcy_hlock_active
            || group.threshold_stress_active
            || group.loss_stale_active
            || group.recovery_reason.is_some()
    }

    fn asset_local_has_position_or_loss_state(group: &MarketGroupV16, asset_index: usize) -> bool {
        if asset_index >= group.config.max_market_slots as usize {
            return true;
        }
        let asset = group.assets[asset_index];
        asset.oi_eff_long_q != 0
            || asset.oi_eff_short_q != 0
            || asset.stored_pos_count_long != 0
            || asset.stored_pos_count_short != 0
            || asset.stale_account_count_long != 0
            || asset.stale_account_count_short != 0
            || asset.b_long_num != 0
            || asset.b_short_num != 0
            || asset.b_epoch_start_long_num != 0
            || asset.b_epoch_start_short_num != 0
            || asset.loss_weight_sum_long != 0
            || asset.loss_weight_sum_short != 0
            || asset.social_loss_remainder_long_num != 0
            || asset.social_loss_remainder_short_num != 0
            || asset.social_loss_dust_long_num != 0
            || asset.social_loss_dust_short_num != 0
            || asset.explicit_unallocated_loss_long != 0
            || asset.explicit_unallocated_loss_short != 0
            || asset.mode_long != SideModeV16::Normal
            || asset.mode_short != SideModeV16::Normal
    }

    fn asset_has_position_or_loss_state(group: &MarketGroupV16, asset_index: usize) -> bool {
        group_has_global_loss_state(group)
            || asset_local_has_position_or_loss_state(group, asset_index)
    }

    fn group_has_position_or_loss_state(group: &MarketGroupV16) -> bool {
        if group_has_global_loss_state(group) {
            return true;
        }
        let n = group.config.max_market_slots as usize;
        let mut i = 0;
        while i < n {
            if asset_local_has_position_or_loss_state(group, i) {
                return true;
            }
            i += 1;
        }
        false
    }

    fn trade_notional_floor(size_q: u128, price: u64) -> Result<u128, ProgramError> {
        Ok(size_q
            .checked_mul(price as u128)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?
            / percolator::POS_SCALE)
    }

    fn risk_notional_ceil(size_q: u128, price: u64) -> Result<u128, ProgramError> {
        let num = size_q
            .checked_mul(price as u128)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        Ok(num
            .checked_add(percolator::POS_SCALE - 1)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?
            / percolator::POS_SCALE)
    }

    fn hybrid_segment_dt(group: &MarketGroupV16, now_slot: u64) -> Result<u64, ProgramError> {
        if now_slot < group.slot_last {
            return Err(PercolatorError::EngineStale.into());
        }
        Ok(core::cmp::min(
            now_slot - group.slot_last,
            group.config.max_accrual_dt_slots,
        ))
    }

    fn hybrid_effective_price_for_crank(
        cfg: &WrapperConfigV16,
        profile: &mut state::AssetOracleProfileV16,
        group: &MarketGroupV16,
        asset_index: usize,
        now_slot: u64,
        now_unix_ts: i64,
        oracle_accounts: &[AccountInfo],
        fallback_price: u64,
    ) -> Result<u64, ProgramError> {
        if oracle_v16::profile_is_hyperp(profile) {
            let target = profile.mark_ewma_e6;
            if target == 0 {
                return Err(PercolatorError::OracleInvalid.into());
            }
            let asset = group.assets[asset_index];
            let exposed = asset.oi_eff_long_q != 0 || asset.oi_eff_short_q != 0;
            let price = oracle_v16::effective_price_from_target(
                asset.effective_price,
                target,
                group.config.max_price_move_bps_per_slot,
                hybrid_segment_dt(group, now_slot)?,
                exposed,
            );
            profile.oracle_target_price_e6 = target;
            return Ok(price);
        }
        if !oracle_v16::profile_is_hybrid(profile) {
            return Ok(fallback_price);
        }
        if asset_index >= group.config.max_market_slots as usize {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        if cfg.permissionless_resolve_stale_slots != 0
            && now_slot.saturating_sub(profile.last_good_oracle_slot)
                >= cfg.permissionless_resolve_stale_slots
        {
            return Err(PercolatorError::EngineRecoveryRequired.into());
        }
        let count = profile.oracle_leg_count as usize;
        let read = if oracle_accounts.len() >= count {
            oracle_v16::read_external_price_e6_profile(profile, oracle_accounts, now_unix_ts)
        } else {
            Err(ProgramError::NotEnoughAccountKeys)
        };
        let target = match read {
            Ok((price, publish_time, advanced)) => {
                profile.oracle_target_price_e6 = price;
                profile.oracle_target_publish_time = publish_time;
                if advanced {
                    profile.last_good_oracle_slot = now_slot;
                }
                price
            }
            Err(e)
                if e == ProgramError::from(PercolatorError::OracleStale)
                    || e == ProgramError::NotEnoughAccountKeys =>
            {
                if !oracle_v16::profile_hybrid_soft_stale_matured(profile, now_slot) {
                    return Err(e);
                }
                profile.mark_ewma_e6
            }
            Err(e) => return Err(e),
        };
        if target == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        let asset = group.assets[asset_index];
        let exposed = asset.oi_eff_long_q != 0 || asset.oi_eff_short_q != 0;
        let price = oracle_v16::effective_price_from_target(
            asset.effective_price,
            target,
            group.config.max_price_move_bps_per_slot,
            hybrid_segment_dt(group, now_slot)?,
            exposed,
        );
        profile.oracle_target_price_e6 = target;
        if !oracle_v16::profile_hybrid_soft_stale_matured(profile, now_slot) {
            profile.mark_ewma_e6 = price;
            profile.mark_ewma_last_slot = now_slot;
        }
        Ok(price)
    }

    fn hybrid_trade_fee_bps(
        cfg: &WrapperConfigV16,
        profile: &state::AssetOracleProfileV16,
        group: &MarketGroupV16,
        asset_index: usize,
        size_q_abs: u128,
        exec_price: u64,
        caller_fee_bps: u64,
    ) -> Result<u64, ProgramError> {
        let base = core::cmp::max(
            core::cmp::max(caller_fee_bps, cfg.trade_fee_base_bps),
            cfg.backing_trade_fee_bps as u64,
        );
        if base > group.config.max_trading_fee_bps {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        if !oracle_v16::profile_is_price_managed(profile) {
            return Ok(base);
        }
        let now_slot = Clock::get().map(|c| c.slot).unwrap_or(group.current_slot);
        if oracle_v16::profile_is_hybrid(profile)
            && !oracle_v16::profile_hybrid_soft_stale_matured(profile, now_slot)
        {
            return Ok(base);
        }
        if asset_index >= group.config.max_market_slots as usize || profile.mark_ewma_e6 == 0 {
            return Ok(base);
        }
        let trade_notional = trade_notional_floor(size_q_abs, exec_price)?;
        let clamped_exec = oracle_v16::clamp_toward_engine_dt(
            group.assets[asset_index].effective_price,
            exec_price,
            group.config.max_price_move_bps_per_slot,
            1,
        );
        let asset = group.assets[asset_index];
        let max_side_oi_q = core::cmp::max(asset.oi_eff_long_q, asset.oi_eff_short_q);
        let max_side_notional =
            risk_notional_ceil(max_side_oi_q, group.assets[asset_index].effective_price)?;
        let mark_externality_notional = core::cmp::max(max_side_notional, trade_notional)
            .checked_mul(2)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        let segment_dt = core::cmp::max(1, hybrid_segment_dt(group, now_slot)?);
        let min_externality_bps = group
            .config
            .max_price_move_bps_per_slot
            .checked_mul(segment_dt)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        let required = policy_v16::dynamic_fee_bps_with_externality_floor(
            base,
            profile.mark_ewma_e6,
            clamped_exec,
            profile.mark_ewma_halflife_slots,
            profile.mark_ewma_last_slot,
            now_slot,
            trade_notional,
            mark_externality_notional,
            profile.mark_min_fee,
            min_externality_bps,
        )
        .ok_or(PercolatorError::EngineInvalidConfig)?;
        let fee = core::cmp::max(base, required);
        if fee > group.config.max_trading_fee_bps {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        Ok(fee)
    }

    fn update_hybrid_mark_after_trade(
        profile: &mut state::AssetOracleProfileV16,
        group: &MarketGroupV16,
        asset_index: usize,
        exec_price: u64,
        fee_paid: u128,
    ) -> Result<(), ProgramError> {
        if !oracle_v16::profile_is_price_managed(profile) {
            return Ok(());
        }
        let now_slot = Clock::get().map(|c| c.slot).unwrap_or(group.current_slot);
        if (oracle_v16::profile_is_hybrid(profile)
            && !oracle_v16::profile_hybrid_soft_stale_matured(profile, now_slot))
            || asset_index >= group.config.max_market_slots as usize
        {
            return Ok(());
        }
        let fee_paid = u64::try_from(fee_paid).unwrap_or(u64::MAX);
        let clamped_exec = oracle_v16::clamp_toward_engine_dt(
            group.assets[asset_index].effective_price,
            exec_price,
            group.config.max_price_move_bps_per_slot,
            1,
        );
        let old = profile.mark_ewma_e6;
        let new_mark = policy_v16::ewma_update(
            old,
            clamped_exec,
            profile.mark_ewma_halflife_slots,
            profile.mark_ewma_last_slot,
            now_slot,
            fee_paid,
            profile.mark_min_fee,
        );
        if new_mark != 0 && new_mark != old {
            profile.mark_ewma_e6 = new_mark;
            profile.mark_ewma_last_slot = now_slot;
        }
        Ok(())
    }

    #[inline(never)]
    fn execute_trade_svm_aware(
        group: &mut MarketGroupV16,
        long_account: &mut PortfolioAccountV16,
        short_account: &mut PortfolioAccountV16,
        request: TradeRequestV16,
        effective_prices: &[u64],
    ) -> Result<TradeOutcomeV16, V16Error> {
        #[cfg(target_os = "solana")]
        {
            group.execute_trade_with_fee_in_place_not_atomic(
                long_account,
                short_account,
                request,
                effective_prices,
            )
        }
        #[cfg(not(target_os = "solana"))]
        {
            group.execute_trade_with_fee_not_atomic(
                long_account,
                short_account,
                request,
                effective_prices,
            )
        }
    }

    fn effective_prices_with_boxed(
        group: &MarketGroupV16,
        asset_index: usize,
        effective_price: u64,
    ) -> V16Result<EffectivePricesBox> {
        let mut prices = effective_prices_boxed(group)?;
        if asset_index < V16_MAX_MARKET_SLOTS_N && effective_price != 0 {
            prices[asset_index] = effective_price;
        }
        Ok(prices)
    }

    fn derive_matcher_delegate(
        program_id: &Pubkey,
        market_key: &Pubkey,
        maker_account: &Pubkey,
        matcher_program: &Pubkey,
        matcher_context: &Pubkey,
    ) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[
                b"matcher",
                market_key.as_ref(),
                maker_account.as_ref(),
                matcher_program.as_ref(),
                matcher_context.as_ref(),
            ],
            program_id,
        )
    }

    fn matcher_lp_account_id(delegate: &Pubkey) -> u64 {
        let bytes = delegate.to_bytes();
        u64::from_le_bytes(bytes[0..8].try_into().unwrap())
    }

    fn invoke_matcher<'a>(
        matcher_prog: &AccountInfo<'a>,
        matcher_ctx: &AccountInfo<'a>,
        matcher_delegate: &AccountInfo<'a>,
        tail: &[AccountInfo<'a>],
        req_id: u64,
        asset_index: u16,
        lp_account_id: u64,
        oracle_price_e6: u64,
        req_size: i128,
        seeds: &[&[u8]],
    ) -> ProgramResult {
        let mut data = [0u8; 67];
        data[0] = 0;
        data[1..9].copy_from_slice(&req_id.to_le_bytes());
        data[9..11].copy_from_slice(&(asset_index as u16).to_le_bytes());
        data[11..19].copy_from_slice(&lp_account_id.to_le_bytes());
        data[19..27].copy_from_slice(&oracle_price_e6.to_le_bytes());
        data[27..43].copy_from_slice(&req_size.to_le_bytes());

        let mut metas = Vec::with_capacity(2 + tail.len());
        metas.push(AccountMeta::new_readonly(*matcher_delegate.key, true));
        metas.push(AccountMeta::new(*matcher_ctx.key, false));
        for ai in tail {
            if ai.is_writable {
                metas.push(AccountMeta::new(*ai.key, ai.is_signer));
            } else {
                metas.push(AccountMeta::new_readonly(*ai.key, ai.is_signer));
            }
        }

        let ix = SolInstruction {
            program_id: *matcher_prog.key,
            accounts: metas,
            data: data.to_vec(),
        };
        let mut infos = Vec::with_capacity(3 + tail.len());
        infos.push(matcher_delegate.clone());
        infos.push(matcher_ctx.clone());
        infos.push(matcher_prog.clone());
        for ai in tail {
            infos.push(ai.clone());
        }
        invoke_signed(&ix, &infos, &[seeds])
    }

    fn amount_to_u64(amount: u128) -> Result<u64, ProgramError> {
        u64::try_from(amount).map_err(|_| PercolatorError::InvalidInstruction.into())
    }

    fn derive_vault_authority(program_id: &Pubkey, market_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"vault", market_key.as_ref()], program_id)
    }

    fn expect_key(ai: &AccountInfo, expected: &Pubkey) -> Result<(), ProgramError> {
        if ai.key != expected {
            return Err(ProgramError::InvalidArgument);
        }
        Ok(())
    }

    fn verify_mint(mint_ai: &AccountInfo) -> Result<(), ProgramError> {
        if mint_ai.owner != &spl_token::ID {
            return Err(PercolatorError::InvalidMint.into());
        }
        if mint_ai.data_len() != spl_token::state::Mint::LEN {
            return Err(PercolatorError::InvalidMint.into());
        }
        let data = mint_ai.try_borrow_data()?;
        spl_token::state::Mint::unpack(&data)
            .map(|_| ())
            .map_err(|_| PercolatorError::InvalidMint.into())
    }

    fn verify_token_program(token_program: &AccountInfo) -> Result<(), ProgramError> {
        if *token_program.key != spl_token::ID || !token_program.executable {
            return Err(PercolatorError::InvalidTokenProgram.into());
        }
        Ok(())
    }

    fn unpack_token_account(
        token_ai: &AccountInfo,
    ) -> Result<spl_token::state::Account, ProgramError> {
        if token_ai.owner != &spl_token::ID {
            return Err(PercolatorError::InvalidTokenAccount.into());
        }
        if token_ai.data_len() != spl_token::state::Account::LEN {
            return Err(PercolatorError::InvalidTokenAccount.into());
        }
        let data = token_ai.try_borrow_data()?;
        spl_token::state::Account::unpack(&data)
            .map_err(|_| PercolatorError::InvalidTokenAccount.into())
    }

    fn verify_user_token_account(
        token_ai: &AccountInfo,
        expected_owner: &Pubkey,
        expected_mint: &Pubkey,
    ) -> Result<(), ProgramError> {
        let token = unpack_token_account(token_ai)?;
        if token.mint != *expected_mint {
            return Err(PercolatorError::InvalidMint.into());
        }
        if token.owner != *expected_owner
            || token.state != spl_token::state::AccountState::Initialized
        {
            return Err(PercolatorError::InvalidTokenAccount.into());
        }
        Ok(())
    }

    fn verify_vault_token_account(
        token_ai: &AccountInfo,
        expected_owner: &Pubkey,
        expected_mint: &Pubkey,
    ) -> Result<(), ProgramError> {
        let token = unpack_token_account(token_ai)?;
        if token.mint != *expected_mint {
            return Err(PercolatorError::InvalidMint.into());
        }
        if token.owner != *expected_owner
            || token.state != spl_token::state::AccountState::Initialized
            || token.delegate.is_some()
            || token.close_authority.is_some()
        {
            return Err(PercolatorError::InvalidVaultAccount.into());
        }
        Ok(())
    }

    fn require_token_balance(token_ai: &AccountInfo, amount: u64) -> Result<(), ProgramError> {
        let token = unpack_token_account(token_ai)?;
        if token.amount < amount {
            return Err(PercolatorError::InvalidTokenAccount.into());
        }
        Ok(())
    }

    fn transfer_tokens<'a>(
        token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            dest.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke(
            &ix,
            &[
                source.clone(),
                dest.clone(),
                authority.clone(),
                token_program.clone(),
            ],
        )
    }

    fn transfer_tokens_signed<'a>(
        token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
        signer_seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        if amount == 0 {
            return Ok(());
        }
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            dest.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke_signed(
            &ix,
            &[
                source.clone(),
                dest.clone(),
                authority.clone(),
                token_program.clone(),
            ],
            signer_seeds,
        )
    }
}

#[cfg(all(not(feature = "no-entrypoint"), not(feature = "anchor-v2")))]
pub mod entrypoint {
    use super::processor;
    #[allow(unused_imports)]
    use alloc::format;
    #[cfg(target_os = "solana")]
    use solana_program::entrypoint::{BumpAllocator, HEAP_START_ADDRESS};
    use solana_program::{
        account_info::AccountInfo,
        entrypoint::{deserialize, ProgramResult, SUCCESS},
        pubkey::Pubkey,
    };

    // The processor still materializes engine runtime structs. This remains
    // bounded at the current fixed asset cap; larger u16-indexed markets need
    // engine zero-copy/page APIs rather than larger fixed runtime arrays.
    pub const V16_HEAP_FRAME_BYTES: usize = 128 * 1024;

    #[cfg(target_os = "solana")]
    #[global_allocator]
    static A: BumpAllocator = BumpAllocator {
        start: HEAP_START_ADDRESS as usize,
        len: V16_HEAP_FRAME_BYTES,
    };

    solana_program::custom_panic_default!();

    /// # Safety
    #[no_mangle]
    pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
        let (program_id, accounts, instruction_data) = unsafe { deserialize(input) };
        match process_instruction(&program_id, &accounts, &instruction_data) {
            Ok(()) => SUCCESS,
            Err(error) => error.into(),
        }
    }

    fn process_instruction<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        processor::process_instruction(program_id, accounts, instruction_data)
    }
}

#[cfg(all(not(feature = "no-entrypoint"), feature = "anchor-v2"))]
#[allow(unsafe_code)]
pub mod entrypoint {
    extern crate alloc;

    use super::processor;
    use alloc::{rc::Rc, vec::Vec};
    use anchor_lang_v2::pinocchio::{
        account::{AccountView, RuntimeAccount},
        address::Address,
        entrypoint,
        error::ProgramError as AnchorProgramError,
        ProgramResult,
    };
    use core::{cell::RefCell, mem::size_of, slice::from_raw_parts_mut};
    use solana_program::{
        account_info::AccountInfo, clock::Epoch, program_error::ProgramError as LegacyProgramError,
        pubkey::Pubkey,
    };

    entrypoint!(process_instruction);

    fn process_instruction(
        program_id: &Address,
        accounts: &mut [AccountView],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let program_id = Pubkey::new_from_array(program_id.to_bytes());
        process_with_legacy_account_infos(&program_id, accounts, instruction_data)
            .map_err(map_legacy_error)
    }

    #[inline(never)]
    fn process_with_legacy_account_infos(
        program_id: &Pubkey,
        accounts: &mut [AccountView],
        instruction_data: &[u8],
    ) -> Result<(), LegacyProgramError> {
        let len = accounts.len();
        let mut lamports = Vec::with_capacity(len);
        let mut data = Vec::with_capacity(len);

        for i in 0..len {
            if let Some(first) = first_duplicate(accounts, i) {
                lamports.push(Rc::clone(&lamports[first]));
                data.push(Rc::clone(&data[first]));
                continue;
            }

            let raw = accounts[i].account_mut_ptr();
            // Anchor v2 / Pinocchio owns the runtime account view. The v16
            // processor still uses AccountInfo internally, so this adapter is
            // the only compatibility bridge; persisted state serialization is
            // handled explicitly by `state`, not by raw Rust layout casts.
            let lamports_ref = unsafe { &mut (*raw).lamports };
            let data_ref = unsafe {
                from_raw_parts_mut(
                    (raw as *mut u8).add(size_of::<RuntimeAccount>()),
                    (*raw).data_len as usize,
                )
            };
            lamports.push(Rc::new(RefCell::new(lamports_ref)));
            data.push(Rc::new(RefCell::new(data_ref)));
        }

        let mut legacy_accounts = Vec::with_capacity(len);
        for (i, account) in accounts.iter().enumerate() {
            let key = unsafe { &*(account.address() as *const Address as *const Pubkey) };
            let owner = unsafe { &*(account.owner() as *const Address as *const Pubkey) };
            legacy_accounts.push(AccountInfo {
                key,
                lamports: Rc::clone(&lamports[i]),
                data: Rc::clone(&data[i]),
                owner,
                rent_epoch: Epoch::default(),
                is_signer: account.is_signer(),
                is_writable: account.is_writable(),
                executable: account.executable(),
            });
        }

        processor::process_instruction(program_id, &legacy_accounts, instruction_data)
    }

    fn first_duplicate(accounts: &[AccountView], index: usize) -> Option<usize> {
        let ptr = accounts[index].account_ptr();
        let mut i = 0;
        while i < index {
            if accounts[i].account_ptr() == ptr {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn map_legacy_error(error: LegacyProgramError) -> AnchorProgramError {
        match error {
            LegacyProgramError::Custom(code) => AnchorProgramError::Custom(code),
            LegacyProgramError::InvalidArgument => AnchorProgramError::InvalidArgument,
            LegacyProgramError::InvalidInstructionData => {
                AnchorProgramError::InvalidInstructionData
            }
            LegacyProgramError::InvalidAccountData => AnchorProgramError::InvalidAccountData,
            LegacyProgramError::AccountDataTooSmall => AnchorProgramError::AccountDataTooSmall,
            LegacyProgramError::InsufficientFunds => AnchorProgramError::InsufficientFunds,
            LegacyProgramError::IncorrectProgramId => AnchorProgramError::IncorrectProgramId,
            LegacyProgramError::MissingRequiredSignature => {
                AnchorProgramError::MissingRequiredSignature
            }
            LegacyProgramError::AccountAlreadyInitialized => {
                AnchorProgramError::AccountAlreadyInitialized
            }
            LegacyProgramError::UninitializedAccount => AnchorProgramError::UninitializedAccount,
            LegacyProgramError::NotEnoughAccountKeys => AnchorProgramError::NotEnoughAccountKeys,
            LegacyProgramError::AccountBorrowFailed => AnchorProgramError::AccountBorrowFailed,
            LegacyProgramError::MaxSeedLengthExceeded => AnchorProgramError::MaxSeedLengthExceeded,
            LegacyProgramError::InvalidSeeds => AnchorProgramError::InvalidSeeds,
            LegacyProgramError::BorshIoError(_) => AnchorProgramError::BorshIoError,
            LegacyProgramError::AccountNotRentExempt => AnchorProgramError::AccountNotRentExempt,
            LegacyProgramError::UnsupportedSysvar => AnchorProgramError::UnsupportedSysvar,
            LegacyProgramError::IllegalOwner => AnchorProgramError::IllegalOwner,
            LegacyProgramError::MaxAccountsDataAllocationsExceeded => {
                AnchorProgramError::MaxAccountsDataAllocationsExceeded
            }
            LegacyProgramError::InvalidRealloc => AnchorProgramError::InvalidRealloc,
            LegacyProgramError::MaxInstructionTraceLengthExceeded => {
                AnchorProgramError::MaxInstructionTraceLengthExceeded
            }
            LegacyProgramError::BuiltinProgramsMustConsumeComputeUnits => {
                AnchorProgramError::BuiltinProgramsMustConsumeComputeUnits
            }
            LegacyProgramError::InvalidAccountOwner => AnchorProgramError::InvalidAccountOwner,
            LegacyProgramError::ArithmeticOverflow => AnchorProgramError::ArithmeticOverflow,
        }
    }
}

pub mod risk {
    pub use percolator::*;
}
