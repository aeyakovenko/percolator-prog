# Percolator Program — Dispatch Table Walkthrough

**Phase 2 deliverable for external audit.**
Prepared from source at `~/percolator-prog/src/percolator.rs` (13,685 lines) and `src/tags.rs`.
All file:line citations refer to `/Users/khubair/percolator-prog/src/percolator.rs` unless noted as `tags.rs`.
Program ID: `ESa89R5Es3rJ5mnwGybVRG1GrNt9etP11Z5V2QWD4edv`

---

## How to read this document

- **Permissions**: `admin` = `header.admin` signer; `keeper` = keeper registered in slab; `permissionless` = any caller; `oracle-authority` = `config.oracle_authority`; `nft-mint-auth` = PDA `["mint_authority"]` from percolator-nft program.
- **State mutations** describe what on-chain fields change; `engine.*` refers to the `RiskEngine` struct embedded in the slab at byte offset `ENGINE_OFF`.
- **CPI calls** are token-program transfers (SPL Token `Transfer`/`MintTo`/`Burn`) or cross-program calls to the matcher program via `invoke_signed`.
- **Fund-loss risk** is rated on reachability × impact: `high` = direct path to draining collateral vault; `medium` = indirect or bounded loss; `low` = theoretical/bounded; `none` = no token flow.

---

## 1. Market Lifecycle

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 0 | InitMarket | percolator.rs:6190 | Permissionless (any signer becomes admin via instruction data) | Writes `SlabHeader` (magic, admin, nonce), `MarketConfig` (all fields), `RiskEngine` (zeroed), vault PDA derived | SPL Token `InitializeAccount` (vault ATA), System `CreateAccount` (slab rent) | magic must equal `PERCOLAT` after init; admin in data must match account[0].key; mint in data must match account[2].key | Admin is set from **instruction data** not signer alone — if a client sends the wrong admin pubkey the real signer loses control. No market count cap. `min_oracle_price_cap_e2bps` not range-checked (can be 0, which disables the circuit breaker). | Medium |
| 12 | UpdateAdmin | percolator.rs:7993 | Admin | Sets `config.pending_admin`; does NOT yet write `header.admin` (two-step) | None | Pending admin must be non-zero | First step only stores `pending_admin`. No event emitted. If pending admin is never called, the transfer silently expires. | Low |
| 82 | AcceptAdmin | percolator.rs:8057 | `config.pending_admin` signer | Writes `header.admin = signer`; clears `pending_admin` | None | Signer must exactly match `pending_admin`; zeroes pending after transfer | No timelock on acceptance — pending admin can accept immediately or never. Old admin can re-call UpdateAdmin to cancel by overwriting `pending_admin`. | Low |
| 13 | CloseSlab | percolator.rs:8097 | Admin | Transfers all lamports from slab to admin; zeroes slab data | None | Requires RESOLVED; requires `num_used_accounts == 0`; requires `dust_base == 0` | No check that vault is empty before closing slab. If vault has residual tokens (e.g. from round-trip rounding) CloseSlab succeeds but vault tokens are orphaned. | Medium |
| 19 | ResolveMarket | percolator.rs:8616 | Admin | Sets `config.flags |= RESOLVED`; writes `authority_price_e6` from oracle; freezes `engine.current_slot` | Reads Pyth/Switchboard oracle | Once resolved, no trades; settlement price is immutable | Settlement price is read from oracle at resolution time — no time-weighted average, vulnerable to single-slot oracle manipulation if admin resolves right after a flash price move. | High |
| 29 | ResolvePermissionless | percolator.rs:9504 | Permissionless | Same as ResolveMarket | Reads oracle to prove staleness | Requires oracle to return `OracleStale`; requires `permissionless_resolve_stale_slots > 0` | Settlement price at permissionless resolution is mark price at time of call. A griefer can front-run a temporarily stale oracle during a slow Pyth slot and resolve at an off-market price. | High |

---

