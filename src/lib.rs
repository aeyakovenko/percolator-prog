//! Percolator: perpetual-markets Solana program built on Anchor v2.
//!
//! Migration in progress. The legacy native-Solana implementation is
//! preserved at `percolator-prog/` (untouched) and at
//! `percolator-prog-v2/src/percolator.rs` (in-tree reference, no
//! longer compiled — see `[lib].path` in Cargo.toml).

#![no_std]
#![allow(unexpected_cfgs)]

extern crate alloc;

use anchor_lang_v2::prelude::*;

declare_id!("Perco1ator111111111111111111111111111111111");

pub mod constants;
pub mod errors;
pub mod guards;
pub mod instructions;
pub mod matcher_abi;
pub mod oracle;
pub mod policy;
pub mod risk_buffer;
pub mod state;
pub mod units;
pub mod zc;

use instructions::UpdateAuthority;
// The `#[program]` macro looks for each Accounts struct's auto-generated
// `__client_accounts_<name>` module at `super::` (= the crate root). Our
// Accounts structs live in submodules under `instructions/`, so re-export
// each one at the crate root.
#[doc(hidden)]
pub use instructions::update_authority::__client_accounts_updateauthority;

#[cfg(not(feature = "no-entrypoint"))]
#[program]
pub mod percolator {
    use super::*;

    /// Smoke handler at discriminator 254 — kept until every legacy tag
    /// has a real handler. Wire format: `[254u8]`.
    #[discrim = 254]
    pub fn ping(_ctx: &mut Context<Ping>) -> Result<()> {
        Ok(())
    }

    /// Tag 32 — rotate or burn one of four scoped authority pubkeys.
    /// See `instructions/update_authority.rs` for wire format + semantics.
    #[discrim = 32]
    pub fn update_authority(
        ctx: &mut Context<UpdateAuthority>,
        kind: u8,
        new_pubkey: [u8; 32],
    ) -> Result<()> {
        instructions::update_authority::handler(ctx, kind, new_pubkey)
    }
}

#[derive(Accounts)]
pub struct Ping {
    pub payer: Signer,
}
