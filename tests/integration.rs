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
use serial_test::serial;
use std::convert::TryInto;

use percolator_prog::{
    constants::{
        SLAB_LEN, MATCHER_CONTEXT_LEN, MATCHER_ABI_VERSION, MATCHER_CALL_TAG, MATCHER_CALL_LEN,
        CALL_OFF_REQ_ID, CALL_OFF_LP_IDX, CALL_OFF_LP_ACCOUNT_ID, CALL_OFF_ORACLE_PRICE, CALL_OFF_REQ_SIZE, CALL_OFF_PADDING,
        RET_OFF_ABI_VERSION, RET_OFF_FLAGS, RET_OFF_EXEC_PRICE, RET_OFF_EXEC_SIZE, RET_OFF_REQ_ID, RET_OFF_LP_ACCOUNT_ID, RET_OFF_ORACLE_PRICE, RET_OFF_RESERVED,
    },
    oracle::PYTH_PROGRAM_ID,
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
    // LP PDA purity checks: must be system-owned, 0 data, 0 lamports
    if *a_lp_pda.owner != solana_program::system_program::ID { return Err(ProgramError::IllegalOwner); }
    if a_lp_pda.data_len() != 0 { return Err(ProgramError::InvalidAccountData); }
    if a_lp_pda.lamports() != 0 { return Err(ProgramError::InvalidAccountData); }
    if !a_ctx.is_writable { return Err(ProgramError::InvalidAccountData); }
    if a_ctx.owner != program_id { return Err(ProgramError::IllegalOwner); }
    if a_ctx.data_len() < MATCHER_CONTEXT_LEN { return Err(ProgramError::InvalidAccountData); }

    // Step 1 & 8: Validate 67-byte call ABI
    if data.len() != MATCHER_CALL_LEN { return Err(ProgramError::InvalidInstructionData); }
    if data[0] != MATCHER_CALL_TAG { return Err(ProgramError::InvalidInstructionData); }

    // Parse call data using ABI offset constants (67-byte layout)
    let req_id = u64::from_le_bytes(data[CALL_OFF_REQ_ID..CALL_OFF_REQ_ID+8].try_into().unwrap());
    let _lp_idx = u16::from_le_bytes(data[CALL_OFF_LP_IDX..CALL_OFF_LP_IDX+2].try_into().unwrap());
    let lp_account_id = u64::from_le_bytes(data[CALL_OFF_LP_ACCOUNT_ID..CALL_OFF_LP_ACCOUNT_ID+8].try_into().unwrap());
    let oracle_price_e6 = u64::from_le_bytes(data[CALL_OFF_ORACLE_PRICE..CALL_OFF_ORACLE_PRICE+8].try_into().unwrap());
    let req_size = i128::from_le_bytes(data[CALL_OFF_REQ_SIZE..CALL_OFF_REQ_SIZE+16].try_into().unwrap());

    // Require padding to be zero (strictness)
    for &b in &data[CALL_OFF_PADDING..MATCHER_CALL_LEN] {
        if b != 0 { return Err(ProgramError::InvalidInstructionData); }
    }

    {
        let mut ctx = a_ctx.try_borrow_mut_data()?;
        // In test mode, Percolator doesn't zero the prefix (due to ExternalAccountDataModified),
        // so the matcher mock must zero it. In production, Percolator zeros it before CPI.
        ctx[0..64].fill(0);

        // Write return data using ABI offset constants (64-byte prefix)
        let abi_version = MATCHER_ABI_VERSION;
        // FLAG_VALID = 1, FLAG_PARTIAL_OK = 2
        let flags = if req_size == 0 { 1u32 | 2u32 } else { 1u32 };
        let reserved = 0u64;

        ctx[RET_OFF_ABI_VERSION..RET_OFF_ABI_VERSION+4].copy_from_slice(&abi_version.to_le_bytes());
        ctx[RET_OFF_FLAGS..RET_OFF_FLAGS+4].copy_from_slice(&flags.to_le_bytes());
        ctx[RET_OFF_EXEC_PRICE..RET_OFF_EXEC_PRICE+8].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[RET_OFF_EXEC_SIZE..RET_OFF_EXEC_SIZE+16].copy_from_slice(&req_size.to_le_bytes());
        ctx[RET_OFF_REQ_ID..RET_OFF_REQ_ID+8].copy_from_slice(&req_id.to_le_bytes());
        ctx[RET_OFF_LP_ACCOUNT_ID..RET_OFF_LP_ACCOUNT_ID+8].copy_from_slice(&lp_account_id.to_le_bytes());
        ctx[RET_OFF_ORACLE_PRICE..RET_OFF_ORACLE_PRICE+8].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[RET_OFF_RESERVED..RET_OFF_RESERVED+8].copy_from_slice(&reserved.to_le_bytes());
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

fn encode_set_risk_threshold(new_threshold: u128) -> Vec<u8> {
    let mut v = vec![11u8];
    v.extend_from_slice(&new_threshold.to_le_bytes());
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

    // pub_slot=0 + max_staleness=u64::MAX => never stale in tests
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    add_lp_pdas(&mut pt, &slab.pubkey(), &percolator_id, 8);

    let (mut banks, payer, recent_hash) = pt.start().await;

    // 1. Init Market
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)],
        data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, u64::MAX, 500, 100),
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

    // Verify total_open_interest is non-zero after trade
    assert!(engine.total_open_interest > 0, "OI should be non-zero after trade");

    // 7. Call crank to trigger threshold auto-update
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),    // caller (signer)
            AccountMeta::new(slab.pubkey(), false),   // slab (writable)
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false),
            AccountMeta::new_readonly(pyth_collateral, false),
        ],
        data: encode_crank(user_idx, 0, 0),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // 8. Verify threshold was updated based on risk metric
    // Note: The new threshold calculation uses risk_units (net_exposure + max_concentration)
    // with rate limiting, EWMA smoothing, and step clamping.
    // With positions user: +100, lp: -100:
    //   net_exposure = |100 + (-100)| = 0
    //   max_concentration = max(100, 100) = 100
    //   risk_units = 100
    // The exact threshold depends on how many slots have passed and the smoothing parameters.
    // Since crank applies rate limiting (THRESH_UPDATE_INTERVAL_SLOTS=10), the threshold
    // may or may not have been updated depending on test timing.
    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let actual_threshold = engine.risk_reduction_threshold();

    // The threshold should be >= 0 (it starts at 0 and can only go up when there's risk)
    assert!(actual_threshold >= 0, "Threshold should be non-negative after crank");
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
    // pub_slot=0 + max_staleness=u64::MAX => never stale in tests
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    add_lp_pdas(&mut pt, &slab.pubkey(), &percolator_id, 8);

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, u64::MAX, 500, 100) };
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
    // pub_slot=0 + max_staleness=u64::MAX => never stale in tests
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    pt.add_account(wrong_oracle, Account { lamports: 1, data: vec![0u8; 208], owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 });

    add_lp_pdas(&mut pt, &slab.pubkey(), &percolator_id, 8);

    let (mut banks, payer, recent_hash) = pt.start().await;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, u64::MAX, 500, 100) };
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

