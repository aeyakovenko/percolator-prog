//! Matcher CPI return wire format and validation.
//! Verbatim port of the legacy `mod matcher_abi` with the import path
//! adjusted from `solana_program::program_error::ProgramError` to the
//! modular `solana_program_error::ProgramError` (same type, same wire
//! semantics — only the crate that owns it changed in Solana 3.x).

use crate::constants::MATCHER_ABI_VERSION;
use solana_program_error::ProgramError;

/// Matcher return flags
pub const FLAG_VALID: u32 = 1; // bit0: response is valid
pub const FLAG_PARTIAL_OK: u32 = 2; // bit1: partial fill, including zero, allowed
pub const FLAG_REJECTED: u32 = 4; // bit2: trade rejected by matcher

/// Matcher return structure.
/// IMPORTANT: exec_price_e6 must be in engine-space (already inverted
/// and scaled). The matcher receives oracle_price_e6 in engine-space
/// and must return exec_price_e6 in the same space. The wrapper stores
/// it directly as the Hyperp mark price without re-normalization.
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
    if ctx.len() < 64 {
        return Err(ProgramError::InvalidAccountData);
    }
    let abi_version = u32::from_le_bytes(ctx[0..4].try_into().unwrap());
    let flags = u32::from_le_bytes(ctx[4..8].try_into().unwrap());
    let exec_price_e6 = u64::from_le_bytes(ctx[8..16].try_into().unwrap());
    let exec_size = i128::from_le_bytes(ctx[16..32].try_into().unwrap());
    let req_id = u64::from_le_bytes(ctx[32..40].try_into().unwrap());
    let lp_account_id = u64::from_le_bytes(ctx[40..48].try_into().unwrap());
    let oracle_price_e6 = u64::from_le_bytes(ctx[48..56].try_into().unwrap());
    let reserved = u64::from_le_bytes(ctx[56..64].try_into().unwrap());

    Ok(MatcherReturn {
        abi_version,
        flags,
        exec_price_e6,
        exec_size,
        req_id,
        lp_account_id,
        oracle_price_e6,
        reserved,
    })
}

pub fn validate_matcher_return(
    ret: &MatcherReturn,
    lp_account_id: u64,
    oracle_price_e6: u64,
    req_size: i128,
    req_id: u64,
) -> Result<(), ProgramError> {
    // Check ABI version
    if ret.abi_version != MATCHER_ABI_VERSION {
        return Err(ProgramError::InvalidAccountData);
    }
    // Reject any flag bits outside the known set. Prevents a future
    // matcher that uses a currently-undefined flag (e.g. a new partial
    // fill semantics) from being silently accepted by this wrapper —
    // upgraders must bump the ABI version to signal new flag meaning.
    const KNOWN_FLAGS: u32 = FLAG_VALID | FLAG_PARTIAL_OK | FLAG_REJECTED;
    if (ret.flags & !KNOWN_FLAGS) != 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    // Must have VALID flag set
    if (ret.flags & FLAG_VALID) == 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    // Must not have REJECTED flag set
    if (ret.flags & FLAG_REJECTED) != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    // Validate echoed fields match request
    if ret.lp_account_id != lp_account_id {
        return Err(ProgramError::InvalidAccountData);
    }
    if ret.oracle_price_e6 != oracle_price_e6 {
        return Err(ProgramError::InvalidAccountData);
    }
    if ret.reserved != 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    if ret.req_id != req_id {
        return Err(ProgramError::InvalidAccountData);
    }

    // Require exec_price_e6 != 0 always - avoids "all zeros but valid flag" ambiguity
    if ret.exec_price_e6 == 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    // Zero exec_size requires PARTIAL_OK flag
    if ret.exec_size == 0 {
        if (ret.flags & FLAG_PARTIAL_OK) == 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        // Zero fill with PARTIAL_OK is allowed - return early
        return Ok(());
    }

    // Size constraints (use unsigned_abs to avoid i128::MIN overflow)
    if ret.exec_size.unsigned_abs() > req_size.unsigned_abs() {
        return Err(ProgramError::InvalidAccountData);
    }
    if req_size != 0 && ret.exec_size.signum() != req_size.signum() {
        return Err(ProgramError::InvalidAccountData);
    }
    if ret.exec_size.unsigned_abs() < req_size.unsigned_abs()
        && (ret.flags & FLAG_PARTIAL_OK) == 0
    {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}