## 2. User Account Lifecycle

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 1 | InitUser | percolator.rs:6571 | Permissionless (payer signs) | Allocates account slot in `engine.accounts[]`; increments `mat_counter`; writes generation table entry | System `Transfer` for `fee_payment` (if non-zero) | Slot must be free (`account.owner == [0;32]`); max accounts not exceeded | `fee_payment` amount is instruction-data-supplied — caller can pass 0 even if admin intended non-zero fees (fee enforcement is advisory only on this path). | Low |
| 2 | InitLP | percolator.rs:6665 | Permissionless (payer signs) | Allocates LP slot; stores `matcher_program` and `matcher_context` in account | System `Transfer` for fee | LP slot must be free; `matcher_program` stored as-is (checked for shape later in TradeCpi) | `matcher_program` and `matcher_context` are stored without immediate on-chain verification — they are validated only when TradeCpi is called. A malicious LP init could store a crafted program address that is upgraded post-init. | Medium |
| 3 | DepositCollateral | percolator.rs:6762 | Account owner signer | `engine.accounts[user_idx].collateral += amount`; `engine.vault += units` | SPL Token `Transfer` (user ATA → vault) | Owner check via `verify::owner_ok`; blocked while paused | No minimum deposit enforced at the instruction level (only InitMarket `min_initial_deposit` in engine logic). Dust deposits possible. | Low |
| 4 | WithdrawCollateral | percolator.rs:6841 | Account owner signer | `engine.accounts[user_idx].collateral -= amount`; `engine.vault -= units` | SPL Token `Transfer` (vault → user ATA) via `invoke_signed` | Owner check; margin check post-withdrawal (engine rejects if below maintenance); blocked while paused; blocked on resolved market | No cooldown between withdrawals. Rapid withdraw-deposit cycling possible. | Medium |
| 8 | CloseAccount | percolator.rs:7783 | Account owner signer | Zeroes account slot; decrements `num_used_accounts`; removes from risk buffer | SPL Token `Transfer` (residual collateral → user ATA) | Position must be fully closed (`effective_pos_q == 0`); pending settlement must be clear | No check for residual `fee_credits` balance — these could be abandoned. CloseAccount does not reject if `pending_settlement` flag is set (cleared separately by keeper). Auditor note: check if `pending_settlement` check was added or is still a gap. | Low |
| 25 | ReclaimEmptyAccount | percolator.rs:9281 | Permissionless | Zeroes account slot if collateral and position are both zero | None | Slot must be empty (zero collateral, zero position) | No signer required — anyone can reclaim a genuinely empty account. Potentially useful for keepers to clean up GC'd accounts. | None |
| 26 | SettleAccount | percolator.rs:9314 | Permissionless | Settles `released_pnl` into collateral balance | None | Requires `released_pnl > 0` | Released PnL to collateral conversion uses `unit_scale` — if scale changes post-init (it cannot, unit_scale is immutable after init), this would be wrong. Currently safe. | None |
| 27 | DepositFeeCredits | percolator.rs:9362 | Permissionless | Transfers tokens into vault; credits `fee_credits` on target account | SPL Token `Transfer` | Account must exist | Anyone can credit fee_credits to anyone's account — could be used to manipulate fee accounting for an account the caller doesn't own. | Low |
| 28 | ConvertReleasedPnl | percolator.rs:9441 | Permissionless | Converts `released_pnl` to collateral, then to base tokens | None | Conservation: total vault unchanged (internal accounting shift) | Permissionless — caller can trigger conversion for any account. No harm expected but slightly surprising API. | None |

---

## 3. Trading

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 6 | TradeNoCpi | percolator.rs:7174 | User signer (account owner) | `engine.execute_trade()` — updates positions, collateral, OI, funding | None (no external program calls) | Owner check; blocked while paused; blocked on resolved market; LP must be LP-type; user must be user-type; OI cap hard block if configured | No oracle staleness check on TradeNoCpi path — LP sets price directly via `matcher_program`. If LP's internal price diverges from oracle, users can trade stale. | Medium |
| 10 | TradeCpi | percolator.rs:7328 | User signer | `engine.execute_trade()` with exec_size from matcher; advances nonce | CPI to matcher program via `invoke_signed` on LP PDA | Matcher identity check (`verify::matcher_identity_ok`); matcher shape check (`verify::matcher_shape_ok`); PDA key check (`verify::pda_key_matches`); nonce monotonic (`verify::nonce_on_success`); blocked while paused; resolved market blocked | Matcher program is validated only by stored pubkey equality — if admin stored a malicious matcher at InitLP (or matcher was upgraded), the CPI target is trusted. Matcher CPI return data is not independently range-checked — return value exec_size is used directly. Stack height check not present on TradeCpi itself (only on UpdateHyperpMark). | High |
| 5 | KeeperCrank | percolator.rs:6966 | Permissionless (caller_idx=0xFFFF) or registered keeper signer | `engine.crank()` — funding, GC, liquidation; updates `engine.current_slot` | None | Blocked while paused; if resolved, uses frozen slot; candidates are caller-supplied but engine validates eligibility | Candidate list is **entirely caller-supplied** — keeper can omit underwater accounts or repeat accounts. No on-chain enforcement that all eligible accounts are processed. GC can sweep accounts immediately after deposit if below `min_initial_deposit` (known bug: `bug_gc_sweeps_fresh_accounts`). | High |
| 7 | LiquidateAtOracle | percolator.rs:7705 | Permissionless | Forcefully closes target position at oracle price; transfers residual to insurance | Reads oracle | Must be below maintenance margin; oracle freshness enforced | Any caller can liquidate — no keeper incentive mechanism on-chain. Liquidation price is oracle spot — no slippage protection for the liquidated user. | Medium |
| 50 | ExecuteAdl | percolator.rs:11308 | Admin | Closes profitable target position at oracle price to deleverage | Reads oracle | Insurance must be fully depleted before ADL; blocked on resolved market; admin check via `require_admin` | ADL target selection is **admin-supplied** — admin can choose which profitable positions to ADL. No independent on-chain ranking verification. PnL cap pre-check enforced if `max_pnl_cap > 0`. | High |

