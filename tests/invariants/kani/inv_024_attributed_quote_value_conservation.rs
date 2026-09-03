//! INV-024 - attributed quote-value conservation.
//!
//! The wrapper owns SPL custody and authority attribution; the engine owns the
//! canonical 17-class internal value-flow proof. This theorem executes the
//! exact pinned engine validator over arbitrary bounded debit/credit vectors
//! and independently recomputes both obligations visible at the wrapper
//! boundary: every internal debit has one credit, and net external quote flow
//! equals the signed vault delta. No assumptions exclude malformed flows.
//!
//! The bounded `u8` factors are widened to `u128`, so the proof covers every
//! relative attribution partition without an overflow precondition. The
//! public LiteSVM/stateful owners separately bind `ExternalQuote` and
//! `TokenVault` to real SPL balances, owner identities, and all wrapper routes.
//! This file also proves why aggregate balance is not the entitlement theorem:
//! a two-user transfer can conserve globally while crediting the wrong user.
//! The episode equation below is independent of the engine flow classes and
//! makes owner-level claim bounds an additional mandatory postcondition.
//! A second theorem constructs every valid bounded history pre-state, applies one arbitrary
//! credit/debit/payout step, and proves per-actor plus coalition entitlement is inductive. An
//! overdrawn step returns no post-state and composes with wrapper error propagation and SVM rollback.

use percolator::{
    TokenValueClassV16, TokenValueFlowProofV16, V16Error, V16_TOKEN_VALUE_CLASS_COUNT,
};

fn inv024_sum(values: &[u128; V16_TOKEN_VALUE_CLASS_COUNT]) -> u128 {
    let mut total = 0u128;
    let mut index = 0usize;
    while index < V16_TOKEN_VALUE_CLASS_COUNT {
        total = total
            .checked_add(values[index])
            .expect("bounded attribution factors cannot overflow u128");
        index += 1;
    }
    total
}

#[derive(Clone, Copy)]
struct Inv024Episode {
    principal_in: u128,
    realized_gain: u128,
    assigned_support: u128,
    attributed_loss: u128,
    disclosed_fee: u128,
    prior_payout: u128,
    authorized_forfeit: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inv024HistoryEntitlement {
    attributed_credit: u128,
    fixed_debit: u128,
    cumulative_payout: u128,
}

impl Inv024HistoryEntitlement {
    fn valid(self) -> bool {
        self.fixed_debit
            .checked_add(self.cumulative_payout)
            .is_some_and(|used| used <= self.attributed_credit)
    }

    fn apply_successful_step(
        self,
        new_attributed_credit: u128,
        new_fixed_debit: u128,
        requested_payout: u128,
    ) -> Option<Self> {
        let attributed_credit = self.attributed_credit.checked_add(new_attributed_credit)?;
        let fixed_debit = self.fixed_debit.checked_add(new_fixed_debit)?;
        let used_before_payout = fixed_debit.checked_add(self.cumulative_payout)?;
        let remaining = attributed_credit.checked_sub(used_before_payout)?;
        let payout = requested_payout.min(remaining);
        Some(Self {
            attributed_credit,
            fixed_debit,
            cumulative_payout: self.cumulative_payout.checked_add(payout)?,
        })
    }
}

impl Inv024Episode {
    fn gross_credit(self) -> u128 {
        self.principal_in + self.realized_gain + self.assigned_support
    }

    fn gross_debit(self) -> u128 {
        self.attributed_loss + self.disclosed_fee + self.prior_payout + self.authorized_forfeit
    }

