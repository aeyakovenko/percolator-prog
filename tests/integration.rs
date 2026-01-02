// tests/integration.rs
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::convert::TryInto;

use percolator_prog::{
    constants::{SLAB_LEN, MATCHER_CONTEXT_LEN},
    processor as percolator_processor,
    zc,
};
use percolator::MAX_ACCOUNTS;

pub const PERCOLATOR_ID: Pubkey = solana_program::pubkey!("Perco1ator111111111111111111111111111111111");

fn matcher_mock_process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() < 3 { return Err(ProgramError::NotEnoughAccountKeys); }
    let a_slab = &accounts[0];
    let a_lp_pda = &accounts[1];
    let a_ctx = &accounts[2];

    if !a_lp_pda.is_signer { return Err(ProgramError::MissingRequiredSignature); }
    if !a_ctx.is_writable { return Err(ProgramError::InvalidAccountData); }
    if a_ctx.owner != program_id { return Err(ProgramError::IllegalOwner); }
    if a_ctx.data_len() < MATCHER_CONTEXT_LEN { return Err(ProgramError::InvalidAccountData); }

    if data.len() != 67 { return Err(ProgramError::InvalidInstructionData); }
    let tag = data[0];
    if tag != 0 { return Err(ProgramError::InvalidInstructionData); }

    let slab_pk = Pubkey::new_from_array(data[1..33].try_into().unwrap());
    if slab_pk != *a_slab.key { return Err(ProgramError::InvalidArgument); }

    let lp_account_id = u64::from_le_bytes(data[35..43].try_into().unwrap());
    let oracle_price_e6 = u64::from_le_bytes(data[43..51].try_into().unwrap());
    let req_size = i128::from_le_bytes(data[51..67].try_into().unwrap());

    {
        let mut ctx = a_ctx.try_borrow_mut_data()?;
        let abi_version = 1u32;
        let flags = 1u32; // OK
        let req_id = 0u64;
        let reserved = 0u64;

        ctx[0..4].copy_from_slice(&abi_version.to_le_bytes());
        ctx[4..8].copy_from_slice(&flags.to_le_bytes());
        ctx[8..16].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[16..32].copy_from_slice(&req_size.to_le_bytes());
        ctx[32..40].copy_from_slice(&req_id.to_le_bytes());
        ctx[40..48].copy_from_slice(&lp_account_id.to_le_bytes());
        ctx[48..56].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[56..64].copy_from_slice(&reserved.to_le_bytes());
    }
    Ok(())
}

fn make_pyth(price: i64, expo: i32, conf: u64, pub_slot: u64) -> Vec<u8> {
    let mut data = vec![0u8; 208];
    data[20..24].copy_from_slice(&expo.to_le_bytes());
    data[176..184].copy_from_slice(&price.to_le_bytes());
    data[184..192].copy_from_slice(&conf.to_le_bytes());
    data[200..208].copy_from_slice(&pub_slot.to_le_bytes());
    data
}

fn encode_init_market(admin: &Pubkey, mint: &Pubkey, pyth_index: &Pubkey, pyth_collateral: &Pubkey, max_staleness: u64, conf_bps: u16, crank_staleness: u64) -> Vec<u8> {
    let mut v = vec![0u8];
    v.extend_from_slice(admin.as_ref());
    v.extend_from_slice(mint.as_ref());
    v.extend_from_slice(pyth_index.as_ref());
    v.extend_from_slice(pyth_collateral.as_ref());
    v.extend_from_slice(&max_staleness.to_le_bytes());
    v.extend_from_slice(&conf_bps.to_le_bytes());
    v.extend_from_slice(&0u64.to_le_bytes()); // RiskParams...
    v.extend_from_slice(&0u64.to_le_bytes());
    v.extend_from_slice(&0u64.to_le_bytes());
    v.extend_from_slice(&0u64.to_le_bytes());
    v.extend_from_slice(&64u64.to_le_bytes());
    v.extend_from_slice(&0u128.to_le_bytes());
    v.extend_from_slice(&0u128.to_le_bytes());
    v.extend_from_slice(&0u128.to_le_bytes());
    v.extend_from_slice(&crank_staleness.to_le_bytes());
    v.extend_from_slice(&0u64.to_le_bytes());
    v.extend_from_slice(&0u128.to_le_bytes());
    v.extend_from_slice(&0u64.to_le_bytes());
    v.extend_from_slice(&0u128.to_le_bytes());
    v
}

fn encode_init_user(fee: u64) -> Vec<u8> {
    let mut v = vec![1u8];
    v.extend_from_slice(&fee.to_le_bytes());
    v
}

fn encode_init_lp(matcher_program: &Pubkey, matcher_ctx: &Pubkey, fee: u64) -> Vec<u8> {
    let mut v = vec![2u8];
    v.extend_from_slice(matcher_program.as_ref());
    v.extend_from_slice(matcher_ctx.as_ref());
    v.extend_from_slice(&fee.to_le_bytes());
    v
}

