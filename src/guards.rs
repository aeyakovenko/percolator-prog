//! Cross-instruction guards that validate slab-body invariants Anchor's
//! `#[derive(Accounts)]` cannot express:
//!
//! - **Magic / version**: the legacy native program shipped its own 4-byte
//!   `MAGIC` + 4-byte `version` at body offset 0. Anchor v2 adds an 8-byte
//!   account discriminator in front of those, but the body fields are
//!   still load-bearing for cross-deployment compatibility — drop them
//!   only via a coordinated state migration. `require_initialized`
//!   verifies `MAGIC`.
//! - **Reentrancy**: TradeCpi flips `FLAG_CPI_IN_PROGRESS` in the slab
//!   header before invoking the matcher. Every other handler must
//!   reject while that bit is set. `require_no_reentrancy` enforces it.
//! - **Admin auth**: `require_admin` matches a stored 32-byte authority
//!   pubkey against the signer; rejects burned (all-zero) authorities
//!   and mismatches with `EngineUnauthorized` — same code legacy emitted.

use crate::constants::{MAGIC, SLAB_LEN};
use crate::errors::PercolatorError;
use crate::state::{self, PercolatorSlab};
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

/// Slab account-shape check.
///
/// `Account<PercolatorSlab>` already validates the 8-byte discriminator and
/// the program owner at handler entry. The minimum-length check
/// (`>= 8 + size_of::<SlabHeader>()`) is also done by the framework.
/// What's NOT covered is the EXACT-length contract: the slab packs the
/// engine + risk buffer + generation table after the header, so the
/// total length must equal `SLAB_LEN`. Anything shorter would let
/// downstream `slab_data_mut` UB-deref past the end; longer would
/// silently ignore the tail and break engine offsets.
pub fn slab_shape_guard(slab: &Account<PercolatorSlab>) -> Result<()> {
    if slab.account().data_len() != SLAB_LEN {
        return Err(PercolatorError::InvalidSlabLen.into());
    }
    Ok(())
}

/// Reject if the slab's MAGIC field has not been written yet
/// (i.e. the slab was zero-initialised but never InitMarket'd).
pub fn require_initialized(data: &[u8]) -> Result<()> {
    let h = state::read_header(data);
    if h.magic != MAGIC {
        return Err(PercolatorError::NotInitialized.into());
    }
    Ok(())
}

/// Reject if a TradeCpi matcher CPI is currently mid-flight on this slab.
/// The bit is flipped in `instructions::trade_cpi::handler`.
pub fn require_no_reentrancy(data: &[u8]) -> Result<()> {
    if state::is_cpi_in_progress(data) {
        return Err(ProgramError::InvalidAccountData.into());
    }
    Ok(())
}

/// Match a stored 32-byte authority field against a signer's address.
/// All-zero authorities are burned: any auth check against them rejects.
pub fn require_admin(stored_authority: [u8; 32], signer: &Address) -> Result<()> {
    let signer_bytes: [u8; 32] = signer.to_bytes();
    if !crate::policy::admin_ok(stored_authority, signer_bytes) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }
    Ok(())
}
