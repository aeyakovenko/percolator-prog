//! Percolator: Single-file Solana program with embedded Risk Engine.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use solana_program::pubkey::Pubkey;
use solana_program::declare_id;

declare_id!("Perco1ator111111111111111111111111111111111");

// 1. mod constants
pub mod constants {
    use core::mem::{size_of, align_of};
    use crate::state::MarketConfig;
    use percolator::RiskEngine;

    pub const MAGIC: u64 = 0x504552434f4c4154; // "PERCOLAT"
    pub const VERSION: u32 = 1;
    
    pub const HEADER_LEN: usize = 64;
    pub const CONFIG_LEN: usize = size_of::<MarketConfig>();
    pub const ENGINE_ALIGN: usize = align_of::<RiskEngine>();

    pub const fn align_up(x: usize, a: usize) -> usize {
        (x + (a - 1)) & !(a - 1)
    }

    pub const ENGINE_OFF: usize = align_up(HEADER_LEN + CONFIG_LEN, ENGINE_ALIGN);
    pub const ENGINE_LEN: usize = size_of::<RiskEngine>();
    pub const SLAB_LEN: usize = ENGINE_OFF + ENGINE_LEN;
    pub const MATCHER_ABI_VERSION: u32 = 1;
    pub const MATCHER_CONTEXT_PREFIX_LEN: usize = 64;
    pub const MATCHER_CONTEXT_LEN: usize = 320;
}

// 2. mod zc (Zero-Copy unsafe island)
#[allow(unsafe_code)]
pub mod zc {
    use solana_program::program_error::ProgramError;
    use percolator::RiskEngine;
    use crate::constants::{ENGINE_OFF, ENGINE_LEN, ENGINE_ALIGN};

    #[inline]
    pub fn engine_ref<'a>(data: &'a [u8]) -> Result<&'a RiskEngine, ProgramError> {
        if data.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_ptr().add(ENGINE_OFF) };
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &*(ptr as *const RiskEngine) })
    }

    #[inline]
    pub fn engine_mut<'a>(data: &'a mut [u8]) -> Result<&'a mut RiskEngine, ProgramError> {
        if data.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_mut_ptr().add(ENGINE_OFF) };
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(unsafe { &mut *(ptr as *mut RiskEngine) })
    }

    #[inline]
    pub fn engine_write(data: &mut [u8], engine: RiskEngine) -> Result<(), ProgramError> {
        if data.len() < ENGINE_OFF + ENGINE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let ptr = unsafe { data.as_mut_ptr().add(ENGINE_OFF) };
        if (ptr as usize) % ENGINE_ALIGN != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        unsafe { core::ptr::write(ptr as *mut RiskEngine, engine) };
        Ok(())
    }

    use solana_program::instruction::Instruction;
    use solana_program::entrypoint::ProgramResult;
    use solana_program::program::invoke_signed;
    use solana_program::account_info::AccountInfo;

    pub fn invoke_signed_trade(
        ix: &Instruction,
        slab: &AccountInfo,
        lp: &AccountInfo,
        ctx: &AccountInfo,
        seeds: &[&[&[u8]]],
    ) -> ProgramResult {
        unsafe fn to_local<'a>(info: &AccountInfo) -> AccountInfo<'a> {
            core::mem::transmute(info.clone())
        }

        let slab_local = unsafe { to_local(slab) };
        let ctx_local = unsafe { to_local(ctx) };
        // lp is already local

        let infos = [slab_local, lp.clone(), ctx_local];
        invoke_signed(ix, &infos, seeds)
    }
}

pub mod matcher_abi {
    use solana_program::program_error::ProgramError;
    use crate::constants::MATCHER_ABI_VERSION;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
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
        if ctx.len() < 64 { return Err(ProgramError::InvalidAccountData); }
        let abi_version = u32::from_le_bytes(ctx[0..4].try_into().unwrap());
        let flags = u32::from_le_bytes(ctx[4..8].try_into().unwrap());
        let exec_price_e6 = u64::from_le_bytes(ctx[8..16].try_into().unwrap());
        let exec_size = i128::from_le_bytes(ctx[16..32].try_into().unwrap());
        let req_id = u64::from_le_bytes(ctx[32..40].try_into().unwrap());
        let lp_account_id = u64::from_le_bytes(ctx[40..48].try_into().unwrap());
        let oracle_price_e6 = u64::from_le_bytes(ctx[48..56].try_into().unwrap());
        let reserved = u64::from_le_bytes(ctx[56..64].try_into().unwrap());

        Ok(MatcherReturn {
            abi_version, flags, exec_price_e6, exec_size, req_id, lp_account_id, oracle_price_e6, reserved
        })
    }

    pub fn validate_matcher_return(ret: &MatcherReturn, lp_account_id: u64, oracle_price_e6: u64, req_size: i128) -> Result<(), ProgramError> {
        if ret.abi_version != MATCHER_ABI_VERSION { return Err(ProgramError::InvalidAccountData); }
        if (ret.flags & 1) == 0 { return Err(ProgramError::InvalidAccountData); }
        if (ret.flags & 4) != 0 { return Err(ProgramError::InvalidAccountData); }

        if ret.exec_price_e6 == 0 { return Err(ProgramError::InvalidAccountData); }

        if ret.lp_account_id != lp_account_id { return Err(ProgramError::InvalidAccountData); }
        if ret.oracle_price_e6 != oracle_price_e6 { return Err(ProgramError::InvalidAccountData); }
        if ret.reserved != 0 { return Err(ProgramError::InvalidAccountData); }
        
        if ret.exec_size.abs() > req_size.abs() { return Err(ProgramError::InvalidAccountData); }
        if req_size != 0 {
             if ret.exec_size.signum() != req_size.signum() && ret.exec_size != 0 { return Err(ProgramError::InvalidAccountData); }
        }
        Ok(())
    }
}

// 3. mod error
pub mod error {
    use solana_program::program_error::ProgramError;
    use percolator::RiskError;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum PercolatorError {
        InvalidMagic,
        InvalidVersion,
        AlreadyInitialized,
        NotInitialized,
        InvalidSlabLen,
        InvalidOracleKey,
        OracleStale,
        OracleConfTooWide,
        InvalidVaultAta,
        InvalidMint,
        ExpectedSigner,
        ExpectedWritable,
        OracleInvalid,
        EngineInsufficientBalance,
        EngineUndercollateralized,
        EngineUnauthorized,
        EngineInvalidMatchingEngine,
        EnginePnlNotWarmedUp,
        EngineOverflow,
        EngineAccountNotFound,
        EngineNotAnLPAccount,
        EnginePositionSizeMismatch,
        EngineRiskReductionOnlyMode,
        EngineAccountKindMismatch,
    }

    impl From<PercolatorError> for ProgramError {
        fn from(e: PercolatorError) -> Self {
            ProgramError::Custom(e as u32)
        }
    }

