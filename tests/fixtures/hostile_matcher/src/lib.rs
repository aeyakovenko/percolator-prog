//! Adversarial matcher for end-to-end testing of the wrapper's validate_matcher_return + the batch
//! CPI return-data plumbing. On tag 3 (batch call) it returns CRAFTED returns; the attack "mode" is
//! read from ctx_account.data[0] (set directly by the test). The wrapper MUST reject every hostile
//! mode and accept only the honest one.
#![allow(unexpected_cfgs)]
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, program::set_return_data,
    program_error::ProgramError, pubkey::Pubkey,
};

entrypoint!(process);

const ABI: u32 = 3;
const FLAG_VALID: u32 = 1;

fn process(_pid: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 18 || data[0] != 3 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let n = data[1] as usize;
    if n == 0 || n > 16 || data.len() != 18 + n * 26 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let req_id = u64::from_le_bytes(data[2..10].try_into().unwrap());
    let lp = u64::from_le_bytes(data[10..18].try_into().unwrap());
    let mode = accounts[1].try_borrow_data()?[0];

    let mut out = [0u8; 16 * 64];
    let mut emit = n;
    for i in 0..n {
        let base = 18 + i * 26;
        let asset = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as u64;
        let oracle = u64::from_le_bytes(data[base + 2..base + 10].try_into().unwrap());
        let req = i128::from_le_bytes(data[base + 10..base + 26].try_into().unwrap());

        // honest defaults (this validates); each mode perturbs ONE field.
        let mut flags = FLAG_VALID;
        let mut price = oracle;
        let mut size = req;
        let mut a = asset;
        let mut o = oracle;
        let mut rid = req_id;
        let mut l = lp;
        match mode {
            0 => size = req.saturating_mul(2),            // over-fill: open 2x the requested position
            1 => size = req.checked_neg().unwrap_or(0),   // reversed direction
            2 => a = asset.wrapping_add(1),               // forged asset echo
            3 => o = oracle.wrapping_add(1),              // forged oracle echo
            4 => rid = req_id.wrapping_add(1),            // forged req_id
            5 => l = lp.wrapping_add(1),                  // forged lp_account_id
            6 => price = 0,                               // zero exec price
            7 => { flags = FLAG_VALID; size = req / 2 }   // unflagged partial (no PARTIAL_OK)
            8 => emit = n.saturating_sub(1),              // short return length (n-1 records)
            _ => {}                                       // honest full fill -> wrapper accepts
        }
        let b = &mut out[i * 64..];
        b[0..4].copy_from_slice(&ABI.to_le_bytes());
        b[4..8].copy_from_slice(&flags.to_le_bytes());
        b[8..16].copy_from_slice(&price.to_le_bytes());
        b[16..32].copy_from_slice(&size.to_le_bytes());
        b[32..40].copy_from_slice(&rid.to_le_bytes());
        b[40..48].copy_from_slice(&l.to_le_bytes());
        b[48..56].copy_from_slice(&o.to_le_bytes());
        b[56..64].copy_from_slice(&a.to_le_bytes());
    }
    set_return_data(&out[..emit * 64]);
    Ok(())
}
