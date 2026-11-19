//! INV-018: a reserve claim is paid in the secondary mint only with its payee's signature,
//! and a live payee can always finish the wind-down and take its claim itself (#477).
//!
//! After resolution anyone can deliver a backing bucket or a terminal insurance budget to a
//! token account its beneficiary owns. `marketauth` names the secondary mint, so a delivery
//! nobody signed is made in the deposit mint. Before this rule the market authority could
//! remove the last empty portfolio and deliver a third party's reserve in its own mint in one
//! transaction, while the beneficiary's own signed withdrawal was refused until that portfolio
//! was gone and only its owner or the authority could remove it.
//!
//! Every wrapper instruction is public. SPL mints and token accounts, the secondary vault's
//! balance among them, are installed directly as in the other INV-018 fixtures.

use super::*;

const RESERVE: u64 = 100_000;
const DEPOSIT: u64 = 1_000_000;

struct ReserveRails {
    env: V16CuEnv,
    backing_authority: Keypair,
    insurance_authority: Keypair,
    secondary: Pubkey,
    secondary_vault: Pubkey,
    /// A depositor that is paid out in the deposit mint and then goes away, leaving an
    /// empty portfolio account behind. Present only when the fixture is asked for one.
    absent_owner: Option<(Keypair, Pubkey)>,
}

/// Third parties hold the backing and insurance roles of asset 1 and fund them in the
/// deposit mint. The market authority has named a secondary mint and funds its vault. The
/// market is resolved; with `absent_owner` a depositor has been paid and has left its empty
/// portfolio in place.
fn resolved_reserves(owner_window: u64, absent_owner: bool) -> ReserveRails {
    let mut env = V16CuEnv::new();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    env.configure_auth_mark_with_cu(0, 100);
    if owner_window != 0 {
        env.configure_permissionless_resolve_with_cu(100, owner_window);
    }
    let backing_authority = Keypair::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    for key in [
        &backing_authority,
        &insurance_authority,
        &insurance_operator,
    ] {
        env.svm.airdrop(&key.pubkey(), 1_000_000_000).unwrap();
    }
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        backing_authority.pubkey(),
        env.admin.pubkey(),
    );
    env.top_up_insurance_domain_with_authority(&insurance_authority, 2, RESERVE.into());
    env.top_up_backing_bucket_with_authority(&backing_authority, 2, RESERVE.into(), 1_000_000);
    let owner = absent_owner.then(|| {
        let owner = Keypair::new();
        let portfolio = env.create_portfolio(&owner);
        env.deposit(&owner, portfolio, DEPOSIT.into());
        (owner, portfolio)
    });
    env.resolve();
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 2 * RESERVE),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    ReserveRails {
        env,
        backing_authority,
        insurance_authority,
        secondary,
        secondary_vault,
        absent_owner: owner,
    }
}

