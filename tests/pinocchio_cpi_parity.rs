//! Byte-parity tests: our custom spl_token instruction builders vs spl-token 6.0.
//!
//! Security flag (pre-mainnet MEDIUM): assert that
//! `percolator_prog::spl_token::{transfer, mint_to, burn, initialize_mint}`
//! produce byte-for-byte identical `Instruction.data` and identical account
//! metas (keys + is_signer + is_writable) to `spl_token 6.0`.
//!
//! These tests MUST pass before deploying to mainnet.
//!
//! Run:
//!   cargo test -p percolator-prog --test pinocchio_cpi_parity
//!   cargo test -p percolator-prog --test pinocchio_cpi_parity -- --nocapture

use solana_program::{instruction::Instruction, pubkey::Pubkey};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Fixed test keys — deterministic, no RNG dependency.
fn key(seed: u8) -> Pubkey {
    let mut b = [0u8; 32];
    b[0] = seed;
    Pubkey::new_from_array(b)
}

/// Assert two `Instruction` values are byte-for-byte identical:
/// same program_id, same data, same account metas (key / is_signer / is_writable).
fn assert_ix_eq(label: &str, got: &Instruction, want: &Instruction) {
    assert_eq!(
        got.program_id, want.program_id,
        "{label}: program_id mismatch — got={} want={}",
        got.program_id, want.program_id
    );
    assert_eq!(
        got.data, want.data,
        "{label}: data mismatch\n  got  = {:?}\n  want = {:?}",
        got.data, want.data
    );
    assert_eq!(
        got.accounts.len(),
        want.accounts.len(),
        "{label}: accounts len mismatch"
    );
    for (i, (g, w)) in got.accounts.iter().zip(want.accounts.iter()).enumerate() {
        assert_eq!(g.pubkey, w.pubkey, "{label}: account[{i}].pubkey mismatch");
        assert_eq!(
            g.is_signer, w.is_signer,
            "{label}: account[{i}].is_signer mismatch"
        );
        assert_eq!(
            g.is_writable, w.is_writable,
            "{label}: account[{i}].is_writable mismatch"
        );
    }
}

// ── Transfer ─────────────────────────────────────────────────────────────────

/// Parity: transfer — amount 0 (edge case)
#[test]
fn test_transfer_amount_zero() {
    let token_prog = spl_token::id();
    let source = key(1);
    let dest = key(2);
    let authority = key(3);

    let our = percolator_prog::spl_token::transfer(&token_prog, &source, &dest, &authority, &[], 0)
        .expect("our transfer failed");
    let spl = spl_token::instruction::transfer(&token_prog, &source, &dest, &authority, &[], 0)
        .expect("spl transfer failed");

    assert_ix_eq("transfer(amount=0)", &our, &spl);
}

/// Parity: transfer — typical amount
#[test]
fn test_transfer_typical_amount() {
    let token_prog = spl_token::id();
    let source = key(10);
    let dest = key(11);
    let authority = key(12);
    let amount = 1_000_000_000u64;

    let our =
        percolator_prog::spl_token::transfer(&token_prog, &source, &dest, &authority, &[], amount)
            .expect("our transfer failed");
    let spl =
        spl_token::instruction::transfer(&token_prog, &source, &dest, &authority, &[], amount)
            .expect("spl transfer failed");

    assert_ix_eq("transfer(amount=1e9)", &our, &spl);
}

/// Parity: transfer — u64::MAX
#[test]
fn test_transfer_u64_max() {
    let token_prog = spl_token::id();
    let source = key(20);
    let dest = key(21);
    let authority = key(22);

    let our = percolator_prog::spl_token::transfer(
        &token_prog,
        &source,
        &dest,
        &authority,
        &[],
        u64::MAX,
    )
    .expect("our transfer failed");
    let spl =
        spl_token::instruction::transfer(&token_prog, &source, &dest, &authority, &[], u64::MAX)
            .expect("spl transfer failed");

    assert_ix_eq("transfer(amount=u64::MAX)", &our, &spl);
}

/// Parity: transfer — all-key variation (ensures key bytes are wired correctly)
#[test]
fn test_transfer_varied_keys() {
    let token_prog = spl_token::id();
    for seed in [5u8, 50, 100, 200, 255] {
        let source = key(seed);
        let dest = key(seed.wrapping_add(1));
        let authority = key(seed.wrapping_add(2));
        let amount = (seed as u64) * 1_234_567;

        let our = percolator_prog::spl_token::transfer(
            &token_prog,
            &source,
            &dest,
            &authority,
            &[],
            amount,
        )
        .expect("our transfer failed");
        let spl =
            spl_token::instruction::transfer(&token_prog, &source, &dest, &authority, &[], amount)
                .expect("spl transfer failed");

        assert_ix_eq(&format!("transfer(seed={seed})"), &our, &spl);
    }
}

// ── MintTo ───────────────────────────────────────────────────────────────────

