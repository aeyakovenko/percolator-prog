//! Percolator v13 Solana wrapper.
//!
//! v13 is account-local: a market-group account stores `MarketGroupV13`, and
//! each trader/LP is an independently supplied `PortfolioAccountV13`. The
//! wrapper deliberately does not recreate the v12 global account slab.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use percolator::{
    AssetStateV13, HealthCertV13, MarketGroupV13, MarketModeV13, PermissionlessCrankActionV13,
    PermissionlessCrankRequestV13, PermissionlessRecoveryReasonV13, PortfolioAccountV13,
    PortfolioLegV13, ProvenanceHeaderV13, ResolvedCloseOutcomeV13, TradeOutcomeV13,
    TradeRequestV13, V13Config, V13Error, V13_MAX_PORTFOLIO_ASSETS_N,
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
    use percolator::{MarketGroupV13Account, PortfolioAccountV13Account};

    pub const MAGIC: u64 = 0x5045_5243_5631_3300; // "PERCV13\0"
    pub const VERSION: u16 = 13;
    pub const KIND_MARKET: u8 = 1;
    pub const KIND_PORTFOLIO: u8 = 2;

    pub const HEADER_LEN: usize = 16;
    pub const WRAPPER_CONFIG_LEN: usize = 176;
    pub const MARKET_GROUP_LEN: usize = size_of::<MarketGroupV13Account>();
    pub const PORTFOLIO_STATE_LEN: usize = size_of::<PortfolioAccountV13Account>();
    pub const MARKET_GROUP_OFF: usize = HEADER_LEN + WRAPPER_CONFIG_LEN;
    pub const MARKET_ACCOUNT_LEN: usize = MARKET_GROUP_OFF + MARKET_GROUP_LEN;
    pub const PORTFOLIO_ACCOUNT_LEN: usize = HEADER_LEN + PORTFOLIO_STATE_LEN;
    pub const MAX_MATCHER_TAIL_ACCOUNTS: usize = 32;
    pub const MATCHER_ABI_VERSION: u32 = 2;
    pub const MATCHER_CONTEXT_MIN_LEN: usize = 64;
}

pub mod error {
    use percolator::V13Error;
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
    }

    impl From<PercolatorError> for ProgramError {
        fn from(value: PercolatorError) -> Self {
            ProgramError::Custom(value as u32)
        }
    }

    pub fn map_v13_error(err: V13Error) -> ProgramError {
        let mapped = match err {
            V13Error::InvalidConfig => PercolatorError::EngineInvalidConfig,
            V13Error::ArithmeticOverflow => PercolatorError::EngineArithmeticOverflow,
            V13Error::ProvenanceMismatch => PercolatorError::EngineProvenanceMismatch,
            V13Error::HiddenLeg => PercolatorError::EngineHiddenLeg,
            V13Error::InvalidLeg => PercolatorError::EngineInvalidLeg,
            V13Error::Stale => PercolatorError::EngineStale,
            V13Error::BStale => PercolatorError::EngineBStale,
            V13Error::LockActive => PercolatorError::EngineLockActive,
            V13Error::NonProgress => PercolatorError::EngineNonProgress,
            V13Error::RecoveryRequired => PercolatorError::EngineRecoveryRequired,
            V13Error::CounterOverflow => PercolatorError::EngineCounterOverflow,
            V13Error::CounterUnderflow => PercolatorError::EngineCounterUnderflow,
        };
        mapped.into()
    }
}

pub mod state {
    use crate::{
        constants::{
            HEADER_LEN, KIND_MARKET, KIND_PORTFOLIO, MAGIC, MARKET_ACCOUNT_LEN, MARKET_GROUP_OFF,
            PORTFOLIO_ACCOUNT_LEN, VERSION, WRAPPER_CONFIG_LEN,
        },
        error::PercolatorError,
    };
    use alloc::boxed::Box;
    use core::ptr::addr_of_mut;
    use percolator::{
        AssetStateV13Account, HealthCertV13Account, MarketGroupV13, MarketGroupV13Account,
        MarketModeV13, PortfolioAccountV13, PortfolioAccountV13Account, PortfolioLegV13Account,
        ProvenanceHeaderV13Account, V13ConfigAccount, V13Error, V13OptionalRecoveryReasonAccount,
        V13PodI128, V13PodU128, V13PodU32, V13PodU64, V13_MAX_PORTFOLIO_ASSETS_N,
    };
    use solana_program::program_error::ProgramError;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
    pub struct WrapperConfigV13 {
        pub admin: [u8; 32],
        pub collateral_mint: [u8; 32],
        pub maintenance_fee_per_slot: u128,
        pub trade_fee_base_bps: u64,
        pub permissionless_resolve_stale_slots: u64,
        pub force_close_delay_slots: u64,
        pub last_good_oracle_slot: u64,
        pub insurance_authority: [u8; 32],
        pub insurance_operator: [u8; 32],
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
    fn map_account_wire_error(_: V13Error) -> ProgramError {
        ProgramError::InvalidAccountData
    }

