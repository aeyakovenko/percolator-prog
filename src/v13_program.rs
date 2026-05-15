//! Percolator v13 Solana wrapper.
//!
//! v13 is account-local: a market-group account stores `MarketGroupV13`, and
//! each trader/LP is an independently supplied `PortfolioAccountV13`. The
//! wrapper deliberately does not recreate the v12 global account slab.

#![no_std]

extern crate alloc;

use percolator::{
    MarketGroupV13, PermissionlessCrankActionV13, PermissionlessCrankRequestV13,
    PermissionlessRecoveryReasonV13, PortfolioAccountV13, ProvenanceHeaderV13,
    ResolvedCloseOutcomeV13, TradeRequestV13, V13Config, V13Error, V13_MAX_PORTFOLIO_ASSETS_N,
};
use solana_program::{
    account_info::AccountInfo, clock::Clock, declare_id, entrypoint::ProgramResult,
    program_error::ProgramError, pubkey::Pubkey, sysvar::Sysvar,
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
            maintenance_margin_bps: u64,
            initial_margin_bps: u64,
            max_trading_fee_bps: u64,
            max_price_move_bps_per_slot: u64,
            max_accrual_dt_slots: u64,
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
        ResolveMarket,
        CloseResolved {
            fee_rate_per_slot: u128,
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
                    maintenance_margin_bps: read_u64(&mut rest)?,
                    initial_margin_bps: read_u64(&mut rest)?,
                    max_trading_fee_bps: read_u64(&mut rest)?,
                    max_price_move_bps_per_slot: read_u64(&mut rest)?,
                    max_accrual_dt_slots: read_u64(&mut rest)?,
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
                19 => Self::ResolveMarket,
                30 => Self::CloseResolved {
                    fee_rate_per_slot: read_u128(&mut rest)?,
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
                    maintenance_margin_bps,
                    initial_margin_bps,
                    max_trading_fee_bps,
                    max_price_move_bps_per_slot,
                    max_accrual_dt_slots,
                    maintenance_fee_per_slot,
                } => {
                    out.push(0);
                    push_u64(&mut out, h_min);
                    push_u64(&mut out, h_max);
                    push_u64(&mut out, initial_price);
                    push_u64(&mut out, maintenance_margin_bps);
                    push_u64(&mut out, initial_margin_bps);
                    push_u64(&mut out, max_trading_fee_bps);
                    push_u64(&mut out, max_price_move_bps_per_slot);
                    push_u64(&mut out, max_accrual_dt_slots);
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
                Self::ResolveMarket => out.push(19),
                Self::CloseResolved { fee_rate_per_slot } => {
                    out.push(30);
                    push_u128(&mut out, fee_rate_per_slot);
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
                maintenance_margin_bps,
                initial_margin_bps,
                max_trading_fee_bps,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                maintenance_fee_per_slot,
            } => handle_init_market(
                program_id,
                accounts,
                h_min,
                h_max,
                initial_price,
                maintenance_margin_bps,
                initial_margin_bps,
                max_trading_fee_bps,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                maintenance_fee_per_slot,
            ),
            Instruction::InitPortfolio => handle_init_portfolio(program_id, accounts),
            Instruction::Deposit { amount } => {
                with_one_portfolio(program_id, accounts, true, |group, portfolio, _cfg| {
                    group.deposit_not_atomic(portfolio, amount)
                })
            }
            Instruction::Withdraw { amount } => {
                with_one_portfolio(program_id, accounts, true, |group, portfolio, _cfg| {
                    let prices = effective_prices(group);
                    group.withdraw_not_atomic(portfolio, amount, &prices)
                })
            }
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
            Instruction::ResolveMarket => handle_resolve_market(program_id, accounts),
            Instruction::CloseResolved { fee_rate_per_slot } => with_one_portfolio(
                program_id,
                accounts,
                false,
                |group, portfolio, _cfg| match group
                    .close_resolved_account_not_atomic(portfolio, fee_rate_per_slot)?
                {
                    ResolvedCloseOutcomeV13::ProgressOnly => Ok(()),
                    ResolvedCloseOutcomeV13::Closed { .. } => Ok(()),
                },
            ),
        }
    }

    #[inline(never)]
    fn handle_init_market<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        h_min: u64,
        h_max: u64,
        initial_price: u64,
        maintenance_margin_bps: u64,
        initial_margin_bps: u64,
        max_trading_fee_bps: u64,
        max_price_move_bps_per_slot: u64,
        max_accrual_dt_slots: u64,
        maintenance_fee_per_slot: u128,
    ) -> ProgramResult {
        let admin = account(accounts, 0)?;
        let market_ai = account(accounts, 1)?;
        expect_signer(admin)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        let mut cfg = V13Config::public_user_fund(1, h_min, h_max);
        cfg.maintenance_margin_bps = maintenance_margin_bps;
        cfg.initial_margin_bps = initial_margin_bps;
        cfg.max_trading_fee_bps = max_trading_fee_bps;
        cfg.max_price_move_bps_per_slot = max_price_move_bps_per_slot;
        cfg.max_accrual_dt_slots = max_accrual_dt_slots;
        cfg.min_funding_lifetime_slots = max_accrual_dt_slots;
        let mut group =
            MarketGroupV13::new(market_ai.key.to_bytes(), cfg).map_err(map_v13_error)?;
        if initial_price == 0 || initial_price > percolator::MAX_ORACLE_PRICE {
            return Err(PercolatorError::EngineInvalidConfig.into());
        }
        group.assets[0].raw_oracle_target_price = initial_price;
        group.assets[0].effective_price = initial_price;
        group.assets[0].fund_px_last = initial_price;
        let wrapper = WrapperConfigV13 {
            admin: admin.key.to_bytes(),
            collateral_mint: [0u8; 32],
            maintenance_fee_per_slot,
            trade_fee_base_bps: max_trading_fee_bps,
            permissionless_resolve_stale_slots: 0,
            force_close_delay_slots: 0,
            last_good_oracle_slot: Clock::get().map(|c| c.slot).unwrap_or(0),
            insurance_authority: admin.key.to_bytes(),
            insurance_operator: admin.key.to_bytes(),
        };
        state::init_market_account(&mut market_ai.try_borrow_mut_data()?, &wrapper, &group)
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
        let (cfg, mut group) = state::read_market(&market_ai.try_borrow_data()?)?;
        let account = PortfolioAccountV13::empty(ProvenanceHeaderV13::new(
            market_ai.key.to_bytes(),
            portfolio_ai.key.to_bytes(),
            owner.key.to_bytes(),
        ));
        group
            .create_portfolio_account(&account)
            .map_err(map_v13_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, &group)?;
        state::init_portfolio_account(&mut portfolio_ai.try_borrow_mut_data()?, &account)
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
        let (cfg, mut group) = state::read_market(&market_ai.try_borrow_data()?)?;
        let mut account_a = state::read_portfolio(&account_a_ai.try_borrow_data()?)?;
        let mut account_b = state::read_portfolio(&account_b_ai.try_borrow_data()?)?;
        expect_portfolio_owner(&account_a, signer_a.key)?;
        expect_portfolio_owner(&account_b, signer_b.key)?;
        let size_abs = if size_q == i128::MIN || size_q == 0 {
            return Err(PercolatorError::InvalidInstruction.into());
        } else {
            size_q.unsigned_abs()
        };
        let prices = effective_prices(&group);
        let req = TradeRequestV13 {
            asset_index: asset_index as usize,
            size_q: size_abs,
            exec_price,
            fee_bps,
        };
        if size_q > 0 {
            group
                .execute_trade_with_fee_not_atomic(&mut account_a, &mut account_b, req, &prices)
                .map_err(map_v13_error)?;
        } else {
            group
                .execute_trade_with_fee_not_atomic(&mut account_b, &mut account_a, req, &prices)
                .map_err(map_v13_error)?;
        }
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, &group)?;
        state::write_portfolio(&mut account_a_ai.try_borrow_mut_data()?, &account_a)?;
        state::write_portfolio(&mut account_b_ai.try_borrow_mut_data()?, &account_b)
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
        let (cfg, mut group) = state::read_market(&market_ai.try_borrow_data()?)?;
        let portfolio = state::read_portfolio(&portfolio_ai.try_borrow_data()?)?;
        expect_portfolio_owner(&portfolio, owner.key)?;
        group
            .close_portfolio_account(&portfolio)
            .map_err(map_v13_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, &group)?;
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
        expect_signer(signer)?;
        expect_writable(market_ai)?;
        expect_owner(market_ai, program_id)?;
        let (cfg, mut group) = state::read_market(&market_ai.try_borrow_data()?)?;
        if cfg.insurance_authority != signer.key.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        group.insurance = group
            .insurance
            .checked_add(amount)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        group.vault = group
            .vault
            .checked_add(amount)
            .ok_or(PercolatorError::EngineArithmeticOverflow)?;
        group.assert_public_invariants().map_err(map_v13_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, &group)
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
        let (cfg, mut group) = state::read_market(&market_ai.try_borrow_data()?)?;
        if cfg.admin != admin.key.to_bytes() {
            return Err(PercolatorError::Unauthorized.into());
        }
        let slot = Clock::get().map(|c| c.slot).unwrap_or(group.current_slot);
        group
            .resolve_market_not_atomic(slot)
            .map_err(map_v13_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, &group)
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
        let (cfg, mut group) = state::read_market(&market_ai.try_borrow_data()?)?;
        let mut portfolio = state::read_portfolio(&portfolio_ai.try_borrow_data()?)?;
        if owner_must_sign {
            expect_portfolio_owner(&portfolio, owner.key)?;
        }
        f(&mut group, &mut portfolio, &cfg).map_err(map_v13_error)?;
        state::write_market(&mut market_ai.try_borrow_mut_data()?, &cfg, &group)?;
        state::write_portfolio(&mut portfolio_ai.try_borrow_mut_data()?, &portfolio)
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