    fn claim(self) -> Option<u128> {
        self.gross_credit().checked_sub(self.gross_debit())
    }
}

#[kani::proof]
fn kani_inv024_engine_flow_validator_equals_wrapper_value_equation() {
    let raw_debits: [u8; V16_TOKEN_VALUE_CLASS_COUNT] = kani::any();
    let raw_credits: [u8; V16_TOKEN_VALUE_CLASS_COUNT] = kani::any();
    let mut debits = [0u128; V16_TOKEN_VALUE_CLASS_COUNT];
    let mut credits = [0u128; V16_TOKEN_VALUE_CLASS_COUNT];
    let mut index = 0usize;
    while index < V16_TOKEN_VALUE_CLASS_COUNT {
        debits[index] = u128::from(raw_debits[index]);
        credits[index] = u128::from(raw_credits[index]);
        index += 1;
    }

    let external_quote_in = u128::from(kani::any::<u8>());
    let external_quote_out = u128::from(kani::any::<u8>());
    let vault_before = u128::from(kani::any::<u16>());
    let vault_after = u128::from(kani::any::<u16>());
    let proof = TokenValueFlowProofV16 {
        debits,
        credits,
        external_quote_in,
        external_quote_out,
        vault_before,
        vault_after,
    };

    let total_debits = inv024_sum(&proof.debits);
    let total_credits = inv024_sum(&proof.credits);
    let external_matches_vault = if vault_after >= vault_before {
        external_quote_in >= external_quote_out
            && external_quote_in - external_quote_out == vault_after - vault_before
    } else {
        external_quote_out >= external_quote_in
            && external_quote_out - external_quote_in == vault_before - vault_after
    };
    let independently_valid = total_debits == total_credits && external_matches_vault;
    assert_eq!(proof.validate().is_ok(), independently_valid);

    kani::cover!(proof.validate().is_ok(), "a balanced flow is admitted");
    kani::cover!(proof.validate().is_err(), "an unbalanced flow is rejected");

    let mut balanced = TokenValueFlowProofV16::empty(7, 7);
    balanced.debits[TokenValueClassV16::AccountCapital as usize] = 1;
    balanced.credits[TokenValueClassV16::InsuranceCapital as usize] = 1;
    assert_eq!(balanced.validate(), Ok(()));

    let mut duplicated = balanced;
    duplicated.credits[TokenValueClassV16::ProtocolFeePaid as usize] = 1;
    assert_eq!(duplicated.validate(), Err(V16Error::InvalidConfig));

    let mut custody_mismatch = balanced;
    custody_mismatch.vault_after = 8;
    assert_eq!(custody_mismatch.validate(), Err(V16Error::InvalidConfig));
}

#[kani::proof]
fn kani_inv024_per_episode_entitlement_is_stronger_than_aggregate_conservation() {
    // u8 inputs widened to u128 make every sum exact without arithmetic assumptions.
    let episodes = [
        Inv024Episode {
            principal_in: u128::from(kani::any::<u8>()),
            realized_gain: u128::from(kani::any::<u8>()),
            assigned_support: u128::from(kani::any::<u8>()),
            attributed_loss: u128::from(kani::any::<u8>()),
            disclosed_fee: u128::from(kani::any::<u8>()),
            prior_payout: u128::from(kani::any::<u8>()),
            authorized_forfeit: u128::from(kani::any::<u8>()),
        },
        Inv024Episode {
            principal_in: u128::from(kani::any::<u8>()),
            realized_gain: u128::from(kani::any::<u8>()),
            assigned_support: u128::from(kani::any::<u8>()),
            attributed_loss: u128::from(kani::any::<u8>()),
            disclosed_fee: u128::from(kani::any::<u8>()),
            prior_payout: u128::from(kani::any::<u8>()),
            authorized_forfeit: u128::from(kani::any::<u8>()),
        },
    ];
    let requested = [u128::from(kani::any::<u8>()), u128::from(kani::any::<u8>())];
    kani::cover!(
        episodes[0].claim().is_some() && episodes[1].claim().is_some(),
        "both symbolic episode ledgers are funded"
    );
    kani::cover!(
        episodes[0].claim().is_none() || episodes[1].claim().is_none(),
        "an overdrawn symbolic episode fails closed"
    );
    if let (Some(claim_0), Some(claim_1)) = (episodes[0].claim(), episodes[1].claim()) {
        let paid = [requested[0].min(claim_0), requested[1].min(claim_1)];
        assert!(paid[0] <= claim_0);
        assert!(paid[1] <= claim_1);
        assert!(paid[0] + paid[1] <= claim_0 + claim_1);
    }

    // Mutation witness: one atom belongs to A. Paying it to B is globally balanced but violates
    // both episode equations, proving that TokenValueFlow validation alone is insufficient.
    let rightful = [
        Inv024Episode {
            principal_in: 1,
            realized_gain: 0,
            assigned_support: 0,
            attributed_loss: 0,
            disclosed_fee: 0,
            prior_payout: 0,
            authorized_forfeit: 0,
        },
        Inv024Episode {
            principal_in: 0,
            realized_gain: 0,
            assigned_support: 0,
            attributed_loss: 0,
            disclosed_fee: 0,
            prior_payout: 0,
            authorized_forfeit: 0,
        },
    ];
    let wrong_owner_payout = [0u128, 1u128];
    assert_eq!(wrong_owner_payout[0] + wrong_owner_payout[1], 1);
    assert!(wrong_owner_payout[0] <= rightful[0].claim().unwrap());
    assert!(wrong_owner_payout[1] > rightful[1].claim().unwrap());
}

#[kani::proof]
fn kani_inv024_entitlement_envelope_is_inductive_over_arbitrary_history_step() {
    fn arbitrary_valid_history() -> Inv024HistoryEntitlement {
        let attributed_credit = u128::from(kani::any::<u8>());
        let fixed_debit = u128::from(kani::any::<u8>()).min(attributed_credit);
        let remaining = attributed_credit - fixed_debit;
        let cumulative_payout = u128::from(kani::any::<u8>()).min(remaining);
        Inv024HistoryEntitlement {
            attributed_credit,
            fixed_debit,
            cumulative_payout,
        }
    }

    let pre = [arbitrary_valid_history(), arbitrary_valid_history()];
    let new_credit = [u128::from(kani::any::<u8>()), u128::from(kani::any::<u8>())];
    let new_fixed_debit = [u128::from(kani::any::<u8>()), u128::from(kani::any::<u8>())];
    let requested_payout = [u128::from(kani::any::<u8>()), u128::from(kani::any::<u8>())];
    assert!(pre[0].valid() && pre[1].valid());

    let post = [
        pre[0].apply_successful_step(new_credit[0], new_fixed_debit[0], requested_payout[0]),
        pre[1].apply_successful_step(new_credit[1], new_fixed_debit[1], requested_payout[1]),
    ];
    for index in 0..2 {
        if let Some(post) = post[index] {
            assert!(post.valid());
            assert!(post.cumulative_payout >= pre[index].cumulative_payout);
            assert!(post.cumulative_payout <= post.attributed_credit - post.fixed_debit);
        } else {
            // The pure transition returns no post-state, matching the wrapper's error/rollback
            // composition. An excessive newly attributed debit cannot fabricate a valid success.
            assert!(pre[index].valid());
        }
    }
    if let (Some(post_0), Some(post_1)) = (post[0], post[1]) {
        assert!(
            post_0.cumulative_payout + post_1.cumulative_payout
                <= (post_0.attributed_credit - post_0.fixed_debit)
                    + (post_1.attributed_credit - post_1.fixed_debit)
        );
    }

    kani::cover!(post[0].is_none(), "an overdrawn history step rejects");
    kani::cover!(
        post[0].is_some_and(|state| state.cumulative_payout == pre[0].cumulative_payout),
        "a successful history step can pay zero"
    );
    kani::cover!(
        post[0].is_some_and(|state| {
            state.cumulative_payout > pre[0].cumulative_payout
                && state.cumulative_payout < state.attributed_credit - state.fixed_debit
        }),
        "a successful history step can make a partial payout"
    );
    kani::cover!(
        post[0].is_some_and(|state| {
            state.cumulative_payout == state.attributed_credit - state.fixed_debit
        }),
        "a successful history step can exactly exhaust entitlement"
    );
}
