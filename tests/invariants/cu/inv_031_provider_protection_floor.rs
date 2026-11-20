//! INV-031: a provider loss is recorded once (percolator-prog#382).
//!
//! A backing provider's protection is the principal that neither a claim on its
//! domain nor an unrepaid consumption has taken. Public instructions lower it (a
//! winner cranked before its loser has its gain fronted by the provider), refill
//! it (the loser is cranked) and lower it again (the next winner's claim spends
//! the refill). A program that keeps depositor shares over one provider position
//! can only read the market when one of its own users acts. A low that was
//! refilled before the next read is gone from the current state, and the
//! counters `SyncBackingDomainLedger` observes do not move at all while a loss is
//! represented by claim encumbrance alone. So the same history charged a loss
//! once or twice depending on when somebody looked.
//!
//! The market keeps, for every backing domain and every generation of its asset,
//! the lowest protection the domain has had, moved only by the provider's own
//! top-ups and principal withdrawals. `state::read_provider_protection` reads it
//! from the market account. No instruction takes an extra account for it.

use super::*;

/// Short-loss domain of asset 0: where a long's gain becomes a claim.
const DOMAIN: u16 = 1;
const PRINCIPAL: u128 = 16;
const OPEN_PRICE: u64 = 100;
const MOVED_PRICE: u64 = 101;
const EXPIRY: u64 = 1_000;

fn protection(env: &V16CuEnv, domain: u16) -> state::ProviderProtectionV16 {
    let market = env.svm.get_account(&env.market).expect("market account");
    state::read_provider_protection(&market.data, usize::from(domain)).expect("market reads")
}

/// `(protection, floor)` of the domain in whole atoms.
fn seen(env: &V16CuEnv, domain: u16) -> (u128, u128) {
    let read = protection(env, domain);
    assert_eq!(read.protection_num % BOUND_SCALE, 0);
    assert_eq!(read.floor_num % BOUND_SCALE, 0);
    assert!(read.floor_num <= read.protection_num && read.protection_num <= read.principal_num);
    (
        read.protection_num / BOUND_SCALE,
        read.floor_num / BOUND_SCALE,
    )
}

fn certificate_is_current(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let group = env.market_state().1;
    let state = env.portfolio_state(portfolio);
    let cert = health_cert(&state);
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&state)
}

/// Cranks one portfolio until its certificate is current. A crank takes the
/// market and the portfolio: no ledger account.
fn refresh(env: &mut V16CuEnv, portfolio: Pubkey, slot: u64) {
    env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(0),
        },
    );
    for _ in 0..4 {
        if certificate_is_current(env, portfolio) {
            return;
        }
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: vec![],
            },
        );
    }
    assert!(certificate_is_current(env, portfolio));
}

struct Staggered {
    env: V16CuEnv,
    owners: Vec<Keypair>,
    /// 0 is long 2 against 1 and 2; 3 is long 1 against 4.
    portfolios: Vec<Pubkey>,
    ledger: Pubkey,
    slot: u64,
}

/// A provider put 16 atoms into the short-loss domain of asset 0. Portfolio 0 is
/// long 2 units against portfolios 1 and 2 (short 1 each), portfolio 3 is long 1
/// against portfolio 4. The mark then rises by one atom per unit and nobody is
/// cranked: portfolio 0 has gained 2, portfolio 3 has gained 1, and portfolios
/// 1, 2 and 4 owe 1 each.
fn staggered() -> Staggered {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    for asset_index in [0u16, 1] {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, OPEN_PRICE);
    }
    let owners: Vec<Keypair> = (0..5).map(|_| Keypair::new()).collect();
    let portfolios: Vec<Pubkey> = owners
        .iter()
        .map(|owner| env.create_portfolio(owner))
        .collect();
    for (owner, portfolio) in owners.iter().zip(&portfolios) {
        env.deposit(owner, *portfolio, 10_000);
    }
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, DOMAIN, PRINCIPAL, EXPIRY);
    assert_eq!(seen(&env, DOMAIN), (PRINCIPAL, PRINCIPAL));
    for (long, short) in [(0usize, 1usize), (0, 2), (3, 4)] {
        env.trade_asset_with_cu(
            0,
            &owners[long],
            portfolios[long],
            &owners[short],
            portfolios[short],
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );
    }
    let slot = 1;
    env.svm.warp_to_slot(slot);
    env.push_auth_mark_for_asset_as_admin(0, slot, MOVED_PRICE);
    env.push_auth_mark_for_asset_as_admin(1, slot, OPEN_PRICE);
    assert_eq!(seen(&env, DOMAIN), (PRINCIPAL, PRINCIPAL));
    Staggered {
        env,
        owners,
        portfolios,
        ledger,
        slot,
    }
}