---

## 4. Oracle

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 17 | PushOraclePrice | percolator.rs:8365 | `config.oracle_authority` signer | Writes `config.authority_price_e6`; records `last_mark_push_slot` | None | Oracle authority must be set and non-zero; price cap circuit breaker applied | Price cap is enforced (`max_change_e2bps`), but the cap itself is admin-configurable via SetOraclePriceCap. A compromised oracle authority + high cap = price manipulation. | High |
| 16 | SetOracleAuthority | percolator.rs:8309 | Admin | Writes `config.oracle_authority` | None | Admin must sign | Setting oracle authority to `[0;32]` would brick PushOraclePrice (zero authority is rejected by `require_oracle_authority`). No minimum key constraint. | Medium |
| 18 | SetOraclePriceCap | percolator.rs:8520 | Admin | Writes `config.oracle_price_cap_e2bps` | None | Cap must be <= `MAX_ORACLE_PRICE_CAP_E2BPS` (1_000_000 = 100%) | Cap can be set to 1_000_000 (100% per slot) — effectively disabling the circuit breaker. No lower bound enforced post-init. | Medium |
| 34 | UpdateHyperpMark | percolator.rs:12938 | Permissionless (CPI rejected via stack height check) | Updates `config.authority_price_e6` via 8-hour EMA from DEX pool; blocked if market not bootstrapped | Reads DEX pool account (Raydium/Meteora/PumpSwap) | Minimum DEX liquidity check (`MIN_DEX_QUOTE_LIQUIDITY = 2e12`); pool address must match `config.dex_pool_pubkey` if set; CPI call blocked (stack height == 1 enforced); 25-slot minimum update interval | DEX pool account is parsed as raw bytes — no CPI to the DEX program to verify integrity. Mint matching is done but pool-type detection is heuristic (checks account size/owner). A sufficiently crafted account matching expected layout could pass. | Medium |
| 56 | AdvanceOraclePhase | percolator.rs:12046 | Permissionless | Advances oracle phase (1 → 2 → 3) based on time and volume milestones | None | Phase transitions are one-way (no rewind) | Phase advancement logic reads volume from engine state — no external attestation. A burst of small trades inflating volume could trigger early phase advance. | Low |

---

## 5. Admin Parameter Setters

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 14 | UpdateConfig | percolator.rs:8207 | Admin | Updates funding parameters only: `funding_horizon_slots`, `funding_k_bps`, `funding_max_premium_bps`, `funding_max_bps_per_slot` | None | Admin check; individual field range checks | No minimum/maximum bounds on `funding_k_bps` other than implicit u64 max — setting this very high could make funding fees extreme. | Low |
| 78 | SetMaxPnlCap | percolator.rs:13247 | Admin | Writes `config.max_pnl_cap`; `0` disables cap | None | Admin check only | Cap=0 disables ADL pre-check entirely. No lower-bound on non-zero values — a tiny cap (cap=1) would ADL every profitable position. | Medium |
| 79 | SetOiCapMultiplier | percolator.rs:13277 | Admin | Writes packed OI cap multiplier into config; `0` disables enforcement | None | Admin check; packed field is lo32=multiplier_bps, hi32=soft_cap_bps | No range validation on the packed u64 fields beyond what the engine itself enforces. Setting both to 0 disables OI-based withdrawal limits. | Low |
| 80 | SetDisputeParams | percolator.rs:13310 | Admin | Writes `dispute_window_slots` and `dispute_bond_amount` | None | Admin check; `window_slots=0` disables dispute mechanism | Dispute window and bond can be changed **after** resolution — admin could retroactively close the dispute window or inflate the bond to block challengers. | High |
| 81 | SetLpCollateralParams | percolator.rs:13350 | Admin | Writes `lp_collateral_enabled` and `lp_collateral_ltv_bps` | None | Admin check; `ltv_bps` checked <= 10_000 | Setting `enabled=0` blocks new deposits but does not force-exit existing LP collateral positions. | Low |
| 70 | SetWalletCap | percolator.rs:12630 | Admin | Writes `config.max_wallet_pos_e6` (0 = disabled) | None | Admin check | Cap is a soft limit enforced only at trade-open; existing over-cap positions are not unwound when cap decreases. | Low |
| 71 | SetOiImbalanceHardBlock | percolator.rs:12676 | Admin | Writes `config.oi_imbalance_hard_block_bps` (0 = disabled) | None | Admin check; value checked 0..=10_000 | Hard block applies to new trades only; existing imbalance is not corrected. | Low |
| 74 | SetDexPool | percolator.rs:13390 | Admin | Writes `config.dex_pool_pubkey` for HYPERP markets | Reads pool account (owner/mint validation) | HYPERP-only; pool owner must be Raydium/Meteora/PumpSwap; collateral mint must match pool | Pool pubkey validation uses heuristic owner-check, not CPI. Admin can re-point pool at any future time, potentially switching to a thin pool after markets open. | Medium |
| 22 | SetInsuranceWithdrawPolicy | percolator.rs:8857 | Admin | Writes `config.ins_withdraw_authority`, `min_withdraw_base`, `max_withdraw_bps`, cooldown into packed field | None | Admin check; bps validated 0..=10_000 | Authority is set from instruction data — admin can set themselves or a separate treasury key. `max_withdraw_bps=10_000` with short cooldown allows rapid full fund extraction. | High |

