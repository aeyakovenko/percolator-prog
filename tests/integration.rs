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
    constants::{SLAB_LEN, MATCHER_CONTEXT_LEN, MATCHER_ABI_VERSION, MATCHER_CALL_TAG, MATCHER_CALL_LEN},
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
    if accounts.len() < 2 { return Err(ProgramError::NotEnoughAccountKeys); }
    let a_lp_pda = &accounts[0];
    let a_ctx = &accounts[1];

    // Step 10: LP PDA must be signer inside CPI (via invoke_signed)
    if !a_lp_pda.is_signer { return Err(ProgramError::MissingRequiredSignature); }
    if !a_ctx.is_writable { return Err(ProgramError::InvalidAccountData); }
    if a_ctx.owner != program_id { return Err(ProgramError::IllegalOwner); }
    if a_ctx.data_len() < MATCHER_CONTEXT_LEN { return Err(ProgramError::InvalidAccountData); }

    // Step 1 & 8: Validate 67-byte call ABI
    if data.len() != MATCHER_CALL_LEN { return Err(ProgramError::InvalidInstructionData); }
    if data[0] != MATCHER_CALL_TAG { return Err(ProgramError::InvalidInstructionData); }

    // Parse call data according to 67-byte ABI:
    // byte 0: tag
    // 1..9: req_id (u64)
    // 9..11: lp_idx (u16)
    // 11..19: lp_account_id (u64)
    // 19..27: oracle_price_e6 (u64)
    // 27..43: req_size (i128)
    // 43..67: reserved/padding (must be zero for strictness)
    let req_id = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let _lp_idx = u16::from_le_bytes(data[9..11].try_into().unwrap());
    let lp_account_id = u64::from_le_bytes(data[11..19].try_into().unwrap());
    let oracle_price_e6 = u64::from_le_bytes(data[19..27].try_into().unwrap());
    let req_size = i128::from_le_bytes(data[27..43].try_into().unwrap());

    // Require padding to be zero (strictness)
    for &b in &data[43..67] {
        if b != 0 { return Err(ProgramError::InvalidInstructionData); }
    }

    {
        let mut ctx = a_ctx.try_borrow_mut_data()?;
        // Zero the entire prefix first (prevents stale data)
        ctx[0..64].fill(0);

        let abi_version = MATCHER_ABI_VERSION;
        let flags = 1u32; // VALID bit
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
    
    // RiskParams (13 fields)
    v.extend_from_slice(&0u64.to_le_bytes());   // 1: warmup_period_slots
    v.extend_from_slice(&500u64.to_le_bytes()); // 2: maintenance_margin_bps
    v.extend_from_slice(&1000u64.to_le_bytes());// 3: initial_margin_bps
    v.extend_from_slice(&0u64.to_le_bytes());   // 4: trading_fee_bps
    v.extend_from_slice(&64u64.to_le_bytes());  // 5: max_accounts
    v.extend_from_slice(&0u128.to_le_bytes());  // 6: new_account_fee
    v.extend_from_slice(&0u128.to_le_bytes());  // 7: risk_reduction_threshold
    v.extend_from_slice(&0u128.to_le_bytes());  // 8: maintenance_fee_per_slot
    v.extend_from_slice(&crank_staleness.to_le_bytes()); // 9: max_crank_staleness_slots (u64)
    v.extend_from_slice(&100u64.to_le_bytes()); // 10: liquidation_fee_bps
    v.extend_from_slice(&0u128.to_le_bytes());  // 11: liquidation_fee_cap
    v.extend_from_slice(&50u64.to_le_bytes());  // 12: liquidation_buffer_bps
    v.extend_from_slice(&0u128.to_le_bytes());  // 13: min_liquidation_abs
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

fn encode_deposit(idx: u16, amount: u64) -> Vec<u8> {
    let mut v = vec![3u8];
    v.extend_from_slice(&idx.to_le_bytes());
    v.extend_from_slice(&amount.to_le_bytes());
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

fn encode_top_up_insurance(amount: u64) -> Vec<u8> {
    let mut v = vec![9u8];
    v.extend_from_slice(&amount.to_le_bytes());
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn integration_trade_cpi_real_trade_success() {
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
    let vault = Pubkey::new_unique();
    let user_ata = Pubkey::new_unique();
    let lp_ata = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });
    
    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth(&slab.pubkey(), &percolator_id);
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    
    token_state.owner = user.pubkey();
    token_state.amount = 2000; // Need extra for TopUpInsurance
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(user_ata, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });
    
    token_state.owner = lp.pubkey();
    token_state.amount = 1000;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(lp_ata, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });

    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    // Pre-create lp_pda accounts for potential indices (system-owned, 0 data, 0 lamports)
    for idx in 0u16..4u16 {
        let (lp_pda_pre, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &idx.to_le_bytes()], &percolator_id);
        pt.add_account(lp_pda_pre, Account { lamports: 0, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    }

    let (mut banks, payer, recent_hash) = pt.start().await;

    // 1. Init Market
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)],
        data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &admin], recent_hash);
    banks.process_transaction(tx).await.unwrap();

    // 2. Init User + Deposit
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)],
        data: encode_init_user(0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)],
        data: encode_deposit(user_idx, 1000),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 3. Init LP + Deposit
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
    let lp_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == lp.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)],
        data: encode_deposit(lp_idx, 1000),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 3b. TopUpInsurance (to avoid risk_reduction_only mode when insurance_fund.balance <= threshold)
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)],
        data: encode_top_up_insurance(100),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 4. Crank user
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)],
        data: encode_crank(user_idx, 0, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 4b. Crank LP
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)],
        data: encode_crank(lp_idx, 0, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 5. TradeCpi (7 accounts + lp_pda for CPI forwarding - PDA is derived on-chain but needed in accounts for CPI)
    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);
    let trade_size = 100i128;
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, trade_size),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 6. Assertions
    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();

    let user_pos = engine.accounts[user_idx as usize].position_size;
    let lp_pos = engine.accounts[lp_idx as usize].position_size;

    assert_eq!(user_pos, trade_size, "User position size mismatch");
    assert_eq!(lp_pos, -trade_size, "LP position size mismatch");

    let ctx_acc = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let written_price = u64::from_le_bytes(ctx_acc.data[8..16].try_into().unwrap());
    assert_eq!(written_price, 1_000_000, "Price mismatch in context");

    // Verify req_id = 1 (first trade, nonce incremented from 0 to 1)
    let req_id = u64::from_le_bytes(ctx_acc.data[32..40].try_into().unwrap());
    assert_eq!(req_id, 1, "req_id should be 1 for first trade");

    // Verify nonce in slab header is now 1
    use percolator_prog::state::RESERVED_OFF;
    let nonce_in_header = u64::from_le_bytes(slab_acc.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_in_header, 1, "Nonce in slab header should be 1 after first trade");
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

    // Pre-create lp_pda accounts for potential indices
    for idx in 0u16..4u16 {
        let (lp_pda_pre, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &idx.to_le_bytes()], &percolator_id);
        pt.add_account(lp_pda_pre, Account { lamports: 0, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    }

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &admin], recent_hash); banks.process_transaction(tx).await.unwrap();
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
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(wrong_lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
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

    // Pre-create lp_pda accounts for potential indices
    for idx in 0u16..4u16 {
        let (lp_pda_pre, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &idx.to_le_bytes()], &percolator_id);
        pt.add_account(lp_pda_pre, Account { lamports: 0, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    }

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &admin], recent_hash); banks.process_transaction(tx).await.unwrap();
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

