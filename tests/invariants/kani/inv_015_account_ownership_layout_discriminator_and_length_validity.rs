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
