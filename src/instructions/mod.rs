//! Per-instruction handlers, one file per active discriminator. Each
//! sub-module exposes `handler(ctx, ...)` and a `#[derive(Accounts)]`
//! struct named after the instruction. The `#[program]` block in
//! `crate::lib` binds each handler to its `#[discrim = N]`.

pub mod catchup_accrue;
pub mod push_hyperp_mark;
pub mod reclaim_empty_account;
pub mod settle_account;
pub mod update_authority;

// Re-export only the `#[derive(Accounts)]` types so `crate::lib`'s
// `#[program]` block can refer to them without an `instructions::*`
// glob (which would pollute the crate root with each handler's
// `handler` fn). Bring more types in as the rest of the 28 land.
pub use catchup_accrue::CatchupAccrue;
pub use push_hyperp_mark::PushHyperpMark;
pub use reclaim_empty_account::ReclaimEmptyAccount;
pub use settle_account::SettleAccount;
pub use update_authority::UpdateAuthority;
