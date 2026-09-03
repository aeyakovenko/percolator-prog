//! INV-018 - Quote mint, vault, token-program, and authority integrity.
//!
//! These proofs execute the wrapper's production SPL-token boundary helpers. The byte proof
//! independently constructs the complete 165-byte classic-SPL account ABI and exhausts every
//! 32-bit option tag, every account-state byte, both program-owner classes, both mint/owner
//! equality classes, and the full `u64` amount. Acceptance is exact: only a structurally valid,
//! initialized account owned by classic SPL Token with the expected mint and token owner returns
//! its encoded amount. Separate full-domain proofs bind the executable token-program identity and
//! balance precondition. Canonical vault/address and actual CPI deltas remain exercised through
//! the invariant's public LiteSVM matrix; Solana's deployed SPL Token execution is an external TCB.

use percolator_prog::kani_token_boundary::{
    require_token_balance, verify_token_program, verify_user_token_account,
};
use solana_program::{account_info::AccountInfo, pubkey::Pubkey, system_program};

#[kani::proof]
#[kani::unwind(40)]
fn kani_inv018_token_program_is_exact_and_executable() {
    let key_matches: bool = kani::any();
    let executable: bool = kani::any();
    let key = if key_matches {
        spl_token::ID
    } else {
        system_program::ID
    };
    let owner = system_program::ID;
    let mut lamports = 0u64;
    let mut data = [];
    let account = AccountInfo::new(
        &key,
        false,
        false,
        &mut lamports,
        &mut data,
        &owner,
        executable,
        0,
    );

    let accepted = verify_token_program(&account).is_ok();
    assert_eq!(accepted, key_matches && executable);
    kani::cover!(accepted, "canonical executable SPL Token program accepted");
    kani::cover!(!key_matches, "wrong program identity rejected");
    kani::cover!(
        key_matches && !executable,
        "non-executable program rejected"
    );
}

#[kani::proof]
#[kani::unwind(40)]
fn kani_inv018_classic_spl_account_bytes_admit_exact_user_state() {
    const TOKEN_ACCOUNT_LEN: usize = 165;
    const MINT_OFF: usize = 0;
    const TOKEN_OWNER_OFF: usize = 32;
    const AMOUNT_OFF: usize = 64;
    const DELEGATE_TAG_OFF: usize = 72;
    const STATE_OFF: usize = 108;
    const NATIVE_TAG_OFF: usize = 109;
    const DELEGATED_AMOUNT_OFF: usize = 121;
    const CLOSE_AUTHORITY_TAG_OFF: usize = 129;

    let program_owner_matches: bool = kani::any();
    let mint_matches: bool = kani::any();
    let token_owner_matches: bool = kani::any();
    let amount: u64 = kani::any();
    let delegated_amount: u64 = kani::any();
    let delegate_tag: u32 = kani::any();
    let account_state: u8 = kani::any();
    let native_tag: u32 = kani::any();
    let close_authority_tag: u32 = kani::any();

    let expected_mint = Pubkey::new_from_array([0x11; 32]);
    let foreign_mint = Pubkey::new_from_array([0x12; 32]);
    let expected_token_owner = Pubkey::new_from_array([0x21; 32]);
    let foreign_token_owner = Pubkey::new_from_array([0x22; 32]);
    let encoded_mint = if mint_matches {
        expected_mint
    } else {
        foreign_mint
    };
    let encoded_token_owner = if token_owner_matches {
        expected_token_owner
    } else {
        foreign_token_owner
    };

    // Construct the canonical classic-SPL Account wire layout independently of Account::pack.
    let mut data = [0u8; TOKEN_ACCOUNT_LEN];
    data[MINT_OFF..MINT_OFF + 32].copy_from_slice(encoded_mint.as_ref());
    data[TOKEN_OWNER_OFF..TOKEN_OWNER_OFF + 32].copy_from_slice(encoded_token_owner.as_ref());
    data[AMOUNT_OFF..AMOUNT_OFF + 8].copy_from_slice(&amount.to_le_bytes());
    data[DELEGATE_TAG_OFF..DELEGATE_TAG_OFF + 4].copy_from_slice(&delegate_tag.to_le_bytes());
    data[STATE_OFF] = account_state;
    data[NATIVE_TAG_OFF..NATIVE_TAG_OFF + 4].copy_from_slice(&native_tag.to_le_bytes());
    data[DELEGATED_AMOUNT_OFF..DELEGATED_AMOUNT_OFF + 8]
        .copy_from_slice(&delegated_amount.to_le_bytes());
    data[CLOSE_AUTHORITY_TAG_OFF..CLOSE_AUTHORITY_TAG_OFF + 4]
        .copy_from_slice(&close_authority_tag.to_le_bytes());

    let key = Pubkey::new_from_array([0x31; 32]);
    let account_owner = if program_owner_matches {
        spl_token::ID
    } else {
        system_program::ID
    };
    let mut lamports = 0u64;
    let account = AccountInfo::new(
        &key,
        false,
        true,
        &mut lamports,
        &mut data,
        &account_owner,
        false,
        0,
    );

    let result = verify_user_token_account(&account, &expected_token_owner, &expected_mint);
    let structurally_valid = delegate_tag <= 1 && native_tag <= 1 && close_authority_tag <= 1;
    let expected_acceptance = program_owner_matches
        && structurally_valid
        && account_state == 1
        && mint_matches
        && token_owner_matches;
    assert_eq!(result.is_ok(), expected_acceptance);
    if let Ok(decoded_amount) = result {
        assert_eq!(decoded_amount, amount);
    }

    kani::cover!(expected_acceptance, "exact initialized account accepted");
    kani::cover!(
        !program_owner_matches,
        "wrong account program owner rejected"
    );
    kani::cover!(delegate_tag > 1, "malformed delegate tag rejected");
    kani::cover!(native_tag > 1, "malformed native tag rejected");
    kani::cover!(
        close_authority_tag > 1,
        "malformed close-authority tag rejected"
    );
    kani::cover!(account_state == 0, "uninitialized account rejected");
    kani::cover!(account_state == 2, "frozen account rejected");
    kani::cover!(account_state > 2, "unknown account state rejected");
    kani::cover!(
        program_owner_matches && structurally_valid && account_state == 1 && !mint_matches,
        "wrong mint rejected"
    );
    kani::cover!(
        program_owner_matches
            && structurally_valid
            && account_state == 1
            && mint_matches
            && !token_owner_matches,
        "wrong token owner rejected"
    );
    kani::cover!(
        expected_acceptance && native_tag == 1,
        "valid native-account encoding remains an admitted classic-SPL account"
    );
}

#[kani::proof]
fn kani_inv018_balance_gate_is_full_width_exact() {
    let balance: u64 = kani::any();
    let amount: u64 = kani::any();
    assert_eq!(
        require_token_balance(balance, amount).is_ok(),
        balance >= amount
    );
    kani::cover!(balance < amount, "underfunded account rejected");
    kani::cover!(balance == amount, "exact balance admitted");
    kani::cover!(balance > amount, "surplus balance admitted");
}