impl ReserveRails {
    /// `WithdrawBackingBucket` for the backing authority's whole bucket, naming its key and
    /// signed by it only when `signed`.
    fn backing_ix(&self, dest: Pubkey, vault: Pubkey, signed: bool) -> Instruction {
        let payee = self.backing_authority.pubkey();
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(payee, signed),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawBackingBucket {
                domain: 2,
                market_id: self.env.asset_market_id(1),
                authority_epoch: self.env.withdrawal_authority_epoch(payee, 1, false),
                amount: RESERVE.into(),
            }
            .encode(),
        }
    }

    /// `WithdrawInsuranceAsset` for the insurance authority's whole budget, likewise.
    fn insurance_ix(&self, dest: Pubkey, vault: Pubkey, signed: bool) -> Instruction {
        let payee = self.insurance_authority.pubkey();
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(payee, signed),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::WithdrawInsuranceAsset {
                asset_index: 1,
                market_id: self.env.asset_market_id(1),
                authority_epoch: self.env.withdrawal_authority_epoch(payee, 1, true),
                amount: RESERVE.into(),
            }
            .encode(),
        }
    }

    /// `ClosePortfolio` for `portfolio`, signed by `closer`.
    fn delete_ix(&self, closer: Pubkey, portfolio: Pubkey) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(closer, true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            data: self.env.close_portfolio_ix(portfolio).encode(),
        }
    }

    fn land(&mut self, instructions: Vec<Instruction>, signers: &[&Keypair]) -> Result<(), String> {
        self.env.svm.expire_blockhash();
        send_raw_ixs(&mut self.env.svm, &self.env.payer, instructions, signers)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }

    /// Anyone pays the absent owner in the deposit mint. Its portfolio is then empty.
    fn pay_the_absent_owner(&mut self) -> Pubkey {
        let (owner, portfolio) = self
            .absent_owner
            .as_ref()
            .map(|(owner, portfolio)| (owner.pubkey(), *portfolio))
            .unwrap();
        let dest = self.env.token_account(owner, 0);
        let close = Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            }
            .encode(),
        };
        self.land(vec![close], &[])
            .expect("the unsigned close pays the absent owner in the deposit mint");
        assert_eq!(self.env.token_amount(dest), DEPOSIT);
        assert!(resolved_portfolio_is_terminal(&self.env, portfolio));
        portfolio
    }

    fn frame(&self, extra: &[Pubkey]) -> Vec<Option<Account>> {
        [self.env.market, self.env.vault, self.secondary_vault]
            .iter()
            .chain(extra)
            .map(|key| self.env.svm.get_account(key))
            .collect()
    }
}

fn is_error(error: &str, expected: PercolatorError) -> bool {
    error.contains(&format!("Custom({})", expected as u32))
}

/// Unsigned, a reserve claim is delivered in the deposit mint, and nothing is left in the
/// primary vault for the market authority.
#[test]
fn v16_attack_unsigned_reserve_payouts_cannot_pay_the_secondary_mint() {
    let mut world = resolved_reserves(0, false);
    let (backing, insurance) = (
        world.backing_authority.pubkey(),
        world.insurance_authority.pubkey(),
    );
    let secondary = [
        world
            .env
            .token_account_for_mint(world.secondary, backing, 0),
        world
            .env
            .token_account_for_mint(world.secondary, insurance, 0),
    ];
    let primary = [
        world.env.token_account(backing, 0),
        world.env.token_account(insurance, 0),
    ];
    assert_eq!(world.env.token_amount(world.env.vault), 2 * RESERVE);

    let before = world.frame(&secondary);
    let ix = world.backing_ix(secondary[0], world.secondary_vault, false);
    let error = world
        .land(vec![ix], &[])
        .expect_err("an unsigned backing delivery must not choose the secondary mint");
    assert!(is_error(&error, PercolatorError::InvalidMint), "{error}");
    let ix = world.insurance_ix(secondary[1], world.secondary_vault, false);
    let error = world
        .land(vec![ix], &[])
        .expect_err("an unsigned insurance delivery must not choose the secondary mint");
    assert!(is_error(&error, PercolatorError::InvalidMint), "{error}");
    assert_eq!(
        world.frame(&secondary),
        before,
        "the refused deliveries roll back"
    );

    let ix = world.backing_ix(primary[0], world.env.vault, false);
    world
        .land(vec![ix], &[])
        .expect("the unsigned backing delivery pays the deposit mint");
    let ix = world.insurance_ix(primary[1], world.env.vault, false);
    world
        .land(vec![ix], &[])
        .expect("the unsigned insurance delivery pays the deposit mint");
    for dest in primary {
        assert_eq!(world.env.token_amount(dest), RESERVE);
    }
    for dest in secondary {
        assert_eq!(world.env.token_amount(dest), 0);
    }
    assert_eq!(world.env.token_amount(world.env.vault), 0);
    assert_eq!(world.env.token_amount(world.secondary_vault), 2 * RESERVE);
    assert_eq!(world.env.market_state().1.vault, 0);
}

