//! INV-031: what a domain's losers pay in backs the gains it is owed to. A
//! backing provider cannot take it out before the winner is refreshed.
//!
//! The engine's provider exit keeps `available backing >= registered claims`,
//! and a gain is registered as a claim only when its account is refreshed.
//! Between the loser's refresh and the winner's, the loser's payment is covered
//! by no claim. `WithdrawBackingBucket` used to let the backing authority take
//! it, and the winner's claim was then registered against no backing.
//!
//! Two layers close it. The engine caps the exit at provider principal while a
//! position on the asset is unsettled. The wrapper's Live withdrawal gate
//! refuses outright in that window: it used to read only the market-wide
//! loss-stale flag, which summarizes the asset touched last, so one crank on
//! another, current asset cleared it while the winner here was still
//! unrefreshed. The fixture reproduces exactly that: asset 1 is touched last.
//!
//! A payment can also outlive its claim. If the move reverses before the winner
//! is refreshed, the winner's gain is gone and nobody ever claims the payment
//! where it sits; the payer's regain is claimed on the twin domain, which the
//! other side never funds. With every position settled the engine treated the
//! payment as surplus and the backing authority could withdraw it, with or
//! without principal of its own in the domain. The engine now leaves in a
//! domain what its losers paid beyond its claims for as long as the twin's
//! claims are short of it.

use super::*;

/// Short-loss domain of asset 0: where the long's gain becomes a claim.
const DOMAIN: u16 = 1;
const PRINCIPAL: u128 = 1_000;
const UNITS: u128 = 100;
const OPEN_PRICE: u64 = 100;
const MOVED_PRICE: u64 = 104;
/// What the short pays in when it is refreshed at `MOVED_PRICE`.
const PAYMENT: u128 = UNITS * (MOVED_PRICE - OPEN_PRICE) as u128;

struct LoserPaidWinnerStale {
    env: V16CuEnv,
    long: Pubkey,
    short: Pubkey,
    slot: u64,
    destination: Pubkey,
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

/// A long of 100 against a short on asset 0, the mark up 4%, and only the short
/// refreshed: its 400 sits in the domain beside `principal` of provider backing,
/// and the long's gain is not a claim yet. Asset 1 is flat and current, and it
/// is the asset touched last.
fn loser_paid_winner_not_refreshed(principal: u128) -> LoserPaidWinnerStale {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    for asset_index in [0u16, 1] {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 0, OPEN_PRICE);
    }
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let observer = env.create_portfolio(&Keypair::new());
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 10_000);
    if principal != 0 {
        env.top_up_backing_bucket(DOMAIN, principal, 1_000);
    }
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        (UNITS * POS_SCALE) as i128,
        OPEN_PRICE,
        0,
    );
    let slot = 1;
    env.svm.warp_to_slot(slot);
    env.push_auth_mark_for_asset_as_admin(0, slot, MOVED_PRICE);
    env.push_auth_mark_for_asset_as_admin(1, slot, OPEN_PRICE);
    refresh(&mut env, short, slot);
    // Any crank that commits asset 1's mark leaves the market-wide flag
    // describing asset 1, where nobody is stale.
    env.crank(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations(1),
        },
    );
    let group = env.market_state().1;
    assert_ne!(
        group.assets[0].stale_account_count_long, 0,
        "the long on asset 0 is still unrefreshed"
    );

    let source = env.market_state().1.source_credit[DOMAIN as usize];
    assert_eq!(
        source.fresh_reserved_backing_num,
        (principal + PAYMENT) * BOUND_SCALE,
        "the short's payment sits beside the provider's principal"
    );
    assert_eq!(
        source.positive_claim_bound_num, 0,
        "the long has not been refreshed: no claim is registered"
    );

    let destination = Pubkey::new_unique();
    env.svm
        .set_account(
            destination,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    LoserPaidWinnerStale {
        env,
        long,
        short,
        slot,
        destination,
    }
}

