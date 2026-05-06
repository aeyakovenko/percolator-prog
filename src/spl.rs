//! Typed SPL Token wrappers for use in `#[derive(Accounts)]` structs.
//!
//! Three types live here:
//!
//!   * `TokenProgram` — `Id` marker, lets handlers declare
//!     `pub token_program: Program<TokenProgram>` (executable + key
//!     validated at load).
//!   * `TokenAccount` — `AnchorAccount` impl that validates SPL Token
//!     account shape at load time (owner = SPL Token program,
//!     `data_len == Account::LEN`, `state == Initialized`). Replaces
//!     the `cpi::verify_token_account` body-time check.
//!   * `Mint` — same shape for SPL Mint accounts (owner = SPL Token
//!     program, `data_len == Mint::LEN`).
//!
//! Mint / authority equality (`token::mint = ...`,
//! `token::authority = ...`) is still checked in handler bodies via
//! `cpi::verify_vault` / `cpi::verify_token_account` — Anchor v2's
//! namespaced-constraint hook (`AccountConstraint<TokenAccount>::check`
//! for `token::mint` etc.) would let those move into attributes too;
//! that's a follow-up sweep.

use anchor_lang_v2::{AnchorAccount, Id};
use pinocchio::{account::AccountView, address::Address};
use solana_program_error::ProgramError;

// ─────────────────────────────────────────────────────────────────────────────
// TokenProgram

/// SPL Token program (legacy, `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`).
pub struct TokenProgram;

impl Id for TokenProgram {
    #[inline(always)]
    fn id() -> Address {
        pinocchio_token::ID
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TokenAccount

/// SPL Token account wrapper. Load-time validation:
///   * owner is the SPL Token program (via `Account::from_account_view`),
///   * data length matches `pinocchio_token::state::Account::LEN`,
///   * state is `AccountState::Initialized`.
///
/// Mint / owner equality against the *expected* values is NOT done here
/// — those checks live in `cpi::verify_vault` / `cpi::verify_token_account`
/// since they depend on the slab's `MarketConfig`. Could be lifted to
/// `token::mint = ...` / `token::authority = ...` constraints once the
/// `AccountConstraint` impls are added.
pub struct TokenAccount {
    view: AccountView,
}

impl TokenAccount {
    #[inline(always)]
    pub fn address(&self) -> &Address {
        self.view.address()
    }

    /// Decoded view of the underlying SPL Token account fields. Each
    /// call re-runs `from_account_view` (cheap; same length + owner
    /// check the load already passed).
    #[inline]
    pub fn typed(
        &self,
    ) -> Result<pinocchio::account::Ref<'_, pinocchio_token::state::Account>, ProgramError> {
        pinocchio_token::state::Account::from_account_view(&self.view)
            .map_err(|_| ProgramError::InvalidAccountData)
    }
}

impl AnchorAccount for TokenAccount {
    type Data = AccountView;

    fn load(
        view: AccountView,
        _program_id: &Address,
    ) -> core::result::Result<Self, ProgramError> {
        let acc = pinocchio_token::state::Account::from_account_view(&view)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        if acc.state() != pinocchio_token::state::AccountState::Initialized {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(Self { view })
    }

    #[inline(always)]
    fn account(&self) -> &AccountView {
        &self.view
    }
}

impl core::ops::Deref for TokenAccount {
    type Target = AccountView;
    #[inline(always)]
    fn deref(&self) -> &AccountView {
        &self.view
    }
}

impl AsRef<AccountView> for TokenAccount {
    #[inline(always)]
    fn as_ref(&self) -> &AccountView {
        &self.view
    }
}

impl AsRef<Address> for TokenAccount {
    #[inline(always)]
    fn as_ref(&self) -> &Address {
        self.view.address()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Mint

/// SPL Mint wrapper. Load-time validation:
///   * owner is the SPL Token program,
///   * data length matches `pinocchio_token::state::Mint::LEN`.
pub struct Mint {
    view: AccountView,
}

impl Mint {
    #[inline(always)]
    pub fn address(&self) -> &Address {
        self.view.address()
    }
}

impl AnchorAccount for Mint {
    type Data = AccountView;

    fn load(
        view: AccountView,
        _program_id: &Address,
    ) -> core::result::Result<Self, ProgramError> {
        // `Mint::from_account_view` validates owner-program + data length.
        let _ = pinocchio_token::state::Mint::from_account_view(&view)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        Ok(Self { view })
    }

    #[inline(always)]
    fn account(&self) -> &AccountView {
        &self.view
    }
}

impl core::ops::Deref for Mint {
    type Target = AccountView;
    #[inline(always)]
    fn deref(&self) -> &AccountView {
        &self.view
    }
}

impl AsRef<AccountView> for Mint {
    #[inline(always)]
    fn as_ref(&self) -> &AccountView {
        &self.view
    }
}

impl AsRef<Address> for Mint {
    #[inline(always)]
    fn as_ref(&self) -> &Address {
        self.view.address()
    }
}