impl Staggered {
    fn refresh(&mut self, index: usize) -> (u128, u128) {
        let (portfolio, slot) = (self.portfolios[index], self.slot);
        refresh(&mut self.env, portfolio, slot);
        seen(&self.env, DOMAIN)
    }

    /// What `SyncBackingDomainLedger` has recorded as lost and recovered.
    fn sync_ledger(&mut self) -> (u128, u128) {
        let ledger = self.ledger;
        self.env.svm.expire_blockhash();
        self.env.sync_backing_domain_ledger_with_cu(ledger, DOMAIN);
        let account = self.env.svm.get_account(&ledger).expect("ledger account");
        let ledger = state::read_backing_domain_ledger(&account.data).expect("ledger reads");
        (
            ledger.cumulative_loss_atoms,
            ledger.cumulative_recovery_atoms,
        )
    }
}

/// The first two required tests of #382 through the program. Protection goes
/// 16 -> 14 when a winner is cranked first, 14 -> 15 when one loser is cranked,
/// and 15 -> 14 when the next winner's claim spends the refill. The floor reads
/// 14 from the first step on and is still 14 when everybody has paid and
/// protection is 16 again.
///
/// The same history with `SyncBackingDomainLedger` run at the low, after the
/// refill, at both, or not at all gives the same floors at every step. The
/// ledger itself records no loss and no recovery in any of them: the counters it
/// watches do not see a loss that is a claim on the backing.
#[test]
fn v16_program_provider_protection_floor_records_a_refilled_loss_once() {
    let run = |sync_at: &[usize]| {
        let mut world = staggered();
        let mut floors = Vec::new();
        for (step, index) in [0usize, 1, 3, 2, 4].into_iter().enumerate() {
            floors.push(world.refresh(index));
            if sync_at.contains(&step) {
                assert_eq!(
                    world.sync_ledger(),
                    (0, 0),
                    "the ledger's counters do not see a loss held as a claim"
                );
                assert_eq!(seen(&world.env, DOMAIN), floors[step]);
            }
        }
        let read = protection(&world.env, DOMAIN);
        assert_eq!(read.principal_num, PRINCIPAL * BOUND_SCALE);
        assert_eq!(read.market_id, world.env.asset_market_id(0));
        assert_eq!(world.sync_ledger(), (0, 0));
        // The twin domain and the other asset never had a provider.
        for domain in [0u16, 2, 3] {
            assert_eq!(seen(&world.env, domain), (0, 0));
        }
        floors
    };
    let unobserved = run(&[]);
    assert_eq!(
        unobserved,
        vec![(14, 14), (15, 14), (14, 14), (15, 14), (16, 14)]
    );
    assert_eq!(run(&[0]), unobserved, "synced at the low");
    assert_eq!(run(&[1]), unobserved, "synced after the refill");
    assert_eq!(run(&[0, 1, 2, 3, 4]), unobserved, "synced after every step");
}

