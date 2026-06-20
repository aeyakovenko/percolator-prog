//! Adversarial matcher for end-to-end testing of the wrapper's validate_matcher_return on BOTH the
//! batch CPI (tag 3, via set_return_data) and the single TradeCpi (tag 0, via the ctx-account return
//! region). It returns CRAFTED returns; the attack "mode" is read from ctx_account.data[0] (or
//! data[64] when nonzero, for stale-return probes). The wrapper MUST reject every hostile mode and
//! accept only the honest one.
#![allow(unexpected_cfgs)]
use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, program::set_return_data,
    program_error::ProgramError, pubkey::Pubkey,
};

entrypoint!(process);

const ABI: u32 = 3;
const FLAG_VALID: u32 = 1;

// Build one crafted 64-byte MatcherReturn; `mode` perturbs exactly one field (default = honest fill).
fn craft(mode: u8, req_id: u64, lp: u64, asset: u64, oracle: u64, req: i128) -> [u8; 64] {
    let mut flags = FLAG_VALID;
    let mut price = oracle;
    let mut size = req;
    let mut a = asset;
    let mut o = oracle;
    let mut rid = req_id;
    let mut l = lp;
    match mode {
        0 => size = req.saturating_mul(2),           // over-fill: open 2x the requested position
        1 => size = req.checked_neg().unwrap_or(0),  // reversed direction
        2 => a = asset.wrapping_add(1),              // forged asset echo
        3 => o = oracle.wrapping_add(1),             // forged oracle echo
        4 => rid = req_id.wrapping_add(1),           // forged req_id
        5 => l = lp.wrapping_add(1),                 // forged lp_account_id
        6 => price = 0,                              // zero exec price
        7 => { flags = FLAG_VALID; size = req / 2 }  // unflagged partial (no PARTIAL_OK)
        _ => {}                                      // honest full fill -> wrapper accepts
    }
    let mut b = [0u8; 64];
    b[0..4].copy_from_slice(&ABI.to_le_bytes());
    b[4..8].copy_from_slice(&flags.to_le_bytes());
    b[8..16].copy_from_slice(&price.to_le_bytes());
    b[16..32].copy_from_slice(&size.to_le_bytes());
    b[32..40].copy_from_slice(&rid.to_le_bytes());
    b[40..48].copy_from_slice(&l.to_le_bytes());
    b[48..56].copy_from_slice(&o.to_le_bytes());
    b[56..64].copy_from_slice(&a.to_le_bytes());
    b
}

fn mode_for_call(accounts: &[AccountInfo]) -> Result<(u8, bool), ProgramError> {
    let mut d = accounts[1].try_borrow_mut_data()?;
    let mode = if d.len() > 64 && d[64] != 0 {
        d[64]
    } else {
        d[0]
    };
    if mode == 13 {
        if d.len() <= 65 {
            return Err(ProgramError::InvalidAccountData);
        }
        if d[65] == 0 {
            d[65] = 1;
            return Ok((9, false));
        }
        return Ok((13, true));
    }
    Ok((mode, false))
}

fn process(_pid: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match data.first() {
        // Tag 0: single matcher call (67 bytes); write the crafted return into ctx[0..64].
        Some(&0) => {
            if data.len() < 67 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let req_id = u64::from_le_bytes(data[1..9].try_into().unwrap());
            let asset = u16::from_le_bytes(data[9..11].try_into().unwrap()) as u64;
            let lp = u64::from_le_bytes(data[11..19].try_into().unwrap());
            let oracle = u64::from_le_bytes(data[19..27].try_into().unwrap());
            let req = i128::from_le_bytes(data[27..43].try_into().unwrap());
            let (mode, no_write) = mode_for_call(accounts)?;
            if no_write {
                return Ok(());
            }
            let rec = craft(mode, req_id, lp, asset, oracle, req);
            let mut d = accounts[1].try_borrow_mut_data()?;
            d[0..64].copy_from_slice(&rec);
            Ok(())
        }
        // Tag 3: batched matcher call (18 + n*26 bytes); emit n returns via set_return_data.
        Some(&3) => {
            let n = data[1] as usize;
            if n == 0 || n > 16 || data.len() != 18 + n * 26 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let req_id = u64::from_le_bytes(data[2..10].try_into().unwrap());
            let lp = u64::from_le_bytes(data[10..18].try_into().unwrap());
            let (mode, no_write) = mode_for_call(accounts)?;
            if no_write {
                return Ok(());
            }
            let mut out = [0u8; 16 * 64];
            let emit = if mode == 8 { n.saturating_sub(1) } else { n }; // mode 8 = short return length
            for i in 0..n {
                let base = 18 + i * 26;
                let asset = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as u64;
                let oracle = u64::from_le_bytes(data[base + 2..base + 10].try_into().unwrap());
                let req = i128::from_le_bytes(data[base + 10..base + 26].try_into().unwrap());
                out[i * 64..i * 64 + 64].copy_from_slice(&craft(mode, req_id, lp, asset, oracle, req));
            }
            set_return_data(&out[..emit * 64]);
            Ok(())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