/// Each payee still chooses the mint with its own signature.
#[test]
fn v16_program_reserve_payees_may_take_the_secondary_mint_with_their_signature() {
    let mut world = resolved_reserves(0, false);
    let secondary = [
        world
            .env
            .token_account_for_mint(world.secondary, world.backing_authority.pubkey(), 0),
        world
            .env
            .token_account_for_mint(world.secondary, world.insurance_authority.pubkey(), 0),
    ];
    let backing_authority = world.backing_authority.insecure_clone();
    let insurance_authority = world.insurance_authority.insecure_clone();
    let ix = world.backing_ix(secondary[0], world.secondary_vault, true);
    world
        .land(vec![ix], &[&backing_authority])
        .expect("the backing authority may take its bucket in the secondary mint");
    let ix = world.insurance_ix(secondary[1], world.secondary_vault, true);
    world
        .land(vec![ix], &[&insurance_authority])
        .expect("the insurance authority may take its budget in the secondary mint");
    for dest in secondary {
        assert_eq!(world.env.token_amount(dest), RESERVE);
    }
    assert_eq!(world.env.token_amount(world.secondary_vault), 0);
    assert_eq!(world.env.token_amount(world.env.vault), 2 * RESERVE);
    assert_eq!(world.env.market_state().1.vault, 0);
}

/// The market authority removes the last empty portfolio and, in the same transaction,
/// delivers a third party's insurance in its own mint. That left the beneficiary no moment in
/// which it could act first. The bundle is now refused whole, and the beneficiary finishes the
/// wind-down itself.
#[test]
fn v16_attack_cleanup_cannot_be_bundled_with_a_delivery_in_the_authority_mint() {
    let mut world = resolved_reserves(0, true);
    let admin = world.env.admin.insecure_clone();
    let insurance_authority = world.insurance_authority.insecure_clone();
    let portfolio = world.pay_the_absent_owner();
    let junk = world
        .env
        .token_account_for_mint(world.secondary, insurance_authority.pubkey(), 0);
    let real = world.env.token_account(insurance_authority.pubkey(), 0);
    assert_eq!(world.env.market_state().1.materialized_portfolio_count, 1);

    // While the empty portfolio exists the budget cannot be withdrawn, signed or not.
    let ix = world.insurance_ix(real, world.env.vault, true);
    let error = world
        .land(vec![ix], &[&insurance_authority])
        .expect_err("terminal insurance waits for the last portfolio to be removed");
    assert!(
        is_error(&error, PercolatorError::EngineLockActive),
        "{error}"
    );

    let before = world.frame(&[portfolio, junk, real]);
    let bundle = vec![
        world.delete_ix(admin.pubkey(), portfolio),
        world.insurance_ix(junk, world.secondary_vault, false),
    ];
    let error = world
        .land(bundle, &[&admin])
        .expect_err("the authority must not deliver the budget in its own mint");
    assert!(is_error(&error, PercolatorError::InvalidMint), "{error}");
    assert_eq!(
        world.frame(&[portfolio, junk, real]),
        before,
        "the refused bundle rolls back, the portfolio included"
    );

    // The beneficiary removes the empty portfolio and takes its budget in one transaction.
    let exit = vec![
        world.delete_ix(insurance_authority.pubkey(), portfolio),
        world.insurance_ix(real, world.env.vault, true),
    ];
    world
        .land(exit, &[&insurance_authority])
        .expect("a live beneficiary finishes the wind-down and exits by itself");
    assert_eq!(world.env.token_amount(real), RESERVE);
    assert_eq!(world.env.token_amount(junk), 0);
    assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    assert_eq!(world.env.market_state().1.insurance, 0);
}

