//! INV-002 - Asset generation binding.
//!
//! These contracts prove the exact predicate used by public generation guards and prove that both
//! backing-withdrawal wire formats preserve the generation field while legacy unbound payloads
//! fail closed. Public LiteSVM tests own handler composition, rollback, and fresh-route liveness.

use percolator_prog::{ix::Instruction, state};

#[kani::proof]
fn kani_v16_asset_generation_binding_is_exact() {
    let current_market_id: u64 = kani::any();
    let expected_market_id: u64 = kani::any();

    assert_eq!(
        state::asset_generation_binding_matches(current_market_id, expected_market_id),
        current_market_id == expected_market_id
    );
}

#[kani::proof]
fn kani_v16_backing_principal_withdrawal_preserves_generation_and_rejects_legacy() {
    let domain: u16 = kani::any();
    let market_id: u64 = kani::any();
    let amount: u128 = kani::any();
    let encoded = Instruction::WithdrawBackingBucket {
        domain,
        market_id,
        amount,
    }
    .encode();

    match Instruction::decode(&encoded).unwrap() {
        Instruction::WithdrawBackingBucket {
            domain: decoded_domain,
            market_id: decoded_market_id,
            amount: decoded_amount,
        } => {
            assert_eq!(decoded_domain, domain);
            assert_eq!(decoded_market_id, market_id);
            assert_eq!(decoded_amount, amount);
        }
        _ => unreachable!(),
    }

    // Old tag-50 payload: tag + domain + amount, with no generation field.
    let mut legacy: [u8; 19] = kani::any();
    legacy[0] = 50;
    assert!(Instruction::decode(&legacy).is_err());
}

#[kani::proof]
fn kani_v16_backing_earnings_withdrawal_preserves_generation_and_rejects_legacy() {
    let domain: u16 = kani::any();
    let market_id: u64 = kani::any();
    let amount: u128 = kani::any();
    let encoded = Instruction::WithdrawBackingBucketEarnings {
        domain,
        market_id,
        amount,
    }
    .encode();

    match Instruction::decode(&encoded).unwrap() {
        Instruction::WithdrawBackingBucketEarnings {
            domain: decoded_domain,
            market_id: decoded_market_id,
            amount: decoded_amount,
        } => {
            assert_eq!(decoded_domain, domain);
            assert_eq!(decoded_market_id, market_id);
            assert_eq!(decoded_amount, amount);
        }
        _ => unreachable!(),
    }

    // Old tag-52 payload: tag + domain + amount, with no generation field.
    let mut legacy: [u8; 19] = kani::any();
    legacy[0] = 52;
    assert!(Instruction::decode(&legacy).is_err());
}
