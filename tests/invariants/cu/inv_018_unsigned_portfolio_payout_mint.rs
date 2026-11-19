//! INV-018: a resolved payout that the portfolio's owner did not sign is made in the
//! deposit mint (#477).
//!
//! `marketauth` names the secondary mint while the market is empty, and the secondary vault
//! holds whatever was sent to it. After resolution `CloseResolved` and the resolved
//! `PermissionlessCrank` need no owner signature once the owner window has passed, and
//! `ClaimResolvedPayoutTopup` never needs one. A caller that could choose the mint then
//! could pay every depositor in a secondary mint of the authority's making: the claims are
//! cleared, the deposits stay in the primary vault, and `CloseSlab` pays them to the
//! authority.
//!
//! Reserve payouts (backing buckets, their earnings, terminal insurance) follow the same rule;
//! their tests are in `inv_018_unsigned_reserve_payout_mint.rs`.
//!
//! Every wrapper instruction is public. SPL mints and token accounts, the secondary vault's
//! balance among them, are installed directly as in the other INV-018 fixtures, and the
//! top-up fixture installs its receipt directly too.

use super::*;

const DEPOSIT: u64 = 1_000_000;

/// The two public instructions that run the resolved close.
#[derive(Clone, Copy, Debug)]
enum CloseRoute {
    CloseResolved,
    Crank,
}

const CLOSE_ROUTES: [CloseRoute; 2] = [CloseRoute::CloseResolved, CloseRoute::Crank];

struct SecondaryRail {
    env: V16CuEnv,
    owner: Keypair,
    portfolio: Pubkey,
    secondary: Pubkey,
    secondary_vault: Pubkey,
    secondary_dest: Pubkey,
    primary_dest: Pubkey,
}

fn fund_secondary_vault(env: &mut V16CuEnv, secondary: Pubkey, amount: u64) -> Pubkey {
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, amount),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    secondary_vault
}

/// An empty market gets a secondary mint from its authority; one owner deposits; the
/// market is resolved and the owner window passes. The secondary vault holds as much of
/// the secondary mint as the owner deposited.
fn resolved_market_with_secondary_rail(force_close_delay_slots: u64) -> SecondaryRail {
    let mut env = V16CuEnv::new();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    env.configure_auth_mark_with_cu(0, 100);
    if force_close_delay_slots != 0 {
        env.configure_permissionless_resolve_with_cu(100, force_close_delay_slots);
    }
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, DEPOSIT.into());
    env.resolve();
    let resolved_slot = env.market_state().1.resolved_slot;
    env.svm
        .warp_to_slot(resolved_slot + force_close_delay_slots);
    let secondary_vault = fund_secondary_vault(&mut env, secondary, DEPOSIT);
    let secondary_dest = env.token_account_for_mint(secondary, owner.pubkey(), 0);
    let primary_dest = env.token_account(owner.pubkey(), 0);
    SecondaryRail {
        env,
        owner,
        portfolio,
        secondary,
        secondary_vault,
        secondary_dest,
        primary_dest,
    }
}

