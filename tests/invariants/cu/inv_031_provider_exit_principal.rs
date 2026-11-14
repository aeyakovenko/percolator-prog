//! INV-031: what a domain's losers pay in backs the gains it is owed to. While
//! a position has not settled the latest price move, a backing provider takes
//! out what it put in and nothing more.
//!
//! The engine's provider exit keeps `available backing >= registered claims`,
//! and a gain is registered as a claim only when its account is refreshed.
//! Between the loser's refresh and the winner's, the loser's payment is covered
//! by no claim. Before the engine capped the exit at provider principal for
//! that window, `WithdrawBackingBucket` let the backing authority take the
//! payment, and the winner's claim was then registered against no backing.
//!
//! The wrapper's own Live withdrawal gate does not close this. It refuses while
//! the market-wide loss-stale flag is set, and that flag summarizes only the
//! asset touched last: one crank on another, current asset clears it while the
//! winner here is still unrefreshed.

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

    /// Refreshes the long and requires its whole gain to be a backed claim.
    fn assert_winner_is_backed(&mut self) {
        refresh(&mut self.env, self.long, self.slot);
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

/// The provider recovers its 1,000 and not an atom of the short's 400, which
/// then backs the long's claim in full.
#[test]
fn v16_program_live_provider_exit_leaves_the_loser_payment_for_the_winner() {
    let mut world = loser_paid_winner_not_refreshed(PRINCIPAL);
    world.assert_refused(PRINCIPAL + PAYMENT);
    world.assert_refused(PRINCIPAL + 1);
    world
        .try_withdraw(PRINCIPAL)
        .expect("a provider takes out what it put in");
    assert_eq!(world.env.token_amount(world.destination), PRINCIPAL as u64);
    world.assert_refused(1);
    world.assert_winner_is_backed();
}

/// With no provider principal in the domain there is nothing to withdraw.
#[test]
fn v16_program_live_provider_exit_is_empty_without_provider_principal() {
    let mut world = loser_paid_winner_not_refreshed(0);
    world.assert_refused(PAYMENT);
    world.assert_refused(1);
    world.assert_winner_is_backed();
}