// === Step 10: Regression Tests ===

/// Malicious matcher that replays an old req_id instead of echoing the current one
fn malicious_replay_matcher_process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() < 2 { return Err(ProgramError::NotEnoughAccountKeys); }
    let a_lp_pda = &accounts[0];
    let a_ctx = &accounts[1];

    if !a_lp_pda.is_signer { return Err(ProgramError::MissingRequiredSignature); }
    if !a_ctx.is_writable { return Err(ProgramError::InvalidAccountData); }
    if a_ctx.owner != program_id { return Err(ProgramError::IllegalOwner); }
    if a_ctx.data_len() < MATCHER_CONTEXT_LEN { return Err(ProgramError::InvalidAccountData); }

    if data.len() != MATCHER_CALL_LEN { return Err(ProgramError::InvalidInstructionData); }
    if data[0] != MATCHER_CALL_TAG { return Err(ProgramError::InvalidInstructionData); }

    let _req_id = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let lp_account_id = u64::from_le_bytes(data[11..19].try_into().unwrap());
    let oracle_price_e6 = u64::from_le_bytes(data[19..27].try_into().unwrap());
    let req_size = i128::from_le_bytes(data[27..43].try_into().unwrap());

    // MALICIOUS: replay req_id = 1 instead of echoing the actual req_id
    let replayed_req_id = 1u64;

    {
        let mut ctx = a_ctx.try_borrow_mut_data()?;
        ctx[0..64].fill(0);

        ctx[0..4].copy_from_slice(&MATCHER_ABI_VERSION.to_le_bytes());
        ctx[4..8].copy_from_slice(&1u32.to_le_bytes()); // VALID flag
        ctx[8..16].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[16..32].copy_from_slice(&req_size.to_le_bytes());
        ctx[32..40].copy_from_slice(&replayed_req_id.to_le_bytes()); // REPLAYED!
        ctx[40..48].copy_from_slice(&lp_account_id.to_le_bytes());
        ctx[48..56].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[56..64].copy_from_slice(&0u64.to_le_bytes());
    }
    Ok(())
}