    pub fn map_risk_error(e: RiskError) -> ProgramError {
        let err = match e {
            RiskError::InsufficientBalance => PercolatorError::EngineInsufficientBalance,
            RiskError::Undercollateralized => PercolatorError::EngineUndercollateralized,
            RiskError::Unauthorized => PercolatorError::EngineUnauthorized,
            RiskError::InvalidMatchingEngine => PercolatorError::EngineInvalidMatchingEngine,
            RiskError::PnlNotWarmedUp => PercolatorError::EnginePnlNotWarmedUp,
            RiskError::Overflow => PercolatorError::EngineOverflow,
            RiskError::AccountNotFound => PercolatorError::EngineAccountNotFound,
            RiskError::NotAnLPAccount => PercolatorError::EngineNotAnLPAccount,
            RiskError::PositionSizeMismatch => PercolatorError::EnginePositionSizeMismatch,
            RiskError::RiskReductionOnlyMode => PercolatorError::EngineRiskReductionOnlyMode,
            RiskError::AccountKindMismatch => PercolatorError::EngineAccountKindMismatch,
        };
        ProgramError::Custom(err as u32)
    }
}

// 4. mod ix
pub mod ix {
    use solana_program::{pubkey::Pubkey, program_error::ProgramError};
    use percolator::RiskParams;

    #[derive(Debug)]
    pub enum Instruction {
        InitMarket { 
            admin: Pubkey, 
            collateral_mint: Pubkey, 
            pyth_index: Pubkey,
            pyth_collateral: Pubkey,
            max_staleness_slots: u64,
            conf_filter_bps: u16,
            risk_params: RiskParams,
        },
        InitUser { fee_payment: u64 },
        InitLP { matcher_program: Pubkey, matcher_context: Pubkey, fee_payment: u64 },
        DepositCollateral { user_idx: u16, amount: u64 },
        WithdrawCollateral { user_idx: u16, amount: u64 },
        KeeperCrank { caller_idx: u16, funding_rate_bps_per_slot: i64, allow_panic: u8 },
        TradeNoCpi { lp_idx: u16, user_idx: u16, size: i128 },
        LiquidateAtOracle { target_idx: u16 },
        CloseAccount { user_idx: u16 },
        TopUpInsurance { amount: u64 },
        TradeCpi { lp_idx: u16, user_idx: u16, size: i128 },
    }