impl SecondaryRail {
    fn close(
        &mut self,
        route: CloseRoute,
        dest: Pubkey,
        vault: Pubkey,
        signed: bool,
    ) -> Result<u64, String> {
        self.env.svm.expire_blockhash();
        let instruction = match route {
            CloseRoute::CloseResolved => ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            CloseRoute::Crank => ProgInstruction::PermissionlessCrank {
                now_slot: self.env.svm.get_sysvar::<Clock>().slot,
                observations: Vec::new(),
            },
        };
        let signers: Vec<&Keypair> = if signed { vec![&self.owner] } else { vec![] };
        self.env.send(
            instruction,
            vec![
                AccountMeta::new_readonly(self.owner.pubkey(), signed),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &signers,
        )
    }

    fn frame(&self) -> Vec<Option<Account>> {
        [
            self.env.market,
            self.portfolio,
            self.env.vault,
            self.secondary_vault,
            self.secondary_dest,
            self.primary_dest,
        ]
        .map(|key| self.env.svm.get_account(&key))
        .to_vec()
    }

    /// The market authority removes the terminal portfolio and closes the slab; the owner
    /// takes no part. Returns what the authority received in the primary and in the secondary
    /// mint.
    fn retire_market(&mut self) -> (u64, u64) {
        let admin = self.env.admin.insecure_clone();
        self.env.svm.expire_blockhash();
        let close_portfolio = self.env.close_portfolio_ix(self.portfolio);
        self.env
            .send(
                close_portfolio,
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(self.env.market, false),
                    AccountMeta::new(self.portfolio, false),
                ],
                &[&admin],
            )
            .expect("the market authority removes the terminal portfolio");
        let primary_dest = self.env.token_account(admin.pubkey(), 0);
        let secondary_dest = self
            .env
            .token_account_for_mint(self.secondary, admin.pubkey(), 0);
        for _ in 0..8 {
            self.env.svm.expire_blockhash();
            let authority_epoch = self.env.control_sequences(0).authority_epoch;
            self.env
                .send(
                    ProgInstruction::CloseSlab { authority_epoch },
                    vec![
                        AccountMeta::new(admin.pubkey(), true),
                        AccountMeta::new(self.env.market, false),
                        AccountMeta::new(self.env.vault, false),
                        AccountMeta::new_readonly(self.env.vault_authority, false),
                        AccountMeta::new(primary_dest, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(self.secondary_vault, false),
                        AccountMeta::new(secondary_dest, false),
                        AccountMeta::new(self.env.mint, false),
                    ],
                    &[&admin],
                )
                .expect("the market authority closes the drained slab");
            let closed = self
                .env
                .svm
                .get_account(&self.env.market)
                .map_or(true, |market| {
                    market.data.len() <= percolator_prog::constants::HEADER_LEN
                });
            if closed {
                return (
                    self.env.token_amount(primary_dest),
                    self.env.token_amount(secondary_dest),
                );
            }
        }
        panic!("CloseSlab did not retire the drained slab in eight calls");
    }
}

fn is_invalid_mint_error(error: &str) -> bool {
    error.contains(&format!("Custom({})", PercolatorError::InvalidMint as u32))
}

/// Nobody but the owner can have its claim paid in the secondary mint. Unsigned, the close
/// pays the deposit mint, and nothing of the deposit is left for the authority to sweep.
#[test]
fn v16_attack_unsigned_resolved_close_cannot_pay_the_secondary_mint() {
    for route in CLOSE_ROUTES {
        for delay in [0u64, 5] {
            let label = format!("{route:?}, owner window {delay}");
            let mut world = resolved_market_with_secondary_rail(delay);
            let before = world.frame();
            let error = world
                .close(route, world.secondary_dest, world.secondary_vault, false)
                .expect_err("an unsigned close must not choose the secondary mint");
            assert!(is_invalid_mint_error(&error), "{label}: {error}");
            assert_eq!(
                world.frame(),
                before,
                "{label}: the refused close rolls back"
            );

            let cu = world
                .close(route, world.primary_dest, world.env.vault, false)
                .unwrap_or_else(|error| {
                    panic!("{label}: the unsigned close pays the deposit mint: {error}")
                });
            assert_cu_within("unsigned primary resolved close", cu, CUSTODY_CU_LIMIT);
            assert_eq!(
                world.env.token_amount(world.primary_dest),
                DEPOSIT,
                "{label}"
            );
            assert_eq!(world.env.token_amount(world.secondary_dest), 0, "{label}");
            assert_eq!(world.env.token_amount(world.env.vault), 0, "{label}");
            assert_eq!(
                world.env.token_amount(world.secondary_vault),
                DEPOSIT,
                "{label}"
            );
            assert!(
                resolved_portfolio_is_terminal(&world.env, world.portfolio),
                "{label}"
            );

            let (primary, secondary) = world.retire_market();
            assert_eq!(
                primary, 0,
                "{label}: none of the deposit goes to the authority"
            );
            assert_eq!(
                secondary, DEPOSIT,
                "{label}: the secondary vault is returned whole"
            );
        }
    }
}

/// The owner still chooses the mint with its signature. The deposit it leaves in the primary
/// vault is then surplus, which `CloseSlab` pays to the authority.
#[test]
fn v16_program_owner_signed_resolved_close_may_take_the_secondary_mint() {
    for route in CLOSE_ROUTES {
        let label = format!("{route:?}");
        let mut world = resolved_market_with_secondary_rail(0);
        let cu = world
            .close(route, world.secondary_dest, world.secondary_vault, true)
            .unwrap_or_else(|error| {
                panic!("{label}: the owner may take the secondary mint: {error}")
            });
        assert_cu_within(
            "owner-signed secondary resolved close",
            cu,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(
            world.env.token_amount(world.secondary_dest),
            DEPOSIT,
            "{label}"
        );
        assert_eq!(world.env.token_amount(world.primary_dest), 0, "{label}");
        assert_eq!(world.env.token_amount(world.env.vault), DEPOSIT, "{label}");
        assert_eq!(world.env.token_amount(world.secondary_vault), 0, "{label}");
        assert!(
            resolved_portfolio_is_terminal(&world.env, world.portfolio),
            "{label}"
        );

        let (primary, secondary) = world.retire_market();
        assert_eq!((primary, secondary), (DEPOSIT, 0), "{label}");
    }
}

struct PendingTopup {
    env: V16CuEnv,
    owner: Keypair,
    portfolio: Pubkey,
    secondary_vault: Pubkey,
    secondary_dest: Pubkey,
    primary_dest: Pubkey,
}

/// A receipt that is owed a top-up of 60: a payout snapshot of 100 with 40 paid and 60 in
/// the engine vault. The state is installed directly, as in the INV-018 top-up fixture
/// above; both vaults hold 60.
fn pending_topup_with_secondary_rail() -> PendingTopup {
    let mut env = V16CuEnv::new();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    let secondary_vault = fund_secondary_vault(&mut env, secondary, 60);
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let mut market_account = env.svm.get_account(&env.market).unwrap();
    let mut portfolio_account = env.svm.get_account(&portfolio).unwrap();
    let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
    let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
    group.mode = MarketModeV16::Resolved;
    group.resolved_slot = 1;
    group.current_slot = 1;
    group.vault = 60;
    group.payout_snapshot_captured = true;
    group.payout_snapshot = 100;
    group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
        snapshot_residual: 100,
        terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
        terminal_claim_bound_unreceipted_num: 0,
        current_payout_rate_num: 100 * BOUND_SCALE,
        current_payout_rate_den: 100 * BOUND_SCALE,
        snapshot_slot: 1,
        payout_halted: false,
        finalized: false,
    };
    account.resolved_payout_receipt =
        percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
            present: true,
            prior_bound_contribution_num: 100 * BOUND_SCALE,
            live_released_face_at_receipt: 0,
            terminal_positive_claim_face: 100,
            paid_effective: 40,
            finalized: false,
        });
    state::write_market(&mut market_account.data, &cfg, &group).unwrap();
    state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
    env.svm.set_account(env.market, market_account).unwrap();
    env.svm.set_account(portfolio, portfolio_account).unwrap();
    let secondary_dest = env.token_account_for_mint(secondary, owner.pubkey(), 0);
    let primary_dest = env.token_account(owner.pubkey(), 0);
    PendingTopup {
        env,
        owner,
        portfolio,
        secondary_vault,
        secondary_dest,
        primary_dest,
    }
}