/// Deleting an empty portfolio: its owner always may, the market authority may once the
/// market is resolved, and anyone may once the owner window has passed. Nobody but the
/// owner can delete a portfolio that still holds something.
#[test]
fn v16_program_anyone_deletes_an_empty_portfolio_once_the_owner_window_has_passed() {
    const WINDOW: u64 = 50;
    // The window has passed, but the portfolio still holds its deposit: nobody removes it.
    let mut world = resolved_reserves(WINDOW, true);
    let stranger = world.insurance_authority.insecure_clone();
    let portfolio = world.absent_owner.as_ref().unwrap().1;
    let resolved_slot = world.env.market_state().1.resolved_slot;
    world.env.svm.warp_to_slot(resolved_slot + WINDOW);
    let ix = world.delete_ix(stranger.pubkey(), portfolio);
    let error = world
        .land(vec![ix], &[&stranger])
        .expect_err("a portfolio that holds capital is not removed");
    assert!(
        is_error(&error, PercolatorError::EngineLockActive),
        "{error}"
    );

    // A second world: the owner closes inside its window, leaving an empty portfolio.
    let mut world = resolved_reserves(WINDOW, true);
    let stranger = world.insurance_authority.insecure_clone();
    let owner = world.absent_owner.as_ref().unwrap().0.insecure_clone();
    let portfolio = world.absent_owner.as_ref().unwrap().1;
    let resolved_slot = world.env.market_state().1.resolved_slot;
    let dest = world.env.token_account(owner.pubkey(), 0);
    let close = Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(owner.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(world.env.vault, false),
            AccountMeta::new_readonly(world.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
        .encode(),
    };
    world
        .land(vec![close], &[&owner])
        .expect("the owner closes inside its window");
    assert!(resolved_portfolio_is_terminal(&world.env, portfolio));

    world.env.svm.warp_to_slot(resolved_slot + WINDOW - 1);
    let before = world.frame(&[portfolio]);
    let ix = world.delete_ix(stranger.pubkey(), portfolio);
    let error = world
        .land(vec![ix], &[&stranger])
        .expect_err("inside the owner window a stranger cannot remove the portfolio");
    assert!(is_error(&error, PercolatorError::Unauthorized), "{error}");
    assert_eq!(world.frame(&[portfolio]), before);

    world.env.svm.warp_to_slot(resolved_slot + WINDOW);
    let market_lamports = world
        .env
        .svm
        .get_account(&world.env.market)
        .unwrap()
        .lamports;
    let portfolio_lamports = world.env.svm.get_account(&portfolio).unwrap().lamports;
    let stranger_lamports = world
        .env
        .svm
        .get_account(&stranger.pubkey())
        .unwrap()
        .lamports;
    let ix = world.delete_ix(stranger.pubkey(), portfolio);
    world
        .land(vec![ix], &[&stranger])
        .expect("once the window has passed anyone removes an empty portfolio");
    assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    assert_eq!(
        world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports,
        market_lamports + portfolio_lamports,
        "the rent goes to the market, as when the authority cleans up"
    );
    assert_eq!(
        world
            .env
            .svm
            .get_account(&stranger.pubkey())
            .unwrap()
            .lamports,
        stranger_lamports,
        "the caller gains nothing"
    );
}

/// In a Live market only its owner deletes a portfolio.
#[test]
fn v16_attack_live_portfolio_is_deleted_by_its_owner_alone() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let stranger = Keypair::new();
    env.svm.airdrop(&stranger.pubkey(), 1_000_000_000).unwrap();
    let admin = env.admin.insecure_clone();
    for closer in [&stranger, &admin] {
        env.svm.expire_blockhash();
        let error = env
            .send(
                env.close_portfolio_ix(portfolio),
                vec![
                    AccountMeta::new(closer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[closer],
            )
            .expect_err("only the owner deletes a portfolio in a Live market");
        assert!(is_error(&error, PercolatorError::Unauthorized), "{error}");
    }
    env.close_portfolio_with_cu(&owner, portfolio);
}