/// Parity: mint_to — amount 0
#[test]
fn test_mint_to_amount_zero() {
    let token_prog = spl_token::id();
    let mint = key(30);
    let dest = key(31);
    let authority = key(32);

    let our = percolator_prog::spl_token::mint_to(&token_prog, &mint, &dest, &authority, &[], 0)
        .expect("our mint_to failed");
    let spl = spl_token::instruction::mint_to(&token_prog, &mint, &dest, &authority, &[], 0)
        .expect("spl mint_to failed");

    assert_ix_eq("mint_to(amount=0)", &our, &spl);
}

/// Parity: mint_to — typical amount
#[test]
fn test_mint_to_typical() {
    let token_prog = spl_token::id();
    let mint = key(40);
    let dest = key(41);
    let authority = key(42);
    let amount = 500_000_000u64;

    let our =
        percolator_prog::spl_token::mint_to(&token_prog, &mint, &dest, &authority, &[], amount)
            .expect("our mint_to failed");
    let spl = spl_token::instruction::mint_to(&token_prog, &mint, &dest, &authority, &[], amount)
        .expect("spl mint_to failed");

    assert_ix_eq("mint_to(amount=5e8)", &our, &spl);
}

/// Parity: mint_to — u64::MAX
#[test]
fn test_mint_to_u64_max() {
    let token_prog = spl_token::id();
    let mint = key(50);
    let dest = key(51);
    let authority = key(52);

    let our =
        percolator_prog::spl_token::mint_to(&token_prog, &mint, &dest, &authority, &[], u64::MAX)
            .expect("our mint_to failed");
    let spl = spl_token::instruction::mint_to(&token_prog, &mint, &dest, &authority, &[], u64::MAX)
        .expect("spl mint_to failed");

    assert_ix_eq("mint_to(amount=u64::MAX)", &our, &spl);
}

// ── Burn ─────────────────────────────────────────────────────────────────────

/// Parity: burn — amount 0
#[test]
fn test_burn_amount_zero() {
    let token_prog = spl_token::id();
    let account = key(60);
    let mint = key(61);
    let authority = key(62);

    let our = percolator_prog::spl_token::burn(&token_prog, &account, &mint, &authority, &[], 0)
        .expect("our burn failed");
    let spl = spl_token::instruction::burn(&token_prog, &account, &mint, &authority, &[], 0)
        .expect("spl burn failed");

    assert_ix_eq("burn(amount=0)", &our, &spl);
}

/// Parity: burn — typical amount
#[test]
fn test_burn_typical() {
    let token_prog = spl_token::id();
    let account = key(70);
    let mint = key(71);
    let authority = key(72);
    let amount = 250_000_000u64;

    let our =
        percolator_prog::spl_token::burn(&token_prog, &account, &mint, &authority, &[], amount)
            .expect("our burn failed");
    let spl = spl_token::instruction::burn(&token_prog, &account, &mint, &authority, &[], amount)
        .expect("spl burn failed");

    assert_ix_eq("burn(amount=2.5e8)", &our, &spl);
}

/// Parity: burn — u64::MAX
#[test]
fn test_burn_u64_max() {
    let token_prog = spl_token::id();
    let account = key(80);
    let mint = key(81);
    let authority = key(82);

    let our =
        percolator_prog::spl_token::burn(&token_prog, &account, &mint, &authority, &[], u64::MAX)
            .expect("our burn failed");
    let spl = spl_token::instruction::burn(&token_prog, &account, &mint, &authority, &[], u64::MAX)
        .expect("spl burn failed");

    assert_ix_eq("burn(amount=u64::MAX)", &our, &spl);
}

// ── InitializeMint ────────────────────────────────────────────────────────────

/// Parity: initialize_mint — no freeze authority
#[test]
fn test_initialize_mint_no_freeze() {
    let token_prog = spl_token::id();
    let mint = key(90);
    let authority = key(91);

    let our = percolator_prog::spl_token::initialize_mint(&token_prog, &mint, &authority, None, 6)
        .expect("our initialize_mint failed");
    let spl = spl_token::instruction::initialize_mint(&token_prog, &mint, &authority, None, 6)
        .expect("spl initialize_mint failed");

    assert_ix_eq("initialize_mint(freeze=None, decimals=6)", &our, &spl);
}

/// Parity: initialize_mint — with freeze authority
#[test]
fn test_initialize_mint_with_freeze() {
    let token_prog = spl_token::id();
    let mint = key(100);
    let authority = key(101);
    let freeze = key(102);

    let our = percolator_prog::spl_token::initialize_mint(
        &token_prog,
        &mint,
        &authority,
        Some(&freeze),
        9,
    )
    .expect("our initialize_mint failed");
    let spl =
        spl_token::instruction::initialize_mint(&token_prog, &mint, &authority, Some(&freeze), 9)
            .expect("spl initialize_mint failed");

    assert_ix_eq("initialize_mint(freeze=Some, decimals=9)", &our, &spl);
}

