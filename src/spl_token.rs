//! Minimal SPL Token helpers — replaces spl-token 6.0 crate dependency.
//!
//! Build instructions via raw wire format and parse state via direct byte reads
//! (same layout as spl-token and pinocchio-token). No unsafe code required since
//! we read individual fields with slice indexing.
//!
//! Wire format is stable: SPL Token is a frozen deployed program.

extern crate alloc;
use alloc::vec::Vec;

use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};

/// SPL Token program ID.
#[inline(always)]
pub fn id() -> Pubkey {
    solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
}

// ─── Instruction tags ────────────────────────────────────────────────────────

const IX_INITIALIZE_MINT: u8 = 0;
const IX_TRANSFER: u8 = 3;
const IX_MINT_TO: u8 = 7;
const IX_BURN: u8 = 8;

// ─── CPI instruction builders ────────────────────────────────────────────────

/// `InitializeMint` (tag 0).  Accounts: [WRITE] mint, [RO] Rent sysvar.
pub fn initialize_mint(
    _program_id: &Pubkey,
    mint: &Pubkey,
    mint_authority: &Pubkey,
    freeze_authority: Option<&Pubkey>,
    decimals: u8,
) -> Result<Instruction, ProgramError> {
    let data = match freeze_authority {
        None => {
            let mut d = [0u8; 35];
            d[0] = IX_INITIALIZE_MINT;
            d[1] = decimals;
            d[2..34].copy_from_slice(mint_authority.as_ref());
            d.to_vec()
        }
        Some(auth) => {
            let mut d = [0u8; 67];
            d[0] = IX_INITIALIZE_MINT;
            d[1] = decimals;
            d[2..34].copy_from_slice(mint_authority.as_ref());
            d[34] = 1;
            d[35..67].copy_from_slice(auth.as_ref());
            d.to_vec()
        }
    };
    let mut accounts = Vec::with_capacity(2);
    accounts.push(AccountMeta::new(*mint, false));
    accounts.push(AccountMeta::new_readonly(
        solana_program::sysvar::rent::id(),
        false,
    ));
    Ok(Instruction {
        program_id: id(),
        accounts,
        data,
    })
}

/// `Transfer` (tag 3).  Accounts: [WRITE] source, [WRITE] dest, [SIGNER] authority.
pub fn transfer(
    _program_id: &Pubkey,
    source: &Pubkey,
    dest: &Pubkey,
    authority: &Pubkey,
    _multisigners: &[&Pubkey],
    amount: u64,
) -> Result<Instruction, ProgramError> {
    let mut data = [0u8; 9];
    data[0] = IX_TRANSFER;
    data[1..9].copy_from_slice(&amount.to_le_bytes());
    let mut accounts = Vec::with_capacity(3);
    accounts.push(AccountMeta::new(*source, false));
    accounts.push(AccountMeta::new(*dest, false));
    accounts.push(AccountMeta::new_readonly(*authority, true));
    Ok(Instruction {
        program_id: id(),
        accounts,
        data: data.to_vec(),
    })
}

/// `MintTo` (tag 7).  Accounts: [WRITE] mint, [WRITE] destination, [SIGNER] authority.
pub fn mint_to(
    _program_id: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    _multisigners: &[&Pubkey],
    amount: u64,
) -> Result<Instruction, ProgramError> {
    let mut data = [0u8; 9];
    data[0] = IX_MINT_TO;
    data[1..9].copy_from_slice(&amount.to_le_bytes());
    let mut accounts = Vec::with_capacity(3);
    accounts.push(AccountMeta::new(*mint, false));
    accounts.push(AccountMeta::new(*destination, false));
    accounts.push(AccountMeta::new_readonly(*authority, true));
    Ok(Instruction {
        program_id: id(),
        accounts,
        data: data.to_vec(),
    })
}

/// `Burn` (tag 8).  Accounts: [WRITE] account, [WRITE] mint, [SIGNER] authority.
pub fn burn(
    _program_id: &Pubkey,
    account: &Pubkey,
    mint: &Pubkey,
    authority: &Pubkey,
    _multisigners: &[&Pubkey],
    amount: u64,
) -> Result<Instruction, ProgramError> {
    let mut data = [0u8; 9];
    data[0] = IX_BURN;
    data[1..9].copy_from_slice(&amount.to_le_bytes());
    let mut accounts = Vec::with_capacity(3);
    accounts.push(AccountMeta::new(*account, false));
    accounts.push(AccountMeta::new(*mint, false));
    accounts.push(AccountMeta::new_readonly(*authority, true));
    Ok(Instruction {
        program_id: id(),
        accounts,
        data: data.to_vec(),
    })
}

// ─── State types (raw byte layout — same as spl-token and pinocchio-token) ───

#[allow(dead_code)]
pub mod state {
    use super::*;

    /// SPL Token account state (mirrors spl_token::state::AccountState).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum AccountState {
        Uninitialized,
        Initialized,
        Frozen,
    }

    pub const MINT_LEN: usize = 82;

    pub struct MintView {
        pub is_initialized: bool,
        pub decimals: u8,
        pub supply: u64,
    }

    impl MintView {
        pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
            if data.len() < MINT_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            Ok(Self {
                is_initialized: data[45] != 0,
                decimals: data[44],
                supply: u64::from_le_bytes(
                    data[36..44]
                        .try_into()
                        .map_err(|_| ProgramError::InvalidAccountData)?,
                ),
            })
        }
    }

    pub fn get_token_account_amount(data: &[u8]) -> Result<u64, ProgramError> {
        if data.len() < ACCOUNT_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(u64::from_le_bytes(
            data[64..72]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
        ))
    }

    pub const ACCOUNT_LEN: usize = 165;

    pub struct TokenAccountView {
        pub mint: Pubkey,
        pub owner: Pubkey,
        pub amount: u64,
        pub state: AccountState,
    }

    impl TokenAccountView {
        pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
            if data.len() < ACCOUNT_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let mint = Pubkey::new_from_array(
                data[0..32]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)?,
            );
            let owner = Pubkey::new_from_array(
                data[32..64]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)?,
            );
            let amount = u64::from_le_bytes(
                data[64..72]
                    .try_into()
                    .map_err(|_| ProgramError::InvalidAccountData)?,
            );
            let state = match data[108] {
                0 => AccountState::Uninitialized,
                1 => AccountState::Initialized,
                _ => AccountState::Frozen,
            };
            Ok(Self {
                mint,
                owner,
                amount,
                state,
            })
        }
    }
}
