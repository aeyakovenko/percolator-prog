//! INV-025/024: reserve replacement is explicit custody surplus, not new claims.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::{
    inv018_create_public_spl_mint, inv018_public_spl_market,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[test]
fn v16_program_active_reserve_swap_preserves_stock_classes_and_owner_claim() {
    const CAPITAL: u64 = 101;
    const INSURANCE: u64 = 43;
    const BACKING: [u64; 2] = [17, 29];
    const SWAP: u64 = 37;
    const SECONDARY: u64 = 211;
    const ADMIN_FUNDS: u64 = INSURANCE + BACKING[0] + BACKING[1] + SWAP;
    let mut env = inv018_public_spl_market(6);
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
    let secondary = inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 6);
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    let mints = [env.mint, secondary];
    let vaults = [
        env.vault,
        create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, secondary),
    ];
    let wallets = [owner.pubkey(), admin.pubkey()];
    let tokens = wallets.map(|wallet| {
        mints.map(|mint| create_ata_for_test(&mut env.svm, &env.payer, wallet, mint))
    });
    let portfolio_key = Keypair::new();
    let portfolio = portfolio_key.pubkey();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &portfolio_key,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .unwrap();
    env.portfolios.push(portfolio);
    let mut funding: Vec<_> = [
        (mints[0], tokens[0][0], CAPITAL),
        (mints[0], tokens[1][0], ADMIN_FUNDS),
        (mints[1], vaults[1], SECONDARY),
    ]
    .map(|(mint, token, amount)| {
        spl_token::instruction::mint_to(&spl_token::ID, &mint, &token, &admin.pubkey(), &[], amount)
            .unwrap()
    })
    .into();
    funding.extend(mints.map(|mint| {
        spl_token::instruction::set_authority(
            &spl_token::ID,
            &mint,
            None,
            spl_token::instruction::AuthorityType::MintTokens,
            &admin.pubkey(),
            &[],
        )
        .unwrap()
    }));
    send_raw_ixs(&mut env.svm, &env.payer, funding, &[&admin]).unwrap();

    struct Stocks {
        capital: u64,
        insurance: u64,
        backing: [u64; 2],
        wallets: [[u64; 2]; 2],
        swapped: u64,
        secondary_paid: u64,
    }
    let mut expected = Stocks {
        capital: 0,
        insurance: 0,
        backing: [0; 2],
        wallets: [[CAPITAL, 0], [ADMIN_FUNDS, 0]],
        swapped: 0,
        secondary_paid: 0,
    };
    let census = |env: &V16CuEnv, e: &Stocks| {
        let market = env.svm.get_account(&env.market).unwrap();
        let header = market_group_header_bytes(&market.data);
        let account = env.portfolio_state(portfolio);
        assert_eq!(account.capital.get(), u128::from(e.capital));
        assert_eq!(account.pnl.get(), 0);
        assert_eq!(account.cancel_deposit_escrow.get(), 0);
        assert_eq!(header.materialized_portfolio_count.get(), 1);
        let mut insurance = 0;
        let mut fresh = 0;
        for asset in 0..state::market_slot_capacity(&market.data).unwrap() {
            let slot = bytemuck::pod_read_unaligned::<percolator::EngineAssetSlotV16Account>(
                market_engine_slot_bytes(&market.data, asset),
            );
            let budget =
                slot.insurance_domain_budget_long.get() + slot.insurance_domain_budget_short.get();
            assert_eq!(budget, if asset == 0 { e.insurance.into() } else { 0 });
            assert_eq!(slot.insurance_domain_spent_long.get(), 0);
            assert_eq!(slot.insurance_domain_spent_short.get(), 0);
            insurance += budget;
            for (side, (bucket, source)) in [
                (slot.backing_long, slot.source_credit_long),
                (slot.backing_short, slot.source_credit_short),
            ]
            .into_iter()
            .enumerate()
            {
                let principal = if asset == 0 { e.backing[side] } else { 0 };
                let num = u128::from(principal) * BOUND_SCALE;
                assert_eq!(bucket.fresh_unliened_backing_num.get(), num);
                assert_eq!(source.fresh_reserved_backing_num.get(), num);
                for zero in [
                    bucket.valid_liened_backing_num.get(),
                    bucket.impaired_liened_backing_num.get(),
                    bucket.consumed_liened_backing_num.get(),
                    bucket.utilization_fee_earnings.get(),
                    source.positive_claim_bound_num.get(),
                    source.exact_positive_claim_num.get(),
                    source.spent_backing_num.get(),
                    source.provider_receivable_num.get(),
                    source.insurance_credit_reserved_num.get(),
                ] {
                    assert_eq!(
                        zero, 0,
                        "no hidden stock or claim in asset {asset} side {side}"
                    );
                }
                fresh += bucket.fresh_unliened_backing_num.get();
            }
        }
        assert_eq!(header.c_tot.get(), account.capital.get());
        assert_eq!(header.insurance.get(), insurance);
        assert_eq!(
            header.insurance_domain_budget_remaining_total.get(),
            insurance
        );
        assert_eq!(header.source_fresh_backing_total_num.get(), fresh);
        assert_eq!(header.backing_provider_earnings_total.get(), 0);
        assert_eq!(header.source_claim_bound_total_num.get(), 0);
        assert_eq!(header.source_insurance_credit_reserved_total_atoms.get(), 0);
        assert_eq!(header.pnl_pos_tot.get(), 0);
        assert_eq!(fresh % BOUND_SCALE, 0);
        let stock = account.capital.get() + insurance + fresh / BOUND_SCALE;
        assert_eq!(header.vault.get(), stock, "raw census, no rounding residue");
        // Both reserve replacement and secondary payouts leave primary atoms that
        // belong to explicit protocol surplus, never to a second engine claim.
        let surplus = e.swapped + e.secondary_paid;
        let balances = [stock as u64 + surplus, SECONDARY - surplus];
        for rail in 0..2 {
            let mint_account = env.svm.get_account(&mints[rail]).unwrap();
            assert_eq!(mint_account.owner, spl_token::ID);
            let mint = Mint::unpack(&mint_account.data).unwrap();
            assert_eq!(mint.supply, [CAPITAL + ADMIN_FUNDS, SECONDARY][rail]);
            assert_eq!(mint.mint_authority, COption::None);
            let mut total = 0;
            for (key, wallet, amount) in [
                (vaults[rail], env.vault_authority, balances[rail]),
                (tokens[0][rail], wallets[0], e.wallets[0][rail]),
                (tokens[1][rail], wallets[1], e.wallets[1][rail]),
            ] {
                let raw = env.svm.get_account(&key).unwrap();
                assert_eq!(raw.owner, spl_token::ID);
                let token = TokenAccount::unpack(&raw.data).unwrap();
                assert_eq!(
                    (token.mint, token.owner, token.amount),
                    (mints[rail], wallet, amount)
                );
                assert_eq!(token.state, AccountState::Initialized);
                assert_eq!(token.delegate, COption::None);
                assert_eq!(token.close_authority, COption::None);
                total += token.amount;
            }
            assert_eq!(total, mint.supply, "exact mint-{rail} custody census");
        }
        assert_eq!(e.capital + e.wallets[0].iter().sum::<u64>(), CAPITAL);
        assert_eq!(
            e.insurance + e.backing.iter().sum::<u64>() + e.wallets[1].iter().sum::<u64>(),
            ADMIN_FUNDS,
            "reserve authority exchanges equal atoms and retains only its own principal"
        );
        assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
    };
    let mut steps = 0;
    let mut peak_cu = 0;
    macro_rules! step {
        ($action:expr) => {{
            let cu = $action;
            assert_cu_within("active reserve stock transition", cu, CUSTODY_CU_LIMIT);
            peak_cu = peak_cu.max(cu);
            steps += 1;
            census(&env, &expected);
        }};
    }
    census(&env, &expected);
    expected.capital = CAPITAL;
    expected.wallets[0][0] = 0;
    step!(env
        .send(
            env.deposit_ix(portfolio, CAPITAL.into()),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(tokens[0][0], false),
                AccountMeta::new(vaults[0], false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .unwrap());
    expected.insurance = INSURANCE;
    expected.wallets[1][0] -= INSURANCE;
    step!(env.top_up_insurance_from_admin_token_with_cu(tokens[1][0], INSURANCE.into()));
    for (side, amount) in BACKING.into_iter().enumerate() {
        expected.backing[side] = amount;
        expected.wallets[1][0] -= amount;
        step!(env.top_up_backing_bucket_from_admin_token_with_cu(
            tokens[1][0],
            side as u16,
            amount.into(),
            100,
        ));
    }
    let swap = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(tokens[1][0], false),
            AccountMeta::new(vaults[0], false),
            AccountMeta::new(tokens[1][1], false),
            AccountMeta::new(vaults[1], false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::SwapSecondaryForPrimary {
            amount: SWAP.into(),
            authority_epoch: 0,
        }
        .encode(),
    };
    let withdraw = |env: &V16CuEnv, rail: usize, amount: u64| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(tokens[0][rail], false),
            AccountMeta::new(vaults[rail], false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(portfolio, amount.into()).encode(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            swap.clone(),
            withdraw(&env, 0, CAPITAL + 1),
        ],
        Some(&env.payer.pubkey()),
        &[&env.payer, &admin, &owner],
        env.svm.latest_blockhash(),
    );
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let mut keys = tx.message.account_keys.clone();
    keys.extend(mints);
    keys.extend(tokens.into_iter().flatten());
    keys.sort_unstable();
    keys.dedup();
    let frame: Vec<_> = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect();
    let failure = env
        .svm
        .send_transaction(tx)
        .expect_err("surplus cannot increase owner capital");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            3,
            InstructionError::Custom(PercolatorError::EngineLockActive as u32)
        )
    );
    assert!(failure
        .meta
        .logs
        .contains(&format!("Program {} success", env.program_id)));
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| **line == format!("Program {} success", spl_token::ID))
            .count(),
        2,
        "both swap token transfers executed before the owner overclaim rejected"
    );
    for (key, mut before) in frame {
        if key == env.payer.pubkey() {
            before.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            env.svm.get_account(&key),
            before,
            "complete rejected frame: {key}"
        );
    }
    step!(failure.meta.compute_units_consumed);

    let market_before = env.svm.get_account(&env.market);
    let portfolio_before = env.svm.get_account(&portfolio);
    expected.swapped = SWAP;
    expected.wallets[1] = [0, SWAP];
    step!(send_raw_tx(&mut env.svm, &env.payer, swap, &[&admin]).unwrap());
    assert_eq!(
        env.svm.get_account(&env.market),
        market_before,
        "swap never creates engine stock"
    );
    assert_eq!(env.svm.get_account(&portfolio), portfolio_before);
    for (rail, amount) in [(1, 61), (0, 40)] {
        let ix = withdraw(&env, rail, amount);
        expected.capital -= amount;
        expected.wallets[0][rail] += amount;
        if rail == 1 {
            expected.secondary_paid += amount;
        }
        step!(send_raw_tx(&mut env.svm, &env.payer, ix, &[&owner]).unwrap());
    }
    for side in [1, 0] {
        expected.backing[side] = 0;
        expected.wallets[1][0] += BACKING[side];
        step!(env.withdraw_backing_bucket_to_admin_token_with_cu(
            tokens[1][0],
            side as u16,
            BACKING[side].into(),
        ));
    }
    expected.insurance = 0;
    expected.wallets[1][0] += INSURANCE;
    step!(env.withdraw_insurance_domain_to_admin_token_with_cu(tokens[1][0], 0, INSURANCE.into()));
    assert_eq!(env.market_state().1.vault, 0);
    assert_eq!(vaults.map(|key| env.token_amount(key)), [98, 113]);
    println!("active reserve stock history: {steps} checked transitions, peak {peak_cu} CU; explicit surplus 98 primary + 113 secondary atoms");
}