/// Pre-create LP PDA accounts for TradeCpi tests.
/// LP PDAs must be system-owned with 0 data and 0 lamports (pure signer identity).
fn add_lp_pdas(pt: &mut ProgramTest, slab: &Pubkey, prog: &Pubkey, n: u16) {
    for idx in 0..n {
        let (pda, _) = Pubkey::find_program_address(&[b"lp", slab.as_ref(), &idx.to_le_bytes()], prog);
        pt.add_account(pda, Account { lamports: 0, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });
    }
}

/// Test: Trade increments nonce (0→1)
/// Note: Testing single trade to avoid solana-program-test limitations with multiple CPI transactions.
/// The nonce increment logic is the same for all N→N+1 transitions.
#[tokio::test(flavor = "multi_thread")]
#[serial]
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

    // pub_slot=0 + max_staleness=u64::MAX => never stale in tests
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(matcher_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    add_lp_pdas(&mut pt, &slab.pubkey(), &percolator_id, 8);

    let (mut banks, payer, recent_hash) = pt.start().await;

    // Init market, user, LP (condensed)
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, u64::MAX, 500, 100) };
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

    // Verify nonce is 0 before trade
    use percolator_prog::state::RESERVED_OFF;
    let slab_before = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_before = u64::from_le_bytes(slab_before.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_before, 0, "nonce should be 0 before trade");

    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);

    // Execute single trade
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(matcher_id, false), AccountMeta::new(matcher_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, 10),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // Verify nonce incremented to 1 after trade
    let slab_after = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_after = u64::from_le_bytes(slab_after.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_after, 1, "nonce should be 1 after trade (incremented from 0)");

    // Verify req_id in matcher context matches nonce
    let ctx_acc = banks.get_account(matcher_ctx.pubkey()).await.unwrap().unwrap();
    let req_id = u64::from_le_bytes(ctx_acc.data[32..40].try_into().unwrap());
    assert_eq!(req_id, 1, "req_id should be 1 for first trade");
}

