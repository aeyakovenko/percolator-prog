//! Anchor v2 `Id` markers for the foreign programs this crate CPIs into.
//!
//! Lets handlers declare `pub token_program: Program<TokenProgram>`
//! instead of `pub token_program: UncheckedAccount` plus a manual
//! `cpi::verify_token_program(...)` call inside the body. `Program<T>`
//! validates `T::id()` against the supplied account address at load
//! time and (with the `guardrails` feature) enforces `executable`.

use anchor_lang_v2::Id;
use pinocchio::address::Address;

/// SPL Token program (legacy, `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`).
/// `pinocchio_token::ID` is the canonical 32-byte address; re-export it
/// behind the Anchor v2 `Id` trait so it composes with `Program<T>`.
pub struct TokenProgram;

impl Id for TokenProgram {
    #[inline(always)]
    fn id() -> Address {
        pinocchio_token::ID
    }
}
