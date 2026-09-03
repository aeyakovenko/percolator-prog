//! INV-015 - Account ownership, layout, discriminator, and length validity.
//!
//! These proofs cover the production account-header predicate without assumptions. Public
//! LiteSVM tests compose this predicate with account ownership, exact dynamic lengths, nested
//! engine-field validation, and exact rollback.

use percolator_prog::{constants, state};

#[kani::proof]
fn kani_v16_account_header_accepts_exactly_magic_version_and_kind() {
    let data: [u8; constants::HEADER_LEN] = kani::any();
    let expected_kind: u8 = kani::any();
    let magic = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let version = u16::from_le_bytes([data[8], data[9]]);
    let expected =
        magic == constants::MAGIC && version == constants::VERSION && data[10] == expected_kind;

    assert_eq!(
        state::kani_check_header(&data, expected_kind).is_ok(),
        expected
    );

    kani::cover!(expected, "a canonical symbolic header is accepted");
    kani::cover!(magic != constants::MAGIC, "bad magic is rejected");
    kani::cover!(version != constants::VERSION, "bad version is rejected");
    kani::cover!(
        magic == constants::MAGIC && version == constants::VERSION && data[10] != expected_kind,
        "wrong account kind is rejected"
    );
}

#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_account_header_rejects_every_short_length() {
    let data: [u8; constants::HEADER_LEN] = kani::any();
    let expected_kind: u8 = kani::any();
    let mut len = 0usize;
    while len < constants::HEADER_LEN {
        assert!(state::kani_check_header(&data[..len], expected_kind).is_err());
        len += 1;
    }
}

/// Secondary owner: INV-001/INV-007. Closing a market overwrites arbitrary prior bytes with an
/// initialized account class that `InitMarket` cannot mistake for a fresh account.
#[kani::proof]
#[kani::unwind(18)]
fn kani_v16_closed_market_tombstone_is_an_exact_initialized_header() {
    let mut data: [u8; constants::HEADER_LEN] = kani::any();

    state::write_closed_market_tombstone(&mut data).unwrap();

    assert_eq!(
        u64::from_le_bytes(data[0..8].try_into().unwrap()),
        constants::MAGIC
    );
    assert_eq!(
        u16::from_le_bytes(data[8..10].try_into().unwrap()),
        constants::VERSION
    );
    assert_eq!(data[10], constants::KIND_CLOSED_MARKET);
    assert!(data[11..].iter().all(|byte| *byte == 0));
    assert!(state::is_initialized(&data));
    assert!(state::kani_check_header(&data, constants::KIND_MARKET).is_err());
    assert!(state::kani_check_header(&data, constants::KIND_CLOSED_MARKET).is_ok());
}