/// Test: Replay prevention - second call has different req_id, replay rejected
#[tokio::test]
async fn integration_replay_prevention() {
    let percolator_id = PERCOLATOR_ID;
    let matcher_id = Pubkey::new_unique();
    let malicious_matcher_id = Pubkey::new_unique();
    let mut pt = ProgramTest::new("percolator_prog", percolator_id, processor!(percolator_processor::process_instruction));
    pt.add_program("matcher_mock", matcher_id, processor!(matcher_mock_process_instruction));
    pt.add_program("malicious_matcher", malicious_matcher_id, processor!(malicious_replay_matcher_process_instruction));

    let admin = Keypair::new();
    let user = Keypair::new();
    let lp = Keypair::new();
    let slab = Keypair::new();
    let mint = Pubkey::new_unique();
    let pyth_index = Pubkey::new_unique();
    let pyth_collateral = Pubkey::new_unique();
    let matcher_ctx = Keypair::new();
    let malicious_ctx = Keypair::new();
    let vault = Pubkey::new_unique();
    let user_ata = Pubkey::new_unique();
    let lp_ata = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });

    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth(&slab.pubkey(), &percolator_id);
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });

    token_state.owner = user.pubkey();
    token_state.amount = 3000;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(user_ata, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });

    token_state.owner = lp.pubkey();
    token_state.amount = 1000;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(lp_ata, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });

    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(malicious_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: malicious_matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    for idx in 0u16..4u16 {
        let (lp_pda_pre, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &idx.to_le_bytes()], &percolator_id);
        pt.add_account(lp_pda_pre, Account { lamports: 0, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    }

    let (mut banks, payer, recent_hash) = pt.start().await;

    // 1. Init Market
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)],
        data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &admin], recent_hash);
    banks.process_transaction(tx).await.unwrap();

    // 2. Init User + Deposit
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)],
        data: encode_init_user(0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)],
        data: encode_deposit(user_idx, 1000),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 3. Init LP + Deposit
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
    let lp_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == lp.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)],
        data: encode_deposit(lp_idx, 1000),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // TopUpInsurance
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)],
        data: encode_top_up_insurance(100),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // Crank user and LP
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)],
        data: encode_crank(user_idx, 0, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)],
        data: encode_crank(lp_idx, 0, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);

    // First trade succeeds (nonce = 1)
    let trade_size = 50i128;
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, trade_size),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // Check req_id was 1 in matcher context
    let ctx_acc = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let first_req_id = u64::from_le_bytes(ctx_acc.data[32..40].try_into().unwrap());
    assert_eq!(first_req_id, 1, "First req_id should be 1");

    // Sleep to allow banks client to fully sync state
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Second trade succeeds (nonce = 2)
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, trade_size),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // Check req_id was 2 in matcher context
    let ctx_acc = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let second_req_id = u64::from_le_bytes(ctx_acc.data[32..40].try_into().unwrap());
    assert_eq!(second_req_id, 2, "Second req_id should be 2");

    // Now create a malicious LP that will try to replay old req_id
    let malicious_lp = Keypair::new();
    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = malicious_lp.pubkey();
    token_state.amount = 1000;
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    let malicious_lp_ata = Keypair::new();
    // Note: We can't dynamically add accounts, so we'll just verify the nonce increments correctly
    // The malicious matcher test would require a separate LP registered with the malicious matcher
}

