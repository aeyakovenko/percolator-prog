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
    // Layout: tag(1) + decimals(1) + mint_authority(32) + freeze_option(1) [+ freeze_authority(32)]
    let mut data = [0u8; 67];
    data[0] = IX_INITIALIZE_MINT;
    data[1] = decimals;
    data[2..34].copy_from_slice(mint_authority.as_ref());
    match freeze_authority {
        Some(auth) => {
            data[34] = 1;
            data[35..67].copy_from_slice(auth.as_ref());
        }
        None => {} // data[34] = 0 already
    }
    let mut accounts = Vec::with_capacity(2);
    accounts.push(AccountMeta::new(*mint, false));
    accounts.push(AccountMeta::new_readonly(
        solana_program::sysvar::rent::id(),
        false,
    ));
    Ok(Instruction {
        program_id: id(),
        accounts,
        data: data.to_vec(),
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

    // Re-export AccountState from pinocchio-token (same repr as spl_token::state::AccountState)
    pub use pinocchio_token::state::AccountState;

    // ── Mint (82 bytes) ──────────────────────────────────────────────────────
    // Offset map (matches spl-token 6.0 and pinocchio-token 0.5.0):
    //   [0..4]   mint_authority_option (u32 LE: 0 = absent, 1 = present)
    //   [4..36]  mint_authority (Pubkey, 32 bytes)
    //   [36..44] supply (u64 LE)
    //   [44]     decimals (u8)
    //   [45]     is_initialized (u8: 0 = false, 1 = true)
    //   [46..50] freeze_authority_option (u32 LE)
    //   [50..82] freeze_authority (Pubkey)

    /// `spl_token::state::Mint::LEN = 82`
    pub const MINT_LEN: usize = 82;

    /// Parsed subset of Mint fields needed by percolator-prog.
    pub struct MintView {
        pub is_initialized: bool,
        pub decimals: u8,
        pub supply: u64,
    }

    impl MintView {
        /// Parse from raw account data (equivalent to `spl_token::state::Mint::unpack`).
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

    /// Write `supply` into a raw mint buffer (offset 36..44).
    pub fn set_mint_supply(buf: &mut [u8], supply: u64) -> Result<(), ProgramError> {
        if buf.len() < MINT_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        buf[36..44].copy_from_slice(&supply.to_le_bytes());
        Ok(())
    }

    /// Read `supply` from a raw mint buffer (offset 36..44).
    pub fn get_mint_supply(data: &[u8]) -> Result<u64, ProgramError> {
        if data.len() < MINT_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(u64::from_le_bytes(
            data[36..44]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
        ))
    }

    /// Write a freshly-initialized Mint to `buf` (test-mode equivalent of `Mint::pack`).
    pub fn pack_mint(
        buf: &mut [u8],
        is_initialized: bool,
        decimals: u8,
        supply: u64,
        mint_authority: Option<&[u8; 32]>,
        freeze_authority: Option<&[u8; 32]>,
    ) -> Result<(), ProgramError> {
        if buf.len() < MINT_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        buf[..MINT_LEN].fill(0);
        match mint_authority {
            Some(k) => {
                buf[0..4].copy_from_slice(&1u32.to_le_bytes());
                buf[4..36].copy_from_slice(k);
            }
            None => buf[0..4].copy_from_slice(&0u32.to_le_bytes()),
        }
        buf[36..44].copy_from_slice(&supply.to_le_bytes());
        buf[44] = decimals;
        buf[45] = is_initialized as u8;
        match freeze_authority {
            Some(k) => {
                buf[46..50].copy_from_slice(&1u32.to_le_bytes());
                buf[50..82].copy_from_slice(k);
            }
            None => buf[46..50].copy_from_slice(&0u32.to_le_bytes()),
        }
        Ok(())
    }

    // ── TokenAccount (165 bytes) ──────────────────────────────────────────────
    // Offset map (matches spl-token 6.0 and pinocchio-token 0.5.0):
    //   [0..32]  mint (Pubkey)
    //   [32..64] owner (Pubkey)
    //   [64..72] amount (u64 LE)
    //   [72..76] delegate_option (u32 LE)
    //   [76..108] delegate (Pubkey)
    //   [108]    state (u8: 0=uninitialized, 1=initialized, 2=frozen)
    //   ... (remaining fields not needed by percolator-prog)

    /// `spl_token::state::Account::LEN = 165`
    pub const ACCOUNT_LEN: usize = 165;

    /// Parsed subset of TokenAccount fields needed by percolator-prog.
    pub struct TokenAccountView {
        pub mint: Pubkey,
        pub owner: Pubkey,
        pub amount: u64,
        pub state: AccountState,
    }

    impl TokenAccountView {
        /// Parse from raw account data (equivalent to `spl_token::state::Account::unpack`).
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
            // AccountState: 0=uninitialized, 1=initialized, 2=frozen
            // pinocchio_token::state::AccountState has the same discriminants.
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

    /// Read `amount` from a raw token-account buffer (offset 64..72).
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

    /// Write `amount` into a raw token-account buffer (offset 64..72).
    pub fn set_token_account_amount(buf: &mut [u8], amount: u64) -> Result<(), ProgramError> {
        if buf.len() < ACCOUNT_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        buf[64..72].copy_from_slice(&amount.to_le_bytes());
        Ok(())
    }
}