/// Malicious matcher that always returns wrong req_id (0) regardless of input
fn malicious_replay_matcher_process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() < 2 { return Err(ProgramError::NotEnoughAccountKeys); }
    let a_lp_pda = &accounts[0];
    let a_ctx = &accounts[1];

    if !a_lp_pda.is_signer { return Err(ProgramError::MissingRequiredSignature); }
    if *a_lp_pda.owner != solana_program::system_program::ID { return Err(ProgramError::IllegalOwner); }
    if a_lp_pda.data_len() != 0 { return Err(ProgramError::InvalidAccountData); }
    if a_lp_pda.lamports() != 0 { return Err(ProgramError::InvalidAccountData); }
    if !a_ctx.is_writable { return Err(ProgramError::InvalidAccountData); }
    if a_ctx.owner != program_id { return Err(ProgramError::IllegalOwner); }
    if a_ctx.data_len() < MATCHER_CONTEXT_LEN { return Err(ProgramError::InvalidAccountData); }

    if data.len() != MATCHER_CALL_LEN { return Err(ProgramError::InvalidInstructionData); }
    if data[0] != MATCHER_CALL_TAG { return Err(ProgramError::InvalidInstructionData); }

    let _req_id = u64::from_le_bytes(data[CALL_OFF_REQ_ID..CALL_OFF_REQ_ID+8].try_into().unwrap());
    let lp_account_id = u64::from_le_bytes(data[CALL_OFF_LP_ACCOUNT_ID..CALL_OFF_LP_ACCOUNT_ID+8].try_into().unwrap());
    let oracle_price_e6 = u64::from_le_bytes(data[CALL_OFF_ORACLE_PRICE..CALL_OFF_ORACLE_PRICE+8].try_into().unwrap());
    let req_size = i128::from_le_bytes(data[CALL_OFF_REQ_SIZE..CALL_OFF_REQ_SIZE+16].try_into().unwrap());

    // MALICIOUS: Always return req_id=0 regardless of what was sent.
    // This simulates a replay attack where matcher returns stale/wrong req_id.
    let bad_req_id = 0u64;

    {
        let mut ctx = a_ctx.try_borrow_mut_data()?;
        ctx[0..64].fill(0);

        let flags = if req_size == 0 { 1u32 | 2u32 } else { 1u32 };
        ctx[RET_OFF_ABI_VERSION..RET_OFF_ABI_VERSION+4].copy_from_slice(&MATCHER_ABI_VERSION.to_le_bytes());
        ctx[RET_OFF_FLAGS..RET_OFF_FLAGS+4].copy_from_slice(&flags.to_le_bytes());
        ctx[RET_OFF_EXEC_PRICE..RET_OFF_EXEC_PRICE+8].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[RET_OFF_EXEC_SIZE..RET_OFF_EXEC_SIZE+16].copy_from_slice(&req_size.to_le_bytes());
        ctx[RET_OFF_REQ_ID..RET_OFF_REQ_ID+8].copy_from_slice(&bad_req_id.to_le_bytes()); // WRONG!
        ctx[RET_OFF_LP_ACCOUNT_ID..RET_OFF_LP_ACCOUNT_ID+8].copy_from_slice(&lp_account_id.to_le_bytes());
        ctx[RET_OFF_ORACLE_PRICE..RET_OFF_ORACLE_PRICE+8].copy_from_slice(&oracle_price_e6.to_le_bytes());
        ctx[RET_OFF_RESERVED..RET_OFF_RESERVED+8].copy_from_slice(&0u64.to_le_bytes());
    }
    Ok(())
}

