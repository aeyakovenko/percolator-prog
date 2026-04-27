//! Per-instruction handlers, one file per active discriminator. Each
//! sub-module exposes `handler(ctx, ...)` and a `#[derive(Accounts)]`
//! struct named after the instruction. The `#[program]` block in
//! `crate::lib` binds each handler to its `#[discrim = N]`.

pub mod update_authority;

// Re-export only the `#[derive(Accounts)]` types so `crate::lib`'s
// `#[program]` block can refer to them without an `instructions::*`
// glob (which would pollute the crate root with each handler's
// `handler` fn). Bring more types in as the rest of the 28 land.
pub use update_authority::UpdateAuthority;