/// Parity: initialize_mint — decimals 0 and 255 boundary cases
#[test]
fn test_initialize_mint_decimals_boundary() {
    let token_prog = spl_token::id();
    let mint = key(110);
    let authority = key(111);

    for dec in [0u8, 1, 8, 9, 18, 255] {
        let our =
            percolator_prog::spl_token::initialize_mint(&token_prog, &mint, &authority, None, dec)
                .expect("our initialize_mint failed");
        let spl =
            spl_token::instruction::initialize_mint(&token_prog, &mint, &authority, None, dec)
                .expect("spl initialize_mint failed");

        assert_ix_eq(&format!("initialize_mint(decimals={dec})"), &our, &spl);
    }
}

/// Parity: initialize_mint — byte-exact length and content when freeze=None (35 bytes)
#[test]
fn test_initialize_mint_data_length_no_freeze() {
    let token_prog = spl_token::id();
    let mint = key(120);
    let authority = key(121);

    let our = percolator_prog::spl_token::initialize_mint(&token_prog, &mint, &authority, None, 6)
        .expect("our initialize_mint failed");
    let spl = spl_token::instruction::initialize_mint(&token_prog, &mint, &authority, None, 6)
        .expect("spl initialize_mint failed");

    // spl-token 6.0 emits 35 bytes for freeze=None.
    // Our impl must match exactly.
    assert_eq!(
        our.data.len(),
        spl.data.len(),
        "initialize_mint(freeze=None) data length mismatch: our={} spl={}",
        our.data.len(),
        spl.data.len()
    );
    assert_eq!(
        our.data, spl.data,
        "initialize_mint(freeze=None) data bytes mismatch"
    );
}

/// Parity: initialize_mint — verify full layout when freeze=Some
#[test]
fn test_initialize_mint_data_length_with_freeze() {
    let token_prog = spl_token::id();
    let mint = key(130);
    let authority = key(131);
    let freeze = key(132);

    let our = percolator_prog::spl_token::initialize_mint(
        &token_prog,
        &mint,
        &authority,
        Some(&freeze),
        6,
    )
    .expect("our initialize_mint failed");
    let spl =
        spl_token::instruction::initialize_mint(&token_prog, &mint, &authority, Some(&freeze), 6)
            .expect("spl initialize_mint failed");

    assert_eq!(
        our.data.len(),
        spl.data.len(),
        "initialize_mint(freeze=Some) data length mismatch: our={} spl={}",
        our.data.len(),
        spl.data.len()
    );
    assert_eq!(
        our.data, spl.data,
        "initialize_mint(freeze=Some) data bytes mismatch"
    );
}

// ── Discriminant / tag spot-checks ───────────────────────────────────────────

/// The first byte of each instruction must match the known SPL Tag.
/// This is an independent sanity check on top of the full byte-parity tests above.
#[test]
fn test_instruction_tags() {
    let prog = spl_token::id();
    let a = key(1);
    let b = key(2);
    let c = key(3);

    let transfer_ix =
        percolator_prog::spl_token::transfer(&prog, &a, &b, &c, &[], 1).expect("transfer");
    assert_eq!(
        transfer_ix.data[0], 3,
        "Transfer tag must be 3, got {}",
        transfer_ix.data[0]
    );

    let mint_to_ix =
        percolator_prog::spl_token::mint_to(&prog, &a, &b, &c, &[], 1).expect("mint_to");
    assert_eq!(
        mint_to_ix.data[0], 7,
        "MintTo tag must be 7, got {}",
        mint_to_ix.data[0]
    );

    let burn_ix = percolator_prog::spl_token::burn(&prog, &a, &b, &c, &[], 1).expect("burn");
    assert_eq!(
        burn_ix.data[0], 8,
        "Burn tag must be 8, got {}",
        burn_ix.data[0]
    );

    let init_ix =
        percolator_prog::spl_token::initialize_mint(&prog, &a, &b, None, 6).expect("init_mint");
    assert_eq!(
        init_ix.data[0], 0,
        "InitializeMint tag must be 0, got {}",
        init_ix.data[0]
    );
}

// ── program_id checks ─────────────────────────────────────────────────────────

/// All four instruction builders must emit the canonical SPL Token program_id.
#[test]
fn test_program_ids_are_canonical_spl_token() {
    let expected = spl_token::id();
    let a = key(1);
    let b = key(2);
    let c = key(3);

    assert_eq!(
        percolator_prog::spl_token::transfer(&expected, &a, &b, &c, &[], 1)
            .unwrap()
            .program_id,
        expected,
        "transfer: wrong program_id"
    );
    assert_eq!(
        percolator_prog::spl_token::mint_to(&expected, &a, &b, &c, &[], 1)
            .unwrap()
            .program_id,
        expected,
        "mint_to: wrong program_id"
    );
    assert_eq!(
        percolator_prog::spl_token::burn(&expected, &a, &b, &c, &[], 1)
            .unwrap()
            .program_id,
        expected,
        "burn: wrong program_id"
    );
    assert_eq!(
        percolator_prog::spl_token::initialize_mint(&expected, &a, &b, None, 6)
            .unwrap()
            .program_id,
        expected,
        "initialize_mint: wrong program_id"
    );
}