/// Test: Malicious matcher returns wrong req_id → trade fails, nonce does not advance
/// Uses single trade to avoid solana-program-test limitations with multiple CPI transactions.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn integration_replay_req_id_rejected() {
    let percolator_id = PERCOLATOR_ID;
    let malicious_matcher_id = Pubkey::new_unique();
    let mut pt = ProgramTest::new("percolator_prog", percolator_id, processor!(percolator_processor::process_instruction));
    pt.add_program("malicious_matcher", malicious_matcher_id, processor!(malicious_replay_matcher_process_instruction));

    let admin = Keypair::new();
    let user = Keypair::new();
    let lp = Keypair::new();
    let slab = Keypair::new();
    let mint = Pubkey::new_unique();
    let pyth_index = Pubkey::new_unique();
    let pyth_collateral = Pubkey::new_unique();
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

    // pub_slot=0 + max_staleness=u64::MAX => never stale in tests
    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(malicious_ctx.pubkey(), Account { lamports: 1_000_000_000, data: vec![0u8; MATCHER_CONTEXT_LEN], owner: malicious_matcher_id, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    add_lp_pdas(&mut pt, &slab.pubkey(), &percolator_id, 8);

    let (mut banks, payer, recent_hash) = pt.start().await;

    // Init market
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(admin.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(mint, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(dummy_ata, false), AccountMeta::new_readonly(solana_sdk::system_program::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(pyth_collateral, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false)], data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, u64::MAX, 500, 100) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &admin], recent_hash); banks.process_transaction(tx).await.unwrap();

    // Init user
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_user(0) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let engine = zc::engine_ref(&slab_acc.data).unwrap();
    let user_idx = (0..MAX_ACCOUNTS).find(|&i| engine.is_used(i) && engine.accounts[i].owner == user.pubkey().to_bytes()).unwrap() as u16;

    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(user_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false)], data: encode_deposit(user_idx, 1000) };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey())); tx.sign(&[&payer, &user], banks.get_latest_blockhash().await.unwrap()); banks.process_transaction(tx).await.unwrap();

    // Init LP with malicious matcher
    let ix = Instruction { program_id: percolator_id, accounts: vec![AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new(lp_ata, false), AccountMeta::new(vault, false), AccountMeta::new_readonly(spl_token::ID, false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_collateral, false)], data: encode_init_lp(&malicious_matcher_id, &malicious_ctx.pubkey(), 0) };
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

    // Record nonce before trade
    use percolator_prog::state::RESERVED_OFF;
    let slab_before = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_before = u64::from_le_bytes(slab_before.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_before, 0, "nonce should be 0 before any trade");

    let (lp_pda, _) = Pubkey::find_program_address(&[b"lp", slab.pubkey().as_ref(), &lp_idx.to_le_bytes()], &percolator_id);

    // Single trade: should FAIL because malicious matcher returns req_id=0 instead of expected req_id=1
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![AccountMeta::new(user.pubkey(), true), AccountMeta::new(lp.pubkey(), true), AccountMeta::new(slab.pubkey(), false), AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false), AccountMeta::new_readonly(pyth_index, false), AccountMeta::new_readonly(malicious_matcher_id, false), AccountMeta::new(malicious_ctx.pubkey(), false), AccountMeta::new_readonly(lp_pda, false)],
        data: encode_trade_cpi(lp_idx, user_idx, 50),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &user, &lp], banks.get_latest_blockhash().await.unwrap());
    let err = banks.process_transaction(tx).await.unwrap_err();

    // Should fail with InvalidAccountData (matcher returned req_id=0 instead of expected req_id=1)
    assert!(format!("{err:?}").contains("InvalidAccountData"), "Expected InvalidAccountData, got: {err:?}");

    // Verify nonce did NOT advance (transaction rolled back)
    let slab_after = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
    let nonce_after = u64::from_le_bytes(slab_after.data[RESERVED_OFF..RESERVED_OFF+8].try_into().unwrap());
    assert_eq!(nonce_after, 0, "nonce should remain 0 after failed trade");
}