/// A top-up raises the floor by the top-up and a principal withdrawal lowers it
/// by the withdrawal. The loss on record, principal less floor, moves with
/// neither, and a later claim inside the refill records nothing.
#[test]
fn v16_program_provider_protection_floor_moves_with_the_providers_own_flows() {
    let mut world = staggered();
    assert_eq!(world.refresh(0), (14, 14));
    let lost = |env: &V16CuEnv| {
        let read = protection(env, DOMAIN);
        (read.principal_num - read.floor_num) / BOUND_SCALE
    };
    assert_eq!(lost(&world.env), 2);

    world.env.top_up_backing_bucket(DOMAIN, 10, EXPIRY);
    assert_eq!(seen(&world.env, DOMAIN), (24, 24));
    assert_eq!(lost(&world.env), 2);

    // Every position is settled before the provider may withdraw in Live.
    assert_eq!(world.refresh(1), (25, 24));
    assert_eq!(world.refresh(2), (26, 24));
    assert_eq!(world.refresh(3), (25, 24));
    assert_eq!(world.refresh(4), (26, 24));

    let admin = world.env.admin.insecure_clone();
    let destination = Pubkey::new_unique();
    world
        .env
        .svm
        .set_account(
            destination,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(world.env.mint, admin.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let withdraw = |env: &mut V16CuEnv, amount: u128| {
        let instruction = ProgInstruction::WithdrawBackingBucket {
            domain: DOMAIN,
            market_id: env.asset_market_id(DOMAIN / 2),
            authority_epoch: env.withdrawal_authority_epoch(
                admin.pubkey(),
                DOMAIN as usize / 2,
                false,
            ),
            amount,
        };
        env.svm.expire_blockhash();
        let (market, vault, vault_authority) = (env.market, env.vault, env.vault_authority);
        env.send(
            instruction,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };
    withdraw(&mut world.env, 5).expect("principal beyond the claims may leave");
    assert_eq!(seen(&world.env, DOMAIN), (21, 19));
    assert_eq!(lost(&world.env), 2);

    // The provider leaves with its whole principal; the claims stay backed by
    // what the losers paid. The floor follows the principal down and stops at
    // zero: 2 of what the provider took out were refilled, not protected.
    withdraw(&mut world.env, 21).expect("the provider's own principal");
    let read = protection(&world.env, DOMAIN);
    assert_eq!(
        (read.principal_num, read.protection_num, read.floor_num),
        (0, 0, 0)
    );
    assert_eq!(world.env.token_amount(destination), 26);
    let source = world.env.market_state().1.source_credit[DOMAIN as usize];
    assert_eq!(source.positive_claim_bound_num, 3 * BOUND_SCALE);
    assert_eq!(source.fresh_reserved_backing_num, 3 * BOUND_SCALE);
    let _ = &world.owners;
}

/// A closing trade settles a winner and the loser that pays it in one
/// instruction. Between the two the domain reads lower by the gain; nobody could
/// have read the market in between, and nothing is recorded.
#[test]
fn v16_program_provider_protection_floor_ignores_a_gain_settled_with_its_loss() {
    let mut world = staggered();
    // A crank of an account with no position applies the new mark to the market
    // and settles nobody.
    let observer = world.env.create_portfolio(&Keypair::new());
    let slot = world.slot;
    world.env.crank(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(0),
        },
    );
    assert_eq!(
        world.env.market_state().1.assets[0].effective_price,
        MOVED_PRICE
    );
    assert_eq!(seen(&world.env, DOMAIN), (PRINCIPAL, PRINCIPAL));

    // Portfolio 3 (long 1, gained 1) closes against portfolio 4 (short 1, owes 1).
    let capital_before = [3usize, 4].map(|index| {
        world
            .env
            .portfolio_state(world.portfolios[index])
            .capital
            .get()
    });
    world.env.trade_asset_with_cu(
        0,
        &world.owners[4],
        world.portfolios[4],
        &world.owners[3],
        world.portfolios[3],
        POS_SCALE as i128,
        MOVED_PRICE,
        0,
    );
    let source = world.env.market_state().1.source_credit[DOMAIN as usize];
    let (claims, backing) = (
        source.positive_claim_bound_num / BOUND_SCALE,
        source.fresh_reserved_backing_num / BOUND_SCALE,
    );
    let loser = world.env.portfolio_state(world.portfolios[4]);
    assert_eq!(
        (loser.capital.get(), loser.pnl.get()),
        (capital_before[1] - 1, 0),
        "the short paid its loss in this instruction"
    );
    assert_eq!(
        (claims, backing),
        (1, PRINCIPAL + 1),
        "the long's gain is a claim, and the short's atom is behind it"
    );
    assert_eq!(seen(&world.env, DOMAIN), (PRINCIPAL, PRINCIPAL));

    // The same kind of gain cranked on its own, before its losers, is recorded.
    assert_eq!(world.refresh(0), (14, 14));
    assert_eq!(world.refresh(1), (15, 14));
    assert_eq!(world.refresh(2), (16, 14));
}