---

## 6. Insurance

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 9 | TopUpInsurance | percolator.rs:7921 | Permissionless | `engine.insurance_fund.balance += units`; `engine.vault += units` | SPL Token `Transfer` (caller ATA → vault) | Vault conservation: vault increases by same units added to insurance | None notable — depositing to insurance is always benign | None |
| 20 | WithdrawInsurance | percolator.rs:8762 | Admin | Zeroes `engine.insurance_fund.balance`; `engine.vault -= units` | SPL Token `Transfer` (vault → admin ATA) via `invoke_signed` | Requires RESOLVED; requires `num_used_accounts == 0`; balance cannot exceed `u64::MAX` (overflow check) | All-or-nothing: cannot do partial insurance withdrawals through this path. If market has dust or residual accounts, admin is blocked. | Medium |
| 23 | WithdrawInsuranceLimited | percolator.rs:8923 | `config.ins_withdraw_authority` signer | Withdraws up to `max_withdraw_bps` of insurance per cooldown period | SPL Token `Transfer` (vault → authority ATA) | Cooldown enforced via packed `authority_timestamp` field; amount <= `max_withdraw_bps`; min base enforced | Cooldown slot stored in 48-bit field alongside bps — packing bug would allow bypassing cooldown. Authority is separately configurable from admin. No whitelist on destination ATA — withdraws to any ATA belonging to authority. | High |
| 41 | FundMarketInsurance | percolator.rs:10418 | Admin | Deposits to market's isolated insurance sub-balance within engine | SPL Token `Transfer` (admin ATA → vault) | Admin check; isolated insurance tracked separately from global fund | Isolated balance is capped by `max_insurance_floor` at init; subsequent top-ups can exceed that cap via FundMarketInsurance since the cap check is not re-applied here. | Low |

---

