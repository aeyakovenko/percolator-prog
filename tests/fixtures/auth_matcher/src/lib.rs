#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint, entrypoint::ProgramResult, program::set_return_data,
    program_error::ProgramError, pubkey::Pubkey,
};

entrypoint!(process);

const ABI: u32 = 3;
const FLAG_VALID: u32 = 1;
const CTX_STATE_OFFSET: usize = 64;
const CTX_MIN_LEN: usize = CTX_STATE_OFFSET + 33;

fn write_return(
    out: &mut [u8],
    req_id: u64,
    lp: u64,
    asset: u64,
    oracle: u64,
    size: i128,
) {
    out[0..4].copy_from_slice(&ABI.to_le_bytes());
    out[4..8].copy_from_slice(&FLAG_VALID.to_le_bytes());
    out[8..16].copy_from_slice(&oracle.to_le_bytes());
    out[16..32].copy_from_slice(&size.to_le_bytes());
    out[32..40].copy_from_slice(&req_id.to_le_bytes());
    out[40..48].copy_from_slice(&lp.to_le_bytes());
    out[48..56].copy_from_slice(&oracle.to_le_bytes());
    out[56..64].copy_from_slice(&asset.to_le_bytes());
}

fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match data.first() {
        Some(&2) => process_init(program_id, accounts),
        Some(&0) => process_single(program_id, accounts, data),
        Some(&3) => process_batch(program_id, accounts, data),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn process_init(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let lp_owner = next_account_info(account_iter)?;
    let delegate = next_account_info(account_iter)?;
    let ctx = next_account_info(account_iter)?;
    let percolator_program = next_account_info(account_iter)?;
    let market = next_account_info(account_iter)?;
    let lp_portfolio = next_account_info(account_iter)?;

    if !lp_owner.is_signer || !ctx.is_writable || ctx.owner != program_id || ctx.data_len() < CTX_MIN_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let expected = Pubkey::find_program_address(
        &[
            b"matcher",
            market.key.as_ref(),
            lp_portfolio.key.as_ref(),
            lp_owner.key.as_ref(),
            program_id.as_ref(),
            ctx.key.as_ref(),
        ],
        percolator_program.key,
    )
    .0;
    if expected != *delegate.key {
        return Err(ProgramError::InvalidSeeds);
    }

    let mut ctx_data = ctx.try_borrow_mut_data()?;
    if ctx_data[CTX_STATE_OFFSET] != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    ctx_data[CTX_STATE_OFFSET] = 1;
    ctx_data[CTX_STATE_OFFSET + 1..CTX_STATE_OFFSET + 33].copy_from_slice(delegate.key.as_ref());
    Ok(())
}

fn check_ctx(program_id: &Pubkey, delegate: &AccountInfo, ctx: &AccountInfo) -> ProgramResult {
    if !delegate.is_signer || ctx.owner != program_id || ctx.data_len() < CTX_MIN_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    {
        let ctx_data = ctx.try_borrow_data()?;
        if ctx_data[CTX_STATE_OFFSET] != 1
            || ctx_data[CTX_STATE_OFFSET + 1..CTX_STATE_OFFSET + 33] != delegate.key.as_ref()[..]
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    Ok(())
}

fn process_single(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 67 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let account_iter = &mut accounts.iter();
    let delegate = next_account_info(account_iter)?;
    let ctx = next_account_info(account_iter)?;
    check_ctx(program_id, delegate, ctx)?;
    let req_id = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let asset = u16::from_le_bytes(data[9..11].try_into().unwrap()) as u64;
    let lp = u64::from_le_bytes(data[11..19].try_into().unwrap());
    let oracle = u64::from_le_bytes(data[19..27].try_into().unwrap());
    let req = i128::from_le_bytes(data[27..43].try_into().unwrap());
    let mut ctx_data = ctx.try_borrow_mut_data()?;
    write_return(&mut ctx_data[0..64], req_id, lp, asset, oracle, req);
    Ok(())
}

fn process_batch(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 18 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let n = data[1] as usize;
    if n == 0 || n > 16 || data.len() != 18 + n * 26 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let account_iter = &mut accounts.iter();
    let delegate = next_account_info(account_iter)?;
    let ctx = next_account_info(account_iter)?;
    check_ctx(program_id, delegate, ctx)?;
    let req_id = u64::from_le_bytes(data[2..10].try_into().unwrap());
    let lp = u64::from_le_bytes(data[10..18].try_into().unwrap());
    let mut out = [0u8; 16 * 64];
    for i in 0..n {
        let base = 18 + i * 26;
        let asset = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as u64;
        let oracle = u64::from_le_bytes(data[base + 2..base + 10].try_into().unwrap());
        let req = i128::from_le_bytes(data[base + 10..base + 26].try_into().unwrap());
        write_return(&mut out[i * 64..i * 64 + 64], req_id, lp, asset, oracle, req);
    }
    set_return_data(&out[..n * 64]);
    Ok(())
}