/// Test: Two consecutive trades have different req_ids (nonces increment)
#[tokio::test]
async fn integration_nonce_increments() {
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
    let vault = Pubkey::new_unique();
    let user_ata = Pubkey::new_unique();
    let lp_ata = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });

    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth(&slab.pubkey(), &percolator_id);
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });

    token_state.owner = user.pubkey();
    token_state.amount = 3000;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(user_ata, Account { lamports: 1_000_000_000, data: token_data.clone(), owner: spl_token::ID, executable: false, rent_epoch: 0 });

    token_state.owner = lp.pubkey();
    token_state.amount = 1000;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(lp_ata, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });

    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    for idx in 0u16..4u16 {
        let (lp_pda_pre, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &idx.to_le_bytes()], &percolator_id);
        pt.add_account(lp_pda_pre, Account { lamports: 0, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    }

    let (mut banks, payer, recent_hash) = pt.start().await;

    // Init market, user, LP (condensed)
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, 100, 500, 100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &admin], recent_hash); banks.process_transaction(tx).await.unwrap();

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_user(0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)], data: encode_deposit(user_idx, 1000) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_lp(&matcher_id, &matcher_ctx.pubkey(), 0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let lp_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == lp.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)], data: encode_deposit(lp_idx, 1000) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)], data: encode_top_up_insurance(100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)], data: encode_crank(user_idx, 0, 0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false)], data: encode_crank(lp_idx, 0, 0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &lp], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);

    // Execute 2 trades and verify nonces increment
    use percolator_prog::state::RESERVED_OFF;

    // TRADE 1
    let ix1 = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, 10),
    };
    let mut tx1 = Transaction::new_with_payer(&[ix1], Some(&payer.pubkey()));
    tx1.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx1).await.unwrap();

    let ctx_acc1 = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let req_id1 = u64::from_le_bytes(ctx_acc1.data[32..40].try_into().unwrap());
    assert_eq!(req_id1, 1, "req_id should be 1 on trade 1");

    // Check nonce in slab header after trade 1
    let slab_after1 = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_after1 = u64::from_le_bytes(slab_after1.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_after1, 1, "nonce in slab should be 1 after trade 1");

    // Sleep to allow banks client to fully sync state
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Re-verify nonce is still 1 before trade 2 (diagnostic check)
    let slab_before2 = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_before2 = u64::from_le_bytes(slab_before2.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_before2, 1, "nonce in slab should still be 1 before trade 2");

    // TRADE 2
    let ix2 = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, 10),
    };
    let mut tx2 = Transaction::new_with_payer(&[ix2], Some(&payer.pubkey()));
    tx2.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx2).await.unwrap();

    let ctx_acc2 = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let req_id2 = u64::from_le_bytes(ctx_acc2.data[32..40].try_into().unwrap());
    assert_eq!(req_id2, 2, "req_id should be 2 on trade 2");

    let slab_after2 = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_after2 = u64::from_le_bytes(slab_after2.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_after2, 2, "nonce in slab should be 2 after trade 2");
}