## 7. LP Vault

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 37 | CreateLpVault | percolator.rs:9765 | Admin | Creates LP vault state PDA and LP share mint PDA | System `CreateAccount`, SPL Token `InitializeMint` | Admin check; vault state must not already exist (magic check); `fee_share_bps <= 10_000` | Vault state PDA and mint PDA derived from slab key — only one LP vault per slab. Cannot create multiple vaults. | None |
| 38 | LpVaultDeposit | percolator.rs:9897 | Permissionless (any user) | Mints LP shares proportional to deposit; increases vault capital | SPL Token `Transfer` (user ATA → vault), `MintTo` (LP shares to user) | Share minting uses `(amount / total_capital) * total_shares` ratio; exchange rate preserved | Deposit ratio calculation uses integer division — small deposits can be rounded to 0 shares (minimum deposit not enforced). First depositor sets the initial exchange rate; can be manipulated with a dust deposit followed by large transfer (inflation attack). | High |
| 39 | LpVaultWithdraw | percolator.rs:10077 | Permissionless (any LP share holder) | Burns LP shares; withdraws proportional collateral | SPL Token `Burn` (LP shares), `Transfer` (vault → user ATA) | OI cap check if configured; queued withdrawal path if OI cap would be exceeded; exchange rate preserved | Withdraw checks OI cap but the cap can be bypassed by withdrawing small increments just below the threshold. Withdrawal does not check pending queued withdrawals — a user could both queue and directly withdraw. | High |
| 40 | LpVaultCrankFees | percolator.rs:10321 | Permissionless | Distributes accrued fee revenue from engine to LP vault capital | None (internal accounting) | Fee distribution uses `fee_share_bps` ratio; conservation check: fee credits must be available | Permissionless — anyone can crank. Fee accrual rate is proportional to trading volume; no oracle check needed. | None |
| 47 | QueueWithdrawal | percolator.rs:10939 | LP share holder signer | Creates withdrawal queue entry PDA; reserves LP shares | SPL Token `Transfer` (LP shares → escrow) | Epoch-based: withdrawal processes after current epoch ends; only one queued withdrawal per user | Queue PDA derived from `[user, slab]` — only one active queued withdrawal. A user with a queued withdrawal and direct LP shares can bypass queue via direct withdraw. | Medium |
| 48 | ClaimQueuedWithdrawal | percolator.rs:11052 | Queued user (implicit via PDA) | Processes one epoch tranche; partial or full depending on OI cap | SPL Token `Burn`, `Transfer` | Epoch must have elapsed; OI checked at claim time | Claim epoch check: if OI cap is lifted after queuing, user can claim full amount in one transaction. | Low |
| 49 | CancelQueuedWithdrawal | percolator.rs:11262 | Queued user (PDA) | Returns escrowed LP shares; closes queue PDA | SPL Token `Transfer` (escrow → user) | Queue PDA must exist and belong to caller | No penalty for cancel — users can queue-and-cancel repeatedly to probe OI cap state. | None |
| 45 | DepositLpCollateral | percolator.rs:10726 | Account owner signer | Transfers LP shares to vault; increases `account.lp_collateral_units` at configured LTV | SPL Token `Transfer` (LP shares → escrow) | LP collateral must be enabled (`lp_collateral_enabled != 0`); LTV applied at deposit | LP shares are valued at current exchange rate at deposit time. If exchange rate drops post-deposit (LP losses), collateral value falls but `lp_collateral_units` is not automatically marked down — engine may over-credit collateral until a crank or margin check runs. | High |
| 46 | WithdrawLpCollateral | percolator.rs:10826 | Account owner signer | Returns LP shares; decrements `lp_collateral_units` | SPL Token `Transfer` (escrow → user) | Position must be fully closed (no open perp position) before LP collateral can be withdrawn | No margin check before withdrawal; withdrawal allowed whenever position is closed. If user withdraws LP collateral and immediately opens a new position, margin is re-checked then. | Low |

---

## 8. NFT (Position NFTs)

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 64 | MintPositionNft | percolator.rs:12113 | Account owner signer | Creates Token-2022 mint PDA; sets `account.nft_mint` | Token-2022 `InitializeMint2`, `InitializeMetadataPointer`, `MintTo` | Position must be open (non-zero size); mint PDA derived from `[b"pos_nft", slab, user_idx]`; account must not already have an NFT | NFT minting requires open position but does not lock the position — user can close the position after minting and hold an NFT for a closed slot. The NFT then points to a zeroed slot. | Low |
| 65 | TransferPositionOwnership | percolator.rs:12319 | NFT holder (Token-2022 TransferHook calls tag 69 CPI) | Changes `account.owner` to new owner | None (hook triggers CPI to tag 69) | Owner change only via NFT transfer path; direct call requires NFT program signer | NFT metadata immutable extension not used — NFT holder cannot update position details displayed in metadata. | Low |
| 66 | BurnPositionNft | percolator.rs:12416 | Account owner signer | Burns NFT mint; clears `account.nft_mint` | Token-2022 `Burn`, `CloseAccount` (mint) | NFT must belong to caller's account; position must be closed (burn only allowed when slot can be released) | Burn does not verify position is closed — an NFT for an open position can be burned. This removes the transferability of the position but does not close it. Auditor: check if open-position NFT burn is intentional. | Low |
| 67 | SetPendingSettlement | percolator.rs:12520 | Keeper (admin check on keeper account) | Sets `account.pending_settlement = 1` | None | Requires admin/keeper signer | Admin can set pending_settlement on any account. If account owner tries to close while flag is set — check CloseAccount for pending_settlement guard (may be absent). | Low |
| 68 | ClearPendingSettlement | percolator.rs:12575 | Keeper | Clears `account.pending_settlement = 0` | None | Requires admin/keeper signer | No event emitted. Settlement lifecycle is off-chain dependent. | None |
| 69 | TransferOwnershipCpi | percolator.rs:11614 | NFT mint authority PDA (CPI-only path from percolator-nft) | Changes `account.owner` to `new_owner` | None | Caller must be `["mint_authority"]` PDA of the NFT program; slab must be owned by this program; NFT program must be executable and BPF-owned | NFT program identity is not pinned to a specific deployment — any program whose `["mint_authority"]` PDA matches could call this. Admin must ensure only the approved percolator-nft program is used. No upgrade-authority freeze check. | Medium |

---