impl LoserPaidWinnerStale {
    fn try_withdraw(&mut self, amount: u128) -> Result<u64, String> {
        let admin = self.env.admin.insecure_clone();
        let instruction = ProgInstruction::WithdrawBackingBucket {
            domain: DOMAIN,
            market_id: self.env.asset_market_id(DOMAIN / 2),
            authority_epoch: self.env.withdrawal_authority_epoch(
                admin.pubkey(),
                DOMAIN as usize / 2,
                false,
            ),
            amount,
        };
        self.env.svm.expire_blockhash();
        self.env.send(
            instruction,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.destination, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    }

    /// The withdrawal is refused and nothing moved.
    fn assert_refused(&mut self, amount: u128) {
        let lock_active = format!("Custom({})", PercolatorError::EngineLockActive as u32);
        let market_before = self.env.svm.get_account(&self.env.market).unwrap().data;
        let vault_before = self.env.token_amount(self.env.vault);
        let destination_before = self.env.token_amount(self.destination);
        let refused = self
            .try_withdraw(amount)
            .expect_err("the loser's payment is not the provider's to withdraw");
        assert!(refused.contains(&lock_active), "{refused}");
        assert_eq!(
            self.env.svm.get_account(&self.env.market).unwrap().data,
            market_before
        );
        assert_eq!(self.env.token_amount(self.env.vault), vault_before);
        assert_eq!(self.env.token_amount(self.destination), destination_before);
    }

    /// The long's whole gain is a claim on exactly the short's payment.
    fn assert_winner_is_backed(&self) {
        assert_eq!(
            self.env.portfolio_state(self.long).pnl.get(),
            PAYMENT as i128
        );
        let source = self.env.market_state().1.source_credit[DOMAIN as usize];
        assert_eq!(source.positive_claim_bound_num, PAYMENT * BOUND_SCALE);
        assert_eq!(source.fresh_reserved_backing_num, PAYMENT * BOUND_SCALE);
        assert_eq!(source.credit_rate_num, percolator::CREDIT_RATE_SCALE);
    }
}

/// While the long is unrefreshed nothing leaves the domain, the provider's own
/// 1,000 included. Once the long's gain is a claim the provider recovers its
/// 1,000 and not an atom more, and the short's 400 backs the claim in full.
#[test]
fn v16_program_live_provider_exit_leaves_the_loser_payment_for_the_winner() {
    let mut world = loser_paid_winner_not_refreshed(PRINCIPAL);
    world.assert_refused(PRINCIPAL + PAYMENT);
    world.assert_refused(PRINCIPAL);
    world.assert_refused(1);

    let (long, slot) = (world.long, world.slot);
    refresh(&mut world.env, long, slot);
    world.assert_refused(PRINCIPAL + 1);
    world
        .try_withdraw(PRINCIPAL)
        .expect("a provider takes out what it put in");
    assert_eq!(world.env.token_amount(world.destination), PRINCIPAL as u64);
    world.assert_refused(1);
    world.assert_winner_is_backed();
}

/// With no provider principal in the domain there is never anything to
/// withdraw: not before the long is refreshed, and not after, when its claim
/// holds the payment.
#[test]
fn v16_program_live_provider_exit_is_empty_without_provider_principal() {
    let mut world = loser_paid_winner_not_refreshed(0);
    world.assert_refused(PAYMENT);
    world.assert_refused(1);

    let (long, slot) = (world.long, world.slot);
    refresh(&mut world.env, long, slot);
    world.assert_refused(1);
    world.assert_winner_is_backed();
}

/// The fixture's move reverses before the long is ever refreshed, and then both
/// accounts are. Nobody gained or lost. The short holds its regain as a claim on
/// the long-loss domain, which nothing backs; its 400 sits in the short-loss
/// domain under no claim, beside `principal`.
fn payment_stranded_by_a_reversal(principal: u128) -> LoserPaidWinnerStale {
    let mut world = loser_paid_winner_not_refreshed(principal);
    world.slot += 1;
    let (long, short, slot) = (world.long, world.short, world.slot);
    world.env.svm.warp_to_slot(slot);
    world.env.push_auth_mark_for_asset_as_admin(0, slot, OPEN_PRICE);
    world.env.push_auth_mark_for_asset_as_admin(1, slot, OPEN_PRICE);
    refresh(&mut world.env, short, slot);
    refresh(&mut world.env, long, slot);

    let group = world.env.market_state().1;
    assert_eq!(
        (
            group.assets[0].stale_account_count_long,
            group.assets[0].stale_account_count_short
        ),
        (0, 0),
        "every position on the asset is settled"
    );
    let long_state = world.env.portfolio_state(long);
    let short_state = world.env.portfolio_state(short);
    assert_eq!((long_state.capital.get(), long_state.pnl.get()), (10_000, 0));
    assert_eq!(
        (short_state.capital.get(), short_state.pnl.get()),
        (10_000 - PAYMENT, PAYMENT as i128)
    );
    let payment = group.source_credit[DOMAIN as usize];
    assert_eq!(payment.positive_claim_bound_num, 0);
    assert_eq!(
        payment.fresh_reserved_backing_num,
        (principal + PAYMENT) * BOUND_SCALE
    );
    let regain = group.source_credit[DOMAIN as usize - 1];
    assert_eq!(regain.positive_claim_bound_num, PAYMENT * BOUND_SCALE);
    assert_eq!(regain.fresh_reserved_backing_num, 0);
    world
}

impl LoserPaidWinnerStale {
    /// The short's payment is still in the domain and its claim is unchanged.
    fn assert_payment_is_still_there(&self, principal_left: u128) {
        let group = self.env.market_state().1;
        assert_eq!(
            group.source_credit[DOMAIN as usize].fresh_reserved_backing_num,
            (principal_left + PAYMENT) * BOUND_SCALE
        );
        let short = self.env.portfolio_state(self.short);
        assert_eq!(
            (short.capital.get(), short.pnl.get()),
            (10_000 - PAYMENT, PAYMENT as i128)
        );
    }
}

/// A provider with 1,000 in the domain recovers its 1,000 and not an atom of
/// the 400 a reversal stranded there.
#[test]
fn v16_program_live_provider_exit_leaves_a_payment_stranded_by_a_reversal() {
    let mut world = payment_stranded_by_a_reversal(PRINCIPAL);
    world.assert_refused(PRINCIPAL + PAYMENT);
    world.assert_refused(PRINCIPAL + 1);
    world.assert_payment_is_still_there(PRINCIPAL);
    world
        .try_withdraw(PRINCIPAL)
        .expect("a provider takes out what it put in");
    assert_eq!(world.env.token_amount(world.destination), PRINCIPAL as u64);
    world.assert_refused(1);
    world.assert_payment_is_still_there(0);
}

/// With no principal in the domain the backing authority has nothing to
/// withdraw. It used to walk away with the short's 400.
#[test]
fn v16_program_live_backing_authority_cannot_take_a_stranded_payment_without_principal() {
    let mut world = payment_stranded_by_a_reversal(0);
    world.assert_refused(PAYMENT);
    world.assert_refused(1);
    world.assert_payment_is_still_there(0);
    assert_eq!(world.env.token_amount(world.destination), 0);
}
