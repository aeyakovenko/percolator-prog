#![no_std]
#![deny(unsafe_code)]

//! Percolator: Single-file Solana program with embedded Risk Engine.

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    pubkey::Pubkey,
    program_error::ProgramError,
    msg,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};

// 1. mod constants
pub mod constants {
    use core::mem::{size_of, align_of};
    use crate::state::{SlabHeader, MarketConfig};
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
        // Engine errors mapped:
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
    use crate::constants::{HEADER_LEN, CONFIG_LEN, ENGINE_OFF, ENGINE_LEN};

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
    
    pub fn engine_region_mut(data: &mut [u8]) -> &mut [u8] {
        &mut data[ENGINE_OFF..ENGINE_OFF + ENGINE_LEN]
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

        // Check confidence: conf * 10000 <= price * conf_bps
        let price_u = price as u128;
        let lhs = (conf as u128) * 10_000;
        let rhs = price_u * (conf_bps as u128);
        if lhs > rhs {
            return Err(PercolatorError::OracleConfTooWide.into());
        }

        // Convert to E6
        // price_e6 = price * 10^(expo + 6)
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
        account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey,
        program::{invoke, invoke_signed},
    };

    pub fn deposit<'a>(
        token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64
    ) -> Result<(), ProgramError> {
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            dest.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke(&ix, &[source.clone(), dest.clone(), authority.clone(), token_program.clone()])
    }

    pub fn withdraw<'a>(
        token_program: &AccountInfo<'a>,
        source: &AccountInfo<'a>,
        dest: &AccountInfo<'a>,
        authority: &AccountInfo<'a>,
        amount: u64,
        signer_seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            dest.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke_signed(&ix, &[source.clone(), dest.clone(), authority.clone(), token_program.clone()], signer_seeds)
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
        constants::{MAGIC, VERSION, SLAB_LEN},
        error::{PercolatorError, map_risk_error},
        oracle,
        collateral,
        zc,
    };
    use percolator::{RiskEngine, NoOpMatcher, MAX_ACCOUNTS};

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
        // Vault Key Check
        if a_vault.key != expected_pubkey {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        // Owner Check
        if a_vault.owner != &spl_token::ID {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        // Len Check
        if a_vault.data_len() != spl_token::state::Account::LEN {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
        // Content Check
        let data = a_vault.try_borrow_data()?;
        let mint = Pubkey::new_from_array(data[0..32].try_into().unwrap());
        let owner = Pubkey::new_from_array(data[32..64].try_into().unwrap());
        if mint != *expected_mint {
            return Err(PercolatorError::InvalidMint.into());
        }
        if owner != *expected_owner {
            return Err(PercolatorError::InvalidVaultAta.into());
        }
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
                // 4 token, 5 ata, 6 system, 7 rent, 8 pyth_idx, 9 pyth_col, 10 clock

                accounts::expect_signer(a_admin)?;
                accounts::expect_writable(a_slab)?;
                
                // Guard (checks owner and length)
                let mut data = state::slab_data_mut(a_slab)?;
                slab_guard(program_id, a_slab, &data)?;

                // Attempt align check via zero-copy borrow
                let _ = zc::engine_mut(&mut data)?;

                let header = state::read_header(&data);
                if header.magic == MAGIC {
                    return Err(PercolatorError::AlreadyInitialized.into());
                }

                // Verify vault
                let (auth, bump) = accounts::derive_vault_authority(program_id, a_slab.key);
                
                // Vault MUST exist and be valid now
                verify_vault(a_vault, &auth, a_mint.key, a_vault.key)?;

                // Zero engine region before writing
                let engine_region = state::engine_region_mut(&mut data);
                for b in engine_region.iter_mut() { *b = 0; }

                // Create fresh engine (on stack) and write BY VALUE
                let engine = RiskEngine::new(risk_params);
                zc::engine_write(&mut data, engine)?;

                // Initialize Config
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

                // Initialize Header
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

                // Verify vault
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

                verify_vault(a_vault, &Pubkey::new_from_array(config.vault_pubkey), &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;
                // Wait, verify_vault 2nd arg is owner. config.vault_pubkey is wrong. Need derived PDA.
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

                // Verify auth
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

                // Verify oracle pubkey matches config
                accounts::expect_key(a_oracle_idx, &Pubkey::new_from_array(config.index_oracle))?;

                // Verify vault authority PDA
                let (derived_pda, _) = accounts::derive_vault_authority(program_id, a_slab.key);
                accounts::expect_key(a_vault_pda, &derived_pda)?;

                // Verify vault
                verify_vault(a_vault, &derived_pda, &Pubkey::new_from_array(config.collateral_mint), &Pubkey::new_from_array(config.vault_pubkey))?;

                // Clock + oracle price
                let clock = Clock::from_account_info(a_clock)?;
                let price = oracle::read_pyth_price_e6(
                    a_oracle_idx,
                    clock.slot,
                    config.max_staleness_slots,
                    config.conf_filter_bps,
                )?;

                // Engine withdrawal
                engine
                    .withdraw(user_idx, amount as u128, clock.slot, price)
                    .map_err(map_risk_error)?;

                // Transfer tokens
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

                // Verify caller ownership if caller_idx valid
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

                // Verify owners
                let u_owner = engine.accounts[user_idx as usize].owner;
                if Pubkey::new_from_array(u_owner) != *a_user.key { return Err(PercolatorError::EngineUnauthorized.into()); }
                let l_owner = engine.accounts[lp_idx as usize].owner;
                if Pubkey::new_from_array(l_owner) != *a_lp.key { return Err(PercolatorError::EngineUnauthorized.into()); }

                let clock = Clock::from_account_info(&accounts[3])?;
                let price = oracle::read_pyth_price_e6(&accounts[4], clock.slot, config.max_staleness_slots, config.conf_filter_bps)?;

                engine.execute_trade(&NoOpMatcher, lp_idx, user_idx, clock.slot, price, size).map_err(map_risk_error)?;
            },
            Instruction::LiquidateAtOracle { target_idx } => {
                let a_slab = &accounts[1];
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
                // 0 user, 1 slab, 2 vault, 3 user_ata, 4 pda, 5 token, 6 clock, 7 oracle
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[2];
                let a_user_ata = &accounts[3];
                let a_pda = &accounts[4];
                let a_token = &accounts[5];

                accounts::expect_signer(a_user)?;
                
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
                // 0 user, 1 slab, 2 user_ata, 3 vault, 4 token
                let a_user = &accounts[0];
                let a_slab = &accounts[1];
                let a_vault = &accounts[3];
                accounts::expect_signer(a_user)?;
                
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