## 9. Resolution & Force Close

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 30 | ForceCloseResolved | percolator.rs:9675 | Permissionless | Settles one account at settlement price; releases collateral | SPL Token `Transfer` (vault → user ATA) | Market must be RESOLVED; uses `config.authority_price_e6` (immutable post-resolve) | Force-close caller gets no reward — purely altruistic action. Accounts may languish if users do not self-close after resolution. | None |
| 21 | AdminForceClose | percolator.rs:9152 | Admin | Closes any account at current oracle price | Reads oracle; SPL Token `Transfer` | Admin check; market must NOT be resolved (use ForceCloseResolved instead); delay check: `force_close_delay_slots` must have elapsed | Admin can force-close any user's position. The delay (`force_close_delay_slots`) provides some protection but is admin-configurable. No user consent required. | High |
| 43 | ChallengeSettlement | percolator.rs:10493 | Permissionless (any challenger) | Creates dispute PDA; deposits bond from challenger | SPL Token `Transfer` (challenger ATA → vault) | Market must be RESOLVED; dispute window must be open; only one active dispute at a time | Bond amount is configurable and could be set prohibitively high by admin post-resolution (via SetDisputeParams) to prevent challenges. Single-challenger limit — a race condition allows only one dispute PDA. | High |
| 44 | ResolveDispute | percolator.rs:10628 | Admin | Closes dispute PDA; optionally updates settlement price; returns or slashes bond | SPL Token `Transfer` (bond return or slash) | Admin check; dispute PDA must exist and be initialized | Admin adjudicates their own market dispute — no independent arbitration mechanism. Admin can always set `accept=0` to reject any challenge. | High |
| 51 | CloseStaleSlabs | percolator.rs:11434 | Admin | Closes slab with wrong-size layout; recovers rent to admin | None (lamport transfer) | Admin check; verifies magic; skips `slab_guard` size check (intentional — stale layout) | Only admin can close stale slabs. No time constraint. Lamports go directly to admin, not to a treasury PDA. | Low |
| 52 | ReclaimSlabRent | percolator.rs:11523 | Slab account signer (proves key possession) | Closes uninitialized slab (magic = 0) and returns rent | None | Magic must be 0 (uninitialized); cannot close initialized slab | The slab account signs — proves caller holds the private key of the account (not a PDA). Caller can set slab to zero by reclaiming. | None |
| 72 | RescueOrphanVault | percolator.rs:12714 | Admin | Transfers orphaned tokens from vault to admin ATA | SPL Token `Transfer` (vault → admin ATA) | No open accounts (`num_used_accounts == 0`); reads raw bytes for admin check (layout-agnostic) | Layout-agnostic byte reads for admin + magic could mismatch if slab is mid-migration. Transfers full vault balance to admin. | High |
| 73 | CloseOrphanSlab | percolator.rs:12841 | Admin | Drains lamports from slab account when vault is zero | None | Vault ATA must have zero token balance | If vault is actually empty but has rent lamports, admin recovers them. No check that insurance sub-balances are zero. | Low |

---

## 10. Audit / Cranks

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 53 | AuditCrank | percolator.rs:11691 | Permissionless | Walks all accounts; verifies conservation (capital, PnL, OI, LP aggregates, solvency); pauses market on violation | None | All conservation invariants verified; Kani proofs exist for engine-level properties | AuditCrank can **pause the market** — a race condition where an attacker deliberately triggers a temporary inconsistency during a trade could trigger false-positive pause. Scan is windowed (RISK_SCAN_WINDOW=32 per call) — state changes between window calls could cause partial audit. | Medium |

---

## 11. Disabled — SharedVault (Tags 59–63)

Tags 59–63 parse successfully (decode arms exist) but the dispatch arms in `process_instruction` emit `msg!("SharedVault subsystem disabled — feature incomplete")` and return `InvalidInstructionData`. The handler code is preserved in source but unreachable.

Source: percolator.rs:6047–6077 (dispatch), tags.rs:115–123

**Root cause of disable**: Multiple critical bugs — no deposit instruction (total_capital always 0), QueueWithdrawalSV accepts arbitrary amounts with no token validation (fund theft vector), AllocateMarket double-counts total_allocated, no deallocation path. See SECURITY(H-2/H-3/H-4) comments.

| Tag | Name | Status |
|-----|------|--------|
| 59 | InitSharedVault | Disabled — `InvalidInstructionData` |
| 60 | AllocateMarket | Disabled — `InvalidInstructionData` |
| 61 | QueueWithdrawalSV | Disabled — `InvalidInstructionData` |
| 62 | ClaimEpochWithdrawal | Disabled — `InvalidInstructionData` |
| 63 | AdvanceEpoch | Disabled — `InvalidInstructionData` |

---

## 12. Removed — Dead Tags (InvalidInstructionData from decode)

These tag numbers have no decode arm — the `match` falls through to `_ => Err(InvalidInstructionData)`. The tags.rs constants exist for documentation/uniqueness testing only.