    #[inline]
    fn read_wrapper_config_from_bytes(data: &[u8]) -> Result<WrapperConfigV13, ProgramError> {
        let bytes = data
            .get(HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN)
            .ok_or(PercolatorError::InvalidAccountLen)?;
        Ok(bytemuck::pod_read_unaligned(bytes))
    }

    #[inline]
    fn write_wrapper_config_to_bytes(
        data: &mut [u8],
        config: &WrapperConfigV13,
    ) -> Result<(), ProgramError> {
        data.get_mut(HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN)
            .ok_or(PercolatorError::InvalidAccountLen)?
            .copy_from_slice(bytemuck::bytes_of(config));
        Ok(())
    }

    #[inline]
    fn market_wire(data: &[u8]) -> Result<&MarketGroupV13Account, ProgramError> {
        let bytes = data
            .get(MARKET_GROUP_OFF..MARKET_GROUP_OFF + core::mem::size_of::<MarketGroupV13Account>())
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn market_wire_mut(data: &mut [u8]) -> Result<&mut MarketGroupV13Account, ProgramError> {
        let bytes = data
            .get_mut(
                MARKET_GROUP_OFF..MARKET_GROUP_OFF + core::mem::size_of::<MarketGroupV13Account>(),
            )
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes_mut(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn portfolio_wire(data: &[u8]) -> Result<&PortfolioAccountV13Account, ProgramError> {
        let bytes = data
            .get(HEADER_LEN..HEADER_LEN + core::mem::size_of::<PortfolioAccountV13Account>())
            .ok_or(PercolatorError::InvalidAccountLen)?;
        bytemuck::try_from_bytes(bytes).map_err(|_| ProgramError::InvalidAccountData)
    }

    #[inline]
    fn portfolio_wire_mut(
        data: &mut [u8],
    ) -> Result<&mut PortfolioAccountV13Account, ProgramError> {
        let bytes = data
            .get_mut(HEADER_LEN..HEADER_LEN + core::mem::size_of::<PortfolioAccountV13Account>())
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
    fn encode_market_mode(mode: MarketModeV13) -> u8 {
        match mode {
            MarketModeV13::Live => 0,
            MarketModeV13::Resolved => 1,
            MarketModeV13::Recovery => 2,
        }
    }

    #[inline]
    fn decode_market_mode(v: u8) -> Result<MarketModeV13, ProgramError> {
        match v {
            0 => Ok(MarketModeV13::Live),
            1 => Ok(MarketModeV13::Resolved),
            2 => Ok(MarketModeV13::Recovery),
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
    // places an entire MarketGroupV13/PortfolioAccountV13 temporary on stack.
    fn market_from_wire_boxed(
        wire: &MarketGroupV13Account,
    ) -> Result<Box<MarketGroupV13>, ProgramError> {
        let mut boxed = Box::<MarketGroupV13>::new_uninit();
        let ptr = boxed.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).market_group_id).write(wire.market_group_id);
            addr_of_mut!((*ptr).config).write(
                wire.config
                    .try_to_runtime()
                    .map_err(map_account_wire_error)?,
            );
            addr_of_mut!((*ptr).vault).write(wire.vault.get());
            addr_of_mut!((*ptr).insurance).write(wire.insurance.get());
            addr_of_mut!((*ptr).c_tot).write(wire.c_tot.get());
            addr_of_mut!((*ptr).pnl_pos_tot).write(wire.pnl_pos_tot.get());
            addr_of_mut!((*ptr).pnl_matured_pos_tot).write(wire.pnl_matured_pos_tot.get());
            addr_of_mut!((*ptr).materialized_portfolio_count)
                .write(wire.materialized_portfolio_count.get());
            addr_of_mut!((*ptr).stale_certificate_count).write(wire.stale_certificate_count.get());
            addr_of_mut!((*ptr).b_stale_account_count).write(wire.b_stale_account_count.get());
            addr_of_mut!((*ptr).negative_pnl_account_count)
                .write(wire.negative_pnl_account_count.get());
            addr_of_mut!((*ptr).risk_epoch).write(wire.risk_epoch.get());
            addr_of_mut!((*ptr).oracle_epoch).write(wire.oracle_epoch.get());
            addr_of_mut!((*ptr).funding_epoch).write(wire.funding_epoch.get());
            addr_of_mut!((*ptr).slot_last).write(wire.slot_last.get());
            addr_of_mut!((*ptr).current_slot).write(wire.current_slot.get());
            for i in 0..V13_MAX_PORTFOLIO_ASSETS_N {
                addr_of_mut!((*ptr).assets[i]).write(
                    wire.assets[i]
                        .try_to_runtime()
                        .map_err(map_account_wire_error)?,
                );
            }
            addr_of_mut!((*ptr).bankruptcy_hlock_active)
                .write(decode_bool(wire.bankruptcy_hlock_active)?);
            addr_of_mut!((*ptr).threshold_stress_active)
                .write(decode_bool(wire.threshold_stress_active)?);
            addr_of_mut!((*ptr).active_bankrupt_close_present)
                .write(decode_bool(wire.active_bankrupt_close_present)?);
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
            let group = boxed.assume_init();
            group
                .assert_public_invariants()
                .map_err(map_account_wire_error)?;
            Ok(group)
        }
    }

    fn write_market_wire(wire: &mut MarketGroupV13Account, group: &MarketGroupV13) {
        wire.market_group_id = group.market_group_id;
        wire.config = V13ConfigAccount::from_runtime(&group.config);
        wire.vault = V13PodU128::new(group.vault);
        wire.insurance = V13PodU128::new(group.insurance);
        wire.c_tot = V13PodU128::new(group.c_tot);
        wire.pnl_pos_tot = V13PodU128::new(group.pnl_pos_tot);
        wire.pnl_matured_pos_tot = V13PodU128::new(group.pnl_matured_pos_tot);
        wire.materialized_portfolio_count = V13PodU64::new(group.materialized_portfolio_count);
        wire.stale_certificate_count = V13PodU64::new(group.stale_certificate_count);
        wire.b_stale_account_count = V13PodU64::new(group.b_stale_account_count);
        wire.negative_pnl_account_count = V13PodU64::new(group.negative_pnl_account_count);
        wire.risk_epoch = V13PodU64::new(group.risk_epoch);
        wire.oracle_epoch = V13PodU64::new(group.oracle_epoch);
        wire.funding_epoch = V13PodU64::new(group.funding_epoch);
        wire.slot_last = V13PodU64::new(group.slot_last);
        wire.current_slot = V13PodU64::new(group.current_slot);
        for i in 0..V13_MAX_PORTFOLIO_ASSETS_N {
            wire.assets[i] = AssetStateV13Account::from_runtime(&group.assets[i]);
        }
        wire.bankruptcy_hlock_active = encode_bool(group.bankruptcy_hlock_active);
        wire.threshold_stress_active = encode_bool(group.threshold_stress_active);
        wire.active_bankrupt_close_present = encode_bool(group.active_bankrupt_close_present);
        wire.loss_stale_active = encode_bool(group.loss_stale_active);
        wire.recovery_reason =
            V13OptionalRecoveryReasonAccount::from_runtime(group.recovery_reason);
        wire.mode = encode_market_mode(group.mode);
        wire.resolved_slot = V13PodU64::new(group.resolved_slot);
        wire.payout_snapshot = V13PodU128::new(group.payout_snapshot);
        wire.payout_snapshot_pnl_pos_tot = V13PodU128::new(group.payout_snapshot_pnl_pos_tot);
        wire.payout_snapshot_captured = encode_bool(group.payout_snapshot_captured);
    }

    fn portfolio_from_wire_boxed(
        wire: &PortfolioAccountV13Account,
    ) -> Result<Box<PortfolioAccountV13>, ProgramError> {
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
        let reserved_pnl = wire.reserved_pnl.get();
        if reserved_pnl > pnl.max(0) as u128 {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut boxed = Box::<PortfolioAccountV13>::new_uninit();
        let ptr = boxed.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).provenance_header).write(provenance_header);
            addr_of_mut!((*ptr).owner).write(wire.owner);
            addr_of_mut!((*ptr).capital).write(wire.capital.get());
            addr_of_mut!((*ptr).pnl).write(pnl);
            addr_of_mut!((*ptr).reserved_pnl).write(reserved_pnl);
            addr_of_mut!((*ptr).fee_credits).write(fee_credits);
            addr_of_mut!((*ptr).last_fee_slot).write(wire.last_fee_slot.get());
            addr_of_mut!((*ptr).active_bitmap).write(wire.active_bitmap.get());
            for i in 0..V13_MAX_PORTFOLIO_ASSETS_N {
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
            Ok(boxed.assume_init())
        }
    }

    fn write_portfolio_wire(wire: &mut PortfolioAccountV13Account, account: &PortfolioAccountV13) {
        wire.provenance_header =
            ProvenanceHeaderV13Account::from_runtime(&account.provenance_header);
        wire.owner = account.owner;
        wire.capital = V13PodU128::new(account.capital);
        wire.pnl = V13PodI128::new(account.pnl);
        wire.reserved_pnl = V13PodU128::new(account.reserved_pnl);
        wire.fee_credits = V13PodI128::new(account.fee_credits);
        wire.last_fee_slot = V13PodU64::new(account.last_fee_slot);
        wire.active_bitmap = V13PodU32::new(account.active_bitmap);
        for i in 0..V13_MAX_PORTFOLIO_ASSETS_N {
            wire.legs[i] = PortfolioLegV13Account::from_runtime(&account.legs[i]);
        }
        wire.health_cert = HealthCertV13Account::from_runtime(&account.health_cert);
        wire.stale_state = encode_bool(account.stale_state);
        wire.b_stale_state = encode_bool(account.b_stale_state);
        wire.rebalance_lock = encode_bool(account.rebalance_lock);
        wire.liquidation_lock = encode_bool(account.liquidation_lock);
    }

    pub fn init_market_account(
        data: &mut [u8],
        config: &WrapperConfigV13,
        group: &MarketGroupV13,
    ) -> Result<(), ProgramError> {
        if data.len() < MARKET_ACCOUNT_LEN {
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
        write_market_wire(market_wire_mut(data)?, group);
        Ok(())
    }

    #[cfg(not(target_os = "solana"))]
    pub fn read_market(data: &[u8]) -> Result<(WrapperConfigV13, MarketGroupV13), ProgramError> {
        if data.len() < MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_wrapper_config_from_bytes(data)?;
        Ok((config, *market_from_wire_boxed(market_wire(data)?)?))
    }

    pub fn read_market_boxed(
        data: &[u8],
    ) -> Result<(WrapperConfigV13, Box<MarketGroupV13>), ProgramError> {
        if data.len() < MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_wrapper_config_from_bytes(data)?;
        Ok((config, market_from_wire_boxed(market_wire(data)?)?))
    }

    pub fn write_market(
        data: &mut [u8],
        config: &WrapperConfigV13,
        group: &MarketGroupV13,
    ) -> Result<(), ProgramError> {
        if data.len() < MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        write_wrapper_config_to_bytes(data, config)?;
        write_market_wire(market_wire_mut(data)?, group);
        Ok(())
    }

    pub fn init_portfolio_account(
        data: &mut [u8],
        account: &PortfolioAccountV13,
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
    pub fn read_portfolio(data: &[u8]) -> Result<PortfolioAccountV13, ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        Ok(*portfolio_from_wire_boxed(portfolio_wire(data)?)?)
    }

    pub fn read_portfolio_boxed(data: &[u8]) -> Result<Box<PortfolioAccountV13>, ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        portfolio_from_wire_boxed(portfolio_wire(data)?)
    }

    pub fn write_portfolio(
        data: &mut [u8],
        account: &PortfolioAccountV13,
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
            asset_index: u8,
            now_slot: u64,
            effective_price: u64,
            funding_rate_e9: i128,
            close_q: u128,
            fee_bps: u64,
            recovery_reason: u8,
        },
        TradeNoCpi {
            asset_index: u8,
            size_q: i128,
            exec_price: u64,
            fee_bps: u64,
        },
        TradeCpi {
            asset_index: u8,
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
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input
                .split_first()
                .ok_or(ProgramError::InvalidInstructionData)?;
            let ix = match tag {
                0 => Self::InitMarket {
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
                    asset_index: read_u8(&mut rest)?,
                    now_slot: read_u64(&mut rest)?,
                    effective_price: read_u64(&mut rest)?,
                    funding_rate_e9: read_i128(&mut rest)?,
                    close_q: read_u128(&mut rest)?,
                    fee_bps: read_u64(&mut rest)?,
                    recovery_reason: read_u8(&mut rest)?,
                },
                6 => Self::TradeNoCpi {
                    asset_index: read_u8(&mut rest)?,
                    size_q: read_i128(&mut rest)?,
                    exec_price: read_u64(&mut rest)?,
                    fee_bps: read_u64(&mut rest)?,
                },
                10 => Self::TradeCpi {
                    asset_index: read_u8(&mut rest)?,
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
                    public_b_chunk_atoms,
                    maintenance_fee_per_slot,
                } => {
                    out.push(0);
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
                    out.push(asset_index);
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
                    out.push(asset_index);
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
                    out.push(asset_index);
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

    fn push_u128(out: &mut Vec<u8>, v: u128) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_i128(out: &mut Vec<u8>, v: i128) {
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
        pub reserved: u64,
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
            reserved: u64::from_le_bytes(ctx[56..64].try_into().unwrap()),
        })
    }

    pub fn validate_matcher_return(
        ret: &MatcherReturn,
        lp_account_id: u64,
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
            || ret.reserved != 0
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

pub mod processor {
    use super::*;
    use crate::{
        error::{map_v13_error, PercolatorError},
        ix::Instruction,
        state::{self, WrapperConfigV13},
    };

    pub const AUTHORITY_ADMIN: u8 = 0;
    pub const AUTHORITY_HYPERP_MARK: u8 = 1;
    pub const AUTHORITY_INSURANCE: u8 = 2;
    pub const AUTHORITY_INSURANCE_OPERATOR: u8 = 4;

    pub fn process_instruction<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        match Instruction::decode(instruction_data)? {
            Instruction::InitMarket {
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
                public_b_chunk_atoms,
                maintenance_fee_per_slot,
            } => handle_init_market(
                program_id,
                accounts,
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
            } => with_one_portfolio(program_id, accounts, false, |group, portfolio, _cfg| {
                crank_one_portfolio(
                    group,
                    portfolio,
                    action,
                    asset_index,
                    now_slot,
                    effective_price,
                    funding_rate_e9,
                    close_q,
                    fee_bps,
                    recovery_reason,
                )
            }),
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
            Instruction::ConvertReleasedPnl { amount } => {
                handle_convert_released_pnl(program_id, accounts, amount)
            }
            Instruction::CloseResolved { fee_rate_per_slot } => {
                handle_close_resolved(program_id, accounts, fee_rate_per_slot)
            }
            Instruction::UpdateAuthority { kind, new_pubkey } => {
                handle_update_authority(program_id, accounts, kind, new_pubkey)
            }
        }
    }

    #[inline(never)]
    fn handle_init_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
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
        if trade_fee_base_bps > max_trading_fee_bps {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        let mut cfg = V13Config::public_user_fund(1, h_min, h_max);
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
        cfg.public_b_chunk_atoms = public_b_chunk_atoms;
        let mut group = new_market_group_boxed(market_ai.key.to_bytes(), cfg)?;
        if initial_price == 0 || initial_price > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        group.assets[0].raw_oracle_target_price = initial_price;
        group.assets[0].effective_price = initial_price;
        group.assets[0].fund_px_last = initial_price;
        let wrapper = WrapperConfigV13 {
            admin: admin.key.to_bytes(),
            collateral_mint: mint_ai.key.to_bytes(),
            maintenance_fee_per_slot,
            trade_fee_base_bps,
            permissionless_resolve_stale_slots: 0,
            force_close_delay_slots: 0,
            last_good_oracle_slot: Clock::get().map(|c| c.slot).unwrap_or(0),
            insurance_authority: admin.key.to_bytes(),
            insurance_operator: admin.key.to_bytes(),
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
        if group.mode != MarketModeV13::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        let account = new_portfolio_boxed(ProvenanceHeaderV13::new(
            market_ai.key.to_bytes(),
            portfolio_ai.key.to_bytes(),
            owner.key.to_bytes(),
        ))?;
        group
            .create_portfolio_account(account.as_ref())
            .map_err(map_v13_error)?;
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
        if group.mode != MarketModeV13::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
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
            .map_err(map_v13_error)?;
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
        if group.mode != MarketModeV13::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
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

        let prices = effective_prices(group.as_ref());
        group
            .withdraw_not_atomic(portfolio.as_mut(), amount, &prices)
            .map_err(map_v13_error)?;
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
        asset_index: u8,
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
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut account_a = state::read_portfolio_boxed(&account_a_ai.try_borrow_data()?)?;
        let mut account_b = state::read_portfolio_boxed(&account_b_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(account_a.as_ref(), account_a_ai.key)?;
        expect_portfolio_account_key(account_b.as_ref(), account_b_ai.key)?;
        expect_portfolio_owner(account_a.as_ref(), signer_a.key)?;
        expect_portfolio_owner(account_b.as_ref(), signer_b.key)?;
        let size_abs = if size_q == i128::MIN || size_q == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        } else {
            size_q.unsigned_abs()
        };
        let prices = effective_prices(group.as_ref());
        let fee_bps = core::cmp::max(fee_bps, cfg.trade_fee_base_bps);
        let req = TradeRequestV13 {
            asset_index: asset_index as usize,
            size_q: size_abs,
            exec_price,
            fee_bps,
        };
        if size_q > 0 {
            execute_trade_svm_aware(
                group.as_mut(),
                account_a.as_mut(),
                account_b.as_mut(),
                req,
                &prices,
            )
            .map_err(map_v13_error)?;
        } else {
            execute_trade_svm_aware(
                group.as_mut(),
                account_b.as_mut(),
                account_a.as_mut(),
                req,
                &prices,
            )
            .map_err(map_v13_error)?;
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut account_a_ai.try_borrow_mut_data()?, account_a.as_ref())?;
        state::write_portfolio(&mut account_b_ai.try_borrow_mut_data()?, account_b.as_ref())
    }

    #[inline(never)]
    fn handle_trade_cpi<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        asset_index: u8,
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

        let (_cfg_pre, group_pre) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let account_a_pre = state::read_portfolio_boxed(&account_a_ai.try_borrow_data()?)?;
        let account_b_pre = state::read_portfolio_boxed(&account_b_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(account_a_pre.as_ref(), account_a_ai.key)?;
        expect_portfolio_account_key(account_b_pre.as_ref(), account_b_ai.key)?;
        expect_portfolio_owner(account_a_pre.as_ref(), signer_a.key)?;
        expect_portfolio_owner(account_b_pre.as_ref(), signer_b.key)?;
        if size_q == 0 || size_q == i128::MIN {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let oracle_price = *group_pre
            .assets
            .get(asset_index as usize)
            .map(|a| &a.effective_price)
            .ok_or(PercolatorError::InvalidInstruction)?;
        if oracle_price == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        }
        let req_id = group_pre.current_slot.wrapping_add(1);
        let lp_account_id = matcher_lp_account_id(&delegate);
        drop(group_pre);
        drop(account_a_pre);
        drop(account_b_pre);

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
        matcher_abi::validate_matcher_return(&ret, lp_account_id, oracle_price, size_q, req_id)?;
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

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut account_a = state::read_portfolio_boxed(&account_a_ai.try_borrow_data()?)?;
        let mut account_b = state::read_portfolio_boxed(&account_b_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(account_a.as_ref(), account_a_ai.key)?;
        expect_portfolio_account_key(account_b.as_ref(), account_b_ai.key)?;
        expect_portfolio_owner(account_a.as_ref(), signer_a.key)?;
        expect_portfolio_owner(account_b.as_ref(), signer_b.key)?;

        let prices = effective_prices(group.as_ref());
        let fee_bps = core::cmp::max(fee_bps, cfg.trade_fee_base_bps);
        let req = TradeRequestV13 {
            asset_index: asset_index as usize,
            size_q: ret.exec_size.unsigned_abs(),
            exec_price: ret.exec_price_e6,
            fee_bps,
        };
        if ret.exec_size > 0 {
            execute_trade_svm_aware(
                group.as_mut(),
                account_a.as_mut(),
                account_b.as_mut(),
                req,
                &prices,
            )
            .map_err(map_v13_error)?;
        } else {
            execute_trade_svm_aware(
                group.as_mut(),
                account_b.as_mut(),
                account_a.as_mut(),
                req,
                &prices,
            )
            .map_err(map_v13_error)?;
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
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
            .map_err(map_v13_error)?;
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
        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if group.mode != MarketModeV13::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        if cfg.insurance_authority != signer.key.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
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
        group.assert_public_invariants().map_err(map_v13_error)?;
        transfer_tokens(token_program, source_token, vault_token, signer, amount_u64)?;
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
        if cfg.admin != admin_dest.key.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        if group.mode != MarketModeV13::Resolved {
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

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        if cfg.insurance_operator != operator.key.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }

        match group.mode {
            MarketModeV13::Live => {
                if group.bankruptcy_hlock_active
                    || group.threshold_stress_active
                    || group.active_bankrupt_close_present
                    || group.loss_stale_active
                    || group.recovery_reason.is_some()
                {
                    return Err(PercolatorError::EngineLockActive.into());
                }
            }
            MarketModeV13::Resolved => {
                if group.materialized_portfolio_count != 0 || group.c_tot != 0 {
                    return Err(PercolatorError::EngineLockActive.into());
                }
            }
            MarketModeV13::Recovery => return Err(PercolatorError::EngineLockActive.into()),
        }

        if amount > group.insurance || amount > group.vault {
            return Err(PercolatorError::EngineLockActive.into());
        }
        group.insurance -= amount;
        group.vault -= amount;
        group.assert_public_invariants().map_err(map_v13_error)?;

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
        with_one_portfolio(program_id, accounts, true, |group, portfolio, _cfg| {
            if group.mode != MarketModeV13::Live {
                return Err(V13Error::LockActive);
            }
            // The v13 engine converts the currently released residual-bounded
            // amount atomically. Preserve the v12 caller cap by staging the
            // conversion and only committing it when the converted amount fits.
            let converted = group.convert_released_pnl_to_capital_not_atomic(portfolio)?;
            if converted == 0 || converted > amount {
                return Err(V13Error::LockActive);
            }
            Ok(())
        })
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
        if group.mode != MarketModeV13::Live {
            return Err(PercolatorError::EngineLockActive.into());
        }
        if cfg.admin != admin.key.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        let slot = Clock::get().map(|c| c.slot).unwrap_or(group.current_slot);
        group
            .resolve_market_not_atomic(slot)
            .map_err(map_v13_error)?;
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
                if cfg.admin != current.key.to_bytes() {
                    return Err(PercolatorError::Unauthorized.into());
                }
                if new_pubkey == [0u8; 32]
                    && (group.mode == MarketModeV13::Live
                        && (cfg.permissionless_resolve_stale_slots == 0
                            || cfg.force_close_delay_slots == 0))
                {
                    return Err(PercolatorError::InvalidInstruction.into());
                }
                cfg.admin = new_pubkey;
            }
            AUTHORITY_HYPERP_MARK => {
                return Err(PercolatorError::InvalidInstruction.into());
            }
            AUTHORITY_INSURANCE => {
                if cfg.insurance_authority != current.key.to_bytes() {
                    return Err(PercolatorError::Unauthorized.into());
                }
                cfg.insurance_authority = new_pubkey;
            }
            AUTHORITY_INSURANCE_OPERATOR => {
                if cfg.insurance_operator != current.key.to_bytes() {
                    return Err(PercolatorError::Unauthorized.into());
                }
                cfg.insurance_operator = new_pubkey;
            }
            _ => return Err(PercolatorError::InvalidInstruction.into()),
        }

        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())
    }

    #[inline(never)]
    fn handle_close_resolved<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        fee_rate_per_slot: u128,
    ) -> ProgramResult {
        let owner = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        let portfolio_ai = account(accounts, 2)?;
        let dest_token = account(accounts, 3)?;
        let vault_token = account(accounts, 4)?;
        let vault_authority_ai = account(accounts, 5)?;
        let token_program = account(accounts, 6)?;
        expect_writable(market_ai)?;
        expect_writable(portfolio_ai)?;
        expect_writable(dest_token)?;
        expect_writable(vault_token)?;
        expect_owner(market_ai, program_id)?;
        expect_owner(portfolio_ai, program_id)?;
        verify_token_program(token_program)?;

        let (cfg, mut group) = state::read_market_boxed(&market_ai.try_borrow_data()?)?;
        let mut portfolio = state::read_portfolio_boxed(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_account_key(portfolio.as_ref(), portfolio_ai.key)?;
        expect_portfolio_owner(portfolio.as_ref(), owner.key)?;
        let mint = Pubkey::new_from_array(cfg.collateral_mint);
        let (vault_authority, bump) = derive_vault_authority(program_id, market_ai.key);
        expect_key(vault_authority_ai, &vault_authority)?;
        verify_user_token_account(dest_token, owner.key, &mint)?;
        verify_vault_token_account(vault_token, &vault_authority, &mint)?;

        let outcome = group
            .close_resolved_account_not_atomic(portfolio.as_mut(), fee_rate_per_slot)
            .map_err(map_v13_error)?;
        if let ResolvedCloseOutcomeV13::Closed { payout } = outcome {
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

    #[inline(never)]
    fn crank_one_portfolio(
        group: &mut MarketGroupV13,
        portfolio: &mut PortfolioAccountV13,
        action: u8,
        asset_index: u8,
        now_slot: u64,
        effective_price: u64,
        funding_rate_e9: i128,
        close_q: u128,
        fee_bps: u64,
        recovery_reason: u8,
    ) -> Result<(), V13Error> {
        let action = match action {
            0 => PermissionlessCrankActionV13::Refresh,
            1 => PermissionlessCrankActionV13::Liquidate(percolator::LiquidationRequestV13 {
                asset_index: asset_index as usize,
                close_q,
                fee_bps,
            }),
            2 => PermissionlessCrankActionV13::SettleB {
                asset_index: asset_index as usize,
            },
            3 => PermissionlessCrankActionV13::Recover(recovery_reason_from_u8(recovery_reason)?),
            _ => return Err(V13Error::InvalidConfig),
        };
        let prices = effective_prices_with(group, asset_index as usize, effective_price);
        group
            .permissionless_crank_not_atomic(
                portfolio,
                PermissionlessCrankRequestV13 {
                    now_slot,
                    asset_index: asset_index as usize,
                    effective_price,
                    funding_rate_e9,
                    action,
                },
                &prices,
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
        config: V13Config,
    ) -> Result<alloc::boxed::Box<MarketGroupV13>, ProgramError> {
        // Keep InitMarket SBF-safe by writing the large market object directly
        // to heap memory. This mirrors `MarketGroupV13::new` field-for-field,
        // then validates through the engine's public invariant checker before
        // the bytes are persisted.
        config.validate_public_user_fund().map_err(map_v13_error)?;
        let raw = alloc_raw::<MarketGroupV13>()?;
        unsafe {
            core::ptr::addr_of_mut!((*raw).market_group_id).write(market_group_id);
            core::ptr::addr_of_mut!((*raw).config).write(config);
            core::ptr::addr_of_mut!((*raw).vault).write(0);
            core::ptr::addr_of_mut!((*raw).insurance).write(0);
            core::ptr::addr_of_mut!((*raw).c_tot).write(0);
            core::ptr::addr_of_mut!((*raw).pnl_pos_tot).write(0);
            core::ptr::addr_of_mut!((*raw).pnl_matured_pos_tot).write(0);
            core::ptr::addr_of_mut!((*raw).materialized_portfolio_count).write(0);
            core::ptr::addr_of_mut!((*raw).stale_certificate_count).write(0);
            core::ptr::addr_of_mut!((*raw).b_stale_account_count).write(0);
            core::ptr::addr_of_mut!((*raw).negative_pnl_account_count).write(0);
            core::ptr::addr_of_mut!((*raw).risk_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).oracle_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).funding_epoch).write(0);
            core::ptr::addr_of_mut!((*raw).slot_last).write(0);
            core::ptr::addr_of_mut!((*raw).current_slot).write(0);
            let assets = core::ptr::addr_of_mut!((*raw).assets) as *mut AssetStateV13;
            let mut i = 0;
            while i < V13_MAX_PORTFOLIO_ASSETS_N {
                assets.add(i).write(AssetStateV13::default());
                i += 1;
            }
            core::ptr::addr_of_mut!((*raw).bankruptcy_hlock_active).write(false);
            core::ptr::addr_of_mut!((*raw).threshold_stress_active).write(false);
            core::ptr::addr_of_mut!((*raw).active_bankrupt_close_present).write(false);
            core::ptr::addr_of_mut!((*raw).loss_stale_active).write(false);
            core::ptr::addr_of_mut!((*raw).recovery_reason).write(None);
            core::ptr::addr_of_mut!((*raw).mode).write(MarketModeV13::Live);
            core::ptr::addr_of_mut!((*raw).resolved_slot).write(0);
            core::ptr::addr_of_mut!((*raw).payout_snapshot).write(0);
            core::ptr::addr_of_mut!((*raw).payout_snapshot_pnl_pos_tot).write(0);
            core::ptr::addr_of_mut!((*raw).payout_snapshot_captured).write(false);
            let group = alloc::boxed::Box::from_raw(raw);
            group.assert_public_invariants().map_err(map_v13_error)?;
            Ok(group)
        }
    }

    #[allow(unsafe_code)]
    #[inline(never)]
    fn new_portfolio_boxed(
        header: ProvenanceHeaderV13,
    ) -> Result<alloc::boxed::Box<PortfolioAccountV13>, ProgramError> {
        // Same pattern as market init: avoid a multi-KB stack temporary in the
        // SBF entrypoint while preserving the engine's canonical empty shape.
        let raw = alloc_raw::<PortfolioAccountV13>()?;
        unsafe {
            core::ptr::addr_of_mut!((*raw).provenance_header).write(header);
            core::ptr::addr_of_mut!((*raw).owner).write(header.owner);
            core::ptr::addr_of_mut!((*raw).capital).write(0);
            core::ptr::addr_of_mut!((*raw).pnl).write(0);
            core::ptr::addr_of_mut!((*raw).reserved_pnl).write(0);
            core::ptr::addr_of_mut!((*raw).fee_credits).write(0);
            core::ptr::addr_of_mut!((*raw).last_fee_slot).write(0);
            core::ptr::addr_of_mut!((*raw).active_bitmap).write(0);
            let legs = core::ptr::addr_of_mut!((*raw).legs) as *mut PortfolioLegV13;
            let mut i = 0;
            while i < V13_MAX_PORTFOLIO_ASSETS_N {
                legs.add(i).write(PortfolioLegV13::EMPTY);
                i += 1;
            }
            core::ptr::addr_of_mut!((*raw).health_cert).write(HealthCertV13 {
                certified_equity: 0,
                certified_initial_req: 0,
                certified_maintenance_req: 0,
                certified_liq_deficit: 0,
                certified_worst_case_loss: 0,
                cert_oracle_epoch: 0,
                cert_funding_epoch: 0,
                cert_risk_epoch: 0,
                active_bitmap_at_cert: 0,
                valid: false,
            });
            core::ptr::addr_of_mut!((*raw).stale_state).write(false);
            core::ptr::addr_of_mut!((*raw).b_stale_state).write(false);
            core::ptr::addr_of_mut!((*raw).rebalance_lock).write(false);
            core::ptr::addr_of_mut!((*raw).liquidation_lock).write(false);
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

    fn expect_portfolio_owner(
        portfolio: &PortfolioAccountV13,
        owner: &Pubkey,
    ) -> Result<(), ProgramError> {
        if portfolio.owner != owner.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        Ok(())
    }

    fn expect_portfolio_account_key(
        portfolio: &PortfolioAccountV13,
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
            &mut MarketGroupV13,
            &mut PortfolioAccountV13,
            &WrapperConfigV13,
        ) -> Result<(), V13Error>,
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
        f(group.as_mut(), portfolio.as_mut(), &cfg).map_err(map_v13_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, group.as_ref())?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, portfolio.as_ref())
    }

    fn effective_prices(group: &MarketGroupV13) -> [u64; V13_MAX_PORTFOLIO_ASSETS_N] {
        let mut prices = [1u64; V13_MAX_PORTFOLIO_ASSETS_N];
        let n = group.config.max_portfolio_assets as usize;
        let mut i = 0;
        while i < n {
            prices[i] = group.assets[i].effective_price;
            i += 1;
        }
        prices
    }

    #[inline(never)]
    fn execute_trade_svm_aware(
        group: &mut MarketGroupV13,
        long_account: &mut PortfolioAccountV13,
        short_account: &mut PortfolioAccountV13,
        request: TradeRequestV13,
        effective_prices: &[u64; V13_MAX_PORTFOLIO_ASSETS_N],
    ) -> Result<TradeOutcomeV13, V13Error> {
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

    fn effective_prices_with(
        group: &MarketGroupV13,
        asset_index: usize,
        effective_price: u64,
    ) -> [u64; V13_MAX_PORTFOLIO_ASSETS_N] {
        let mut prices = effective_prices(group);
        if asset_index < V13_MAX_PORTFOLIO_ASSETS_N && effective_price != 0 {
            prices[asset_index] = effective_price;
        }
        prices
    }

    fn recovery_reason_from_u8(v: u8) -> Result<PermissionlessRecoveryReasonV13, V13Error> {
        match v {
            0 => Ok(PermissionlessRecoveryReasonV13::BelowProgressFloor),
            1 => Ok(PermissionlessRecoveryReasonV13::BlockedSegmentHeadroomOrRepresentability),
            2 => Ok(PermissionlessRecoveryReasonV13::AccountBSettlementCannotProgress),
            3 => Ok(PermissionlessRecoveryReasonV13::BIndexHeadroomExhausted),
            4 => Ok(PermissionlessRecoveryReasonV13::ActiveBankruptCloseCannotProgress),
            5 => Ok(PermissionlessRecoveryReasonV13::ExplicitLossOrDustAuditOverflow),
            6 => {
                Ok(PermissionlessRecoveryReasonV13::OracleOrTargetUnavailableByAuthenticatedPolicy)
            }
            7 => Ok(PermissionlessRecoveryReasonV13::CounterOrEpochOverflowDeclaredRecovery),
            _ => Err(V13Error::InvalidConfig),
        }
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
        asset_index: u8,
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
    use solana_program::{
        account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
    };

    entrypoint!(process_instruction);

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
            // Anchor v2 / Pinocchio owns the runtime account view. The v13
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
