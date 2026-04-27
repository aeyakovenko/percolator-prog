//! Zero-copy unsafe island for the slab's embedded `RiskEngine`.
//!
//! Exposes `engine_ref` / `engine_mut` that cast a window of slab bytes
//! into a `&RiskEngine` / `&mut RiskEngine`. All ENGINE_OFF / ENGINE_LEN
//! constants live in `crate::constants` and already include the
//! 8-byte Anchor v2 disc prefix, so callers pass the FULL slab account
//! data slice (including the disc) just like every other byte-window
//! helper in `crate::state`.
//!
//! The legacy `invoke_signed_trade` matcher CPI glue is NOT ported here;
//! it lands in `crate::cpi` (Phase 3) where the SPL transfer helpers
//! also live.

#![allow(unsafe_code)]

use crate::constants::{ENGINE_ALIGN, ENGINE_LEN, ENGINE_OFF};
use core::mem::offset_of;
use percolator::RiskEngine;
use solana_program_error::ProgramError;

/// Public accessor for the engine's `accounts` array offset (used by tests).
pub const ACCOUNTS_OFFSET: usize = offset_of!(RiskEngine, accounts);

/// Offset of `side_mode_long` (repr(u8) enum) within `RiskEngine`.
const SM_LONG_OFF: usize = offset_of!(RiskEngine, side_mode_long);
/// Offset of `side_mode_short` (repr(u8) enum) within `RiskEngine`.
const SM_SHORT_OFF: usize = offset_of!(RiskEngine, side_mode_short);
/// Offset of `market_mode` (repr(u8) enum) within `RiskEngine`.
const MM_OFF: usize = offset_of!(RiskEngine, market_mode);

/// Validate every field with invalid bit patterns from raw bytes BEFORE
/// casting the slab to `&RiskEngine` / `&mut RiskEngine`. The cast is
/// `unsafe`; a Rust reference to a struct containing an invalid bit
/// pattern is UB on first field access whether or not we read the
/// field. The only fields with invalid bit patterns today are the
/// three `#[repr(u8)]` enums below — see the legacy `mod zc` for the
/// full audit trail.
#[inline]
fn validate_raw_discriminants(data: &[u8]) -> Result<(), ProgramError> {
    let base = ENGINE_OFF;
    // SideMode: valid 0 (Normal), 1 (DrainOnly), 2 (ResetPending)
    let sm_long = data[base + SM_LONG_OFF];
    let sm_short = data[base + SM_SHORT_OFF];
    if sm_long > 2 || sm_short > 2 {
        return Err(ProgramError::InvalidAccountData);
    }
    // MarketMode: valid 0 (Live), 1 (Resolved)
    let mm = data[base + MM_OFF];
    if mm > 1 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

pub fn engine_ref(data: &[u8]) -> Result<&RiskEngine, ProgramError> {
    if data.len() < ENGINE_OFF + ENGINE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let ptr = unsafe { data.as_ptr().add(ENGINE_OFF) };
    if (ptr as usize) % ENGINE_ALIGN != 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    validate_raw_discriminants(data)?;
    Ok(unsafe { &*(ptr as *const RiskEngine) })
}

#[inline]
pub fn engine_mut(data: &mut [u8]) -> Result<&mut RiskEngine, ProgramError> {
    if data.len() < ENGINE_OFF + ENGINE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let ptr = unsafe { data.as_mut_ptr().add(ENGINE_OFF) };
    if (ptr as usize) % ENGINE_ALIGN != 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    validate_raw_discriminants(data)?;
    Ok(unsafe { &mut *(ptr as *mut RiskEngine) })
}