impl PendingTopup {
    fn claim(&mut self, dest: Pubkey, vault: Pubkey, signed: bool) -> Result<u64, String> {
        self.env.svm.expire_blockhash();
        let signers: Vec<&Keypair> = if signed { vec![&self.owner] } else { vec![] };
        self.env.send(
            ProgInstruction::ClaimResolvedPayoutTopup,
            vec![
                AccountMeta::new_readonly(self.owner.pubkey(), signed),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &signers,
        )
    }

    fn frame(&self) -> Vec<Option<Account>> {
        [
            self.env.market,
            self.portfolio,
            self.env.vault,
            self.secondary_vault,
            self.secondary_dest,
            self.primary_dest,
        ]
        .map(|key| self.env.svm.get_account(&key))
        .to_vec()
    }

    fn receipt_is_paid_in_full(&self) -> bool {
        let account = self.env.portfolio_state(self.portfolio);
        let receipt = resolved_receipt(&account);
        receipt.paid_effective == 100 && receipt.finalized
    }
}

/// Anyone can claim a receipt top-up for its owner. Unsigned, it is paid in the deposit mint.
#[test]
fn v16_attack_unsigned_topup_claim_cannot_pay_the_secondary_mint() {
    let mut world = pending_topup_with_secondary_rail();
    let before = world.frame();
    let error = world
        .claim(world.secondary_dest, world.secondary_vault, false)
        .expect_err("an unsigned top-up claim must not choose the secondary mint");
    assert!(is_invalid_mint_error(&error), "{error}");
    assert_eq!(world.frame(), before, "the refused claim rolls back");

    world
        .claim(world.primary_dest, world.env.vault, false)
        .expect("the unsigned top-up claim pays the deposit mint");
    assert_eq!(world.env.token_amount(world.primary_dest), 60);
    assert_eq!(world.env.token_amount(world.secondary_dest), 0);
    assert_eq!(world.env.token_amount(world.env.vault), 0);
    assert_eq!(world.env.token_amount(world.secondary_vault), 60);
    assert_eq!(world.env.market_state().1.vault, 0);
    assert!(world.receipt_is_paid_in_full());
}

/// The owner's signature still selects the secondary mint for its top-up.
#[test]
fn v16_program_owner_signed_topup_claim_may_take_the_secondary_mint() {
    let mut world = pending_topup_with_secondary_rail();
    world
        .claim(world.secondary_dest, world.secondary_vault, true)
        .expect("the owner may take its top-up in the secondary mint");
    assert_eq!(world.env.token_amount(world.secondary_dest), 60);
    assert_eq!(world.env.token_amount(world.primary_dest), 0);
    assert_eq!(world.env.token_amount(world.secondary_vault), 0);
    assert_eq!(world.env.token_amount(world.env.vault), 60);
    assert_eq!(world.env.market_state().1.vault, 0);
    assert!(world.receipt_is_paid_in_full());
}