| Tag | Old name (upstream) | Reason |
|-----|--------------------|----|
| 11 | SetRiskThreshold | Removed in upstream fork |
| 15 | SetMaintenanceFee | Removed in upstream fork |
| 31 | (gap) | No upstream use, no constant |
| 32 | SetPythOracle | Replaced by feed_id in InitMarket data |
| 33 | UpdateMarkPrice | Replaced by UpdateHyperpMark |
| 35 | TradeCpiV2 | Merged into TradeCpi |
| 36 | UnresolveMarket | Removed — resolution is permanent |
| 42 | SetInsuranceIsolation | Removed after PERC-306 redesign |
| 57 | (gap, keeper fund) | KeeperFund removed |
| 58 | SlashCreationDeposit | Stub — PERC-629 unimplemented. Decode arm absent. Returns `InvalidInstructionData`. (tags.rs:113, guarded by `tag_slash_creation_deposit_is_unimplemented_stub` test) |

---

## 13. Cross-Margin

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 54 | SetOffsetPair | percolator.rs:11789 | Admin | Creates/updates `OffsetPairConfig` PDA at `["cmor_pair", slab_a, slab_b]` | System `CreateAccount` (if new) | Admin check; slab ordering enforced (slab_a < slab_b by key) | Offset BPS not range-checked beyond u16 max. Admin can set arbitrarily large offsets granting near-unlimited cross-margin credit. | Medium |
| 55 | AttestCrossMargin | percolator.rs:11893 | Permissionless | Creates/updates `CrossMarginAttestation` PDA at `["cmor", user, slab_a, slab_b]` | None | OffsetPairConfig must exist; user must have slots in both slabs | Attestation is permissionless — anyone can attest anyone's cross-margin status. However, attestation only reads engine state (positions), so a malicious attestation cannot invent margin credit. Stale attestations (positions closed post-attest) are not auto-invalidated. | Low |

---

## 14. Matcher Setup

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 75 | InitMatcherCtx | percolator.rs:13487 | Admin | CPI to matcher program to initialize the matcher context PDA | CPI to matcher program `initialize` instruction via `invoke_signed` on LP PDA | Admin check; LP slot must have matcher_program registered; matcher_program identity verified; matcher_ctx owner must be matcher_program; LP PDA derived and verified | The matcher program receives a CPI with admin-level trust. The matcher context ABI (72-byte layout) is opaque to this program — if the matcher program has bugs in its init handler, this instruction blindly passes parameters through. | Medium |

---

## 15. Pause / Unpause

| Tag | Name | Handler file:line | Permissions | State mutations | CPI calls | Critical invariants | Known validation gaps | Fund-loss risk |
|-----|------|------------------|-------------|-----------------|-----------|--------------------|-----------------------|----------------|
| 76 | PauseMarket | percolator.rs:13195 | Admin | Sets `config.flags |= PAUSED`; blocks Trade, Deposit, Withdraw, InitUser | None | Admin check; does not block Crank, Liquidate, admin ops | No event log. Pause is silent. Monitoring must poll flags field. | None |
| 77 | UnpauseMarket | percolator.rs:13220 | Admin | Clears `config.flags &= ~PAUSED` | None | Admin check | Same as Pause — no event. Admin can pause/unpause freely with no rate limit or governance delay. | None |

---

## Summary Statistics

| Category | Count |
|----------|-------|
| Total defined tags (0–82, with gaps) | 83 positions |
| Active (have dispatch handler) | 57 |
| SharedVault disabled (decode exists, dispatch returns error) | 5 (tags 59–63) |
| Removed / no decode arm | 10 (tags 11, 15, 31, 32, 33, 35, 36, 42, 57, 58) |
| Stub (tag defined, no decode, no dispatch) | 1 (tag 58) |
| **Admin-only** | 23 |
| **Keeper (admin-gated)** | 2 (SetPendingSettlement, ClearPendingSettlement, ExecuteAdl) |
| **Permissionless** | 18 |
| **Owner-signer (user/LP)** | 9 |
| **Oracle authority** | 1 (PushOraclePrice) |
| **NFT mint authority (CPI-only)** | 1 (TransferOwnershipCpi) |

---

## Top 5 Highest-Risk Handlers

### 1. TradeCpi (Tag 10) — Fund-loss risk: HIGH

**Why**: The matcher CPI is the primary execution path for all leveraged trades. The matcher program identity is validated by pubkey equality to a value set at `InitLP` time. If the matcher program is upgradeable (no freeze), an admin or matcher program upgrade authority can swap logic post-deployment. Exec_size returned from the CPI is used **directly** to adjust positions and collateral without independent range checking. A malicious or buggy matcher returning extreme exec_size values can drain LP capital. Additionally, `TradeCpi` has no stack height restriction — it can be composed inside other CPI chains (unlike `UpdateHyperpMark`).