fn encode_crank(caller_idx: u16, rate: i64, allow_panic: u8) -> Vec<u8> {
    let mut v = vec![5u8];
    v.extend_from_slice(&caller_idx.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.push(allow_panic);
    v
}

fn encode_trade_cpi(lp_idx: u16, user_idx: u16, size: i128) -> Vec<u8> {
    let mut v = vec![10u8];
    v.extend_from_slice(&lp_idx.to_le_bytes());
    v.extend_from_slice(&user_idx.to_le_bytes());
    v.extend_from_slice(&size.to_le_bytes());
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn integration_trade_cpi_success_writes_ctx_prefix() {
    let percolator_id = PERCOLATOR_ID;
    let matcher_id = Pubkey::new_unique();
    let mut pt = ProgramTest::new("percolator_prog", percolator_id, processor!(percolator_processor::process_instruction));
    pt.add_program("matcher_mock", matcher_id, processor!(matcher_mock_process_instruction));

    let admin = Keypair::new();
    let user = Keypair::new();
    let lp = Keypair::new();
    let slab = Keypair::new();
    let mint = Pubkey::new_unique(); 
    let pyth_index = Pubkey::new_unique();
    let pyth_collateral = Pubkey::new_unique();
    let matcher_ctx = Keypair::new();
    let (vault_auth, _) = Pubkey::find_program_address(&[b"vault", slab.pubkey().as_ref()], &percolator_id);
    let vault = Pubkey::new_unique();
    let user_ata = Pubkey::new_unique();
    let lp_ata = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });
    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth;
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    token_state.owner = user.pubkey();
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(user_ata, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    token_state.owner = lp.pubkey();
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(lp_ata, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    // Pre-allocate LP PDA (idx=1) to ensure existence for CPI
    let (lp_pda_assumed, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &1u16.to_le_bytes()], &percolator_id);
    pt.add_account(lp_pda_assumed, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)],
        data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &admin], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)],
        data: encode_init_user(0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)],
        data: encode_init_lp(&matcher_id, &matcher_ctx.pubkey(), 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;
    let lp_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == lp.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)],
        data: encode_crank(user_idx, 0, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);
    assert_eq!(lp_pda, lp_pda_assumed, "Assumed LP PDA matches actual");

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let ctx_acc = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let written_price = u64::from_le_bytes(ctx_acc.data[8..16].try_into().unwrap());
    assert_eq!(written_price, 1_000_000, "Price mismatch");
}

#[tokio::test(flavor = "multi_thread")]
async fn integration_trade_cpi_wrong_lp_signer_rejected() {
    let percolator_id = PERCOLATOR_ID;
    let matcher_id = Pubkey::new_unique();
    let mut pt = ProgramTest::new("percolator_prog", percolator_id, processor!(percolator_processor::process_instruction));
    pt.add_program("matcher_mock", matcher_id, processor!(matcher_mock_process_instruction));

    let admin = Keypair::new();
    let user = Keypair::new();
    let lp = Keypair::new();
    let slab = Keypair::new();
    let mint = Pubkey::new_unique(); 
    let pyth_index = Pubkey::new_unique();
    let pyth_collateral = Pubkey::new_unique();
    let matcher_ctx = Keypair::new();
    let user_ata = Pubkey::new_unique();
    let lp_ata = Pubkey::new_unique();
    let vault = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();
    let wrong_lp = Keypair::new();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });
    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth(&slab.pubkey(), &percolator_id);
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    token_state.owner = user.pubkey();
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(user_ata, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    token_state.owner = lp.pubkey();
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(lp_ata, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &admin], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_user(0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_lp(&matcher_id, &matcher_ctx.pubkey(), 0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;
    let lp_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == lp.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(wrong_lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false)],
        data: encode_trade_cpi(lp_idx, user_idx, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &wrong_lp], banks.get_latest_blockhash().await.unwrap());
    let err = banks.process_transaction(tx).await.unwrap_err();
    assert!(format!("{err:?}").contains(&format!("Custom({})", percolator_prog::error::PercolatorError::EngineUnauthorized as u32)));
}

#[tokio::test(flavor = "multi_thread")]
async fn integration_trade_cpi_wrong_oracle_fails() {
    let percolator_id = PERCOLATOR_ID;
    let matcher_id = Pubkey::new_unique();
    let mut pt = ProgramTest::new("percolator_prog", percolator_id, processor!(percolator_processor::process_instruction));
    pt.add_program("matcher_mock", matcher_id, processor!(matcher_mock_process_instruction));

    let admin = Keypair::new();
    let user = Keypair::new();
    let lp = Keypair::new();
    let slab = Keypair::new();
    let mint = Pubkey::new_unique(); 
    let pyth_index = Pubkey::new_unique();
    let pyth_collateral = Pubkey::new_unique();
    let matcher_ctx = Keypair::new();
    let user_ata = Pubkey::new_unique();
    let lp_ata = Pubkey::new_unique();
    let vault = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();
    let wrong_oracle = Pubkey::new_unique();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });
    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth(&slab.pubkey(), &percolator_id);
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    token_state.owner = user.pubkey();
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(user_ata, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    token_state.owner = lp.pubkey();
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(lp_ata, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    pt.add_account(wrong_oracle, Account { lamports: 1, data: vec![0u8; 208], owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &admin], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_user(0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_lp(&matcher_id, &matcher_ctx.pubkey(), 0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;
    let lp_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == lp.pubkey().to_bytes()).unwrap() as u16;

    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(wrong_oracle, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    let err = banks.process_transaction(tx).await.unwrap_err();
    assert!(format!("{err:?}").contains("InvalidArgument"));
}

fn vault_auth(slab: &Pubkey, prog: &Pubkey) -> Pubkey {
    let (pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], prog);
    pda
}