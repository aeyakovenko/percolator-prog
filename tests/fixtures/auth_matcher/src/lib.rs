#![allow(unexpected_cfgs)]

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint,
    entrypoint::ProgramResult,
    program::set_return_data,
    program_error::ProgramError,
    pubkey::Pubkey,
};

entrypoint!(process);

const ABI: u32 = 3;
const FLAG_VALID: u32 = 1;
const FLAG_BACKING_FEE_CAP_SHIFT: u32 = 8;
const CTX_STATE_OFFSET: usize = 64;
const CTX_DELEGATE_OFFSET: usize = CTX_STATE_OFFSET + 1;
const CTX_OWNER_OFFSET: usize = CTX_DELEGATE_OFFSET + 32;
const CTX_BID_SPREAD_OFFSET: usize = CTX_OWNER_OFFSET + 32;
const CTX_ASK_SPREAD_OFFSET: usize = CTX_BID_SPREAD_OFFSET + 8;
const CTX_BACKING_FEE_CAP_OFFSET: usize = CTX_ASK_SPREAD_OFFSET + 8;
const CTX_MIN_LEN: usize = CTX_BACKING_FEE_CAP_OFFSET + 2;

fn write_return(
    out: &mut [u8],
    req_id: u64,
    lp: u64,
    asset: u64,
    exec_price: u64,
    oracle_price: u64,
    size: i128,
    backing_fee_cap_bps: u16,
) {
    let flags = FLAG_VALID | ((backing_fee_cap_bps as u32) << FLAG_BACKING_FEE_CAP_SHIFT);
    out[0..4].copy_from_slice(&ABI.to_le_bytes());
    out[4..8].copy_from_slice(&flags.to_le_bytes());
    out[8..16].copy_from_slice(&exec_price.to_le_bytes());
    out[16..32].copy_from_slice(&size.to_le_bytes());
    out[32..40].copy_from_slice(&req_id.to_le_bytes());
    out[40..48].copy_from_slice(&lp.to_le_bytes());
    out[48..56].copy_from_slice(&oracle_price.to_le_bytes());
    out[56..64].copy_from_slice(&asset.to_le_bytes());
}

fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    match data.first() {
        Some(&2) => process_init(program_id, accounts),
        Some(&0) => process_single(program_id, accounts, data),
        Some(&3) => process_batch(program_id, accounts, data),
        Some(&4) => process_configure(program_id, accounts, data),
        Some(&5) => process_configure_backing_fee_cap(program_id, accounts, data),
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

    if !lp_owner.is_signer
        || !ctx.is_writable
        || ctx.owner != program_id
        || ctx.data_len() < CTX_MIN_LEN
    {
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
    ctx_data[CTX_DELEGATE_OFFSET..CTX_OWNER_OFFSET].copy_from_slice(delegate.key.as_ref());
    ctx_data[CTX_OWNER_OFFSET..CTX_BID_SPREAD_OFFSET].copy_from_slice(lp_owner.key.as_ref());
    ctx_data[CTX_BID_SPREAD_OFFSET..CTX_ASK_SPREAD_OFFSET].copy_from_slice(&0u64.to_le_bytes());
    ctx_data[CTX_ASK_SPREAD_OFFSET..CTX_BACKING_FEE_CAP_OFFSET]
        .copy_from_slice(&0u64.to_le_bytes());
    ctx_data[CTX_BACKING_FEE_CAP_OFFSET..CTX_MIN_LEN].copy_from_slice(&0u16.to_le_bytes());
    Ok(())
}

fn process_configure(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() != 17 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let account_iter = &mut accounts.iter();
    let lp_owner = next_account_info(account_iter)?;
    let ctx = next_account_info(account_iter)?;
    if !lp_owner.is_signer
        || !ctx.is_writable
        || ctx.owner != program_id
        || ctx.data_len() < CTX_MIN_LEN
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let bid_spread_bps = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let ask_spread_bps = u64::from_le_bytes(data[9..17].try_into().unwrap());
    if bid_spread_bps > 10_000 || ask_spread_bps > 10_000 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut ctx_data = ctx.try_borrow_mut_data()?;
    if ctx_data[CTX_STATE_OFFSET] != 1
        || ctx_data[CTX_OWNER_OFFSET..CTX_BID_SPREAD_OFFSET] != lp_owner.key.as_ref()[..]
    {
        return Err(ProgramError::InvalidAccountData);
    }
    ctx_data[CTX_BID_SPREAD_OFFSET..CTX_ASK_SPREAD_OFFSET]
        .copy_from_slice(&bid_spread_bps.to_le_bytes());
    ctx_data[CTX_ASK_SPREAD_OFFSET..CTX_BACKING_FEE_CAP_OFFSET]
        .copy_from_slice(&ask_spread_bps.to_le_bytes());
    Ok(())
}

fn process_configure_backing_fee_cap(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 3 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let account_iter = &mut accounts.iter();
    let lp_owner = next_account_info(account_iter)?;
    let ctx = next_account_info(account_iter)?;
    if !lp_owner.is_signer
        || !ctx.is_writable
        || ctx.owner != program_id
        || ctx.data_len() < CTX_MIN_LEN
    {
        return Err(ProgramError::InvalidAccountData);
    }
    let backing_fee_cap_bps = u16::from_le_bytes(data[1..3].try_into().unwrap());
    if backing_fee_cap_bps > 10_000 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut ctx_data = ctx.try_borrow_mut_data()?;
    if ctx_data[CTX_STATE_OFFSET] != 1
        || ctx_data[CTX_OWNER_OFFSET..CTX_BID_SPREAD_OFFSET] != lp_owner.key.as_ref()[..]
    {
        return Err(ProgramError::InvalidAccountData);
    }
    ctx_data[CTX_BACKING_FEE_CAP_OFFSET..CTX_MIN_LEN]
        .copy_from_slice(&backing_fee_cap_bps.to_le_bytes());
    Ok(())
}

fn check_ctx(program_id: &Pubkey, delegate: &AccountInfo, ctx: &AccountInfo) -> ProgramResult {
    if !delegate.is_signer || ctx.owner != program_id || ctx.data_len() < CTX_MIN_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    {
        let ctx_data = ctx.try_borrow_data()?;
        if ctx_data[CTX_STATE_OFFSET] != 1
            || ctx_data[CTX_DELEGATE_OFFSET..CTX_OWNER_OFFSET] != delegate.key.as_ref()[..]
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    Ok(())
}

fn quote_price(ctx: &AccountInfo, oracle: u64, request: i128) -> Result<u64, ProgramError> {
    let ctx_data = ctx.try_borrow_data()?;
    let bid_spread_bps = u64::from_le_bytes(
        ctx_data[CTX_BID_SPREAD_OFFSET..CTX_ASK_SPREAD_OFFSET]
            .try_into()
            .unwrap(),
    );
    let ask_spread_bps = u64::from_le_bytes(
        ctx_data[CTX_ASK_SPREAD_OFFSET..CTX_BACKING_FEE_CAP_OFFSET]
            .try_into()
            .unwrap(),
    );
    let multiplier = if request < 0 {
        10_000u64
            .checked_sub(bid_spread_bps)
            .ok_or(ProgramError::InvalidInstructionData)?
    } else {
        10_000u64
            .checked_add(ask_spread_bps)
            .ok_or(ProgramError::ArithmeticOverflow)?
    };
    oracle
        .checked_mul(multiplier)
        .and_then(|value| value.checked_div(10_000))
        .ok_or(ProgramError::ArithmeticOverflow)
}

fn backing_fee_cap_bps(ctx: &AccountInfo) -> Result<u16, ProgramError> {
    let data = ctx.try_borrow_data()?;
    Ok(u16::from_le_bytes(
        data[CTX_BACKING_FEE_CAP_OFFSET..CTX_MIN_LEN]
            .try_into()
            .unwrap(),
    ))
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
    let quote = quote_price(ctx, oracle, req)?;
    let backing_fee_cap_bps = backing_fee_cap_bps(ctx)?;
    let mut ctx_data = ctx.try_borrow_mut_data()?;
    write_return(
        &mut ctx_data[0..64],
        req_id,
        lp,
        asset,
        quote,
        oracle,
        req,
        backing_fee_cap_bps,
    );
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
    let backing_fee_cap_bps = backing_fee_cap_bps(ctx)?;
    let mut out = [0u8; 16 * 64];
    for i in 0..n {
        let base = 18 + i * 26;
        let asset = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as u64;
        let oracle = u64::from_le_bytes(data[base + 2..base + 10].try_into().unwrap());
        let req = i128::from_le_bytes(data[base + 10..base + 26].try_into().unwrap());
        let quote = quote_price(ctx, oracle, req)?;
        write_return(
            &mut out[i * 64..i * 64 + 64],
            req_id,
            lp,
            asset,
            quote,
            oracle,
            req,
            backing_fee_cap_bps,
        );
    }
    set_return_data(&out[..n * 64]);
    Ok(())
}