### 2. KeeperCrank (Tag 5) — Fund-loss risk: HIGH

**Why**: The candidate list is **entirely caller-supplied**. A keeper can selectively omit undercollateralized accounts from the crank list, preventing liquidation and letting bad debt accumulate. The GC sweep (engine-level) can close accounts that are below `min_initial_deposit` immediately after a deposit when a fee has just been deducted — a known bug (`bug_gc_sweeps_fresh_accounts`). This is the primary state-advancing instruction and runs with no output log of what was processed.

### 3. AdminForceClose (Tag 21) — Fund-loss risk: HIGH

**Why**: Admin can close **any user's position** at oracle price with only a slot delay (`force_close_delay_slots`). The delay is itself admin-configurable (set at InitMarket, currently no update instruction — but delay=0 is possible at market init). A compromised admin key can systematically force-close all profitable positions at unfavorable oracle moments, effectively extracting value to the insurance fund or LP.

### 4. WithdrawInsuranceLimited (Tag 23) — Fund-loss risk: HIGH

**Why**: Insurance fund withdrawals are gated by cooldown and BPS cap, but the cap is admin-configurable (`max_withdraw_bps` up to 100%) and the cooldown can be short. The `ins_withdraw_authority` is a separate key from admin, widening the attack surface. The cooldown slot is packed into 48 bits of `authority_timestamp` — a packing bug or deliberate manipulation of the timestamp field allows cooldown bypass. The destination ATA is any ATA owned by the authority (no treasury whitelist).

### 5. ResolveMarket / ResolvePermissionless (Tags 19/29) — Fund-loss risk: HIGH

**Why**: Settlement price is read from oracle at the moment of resolution — a single Pyth price update, not a TWAP. A flash loan attack or coordinated oracle manipulation in the same slot can set an off-market settlement price, affecting all outstanding positions. `ResolvePermissionless` is particularly concerning: any caller can trigger it when the oracle is temporarily stale (e.g., Pyth publisher offline for a few seconds during volatility), locking in a potentially stale or manipulated price permanently.

---

## Known Gaps Summary (from Phase 4 CPI Surface + Phase 5 Admin Threat Model)

### CPI Surface Gaps (Phase 4)

1. **Matcher program upgradeability not frozen**: The matcher program stored in `lp_acc.matcher_program` is checked by address equality but the program's upgrade authority is not verified. If the matcher program's upgrade authority is not burned, a compromise there affects all TradeCpi executions.

2. **UpdateHyperpMark raw pool parsing**: DEX pool state is parsed as raw bytes without CPI into the DEX program for verified state. The heuristic detection (account size / owner) is bypassable with a crafted account matching the expected structure.

3. **TransferOwnershipCpi NFT program identity**: Any program whose `["mint_authority"]` PDA matches could call tag 69. The percolator-nft program ID is not pinned in the main program code. Admin rotation of the NFT program without updating this check would be a critical gap.

4. **Matcher CPI return data unchecked range**: exec_size from matcher is used directly. An adversarial matcher could return values outside any reasonable trade size, bounded only by the engine's internal checks (which are risk-engine logic, not a hard cap).

### Admin Threat Model Gaps (Phase 5)

1. **SetDisputeParams after resolution**: Admin can change dispute window and bond amount **after** a market resolves and **after** a challenge is filed. This allows retroactive manipulation of the dispute outcome.

2. **SetInsuranceWithdrawPolicy has no minimum cooldown enforcement**: If `cooldown_slots=1` is set and `max_withdraw_bps=10000`, a single authority can drain the entire insurance fund in a 2-slot sequence.

3. **AdminForceClose delay is init-time configurable at 0**: A market initialized with `force_close_delay_slots=0` gives admin immediate force-close capability with no user protection window.

4. **AuditCrank can be weaponized for DoS**: Any caller can trigger a market pause if conservation invariants temporarily diverge during a multi-instruction transaction. An attacker who can observe pending transactions could race to audit between a deposit CPI and the subsequent balance update, potentially triggering a false pause.

5. **Two-step admin transfer AcceptAdmin has no timeout**: A pending admin in `config.pending_admin` persists indefinitely. If the current admin's key is lost after UpdateAdmin but before AcceptAdmin, the market admin is permanently locked (old admin key gone, new admin not yet confirmed). A governance timelock and cancel mechanism are absent.

6. **RescueOrphanVault uses raw byte offsets for admin check**: Tag 72 reads admin from raw slab bytes (layout-agnostic by design) rather than typed accessors. If the slab layout ever changes and a stale slab has a garbage admin field at the expected offset, the check can pass for a wrong address.

---

*Document generated 2026-04-16. Source: `/Users/khubair/percolator-prog/src/percolator.rs` (rev HEAD). All line numbers are approximate within ±5 lines due to active development.*
