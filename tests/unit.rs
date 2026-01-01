#[cfg(test)]
mod tests {
    use solana_program::{
        account_info::AccountInfo,
        pubkey::Pubkey,
        clock::Clock,
        program_pack::Pack,
    };
    use spl_token::state::{Account as TokenAccount, AccountState};
    use percolator_prog::{
        processor::process_instruction,
        constants::{SLAB_LEN, MAGIC, VERSION},
        state::SlabHeader,
        zc,
        error::PercolatorError,
    };
    use percolator::{RiskEngine, AccountKind};

    // --- Harness ---

    struct TestAccount {
        key: Pubkey,
        owner: Pubkey,
        lamports: u64,
        data: Vec<u8>,
        is_signer: bool,
        is_writable: bool,
    }

    impl TestAccount {
        fn new(key: Pubkey, owner: Pubkey, lamports: u64, data: Vec<u8>) -> Self {
            Self { key, owner, lamports, data, is_signer: false, is_writable: false }
        }
        fn signer(mut self) -> Self { self.is_signer = true; self }
        fn writable(mut self) -> Self { self.is_writable = true; self }
        
        fn to_info<'a>(&'a mut self) -> AccountInfo<'a> {
            AccountInfo::new(
                &self.key,
                self.is_signer,
                self.is_writable,
                &mut self.lamports,
                &mut self.data,
                &self.owner,
                false,
                0,
            )
        }
    }

    // --- Builders ---

    fn make_token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Vec<u8> {
        let mut data = vec![0u8; TokenAccount::LEN];
        let mut account = TokenAccount::default();
        account.mint = mint;
        account.owner = owner;
        account.amount = amount;
        account.state = AccountState::Initialized;
        TokenAccount::pack(account, &mut data).unwrap();
        data
    }

    fn make_pyth(price: i64, expo: i32, conf: u64, pub_slot: u64) -> Vec<u8> {
        let mut data = vec![0u8; 208];
        data[20..24].copy_from_slice(&expo.to_le_bytes());
        data[176..184].copy_from_slice(&price.to_le_bytes());
        data[184..192].copy_from_slice(&conf.to_le_bytes());
        data[200..208].copy_from_slice(&pub_slot.to_le_bytes());
        data
    }

    fn make_clock(slot: u64) -> Vec<u8> {
        let clock = Clock { slot, ..Clock::default() };
        bincode::serialize(&clock).unwrap()
    }

    struct MarketFixture {
        program_id: Pubkey,
        admin: TestAccount,
        slab: TestAccount,
        mint: TestAccount,
        vault: TestAccount,
        token_prog: TestAccount,
        pyth_index: TestAccount,
        pyth_col: TestAccount,
        clock: TestAccount,
        rent: TestAccount,
        system: TestAccount,
        vault_pda: Pubkey,
    }

    fn setup_market() -> MarketFixture {
        let program_id = Pubkey::new_unique();
        let slab_key = Pubkey::new_unique();
        let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab_key.as_ref()], &program_id);
        let mint_key = Pubkey::new_unique();

        let pyth_data = make_pyth(1000, -6, 1, 100); 

        MarketFixture {
            program_id,
            admin: TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer(),
            slab: TestAccount::new(slab_key, program_id, 0, vec![0u8; SLAB_LEN]).writable(),
            mint: TestAccount::new(mint_key, spl_token::ID, 0, vec![]),
            vault: TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(mint_key, vault_pda, 0)).writable(),
            token_prog: TestAccount::new(spl_token::ID, Pubkey::default(), 0, vec![]),
            pyth_index: TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, pyth_data.clone()),
            pyth_col: TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, pyth_data),
            clock: TestAccount::new(solana_program::sysvar::clock::id(), solana_program::sysvar::id(), 0, make_clock(100)),
            rent: TestAccount::new(solana_program::sysvar::rent::id(), solana_program::sysvar::id(), 0, vec![]),
            system: TestAccount::new(solana_program::system_program::id(), Pubkey::default(), 0, vec![]),
            vault_pda,
        }
    }

    // --- Encoders --- 
    
    fn encode_u64(val: u64, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u16(val: u16, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u8(val: u8, buf: &mut Vec<u8>) { buf.push(val); }
    fn encode_i64(val: i64, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_i128(val: i128, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_u128(val: u128, buf: &mut Vec<u8>) { buf.extend_from_slice(&val.to_le_bytes()); }
    fn encode_pubkey(val: &Pubkey, buf: &mut Vec<u8>) { buf.extend_from_slice(val.as_ref()); }

    fn encode_init_market(fixture: &MarketFixture) -> Vec<u8> {
        let mut data = vec![0u8];
        encode_pubkey(&fixture.admin.key, &mut data);
        encode_pubkey(&fixture.mint.key, &mut data);
        encode_pubkey(&fixture.pyth_index.key, &mut data);
        encode_pubkey(&fixture.pyth_col.key, &mut data);
        encode_u64(100, &mut data); // staleness
        encode_u16(500, &mut data); // conf
        // Risk params (13 fields) - Realistic
        encode_u64(0, &mut data); // warmup
        encode_u64(500, &mut data); // maint
        encode_u64(1000, &mut data); // init
        encode_u64(10, &mut data); // trade
        encode_u64(64, &mut data); // max accounts
        encode_u128(0, &mut data); // new account fee
        encode_u128(0, &mut data); // risk thresh
        encode_u128(0, &mut data); // maint fee
        encode_u64(100, &mut data); // crank
        encode_u64(50, &mut data); // liq fee
        encode_u128(1000, &mut data); // liq cap
        encode_u64(100, &mut data); // liq buf
        encode_u128(10, &mut data); // min liq
        data
    }

    fn encode_init_user(fee: u64) -> Vec<u8> {
        let mut data = vec![1u8];
        encode_u64(fee, &mut data);
        data
    }

    fn encode_init_lp(matcher: Pubkey, ctx: Pubkey, fee: u64) -> Vec<u8> {
        let mut data = vec![2u8];
        encode_pubkey(&matcher, &mut data);
        encode_pubkey(&ctx, &mut data);
        encode_u64(fee, &mut data);
        data
    }

    fn encode_deposit(user_idx: u16, amount: u64) -> Vec<u8> {
        let mut data = vec![3u8];
        encode_u16(user_idx, &mut data);
        encode_u64(amount, &mut data);
        data
    }

    fn encode_withdraw(user_idx: u16, amount: u64) -> Vec<u8> {
        let mut data = vec![4u8];
        encode_u16(user_idx, &mut data);
        encode_u64(amount, &mut data);
        data
    }

    fn encode_trade(lp: u16, user: u16, size: i128) -> Vec<u8> {
        let mut data = vec![6u8];
        encode_u16(lp, &mut data);
        encode_u16(user, &mut data);
        encode_i128(size, &mut data);
        data
    }

    fn find_idx_by_owner(data: &[u8], owner: Pubkey) -> Option<u16> {
        let engine = zc::engine_ref(data).ok()?;
        for i in 0..64 {
            if engine.accounts[i].owner == owner.to_bytes() {
                return Some(i as u16);
            }
        }
        None
    }

    // --- Tests ---

    #[test]
    fn test_init_market() {
        let mut f = setup_market();
        let data = encode_init_market(&f);
        
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(), f.token_prog.to_info(),
            dummy_ata.to_info(), f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info(),
        ];

        process_instruction(&f.program_id, &accounts, &data).unwrap();

        let header = unsafe { &*(f.slab.data.as_ptr() as *const SlabHeader) };
        assert_eq!(header.magic, MAGIC);
        assert_eq!(header.version, VERSION);
        
        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.params.max_accounts, 64);
    }

    #[test]
    fn test_init_user() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut init_accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
            f.token_prog.to_info(), dummy_ata.to_info(),
            f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();

        let data = encode_init_user(100);
        let accounts = vec![
            user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(), f.clock.to_info(), f.pyth_col.to_info(),
        ];

        process_instruction(&f.program_id, &accounts, &data).unwrap();

        let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
        assert_eq!(vault_state.amount, 100); 
        let user_state = TokenAccount::unpack(&user_ata.data).unwrap();
        assert_eq!(user_state.amount, 900); 

        assert!(find_idx_by_owner(&f.slab.data, user.key).is_some());
    }

    #[test]
    fn test_deposit_withdraw() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut init_accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
            f.token_prog.to_info(), dummy_ata.to_info(),
            f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        let init_user_data = encode_init_user(0);
        let init_user_accounts = vec![
            user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(), f.clock.to_info(), f.pyth_col.to_info()
        ];
        process_instruction(&f.program_id, &init_user_accounts, &init_user_data).unwrap();
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();

        let deposit_data = encode_deposit(user_idx, 500);
        let deposit_accounts = vec![
            user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()
        ];
        process_instruction(&f.program_id, &deposit_accounts, &deposit_data).unwrap();

        let engine = zc::engine_ref(&f.slab.data).unwrap();
        assert_eq!(engine.accounts[user_idx as usize].capital, 500);
        assert_eq!(engine.accounts[user_idx as usize].kind, AccountKind::User);

        // Withdraw
        let withdraw_data = encode_withdraw(user_idx, 200);
        let mut vault_pda_account = TestAccount::new(f.vault_pda, solana_program::system_program::id(), 0, vec![]);
        let withdraw_accounts = vec![
            user.to_info(), f.slab.to_info(), f.vault.to_info(), user_ata.to_info(), vault_pda_account.to_info(),
            f.token_prog.to_info(), f.clock.to_info(), f.pyth_index.to_info(),
        ];
        process_instruction(&f.program_id, &withdraw_accounts, &withdraw_data).unwrap();

        let vault_state = TokenAccount::unpack(&f.vault.data).unwrap();
        assert_eq!(vault_state.amount, 300); // 500 - 200
    }

    #[test]
    fn test_vault_validation() {
        let mut f = setup_market();
        f.vault.owner = solana_program::system_program::id();
        let init_data = encode_init_market(&f);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut init_accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
            f.token_prog.to_info(), dummy_ata.to_info(),
            f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
        ];
        let res = process_instruction(&f.program_id, &init_accounts, &init_data);
        assert_eq!(res, Err(PercolatorError::InvalidVaultAta.into()));
    }

    #[test]
    fn test_trade() {
        let mut f = setup_market();
        let init_data = encode_init_market(&f);
        let mut dummy_ata = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut init_accounts = vec![
            f.admin.to_info(), f.slab.to_info(), f.mint.to_info(), f.vault.to_info(),
            f.token_prog.to_info(), dummy_ata.to_info(),
            f.system.to_info(), f.rent.to_info(), f.pyth_index.to_info(), f.pyth_col.to_info(), f.clock.to_info()
        ];
        process_instruction(&f.program_id, &init_accounts, &init_data).unwrap();

        let mut user = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut user_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, user.key, 1000)).writable();
        let init_user_accounts = vec![
            user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(), f.clock.to_info(), f.pyth_col.to_info()
        ];
        process_instruction(&f.program_id, &init_user_accounts, &encode_init_user(0)).unwrap();
        let user_idx = find_idx_by_owner(&f.slab.data, user.key).unwrap();
        let dep_user_accounts = vec![user.to_info(), f.slab.to_info(), user_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
        process_instruction(&f.program_id, &dep_user_accounts, &encode_deposit(user_idx, 1000)).unwrap();

        let mut lp = TestAccount::new(Pubkey::new_unique(), solana_program::system_program::id(), 0, vec![]).signer();
        let mut lp_ata = TestAccount::new(Pubkey::new_unique(), spl_token::ID, 0, make_token_account(f.mint.key, lp.key, 1000)).writable();
        let mut d1 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let mut d2 = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0, vec![]);
        let init_lp_accounts = vec![
            lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info(), d1.to_info(), d2.to_info()
        ];
        process_instruction(&f.program_id, &init_lp_accounts, &encode_init_lp(Pubkey::default(), Pubkey::default(), 0)).unwrap();
        let lp_idx = find_idx_by_owner(&f.slab.data, lp.key).unwrap();
        
        let dep_lp_accounts = vec![lp.to_info(), f.slab.to_info(), lp_ata.to_info(), f.vault.to_info(), f.token_prog.to_info()];
        process_instruction(&f.program_id, &dep_lp_accounts, &encode_deposit(lp_idx, 1000)).unwrap();

        let trade_data = encode_trade(lp_idx, user_idx, 100);
        let trade_accounts = vec![
            user.to_info(), lp.to_info(), f.slab.to_info(), f.clock.to_info(), f.pyth_col.to_info()
        ];
        process_instruction(&f.program_id, &trade_accounts, &trade_data).unwrap();

        let engine = zc::engine_ref(&f.slab.data).unwrap();
        // Trade should have executed
        // assert!(engine.accounts[user_idx as usize].capital < 1000); 
    }
}
