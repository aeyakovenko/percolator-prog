//! Percolator v13 Solana wrapper.
//!
//! v13 is account-local: a market-group account stores `MarketGroupV13`, and
//! each trader/LP is an independently supplied `PortfolioAccountV13`. The
//! wrapper deliberately does not recreate the v12 global account slab.

#![no_std]

extern crate alloc;

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
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    sysvar::Sysvar,
};

declare_id!("Perco1ator111111111111111111111111111111111");

pub mod constants {
    use core::mem::size_of;
    use percolator::{MarketGroupV13, PortfolioAccountV13};

    pub const MAGIC: u64 = 0x5045_5243_5631_3300; // "PERCV13\0"
    pub const VERSION: u16 = 13;
    pub const KIND_MARKET: u8 = 1;
    pub const KIND_PORTFOLIO: u8 = 2;

    pub const HEADER_LEN: usize = 16;
    pub const WRAPPER_CONFIG_LEN: usize = size_of::<crate::state::WrapperConfigV13>();
    pub const MARKET_GROUP_OFF: usize = HEADER_LEN + WRAPPER_CONFIG_LEN;
    pub const MARKET_ACCOUNT_LEN: usize = MARKET_GROUP_OFF + size_of::<MarketGroupV13>();
    pub const PORTFOLIO_ACCOUNT_LEN: usize = HEADER_LEN + size_of::<PortfolioAccountV13>();
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
    use core::alloc::Layout;
    use core::mem::{align_of, size_of};
    use percolator::{MarketGroupV13, PortfolioAccountV13};
    use solana_program::program_error::ProgramError;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

    #[allow(unsafe_code)]
    #[inline]
    fn read_copy<T: Copy>(data: &[u8], off: usize) -> Result<T, ProgramError> {
        if data.len() < off + size_of::<T>() {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        let ptr = unsafe { data.as_ptr().add(off) as *const T };
        Ok(unsafe { core::ptr::read_unaligned(ptr) })
    }

    #[allow(unsafe_code)]
    #[inline]
    fn write_copy<T: Copy>(data: &mut [u8], off: usize, value: &T) -> Result<(), ProgramError> {
        if data.len() < off + size_of::<T>() {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        let src = value as *const T as *const u8;
        let dst = unsafe { data.as_mut_ptr().add(off) };
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, size_of::<T>());
        }
        Ok(())
    }

    #[allow(unsafe_code)]
    #[inline]
    fn read_boxed<T: Copy>(data: &[u8], off: usize) -> Result<Box<T>, ProgramError> {
        if data.len() < off + size_of::<T>() {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        let layout = Layout::new::<T>();
        let raw = unsafe { alloc::alloc::alloc(layout) };
        if raw.is_null() {
            return Err(ProgramError::InvalidAccountData);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr().add(off), raw, size_of::<T>());
            Ok(Box::from_raw(raw as *mut T))
        }
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
        write_copy(data, HEADER_LEN, config)?;
        write_copy(data, MARKET_GROUP_OFF, group)
    }

    pub fn read_market(data: &[u8]) -> Result<(WrapperConfigV13, MarketGroupV13), ProgramError> {
        if data.len() < MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_copy(data, HEADER_LEN)?;
        let group = read_copy(data, MARKET_GROUP_OFF)?;
        Ok((config, group))
    }

    pub fn read_market_boxed(
        data: &[u8],
    ) -> Result<(WrapperConfigV13, Box<MarketGroupV13>), ProgramError> {
        if data.len() < MARKET_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_MARKET)?;
        let config = read_copy(data, HEADER_LEN)?;
        let group = read_boxed(data, MARKET_GROUP_OFF)?;
        Ok((config, group))
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
        write_copy(data, HEADER_LEN, config)?;
        write_copy(data, MARKET_GROUP_OFF, group)
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
        write_copy(data, HEADER_LEN, account)
    }

    pub fn read_portfolio(data: &[u8]) -> Result<PortfolioAccountV13, ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        read_copy(data, HEADER_LEN)
    }

    pub fn read_portfolio_boxed(data: &[u8]) -> Result<Box<PortfolioAccountV13>, ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        read_boxed(data, HEADER_LEN)
    }

    pub fn write_portfolio(
        data: &mut [u8],
        account: &PortfolioAccountV13,
    ) -> Result<(), ProgramError> {
        if data.len() < PORTFOLIO_ACCOUNT_LEN {
            return Err(PercolatorError::InvalidAccountLen.into());
        }
        check_header(data, KIND_PORTFOLIO)?;
        write_copy(data, HEADER_LEN, account)
    }

    pub const fn alignment_note() -> usize {
        align_of::<MarketGroupV13>()
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

#[cfg(not(feature = "no-entrypoint"))]
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

pub mod risk {
    pub use percolator::*;
}
