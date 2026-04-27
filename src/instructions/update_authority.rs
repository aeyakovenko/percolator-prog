//! Tag 32 — UpdateAuthority. Rotate or burn one of four scoped
//! authority pubkeys (admin / hyperp_mark / insurance / insurance_op).
//!
//! Wire format (1-byte disc + Borsh args):
//! ```text
//! [32u8] [kind: u8] [new_pubkey: [u8; 32]]
//! ```
//!
//! Account order — strict, matches legacy:
//! 1. `current` (signer)         — current authority pubkey for `kind`
//! 2. `new_authority`            — must sign + match `new_pubkey` unless burn
//! 3. `slab` (mut, owned by ID)  — market state
//!
//! Burn semantics: setting `new_pubkey == [0u8; 32]` permanently
//! revokes the chosen authority. AUTHORITY_ADMIN burn requires the
//! market be configured for permissionless lifecycle completion (see
//! per-kind invariants). Other kinds may burn freely.

use crate::errors::PercolatorError;
use crate::guards::{require_admin, require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;

// ── Authority kind constants — keep stable across deploys ──────────────────

pub const AUTHORITY_ADMIN: u8 = 0;
pub const AUTHORITY_HYPERP_MARK: u8 = 1;
pub const AUTHORITY_INSURANCE: u8 = 2;
// Tag 3 (AUTHORITY_CLOSE) deleted — close_authority merged into admin.
/// Scoped live-withdrawal authority. Cannot call tag 20 (unbounded);
/// only tag 23 (`WithdrawInsuranceLimited`).
pub const AUTHORITY_INSURANCE_OPERATOR: u8 = 4;

#[derive(Accounts)]
pub struct UpdateAuthority {
    pub current: Signer,
    /// CHECK: signer-ness is conditional on `new_pubkey != [0u8; 32]`
    /// (burn skips the check) — `Signer` cannot express that, so the
    /// handler validates `is_signer` + key equality manually.
    pub new_authority: UncheckedAccount,
    /// `Account<SlabHeader>` validates the v2 disc + program owner.
    /// Body bytes (engine, market config) reached via
    /// `state::slab_data_mut` after `slab_shape_guard` confirms the
    /// exact length.
    #[account(mut)]
    pub slab: Account<SlabHeader>,
}

pub fn handler(
    ctx: &mut Context<UpdateAuthority>,
    kind: u8,
    new_pubkey: [u8; 32],
) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;

    // Reach the full slab data buffer (disc + body) for byte-window helpers.
    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);

    require_no_reentrancy(data)?;
    require_initialized(data)?;

    let is_burn = new_pubkey == [0u8; 32];

    // Hard-timeout gate for non-burn updates only. Burns strictly REMOVE
    // power and are how operators reach the fully admin-free terminal
    // state — blocking them past maturity would permanently trap a market
    // in a partially-burned state. Transfers (non-burn) past maturity are
    // still rejected, consistent with "matured markets are terminal."
    if !is_burn {
        let clock_slot = Clock::get()
            .map_err(|_| anchor_lang_v2::Error::UnsupportedSysvar)?
            .slot;
        let cfg_gate = state::read_config(data);
        if oracle::permissionless_stale_matured(&cfg_gate, clock_slot) {
            return Err(PercolatorError::OracleStale.into());
        }
    }

    // New pubkey must consent unless this is a burn.
    if !is_burn {
        let new_view = ctx.accounts.new_authority.account();
        if !new_view.is_signer() {
            return Err(PercolatorError::ExpectedSigner.into());
        }
        if new_view.address().to_bytes() != new_pubkey {
            return Err(anchor_lang_v2::Error::InvalidArgument.into());
        }
    }

    let mut header = state::read_header(data);
    let mut config = state::read_config(data);

    let stored_authority = match kind {
        AUTHORITY_ADMIN => header.admin,
        AUTHORITY_HYPERP_MARK => config.hyperp_authority,
        AUTHORITY_INSURANCE => header.insurance_authority,
        AUTHORITY_INSURANCE_OPERATOR => header.insurance_operator,
        _ => return Err(anchor_lang_v2::Error::InvalidInstructionData.into()),
    };
    require_admin(stored_authority, ctx.accounts.current.address())?;

    // Per-kind invariants at assignment time.
    match kind {
        AUTHORITY_ADMIN => {
            if is_burn {
                // Burning admin requires the market be configured to
                // complete its lifecycle without an admin (otherwise we
                // strand capital). Either it's already resolved with no
                // accounts, or it has both permissionless paths armed.
                let (resolved, has_accounts) = {
                    let engine = zc::engine_ref(data)?;
                    (engine.is_resolved(), engine.num_used_accounts > 0)
                };
                if !resolved {
                    if config.permissionless_resolve_stale_slots == 0
                        || config.force_close_delay_slots == 0
                    {
                        return Err(PercolatorError::InvalidConfigParam.into());
                    }
                } else if has_accounts && config.force_close_delay_slots == 0 {
                    return Err(PercolatorError::InvalidConfigParam.into());
                }
                // No is_policy_configured check — under the 4-way auth
                // split, admin and insurance_authority are independent.
                // Burning admin doesn't retain a back-channel; the
                // insurance withdrawal policy is whatever admin
                // configured before the burn. Operators who want full
                // rug-proofing also burn insurance_authority.
            }
        }
        AUTHORITY_HYPERP_MARK => {
            // AUTHORITY_HYPERP_MARK is Hyperp-only — it's the mark-push
            // signer for `PushHyperpMark`. Non-Hyperp markets have no
            // such role.
            if !oracle::is_hyperp_mode(&config) {
                return Err(PercolatorError::InvalidConfigParam.into());
            }
            // Burning is only safe once the EWMA is bootstrapped (else
            // the mark source is gone and no settlement path remains).
            if is_burn && config.mark_ewma_e6 == 0 {
                return Err(PercolatorError::InvalidConfigParam.into());
            }
        }
        AUTHORITY_INSURANCE | AUTHORITY_INSURANCE_OPERATOR => {
            // No per-kind invariants. Burning is a legitimate no-rug
            // configuration; setting to any pubkey is normal delegation.
            // The insurance_operator kind is structurally prevented from
            // calling tag 20 (unbounded WithdrawInsurance) because that
            // path checks `header.insurance_authority` — auth scopes are
            // disjoint.
        }
        _ => unreachable!(),
    }

    // Commit the assignment.
    match kind {
        AUTHORITY_ADMIN => {
            header.admin = new_pubkey;
            state::write_header(data, &header);
        }
        AUTHORITY_HYPERP_MARK => {
            config.hyperp_authority = new_pubkey;
            state::write_config(data, &config);
        }
        AUTHORITY_INSURANCE => {
            header.insurance_authority = new_pubkey;
            state::write_header(data, &header);
        }
        AUTHORITY_INSURANCE_OPERATOR => {
            header.insurance_operator = new_pubkey;
            state::write_header(data, &header);
        }
        _ => unreachable!(),
    }
    Ok(())
}