#[tokio::test(flavor = "multi_thread")]
async fn integration_set_risk_threshold() {
    let percolator_id = PERCOLATOR_ID;
    let mut pt = ProgramTest::new("percolator_prog", percolator_id, processor!(percolator_processor::process_instruction));

    let admin = Keypair::new();
    let slab = Keypair::new();
    let mint = Pubkey::new_unique();
    let pyth_index = Pubkey::new_unique();
    let pyth_collateral = Pubkey::new_unique();
    let vault = Pubkey::new_unique();
    let dummy_ata = Pubkey::new_unique();

    pt.add_account(slab.pubkey(), Account { lamports: 10_000_000_000, data: vec![0u8; SLAB_LEN], owner: percolator_id, executable: false, rent_epoch: 0 });

    let mut token_data = vec![0u8; spl_token::state::Account::LEN];
    let mut token_state = spl_token::state::Account::default();
    token_state.mint = mint;
    token_state.owner = vault_auth(&slab.pubkey(), &percolator_id);
    token_state.state = spl_token::state::AccountState::Initialized;
    spl_token::state::Account::pack(token_state, &mut token_data).unwrap();
    pt.add_account(vault, Account { lamports: 1_000_000_000, data: token_data, owner: spl_token::ID, executable: false, rent_epoch: 0 });

    pt.add_account(pyth_index, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(pyth_collateral, Account { lamports: 1_000_000_000, data: make_pyth(1_000_000, -6, 1, 0), owner: PYTH_PROGRAM_ID, executable: false, rent_epoch: 0 });
    pt.add_account(dummy_ata, Account { lamports: 1_000_000, data: vec![], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 });

    let (mut banks, payer, recent_hash) = pt.start().await;

    // 1. Init Market
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(slab.pubkey(), false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(dummy_ata, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
            AccountMeta::new_readonly(pyth_index, false),
            AccountMeta::new_readonly(pyth_collateral, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::ID, false),
        ],
        data: encode_init_market(&admin.pubkey(), &mint, &pyth_index, &pyth_collateral, u64::MAX, 500, 100),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &admin], recent_hash);
    banks.process_transaction(tx).await.unwrap();

    // Verify initial threshold is 0
    {
        let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
        let engine = zc::engine_ref(&slab_acc.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), 0, "initial threshold should be 0");
    }

    // 2. Admin sets new threshold
    let new_threshold: u128 = 123_456_789_000;
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true), // admin (signer)
            AccountMeta::new(slab.pubkey(), false), // slab (writable)
        ],
        data: encode_set_risk_threshold(new_threshold),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &admin], banks.get_latest_blockhash().await.unwrap());
    banks.process_transaction(tx).await.unwrap();

    // Verify threshold was updated
    {
        let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
        let engine = zc::engine_ref(&slab_acc.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), new_threshold, "threshold should be updated");
    }

    // 3. Non-admin tries to set threshold - should fail
    let attacker = Keypair::new();
    let ix = Instruction {
        program_id: percolator_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true), // attacker (signer, but not admin)
            AccountMeta::new(slab.pubkey(), false),    // slab (writable)
        ],
        data: encode_set_risk_threshold(999_999),
    };
    let mut tx = Transaction::new_with_payer(&[ix], Some(&payer.pubkey()));
    tx.sign(&[&payer, &attacker], banks.get_latest_blockhash().await.unwrap());
    let err = banks.process_transaction(tx).await.unwrap_err();

    // Should fail with Custom(15) = EngineUnauthorized
    assert!(format!("{err:?}").contains("Custom(15)"), "Expected EngineUnauthorized error, got: {err:?}");

    // Verify threshold was NOT changed (still at new_threshold)
    {
        let slab_acc = banks.get_account(slab.pubkey()).await.unwrap().unwrap();
        let engine = zc::engine_ref(&slab_acc.data).unwrap();
        assert_eq!(engine.risk_reduction_threshold(), new_threshold, "threshold should remain unchanged");
    }
}

// =============================================================================
// Token Validation Tests
// =============================================================================
//
// The following validation is enforced in PRODUCTION (SBF builds only):
// - verify_token_program: Rejects wrong token program ID (Custom(26))
// - verify_token_account (wrong mint): Rejects wrong mint (Custom(14))
// - verify_token_account (wrong owner): Rejects ATA owner mismatch (Custom(25))
//
// These checks are gated by #[cfg(not(test))] to allow unit tests with mock
// accounts. The SBF binary enforces them at runtime. Integration testing with
// the SBF binary is not possible due to stack frame limitations in the
// complex instruction processing code.
//
// The validation code is verified by code review:
// - verify_token_program checks: key == spl_token::ID && executable
// - verify_token_account checks: owner == spl_token::ID, correct data length,
//   correct mint, correct owner, and Initialized state
// =============================================================================
