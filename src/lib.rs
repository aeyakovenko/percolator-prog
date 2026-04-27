//! Percolator: perpetual-markets Solana program built on Anchor v2.
//!
//! Migration in progress — see /home/jamie/.claude/plans/please-prepare-a-plan-temporal-frost.md
//! for the phase-by-phase plan. The legacy native-Solana implementation is
//! preserved at `percolator-prog/` (untouched) and `percolator-prog-v2/src/percolator.rs`
//! (in-tree reference, no longer compiled).

#![no_std]
#![allow(unexpected_cfgs)]

extern crate alloc;

use anchor_lang_v2::prelude::*;

declare_id!("Perco1ator111111111111111111111111111111111");

pub mod constants;
pub mod errors;
pub mod risk_buffer;
pub mod state;

#[cfg(not(feature = "no-entrypoint"))]
#[program]
pub mod percolator {
    use super::*;

    /// Phase 0/1 stub. Replaced as instructions land in Phase 2.
    /// Discriminator 0xFE picked to avoid collision with the existing
    /// tag space (0..=32 with gaps).
    #[discrim = 254]
    pub fn ping(_ctx: &mut Context<Ping>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Ping {
    pub payer: Signer,
}