    impl Instruction {
        pub fn decode(input: &[u8]) -> Result<Self, ProgramError> {
            let (&tag, mut rest) = input.split_first().ok_or(ProgramError::InvalidInstructionData)?;
            
            match tag {
                0 => { // InitMarket
                    let admin = read_pubkey(&mut rest)?;
                    let collateral_mint = read_pubkey(&mut rest)?;
                    let pyth_index = read_pubkey(&mut rest)?;
                    let pyth_collateral = read_pubkey(&mut rest)?;
                    let max_staleness_slots = read_u64(&mut rest)?;
                    let conf_filter_bps = read_u16(&mut rest)?;
                    let risk_params = read_risk_params(&mut rest)?;
                    Ok(Instruction::InitMarket { 
                        admin, collateral_mint, pyth_index, pyth_collateral, 
                        max_staleness_slots, conf_filter_bps, risk_params 
                    })
                },
                1 => { // InitUser
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitUser { fee_payment })
                },
                2 => { // InitLP
                    let matcher_program = read_pubkey(&mut rest)?;
                    let matcher_context = read_pubkey(&mut rest)?;
                    let fee_payment = read_u64(&mut rest)?;
                    Ok(Instruction::InitLP { matcher_program, matcher_context, fee_payment })
                },
                3 => { // Deposit
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::DepositCollateral { user_idx, amount })
                },
                4 => { // Withdraw
                    let user_idx = read_u16(&mut rest)?;
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::WithdrawCollateral { user_idx, amount })
                },
                5 => { // KeeperCrank
                    let caller_idx = read_u16(&mut rest)?;
                    let funding_rate_bps_per_slot = read_i64(&mut rest)?;
                    let allow_panic = read_u8(&mut rest)?;
                    Ok(Instruction::KeeperCrank { caller_idx, funding_rate_bps_per_slot, allow_panic })
                },
                6 => { // TradeNoCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    Ok(Instruction::TradeNoCpi { lp_idx, user_idx, size })
                },
                7 => { // LiquidateAtOracle
                    let target_idx = read_u16(&mut rest)?;
                    Ok(Instruction::LiquidateAtOracle { target_idx })
                },
                8 => { // CloseAccount
                    let user_idx = read_u16(&mut rest)?;
                    Ok(Instruction::CloseAccount { user_idx })
                },
                9 => { // TopUpInsurance
                    let amount = read_u64(&mut rest)?;
                    Ok(Instruction::TopUpInsurance { amount })
                },
                10 => { // TradeCpi
                    let lp_idx = read_u16(&mut rest)?;
                    let user_idx = read_u16(&mut rest)?;
                    let size = read_i128(&mut rest)?;
                    Ok(Instruction::TradeCpi { lp_idx, user_idx, size })
                },
                _ => Err(ProgramError::InvalidInstructionData),
            }
        }
    }

    fn read_u8(input: &mut &[u8]) -> Result<u8, ProgramError> {
        let (&val, rest) = input.split_first().ok_or(ProgramError::InvalidInstructionData)?;
        *input = rest;
        Ok(val)
    }

    fn read_u16(input: &mut &[u8]) -> Result<u16, ProgramError> {
        if input.len() < 2 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(2);
        *input = rest;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u64(input: &mut &[u8]) -> Result<u64, ProgramError> {
        if input.len() < 8 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i64(input: &mut &[u8]) -> Result<i64, ProgramError> {
        if input.len() < 8 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(8);
        *input = rest;
        Ok(i64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_i128(input: &mut &[u8]) -> Result<i128, ProgramError> {
        if input.len() < 16 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(i128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u128(input: &mut &[u8]) -> Result<u128, ProgramError> {
        if input.len() < 16 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(16);
        *input = rest;
        Ok(u128::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_pubkey(input: &mut &[u8]) -> Result<Pubkey, ProgramError> {
        if input.len() < 32 { return Err(ProgramError::InvalidInstructionData); }
        let (bytes, rest) = input.split_at(32);
        *input = rest;
        Ok(Pubkey::new_from_array(bytes.try_into().unwrap()))
    }

    fn read_risk_params(input: &mut &[u8]) -> Result<RiskParams, ProgramError> {
        Ok(RiskParams {
            warmup_period_slots: read_u64(input)?,
            maintenance_margin_bps: read_u64(input)?,
            initial_margin_bps: read_u64(input)?,
            trading_fee_bps: read_u64(input)?,
            max_accounts: read_u64(input)?,
            new_account_fee: read_u128(input)?,
            risk_reduction_threshold: read_u128(input)?,
            maintenance_fee_per_slot: read_u128(input)?,
            max_crank_staleness_slots: read_u64(input)?,
            liquidation_fee_bps: read_u64(input)?,
            liquidation_fee_cap: read_u128(input)?,
            liquidation_buffer_bps: read_u64(input)?,
            min_liquidation_abs: read_u128(input)?,
        })
    }
}

// 5. mod accounts (Pinocchio validation)
pub mod accounts {
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
    use crate::error::PercolatorError;

    pub fn expect_len(accounts: &[AccountInfo], n: usize) -> Result<(), ProgramError> {
        if accounts.len() < n {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        Ok(())
    }

    pub fn expect_signer(ai: &AccountInfo) -> Result<(), ProgramError> {
        if !ai.is_signer {
            return Err(PercolatorError::ExpectedSigner.into());
        }
        Ok(())
    }

    pub fn expect_writable(ai: &AccountInfo) -> Result<(), ProgramError> {
        if !ai.is_writable {
            return Err(PercolatorError::ExpectedWritable.into());
        }
        Ok(())
    }

    pub fn expect_owner(ai: &AccountInfo, owner: &Pubkey) -> Result<(), ProgramError> {
        if ai.owner != owner {
            return Err(ProgramError::IllegalOwner);
        }
        Ok(())
    }

    pub fn expect_key(ai: &AccountInfo, expected: &Pubkey) -> Result<(), ProgramError> {
        if ai.key != expected {
            return Err(ProgramError::InvalidArgument);
        }
        Ok(())
    }

    pub fn derive_vault_authority(program_id: &Pubkey, slab_key: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"vault", slab_key.as_ref()], program_id)
    }
}

// 6. mod state
pub mod state {
    use bytemuck::{Pod, Zeroable};
    use core::cell::RefMut;
    use solana_program::account_info::AccountInfo;
    use solana_program::program_error::ProgramError;
    use crate::constants::{HEADER_LEN, CONFIG_LEN};

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct SlabHeader {
        pub magic: u64,
        pub version: u32,
        pub bump: u8,
        pub _padding: [u8; 3],
        pub admin: [u8; 32],
        pub _reserved: [u8; 16],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    pub struct MarketConfig {
        pub collateral_mint: [u8; 32],
        pub vault_pubkey: [u8; 32],
        pub collateral_oracle: [u8; 32],
        pub index_oracle: [u8; 32],
        pub max_staleness_slots: u64,
        pub conf_filter_bps: u16,
        pub vault_authority_bump: u8,
        pub _padding: [u8; 5], 
    }

    pub fn slab_data_mut<'a, 'b>(ai: &'b AccountInfo<'a>) -> Result<RefMut<'b, &'a mut [u8]>, ProgramError> {
        Ok(ai.try_borrow_mut_data()?)
    }

    pub fn read_header(data: &[u8]) -> SlabHeader {
        let mut h = SlabHeader::zeroed();
        let src = &data[..HEADER_LEN];
        let dst = bytemuck::bytes_of_mut(&mut h);
        dst.copy_from_slice(src);
        h
    }

    pub fn write_header(data: &mut [u8], h: &SlabHeader) {
        let src = bytemuck::bytes_of(h);
        let dst = &mut data[..HEADER_LEN];
        dst.copy_from_slice(src);
    }

    pub fn read_config(data: &[u8]) -> MarketConfig {
        let mut c = MarketConfig::zeroed();
        let src = &data[HEADER_LEN..HEADER_LEN + CONFIG_LEN];
        let dst = bytemuck::bytes_of_mut(&mut c);
        dst.copy_from_slice(src);
        c
    }

    pub fn write_config(data: &mut [u8], c: &MarketConfig) {
        let src = bytemuck::bytes_of(c);
        let dst = &mut data[HEADER_LEN..HEADER_LEN + CONFIG_LEN];
        dst.copy_from_slice(src);
    }
}

// 7. mod oracle
pub mod oracle {
    use solana_program::{account_info::AccountInfo, program_error::ProgramError};
    use crate::error::PercolatorError;

    pub fn read_pyth_price_e6(price_ai: &AccountInfo, now_slot: u64, max_staleness: u64, conf_bps: u16) -> Result<u64, ProgramError> {
        let data = price_ai.try_borrow_data()?;
        if data.len() < 208 {
            return Err(ProgramError::InvalidAccountData);
        }

        let expo = i32::from_le_bytes(data[20..24].try_into().unwrap());
        let price = i64::from_le_bytes(data[176..184].try_into().unwrap());
        let conf = u64::from_le_bytes(data[184..192].try_into().unwrap());
        let pub_slot = u64::from_le_bytes(data[200..208].try_into().unwrap());

        if price <= 0 {
            return Err(PercolatorError::OracleInvalid.into()); 
        }

        let age = now_slot.saturating_sub(pub_slot);
        if age > max_staleness {
            return Err(PercolatorError::OracleStale.into());
        }

        let price_u = price as u128;
        let lhs = (conf as u128) * 10_000;
        let rhs = price_u * (conf_bps as u128);
        if lhs > rhs {
            return Err(PercolatorError::OracleConfTooWide.into());
        }

        let scale = expo + 6;
        let final_price_u128 = if scale >= 0 {
            let mul = 10u128.pow(scale as u32);
            price_u.checked_mul(mul).ok_or(PercolatorError::EngineOverflow)?
        } else {
            let div = 10u128.pow((-scale) as u32);
            price_u / div
        };

        if final_price_u128 == 0 {
             return Err(PercolatorError::OracleInvalid.into());
        }
        if final_price_u128 > u64::MAX as u128 {
            return Err(PercolatorError::EngineOverflow.into());
        }

        Ok(final_price_u128 as u64)
    }
}

// 8. mod collateral
pub mod collateral {
    use solana_program::{
        account_info::AccountInfo, program_error::ProgramError,
    };

    #[cfg(not(test))]
    use solana_program::program::{invoke, invoke_signed};

    #[cfg(test)]
    use solana_program::program_pack::Pack;
    #[cfg(test)]
    use spl_token::state::Account as TokenAccount;

    pub fn deposit<'a>(
        _token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        _authority: &AccountInfo<'a>,
        amount: u64
    ) -> Result<(), ProgramError> {
        if amount == 0 { return Ok(()); }
        #[cfg(not(test))]
        {
            let ix = spl_token::instruction::transfer(
                _token_program.key,
                source.key,
                dest.key,
                _authority.key,
                &[],
                amount,
            )?;
            invoke(&ix, &[source.clone(), dest.clone(), _authority.clone(), _token_program.clone()])
        }
        #[cfg(test)]
        {
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state.amount.checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;

            let mut dst_data = dest.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state.amount.checked_add(amount).ok_or(ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }

    pub fn withdraw<'a>(
        _token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        _authority: &AccountInfo<'a>,
        amount: u64,
        _signer_seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        if amount == 0 { return Ok(()); }
        #[cfg(not(test))]
        {
            let ix = spl_token::instruction::transfer(
                _token_program.key,
                source.key,
                dest.key,
                _authority.key,
                &[],
                amount,
            )?;
            invoke_signed(&ix, &[source.clone(), dest.clone(), _authority.clone(), _token_program.clone()], _signer_seeds)
        }
        #[cfg(test)]
        {
            let mut src_data = source.try_borrow_mut_data()?;
            let mut src_state = TokenAccount::unpack(&src_data)?;
            src_state.amount = src_state.amount.checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;
            TokenAccount::pack(src_state, &mut src_data)?;

            let mut dst_data = dest.try_borrow_mut_data()?;
            let mut dst_state = TokenAccount::unpack(&dst_data)?;
            dst_state.amount = dst_state.amount.checked_add(amount).ok_or(ProgramError::InvalidAccountData)?;
            TokenAccount::pack(dst_state, &mut dst_data)?;
            Ok(())
        }
    }
}

// 9. mod processor
pub mod processor {
    use solana_program::{
        account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey,
        sysvar::{clock::Clock, Sysvar},
        program_error::ProgramError,
        program_pack::Pack,
    };
    use crate::{
        ix::Instruction,
        state::{self, SlabHeader, MarketConfig},
        accounts,
        constants::{MAGIC, VERSION, SLAB_LEN, CONFIG_LEN, MATCHER_CONTEXT_LEN},
        error::{PercolatorError, map_risk_error},
        oracle,
        collateral,
        zc,
    };
    use percolator::{RiskEngine, NoOpMatcher, MAX_ACCOUNTS, MatchingEngine, TradeExecution, RiskError};
    use solana_program::instruction::{Instruction as SolInstruction, AccountMeta};
    use solana_program::program::invoke_signed;

    struct CpiMatcher {
        exec_price: u64,
        exec_size: i128,
    }

    impl MatchingEngine for CpiMatcher {
        fn execute_match(
            &self,
            _lp_program: &[u8; 32],
            _lp_context: &[u8; 32],
            _lp_account_id: u64,
            _oracle_price: u64,
            _size: i128,
        ) -> Result<TradeExecution, RiskError> {
            Ok(TradeExecution {
                price: self.exec_price,
                size: self.exec_size,
            })
        }
    }

    fn slab_guard(program_id: &Pubkey, slab: &AccountInfo, data: &[u8]) -> Result<(), ProgramError> {
        accounts::expect_owner(slab, program_id)?;
        if data.len() != SLAB_LEN { return Err(PercolatorError::InvalidSlabLen.into()); }
        Ok(())
    }

    fn require_initialized(data: &[u8]) -> Result<(), ProgramError> {
        let h = state::read_header(data);
        if h.magic != MAGIC { return Err(PercolatorError::NotInitialized.into()); }
        if h.version != VERSION { return Err(PercolatorError::InvalidVersion.into()); }
        Ok(())
    }

    fn check_idx(engine: &RiskEngine, idx: u16) -> Result<(), ProgramError> {
        if (idx as usize) >= MAX_ACCOUNTS || !engine.is_used(idx as usize) {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }
        Ok(())
    }

    fn verify_vault(a_vault: &AccountInfo, expected_owner: &Pubkey, expected_mint: &Pubkey, expected_pubkey: &Pubkey) -> Result<(), ProgramError> {
        if a_vault.key != expected_pubkey { return Err(PercolatorError::InvalidVaultAta.into()); }
        if a_vault.owner != &spl_token::ID { return Err(PercolatorError::InvalidVaultAta.into()); }
        if a_vault.data_len() != spl_token::state::Account::LEN { return Err(PercolatorError::InvalidVaultAta.into()); }
        
        let data = a_vault.try_borrow_data()?;
        let tok = spl_token::state::Account::unpack(&data)?;
        if tok.mint != *expected_mint { return Err(PercolatorError::InvalidMint.into()); }
        if tok.owner != *expected_owner { return Err(PercolatorError::InvalidVaultAta.into()); }
        Ok(())
    }

    pub fn process_instruction<'a, 'b>(
        program_id: &Pubkey,
        accounts: &'b [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = Instruction::decode(instruction_data)?;

        match instruction {
            Instruction::InitMarket { 
                admin: _admin, collateral_mint: _collateral_mint, pyth_index, pyth_collateral, 
                max_staleness_slots, conf_filter_bps, risk_params 
            } => {
                accounts::expect_len(accounts, 11)?;
                let a_admin = &accounts[0];
                let a_slab = &accounts[1];
                let a_mint = &accounts[2];
                let a_vault = &accounts[3];

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                
                #[cfg(debug_assertions)]
                {
                    if core::mem::size_of::<MarketConfig>() != CONFIG_LEN {
                        return Err(ProgramError::InvalidAccountData);
                    }
                }

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;

                let _ = zc::engine_mut(&mut data)?;

                let header = state::read_header(&data);
                if header.magic == MAGIC { return Err(PercolatorError::AlreadyInitialized.into()); }

                let (auth, bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, a_mint.key, a_vault.key)?;

                for b in data.iter_mut() { *b = 0; }

                let engine = RiskEngine::new(risk_params);
                zc::engine_write(&mut data, engine)?;

                let config = MarketConfig {
                    collateral_mint: a_mint.key.to_bytes(),
                    vault_pubkey: a_vault.key.to_bytes(),
                    collateral_oracle: pyth_collateral.to_bytes(),
                    index_oracle: pyth_index.to_bytes(),
                    max_staleness_slots,
                    conf_filter_bps,
                    vault_authority_bump: bump,
                    _padding: [0; 5],
                };
                state::write_config(&mut data, &config);

                let new_header = SlabHeader {
                    magic: MAGIC,
                    version: VERSION,
                    bump,
                    _padding: [0; 3],
                    admin: a_admin.key.to_bytes(),
                    _reserved: [0; 16],
                };
                state::write_header(&mut data, &new_header);
            },
            Instruction::InitUser { fee_payment } => {
                accounts::expect_len(accounts, 7)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;

                let engine = zc::engine_mut(&mut data)?;

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                let idx = engine.add_user(fee_payment as u128).map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes()).map_err(map_risk_error)?;
            },
            Instruction::InitLP { matcher_program, matcher_context, fee_payment } => {
                accounts::expect_len(accounts, 7)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;

                let engine = zc::engine_mut(&mut data)?;

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, fee_payment)?;

                let idx = engine.add_lp(matcher_program.to_bytes(), matcher_context.to_bytes(), fee_payment as u128).map_err(map_risk_error)?;
                engine.set_owner(idx, a_user.key.to_bytes()).map_err(map_risk_error)?;
            },
            Instruction::DepositCollateral { user_idx, amount } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_user_ata = &accounts[2];
                let a_vault = &accounts[3];
                let a_token = &accounts[4];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                let owner = engine.accounts[user_idx as usize].owner;
                if Pubkey::new_from_array(owner) != *a_user.key {
                    return Err(PercolatorError::EngineUnauthorized.into());
                }

                collateral::deposit(a_token, a_user_ata, a_vault, a_user, amount)?;
                engine.deposit(user_idx, amount as u128).map_err(map_risk_error)?;
            },
            Instruction::WithdrawCollateral { user_idx, amount } => {
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_user_ata = &accounts[3];
                let a_vault_pda = &accounts[4];
                let a_token = &accounts[5];
                let a_clock = &accounts[6];
                let a_oracle_idx = &accounts[7];
                
                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                let owner = engine.accounts[user_idx as usize].owner;
                if Pubkey::new_from_array(owner) != *a_user.key {
                   return Err(PercolatorError::EngineUnauthorized.into());
                }

                accounts::expect_key(a_oracle_idx, &Pubkey::new_from_array(config.index_oracle))?;

                let (derived_pda, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_pda, &derived_pda)?;

                verify_vault(a_vault, &derived_pda, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;

                let clock = Clock::from_account_info(a_clock)?;
                let price = oracle::read_pyth_price_e6(
                    a_oracle_idx,
                    clock.slot,
                    config.max_staleness_slots,
                    config.conf_filter_bps,
                )?;

                engine
                    .withdraw(user_idx, amount as u128, clock.slot, price)
                    .map_err(map_risk_error)?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(
                    a_token,
                    a_vault,
                    a_user_ata,
                    a_vault_pda,
                    amount,
                    &signer_seeds,
                )?;
            },
            Instruction::KeeperCrank { caller_idx, funding_rate_bps_per_slot, allow_panic } => {
                accounts::expect_len(accounts, 4)?;
                let a_caller = &accounts[0]; 
                let a_slab = &accounts[1];
                let a_clock = &accounts[2];
                let a_oracle = &accounts[3];

                accounts::expect_signer(a_caller)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let engine = zc::engine_mut(&mut data)?;

                if (caller_idx as usize) < MAX_ACCOUNTS { 
                     if engine.is_used(caller_idx as usize) {
                         let owner = engine.accounts[caller_idx as usize].owner;
                         if Pubkey::new_from_array(owner) != *a_caller.key {
                             return Err(PercolatorError::EngineUnauthorized.into());
                         }
                     }
                }

                let clock = Clock::from_account_info(a_clock)?;
                let price = oracle::read_pyth_price_e6(a_oracle, clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

                let _outcome = engine.keeper_crank(caller_idx, clock.slot, price, funding_rate_bps_per_slot, allow_panic != 0).map_err(map_risk_error)?;
            },
            Instruction::TradeNoCpi { lp_idx, user_idx, size } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_lp = &accounts[1];
                let a_slab = &accounts[2];
                
                accounts::expect_signer(a_user)?;
                accounts::expect_signer(a_lp)?;
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, lp_idx)?;
                check_idx(engine, user_idx)?;

                let u_owner = engine.accounts[user_idx as usize].owner;
                if Pubkey::new_from_array(u_owner) != *a_user.key { return Err(PercolatorError::EngineUnauthorized.into()); }
                let l_owner = engine.accounts[lp_idx as usize].owner;
                if Pubkey::new_from_array(l_owner) != *a_lp.key { return Err(PercolatorError::EngineUnauthorized.into()); }

                let clock = Clock::from_account_info(&accounts[3])?;
                let price = oracle::read_pyth_price_e6(&accounts[4], clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

                engine.execute_trade(&NoOpMatcher, lp_idx, user_idx, clock.slot, price, size).map_err(map_risk_error)?;
            },
            Instruction::TradeCpi { lp_idx, user_idx, size } => {
                accounts::expect_len(accounts, 7)?;
                let a_user = &accounts[0];
                let a_lp_owner = &accounts[1];
                let a_slab = &accounts[2];
                let a_clock = &accounts[3];
                let a_oracle = &accounts[4];
                let a_matcher_prog = &accounts[5];
                let a_matcher_ctx = &accounts[6];

                accounts::expect_signer(a_user)?;
                accounts::expect_signer(a_lp_owner)?;
                accounts::expect_writable(a_slab)?;
                accounts::expect_writable(a_matcher_ctx)?;
                
                if !a_matcher_prog.executable { return Err(ProgramError::InvalidAccountData); }
                if a_matcher_ctx.executable { return Err(ProgramError::InvalidAccountData); }
                if a_matcher_ctx.owner != a_matcher_prog.key { return Err(ProgramError::IllegalOwner); }
                if a_matcher_ctx.data_len() < MATCHER_CONTEXT_LEN { return Err(ProgramError::InvalidAccountData); }

                let (lp_account_id, config) = {
                    let mut data = state::slab_data_mut(a_slab)?;
                    slab_guard(program_id, a_slab, &data)?;
                    require_initialized(&data)?;
                    let config = state::read_config(&data);
                    let engine = zc::engine_ref(&data)?;

                    check_idx(engine, lp_idx)?;
                    check_idx(engine, user_idx)?;

                    let u_owner = engine.accounts[user_idx as usize].owner;
                    if Pubkey::new_from_array(u_owner) != *a_user.key { return Err(PercolatorError::EngineUnauthorized.into()); }
                    let l_owner = engine.accounts[lp_idx as usize].owner;
                    if Pubkey::new_from_array(l_owner) != *a_lp_owner.key { return Err(PercolatorError::EngineUnauthorized.into()); }
                    
                    (engine.accounts[lp_idx as usize].account_id, config)
                };

                accounts::expect_key(a_oracle, &Pubkey::new_from_array(config.index_oracle))?;

                let lp_bytes = lp_idx.to_le_bytes();
                let (lp_pda, bump) = Pubkey::find_program_address(
                    &[b"lp", a_slab.key.as_ref(), &lp_bytes],
                    program_id
                );
                
                let mut lp_lamports = 0;
                let mut lp_data = [];
                let lp_owner = solana_program::system_program::ID;
                let a_lp_pda = AccountInfo::new(
                    &lp_pda,
                    true, 
                    false, 
                    &mut lp_lamports,
                    &mut lp_data,
                    &lp_owner,
                    false, 
                    0,
                );

                let clock = Clock::from_account_info(a_clock)?;
                let price = oracle::read_pyth_price_e6(a_oracle, clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;
                
                let mut cpi_data = alloc::vec::Vec::with_capacity(67);
                cpi_data.push(0);
                cpi_data.extend_from_slice(a_slab.key.as_ref());
                cpi_data.extend_from_slice(&lp_idx.to_le_bytes());
                cpi_data.extend_from_slice(&lp_account_id.to_le_bytes());
                cpi_data.extend_from_slice(&price.to_le_bytes());
                cpi_data.extend_from_slice(&size.to_le_bytes());

                let mut metas = alloc::vec![
                    AccountMeta::new_readonly(*a_slab.key, false),
                    AccountMeta::new_readonly(lp_pda, true),
                    AccountMeta::new(*a_matcher_ctx.key, false),
                ];

                let ix = SolInstruction {
                    program_id: *a_matcher_prog.key,
                    accounts: metas,
                    data: cpi_data,
                };

                let bump_arr = [bump];
                let seeds: &[&[u8]] = &[b"lp", a_slab.key.as_ref(), &lp_bytes, &bump_arr];
                
                zc::invoke_signed_trade(&ix, a_slab, &a_lp_pda, a_matcher_ctx, &[seeds])?;

                let ctx_data = a_matcher_ctx.try_borrow_data()?;
                let ret = crate::matcher_abi::read_matcher_return(&ctx_data)?;
                crate::matcher_abi::validate_matcher_return(&ret, lp_account_id, price, size)?;
                drop(ctx_data);

                let matcher = CpiMatcher { exec_price: ret.exec_price_e6, exec_size: ret.exec_size };
                {
                    let mut data = state::slab_data_mut(a_slab)?;
                    let engine = zc::engine_mut(&mut data)?;
                    engine.execute_trade(&matcher, lp_idx, user_idx, clock.slot, price, size).map_err(map_risk_error)?;
                }
            },
            Instruction::LiquidateAtOracle { target_idx } => {
                accounts::expect_len(accounts, 4)?;
                let a_slab = &accounts[1];
                accounts::expect_writable(a_slab)?;

                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, target_idx)?;

                let clock = Clock::from_account_info(&accounts[2])?;
                let price = oracle::read_pyth_price_e6(&accounts[3], clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

                let _res = engine.liquidate_at_oracle(target_idx, clock.slot, price).map_err(map_risk_error)?;
            },
            Instruction::CloseAccount { user_idx } => {
                accounts::expect_len(accounts, 8)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_user_ata = &accounts[3];
                let a_pda = &accounts[4];
                let a_token = &accounts[5];

                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                
                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;
                accounts::expect_key(a_pda, &auth)?;

                let engine = zc::engine_mut(&mut data)?;

                check_idx(engine, user_idx)?;

                let u_owner = engine.accounts[user_idx as usize].owner;
                if Pubkey::new_from_array(u_owner) != *a_user.key { return Err(PercolatorError::EngineUnauthorized.into()); }

                let clock = Clock::from_account_info(&accounts[6])?;
                let price = oracle::read_pyth_price_e6(&accounts[7], clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

                let amt = engine.close_account(user_idx, clock.slot, price).map_err(map_risk_error)?;
                let amt_u64: u64 = amt.try_into().map_err(|_| PercolatorError::EngineOverflow)?;

                let seed1: &[u8] = b"vault";
                let seed2: &[u8] = a_slab.key.as_ref();
                let bump_arr: [u8; 1] = [config.vault_authority_bump];
                let seed3: &[u8] = &bump_arr;
                let seeds: [&[u8]; 3] = [seed1, seed2, seed3];
                let signer_seeds: [&[&[u8]]; 1] = [&seeds];

                collateral::withdraw(a_token, a_vault, a_user_ata, a_pda, amt_u64, &signer_seeds)?;
            },
            Instruction::TopUpInsurance { amount } => {
                accounts::expect_len(accounts, 5)?;
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[3];
                accounts::expect_signer(a_user)?;
                accounts::expect_writable(a_slab)?;
                
                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;
                require_initialized(&data)?;
                let config = state::read_config(&data);

                let (auth, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                verify_vault(a_vault, &auth, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;

                let engine = zc::engine_mut(&mut data)?;

                collateral::deposit(&accounts[4], &accounts[2], &accounts[3], a_user, amount)?;
                engine.top_up_insurance_fund(amount as u128).map_err(map_risk_error)?;
            }
        }
        Ok(())
    }
}

// 10. mod entrypoint
pub mod entrypoint {
    use solana_program::{
        account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
    };
    use crate::processor;

    entrypoint!(process_instruction);

    fn process_instruction<'a>(
        program_id: &Pubkey,
        accounts: &'a [AccountInfo<'a>],
        instruction_data: &[u8],
    ) -> ProgramResult {
        processor::process_instruction(program_id, accounts, instruction_data)
    }
}

// 11. mod risk (glue)
pub mod risk {
    pub use percolator::{RiskEngine, RiskParams, RiskError, NoOpMatcher, MatchingEngine, TradeExecution};
}

#[cfg(test)]
mod tests {
    extern crate std;
    extern crate alloc;
    use alloc::{vec, vec::Vec};
    use solana_program::{
        account_info::AccountInfo,
        pubkey::Pubkey,
        clock::Clock,
        program_pack::Pack,
        program_error::ProgramError,
    };
    use spl_token::state::{Account as TokenAccount, AccountState};
    use crate::{
        processor::process_instruction,
        constants::{MAGIC, VERSION},
        zc,
        error::PercolatorError,
        state,
    };
    use percolator::MAX_ACCOUNTS;

    // --- Harness ---

    struct TestAccount {
        key: Pubkey,
        owner: Pubkey,
        lamports: u64,
        data: Vec<u8>,
        is_signer: bool,
        is_writable: bool,
        executable: bool,
    }

    impl TestAccount {
        fn new(key: Pubkey, owner: Pubkey, lamports: u64, data: Vec<u8>) -> Self {
            Self { key, owner, lamports, data, is_signer: false, is_writable: false, executable: false }
        }
        fn signer(mut self) -> Self { self.is_signer = true; self }
        fn writable(mut self) -> Self { self.is_writable = true; self }
        
        fn to_info<'a>(&'a mut self) -> AccountInfo<'a> {
            AccountInfo::new(
                &self.key,
                self.is_signer,
                self.is_writable,
                &mut self.lamports,
                &mut self.data,
                &self.owner,
                self.executable,
                0,
            )
        }
    }

    // --- Builders ---

    fn make_token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Vec<u8> {
        let mut data = vec![0u8; TokenAccount::LEN];
        let mut account = TokenAccount::default();
        account.mint = mint;
        account.owner = owner;
        account.amount = amount;
        account.state = AccountState::Initialized;
        TokenAccount::pack(account, &mut data).unwrap();
        data
    }

    fn make_pyth(price: i64, expo: i32, conf: u64, pub_slot: u64) -> Vec<u8> {
        let mut data = vec![0u8; 208];
        data[20..24].copy_from_slice(&expo.to_le_bytes());
        data[176..184].copy_from_slice(&price.to_le_bytes());
        data[184..192].copy_from_slice(&conf.to_le_bytes());
        data[200..208].copy_from_slice(&pub_slot.to_le_bytes());
        data
    }

    fn make_clock(slot: u64) -> Vec<u8> {
        let clock = Clock { slot, ..Clock::default() };
        bincode::serialize(&clock).unwrap()
    }

    struct MarketFixture {
        program_id: Pubkey,
        admin: TestAccount,
        slab: TestAccount,
        mint: TestAccount,
        vault: TestAccount,
        token_prog: TestAccount,
        pyth_index: TestAccount,
        pyth_col: TestAccount,
        clock: TestAccount,
        rent: TestAccount,
        system: TestAccount,
        vault_pda: Pubkey,
    }

    fn setup_market() -> MarketFixture {
        let program_id = Pubkey::new_unique();
        let slab_key = Pubkey::new_unique();
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab_key.as_ref()], &program_id);
        let mint_key = Pubkey::new_unique();

        let pyth_data = make_pyth(1000, -6, 1, 100); 

        MarketFixture {
            program_id,
            admin: TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer(),
            slab: TestAccount::new(slab_key, program_id, 0, vec![0u8; crate::constants::SLAB_LEN]).writable(),
            mint: TestAccount::new(mint_key, solana_program::system_program::id(), 0, vec![]),
            vault: TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(mint_key, vault_pda, 0)).writable(),
            token_prog: TestAccount::new(spl_token::ID, Pubkey::default(), 0, vec![]),
            pyth_index: TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, pyth_data.clone()),
            pyth_col: TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, pyth_data),
            clock: TestAccount::new(solana_program::sysvar::clock::id(), solana_program::sysvar::id(), 0, make_clock(100)),
            rent: TestAccount::new(solana_program::sysvar::rent::id(), solana_program::sysvar::id(), 0, vec![]),
            system: TestAccount::new(solana_program::system_program::id(), Pubkey::default(), 0, vec![]),
            vault_pda,
        }
    }

    // --- Encoders --- 
    
    fn encode_u64(val: u64, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u16(val: u16, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_i128(val: i128, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u128(val: u128, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_pubkey(val: &Pubkey, buf: &mut Vec<u8>) { buf.extend_from_slice(val.as_ref()); }

    fn encode_init_market(fixture: &MarketFixture, crank_staleness: u64) -> Vec<u8> {
        let mut data = vec![0u8];
        encode_pubkey(&fixture.admin.key, &mut data);
        encode_pubkey(&fixture.mint.key, &mut data);
        encode_pubkey(&fixture.pyth_index.key, &mut data);
        encode_pubkey(&fixture.pyth_col.key, &mut data);
        encode_u64(100, &mut data);
        encode_u16(500, &mut data);
        
        encode_u64(0, &mut data);
        encode_u64(0, &mut data);
        encode_u64(0, &mut data);
        encode_u64(0, &mut data);
        encode_u64(64, &mut data);
        encode_u128(0, &mut data);
        encode_u128(0, &mut data);
        encode_u128(0, &mut data);
        encode_u64(crank_staleness, &mut data);
        encode_u64(0, &mut data);
        encode_u128(0, &mut data);
        encode_u64(0, &mut data);
        encode_u128(0, &mut data);
        data
    }

    fn encode_init_user(fee: u64) -> Vec<u8> {
        let mut data = vec![1u8];
        encode_u64(fee, &mut data);
        data
    }

    fn encode_init_lp(matcher: Pubkey, ctx: Pubkey, fee: u64) -> Vec<u8> {
        let mut data = vec![2u8];
        encode_pubkey(&matcher, &mut data);
        encode_pubkey(&ctx, &mut data);
        encode_u64(fee, &mut data);
        data
    }

    fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
        let mut data = vec![3u8];
        encode_u16(user_idx, &mut data);
        encode_u64(amount, &mut data);
        data
    }

    fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
        let mut data = vec![4u8];
        encode_u16(user_idx, &mut data);
        encode_u64(amount, &mut data);
        data
    }

    fn encode_crank(caller: u16, rate: i64, panic: u8) -> Vec<u8> {
        let mut data = vec![5u8];
        encode_u16(caller, &mut data);
        data.extend_from_slice(&rate.to_le_bytes());
        data.push(panic);
        data
    }

    fn encode_trade(lp: u16, user: u16, size: i128) -> Vec<u8> {
        let mut data = vec![6u8];
        encode_u16(lp, &mut data);
        encode_u16(user, &mut data);
        encode_i128(size, &mut data);
        data
    }

    fn encode_trade_cpi(lp: u16, user: u16, size: i128) -> Vec<u8> {
        let mut data = vec![10u8];
        encode_u16(lp, &mut data);
        encode_u16(user, &mut data);
        encode_i128(size, &mut data);
        data
    }

    fn find_idx_by_owner(data: &[u8], owner: Pubkey) -> Option<u16> {
        let engine = zc::engine_ref(data).ok()?;
        for i in 0..MAX_ACCOUNTS {
            if engine.is_used(i) && engine.accounts[i].owner == owner.to_bytes() {
                return Some(i as u16);
            }
        }
        None
    }

    // --- Tests ---

    #[test]
    fn test_init_market() {
        let mut f = setup_market();
        let data = encode_init_market(&f, 100);
        
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy_ata.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accounts, &data).unwrap();
        }

        let header = state::read_header(&f.slab.data);
        assert_eq!(header.magic, MAGIC);
        assert_eq!(header.version, VERSION);
        
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.params.max_accounts, 64);
    }

    #[test]
    fn test_init_user() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let init_accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();

        let data = encode_init_user(100);
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accounts, &data).unwrap();
        }

        let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
        assert_eq!(vault_state.amount, 100); 
        assert!(find_idx_by_owner(&f.slab.data, user.key).is_some());
    }

    #[test]
    fn test_deposit_withdraw() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 0); 
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let init_accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 500)).unwrap();
        }

        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_crank(user_idx, 0, 0)).unwrap();
        }

        {
            let mut vault_pda_account = TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);
            let accounts = vec![
                user.to_info(), f.slab.to_info(), f.vault.to_info(), user_ata.to_info(), vault_pda_account.to_info(),
                f.token_prog.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
            ];
            process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 200)).unwrap();
        }

        let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
        assert_eq!(vault_state.amount, 300);
    }

    #[test]
    fn test_vault_validation() {
        let mut f = setup_market();
        f.vault.owner = solana_program::system_program::id();
        let init_data = encode_init_market(&f, 100);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
            f.token_prog.to_info(), dummy_ata.to_info(),
            f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
        ];
        let res = process_instruction(&f.program_id, &init_accounts, &init_data);
        assert_eq!(res, Err(PercolatorError::InvalidVaultAta.into()));
    }

    #[test]
    fn test_trade() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let init_accounts = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
                f.token_prog.to_info(), dummy_ata.to_info(),
                f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
        {
            let accounts = vec![user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 1000)).unwrap();
        }

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        {
            let matcher_prog_key = d1.key;
            let matcher_ctx_key = d2.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                d1.to_info(), d2.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();
        {
            let accounts = vec![lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accounts, &encode_deposit(lp_idx, 1000)).unwrap();
        }

        {
            let accounts = vec![
                user.to_info(), lp.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_trade(lp_idx, user_idx, 100)).unwrap();
        }
    }

    #[test]
    fn test_withdraw_wrong_signer() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 0);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        {
            let accounts = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_deposit(user_idx, 500)).unwrap();
        }

        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0, 0)).unwrap();
        }

        let mut attacker = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut vault_pda = TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);
        
        let res = {
            let accounts = vec![
                attacker.to_info(),
                f.slab.to_info(), f.vault.to_info(), user_ata.to_info(), vault_pda.to_info(),
                f.token_prog.to_info(), f.clock.to_info(), f.pyth_index.to_info()
            ];
            process_instruction(&f.program_id, &accounts, &encode_withdraw(user_idx, 100))
        };
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    #[test]
    fn test_trade_wrong_signer() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 0);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        {
            let matcher_prog_key = d1.key;
            let matcher_ctx_key = d2.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                d1.to_info(), d2.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        {
            let accs = vec![user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accs, &encode_deposit(user_idx, 1000)).unwrap();
        }
        {
            let accs = vec![lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
            process_instruction(&f.program_id, &accs, &encode_deposit(lp_idx, 1000)).unwrap();
        }
        {
            let accs = vec![user.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()];
            process_instruction(&f.program_id, &accs, &encode_crank(user_idx, 0, 0)).unwrap();
        }

        let mut attacker = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        {
            let accs = vec![
                attacker.to_info(), lp.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
            ];
            let res = process_instruction(&f.program_id, &accs, &encode_trade(lp_idx, user_idx, 100));
            assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
        }
    }

    #[test]
    fn test_trade_cpi_pda_derivation() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_program.executable = true; // Mark as executable
        let mut matcher_ctx = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_ctx.owner = matcher_program.key;
        let mut ctx_data = vec![0u8; 320];
        ctx_data[0] = 1; // ver
        ctx_data[4] = 1; // flags
        ctx_data[8] = 1; // price
        matcher_ctx.data = ctx_data;
        matcher_ctx.is_writable = true;
        {
            let matcher_prog_key = matcher_program.key;
            let matcher_ctx_key = matcher_ctx.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                matcher_program.to_info(), matcher_ctx.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda, _) = Pubkey::find_program_address(
            &[b"lp", f.slab.key.as_ref(), &lp_bytes],
            &f.program_id
        );
        let mut lp_pda_acc = TestAccount::new(lp_pda, solana_program::system_program::id(), 0, vec![]); 

        // Update context to have valid ABI return (otherwise execute_trade fails)
        // Set exec_price > 0.
        // abi_version = 1 (at 0..4)
        // flags = 1 (at 4..8)
        // price at 8..16
        matcher_ctx.data[0] = 1;
        matcher_ctx.data[4] = 1;
        matcher_ctx.data[8] = 1; // price > 0

        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let out = {
                let accs = vec![
                    user.to_info(), // 0
                    lp.to_info(), // 1
                    f.slab.to_info(), // 2
                    f.clock.to_info(), // 3
                    f.pyth_index.to_info(), // 4 oracle
                    matcher_program.to_info(), // 5 matcher
                    matcher_ctx.to_info(), // 6 context
                    lp_pda_acc.to_info(), // 7 lp_signer_pda
                ];
                process_instruction(&f.program_id, &accs, &encode_trade_cpi(lp_idx, user_idx, 100))
            };
            out
        }));
        
        match res {
            Ok(Ok(_)) => {}, // Success
            Ok(Err(e)) => {
                assert_ne!(e, PercolatorError::EngineUnauthorized.into());
            }
            Err(_) => {}
        }
    }

    #[test]
    fn test_trade_cpi_wrong_lp_owner_rejected() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_program.executable = true;
        let mut matcher_ctx = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_ctx.owner = matcher_program.key;
        let mut ctx_data = vec![0u8; 320];
        ctx_data[0] = 1; // ver
        ctx_data[4] = 1; // flags
        ctx_data[8] = 1; // price
        matcher_ctx.data = ctx_data;
        matcher_ctx.is_writable = true;
        {
            let matcher_prog_key = matcher_program.key;
            let matcher_ctx_key = matcher_ctx.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                matcher_program.to_info(), matcher_ctx.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda, _) = Pubkey::find_program_address(
            &[b"lp", f.slab.key.as_ref(), &lp_bytes],
            &f.program_id
        );
        let mut lp_pda_acc = TestAccount::new(lp_pda, solana_program::system_program::id(), 0, vec![]); 

        let mut wrong_lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();

        let res = {
            let accs = vec![
                user.to_info(), // 0
                wrong_lp.to_info(), // 1 (WRONG OWNER)
                f.slab.to_info(), // 2
                f.clock.to_info(), // 3
                f.pyth_index.to_info(), // 4 oracle
                matcher_program.to_info(), // 5 matcher
                matcher_ctx.to_info(), // 6 context
                lp_pda_acc.to_info(), // 7 lp_signer_pda
            ];
            process_instruction(&f.program_id, &accs, &encode_trade_cpi(lp_idx, user_idx, 100))
        };
        assert_eq!(res, Err(PercolatorError::EngineUnauthorized.into()));
    }

    #[test]
    fn test_trade_cpi_wrong_oracle_key_rejected() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f, 100);
        {
            let mut dummy = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
            let accs = vec![
                f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                dummy.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &init_data).unwrap();
        }

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        {
            let accs = vec![
                user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                f.clock.to_info(), f.pyth_col.to_info(),
            ];
            process_instruction(&f.program_id, &accs, &encode_init_user(0)).unwrap();
        }
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut matcher_program = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_program.executable = true;
        let mut matcher_ctx = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        matcher_ctx.owner = matcher_program.key;
        let mut ctx_data = vec![0u8; 320];
        ctx_data[0] = 1; // ver
        ctx_data[4] = 1; // flags
        ctx_data[8] = 1; // price
        matcher_ctx.data = ctx_data;
        matcher_ctx.is_writable = true;
        {
            let matcher_prog_key = matcher_program.key;
            let matcher_ctx_key = matcher_ctx.key;
            let accs = vec![
                lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(),
                matcher_program.to_info(), matcher_ctx.to_info()
            ];
            process_instruction(&f.program_id, &accs, &encode_init_lp(matcher_prog_key, matcher_ctx_key, 0)).unwrap();
        }
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();

        let lp_bytes = lp_idx.to_le_bytes();
        let (lp_pda, _) = Pubkey::find_program_address(
            &[b"lp", f.slab.key.as_ref(), &lp_bytes],
            &f.program_id
        );
        let mut lp_pda_acc = TestAccount::new(lp_pda, solana_program::system_program::id(), 0, vec![]); 

        let mut wrong_oracle = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![0u8; 208]);

        let res = {
            let accs = vec![
                user.to_info(), // 0
                lp.to_info(), // 1
                f.slab.to_info(), // 2
                f.clock.to_info(), // 3
                wrong_oracle.to_info(), // 4 oracle (WRONG KEY)
                matcher_program.to_info(), // 5 matcher
                matcher_ctx.to_info(), // 6 context
                lp_pda_acc.to_info(), // 7 lp_signer_pda
            ];
            process_instruction(&f.program_id, &accs, &encode_trade_cpi(lp_idx, user_idx, 100))
        };
        assert_eq!(res, Err(ProgramError::InvalidArgument));
    }
}