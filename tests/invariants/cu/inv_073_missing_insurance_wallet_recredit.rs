//! INV-073 / row421: an insurance claim outlives both its wallet and token account.
//! Public loss settlement consumes the insurance after its roles disappear. Following
//! backing expiry, a keeper recreates custody and recredits/pays the claim unsigned.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const LIMIT: u64 = 300_000;

#[test]
fn v16_program_recredited_insurance_reaches_terminal_exit_without_wallets_or_signatures() {
    absent_reserve_progress(ProviderHistory::FreshMissingWallets, None);
}

pub(super) fn finish(
    env: &mut V16CuEnv,
    admin: &Keypair,
    asset: usize,
    absent: [Pubkey; 3],
    destination: Pubkey,
    entitlement: u64,
    close: &Instruction,
    tracked: &[Pubkey],
) -> u64 {
    let before = env.market_state();
    let domain = 2 * asset;
    let vault_frame = env.svm.get_account(&env.vault).unwrap();
    let mint_frame = env.svm.get_account(&env.mint).unwrap();
    let absent_frames = absent.map(|key| env.svm.get_account(&key));
    let missing_destination = env.svm.get_account(&destination);
    let profiles = [0, 1].map(|index| {
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, index)
            .unwrap()
    });
    let sequences = [0, 1].map(|index| env.control_sequences(index));
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let burned = u64::try_from(before.1.vault).unwrap() - entitlement;
    assert_eq!(before.1.insurance_domain_spent[domain], 100);
    assert_eq!(before.1.insurance, u128::from(entitlement - 100));
    assert_eq!(before.1.materialized_portfolio_count, 0);
    assert_eq!(
        before.1.source_backing_buckets[domain].status,
        BackingBucketStatusV16::Expired
    );
    assert!(entitlement > 37 && burned > 0);
    assert!(!absent.contains(&env.payer.pubkey()));
    assert!(!absent.contains(&admin.pubkey()));
    assert_ne!(env.payer.pubkey(), admin.pubkey());

    let land = |env: &mut V16CuEnv,
                instructions: &[Instruction],
                signed_close: bool,
                changed: &[Pubkey],
                creation_rent: u64,
                rejection: Option<(u8, PercolatorError)>| {
        env.svm.expire_blockhash();
        let mut batch = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        ];
        batch.extend_from_slice(instructions);
        let mut signers = vec![&env.payer];
        if signed_close {
            signers.push(admin);
        }
        let tx = Transaction::new_signed_with_payer(
            &batch,
            Some(&env.payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        let required = usize::from(tx.message.header.num_required_signatures);
        assert_eq!(required, 1 + usize::from(signed_close));
        assert!(absent
            .iter()
            .all(|key| !tx.message.account_keys[..required].contains(key)));
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let mut keys = tx.message.account_keys.clone();
        keys.extend_from_slice(tracked);
        keys.sort_unstable();
        keys.dedup();
        let frames: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let rejected = rejection.is_some();
        let result = env.svm.send_transaction(tx);
        let meta = if let Some((index, error)) = rejection {
            let failure = result.expect_err("unpaid restored insurance still protects close");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
            );
            for program in [associated_token_program_id(), env.program_id] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    instructions[..usize::from(index) - 2]
                        .iter()
                        .filter(|ix| ix.program_id == program)
                        .count(),
                    "custody creation and actual recredit/payout precede the rejected close",
                );
            }
            failure.meta
        } else {
            result.expect("bounded public insurance progress with no insurance wallet accounts")
        };
        for (key, mut frame) in keys.into_iter().zip(frames) {
            if key == env.payer.pubkey() {
                frame.as_mut().unwrap().lamports -= required as u64
                    * FeeStructure::default().lamports_per_signature
                    + if rejected { 0 } else { creation_rent };
            }
            if rejected || !changed.contains(&key) {
                assert_eq!(
                    env.svm.get_account(&key),
                    frame,
                    "complete Account frame {key}"
                );
            }
        }
        assert_eq!(absent.map(|key| env.svm.get_account(&key)), absent_frames);
        assert_cu_within(
            "INV-073 absent-wallet recredit",
            meta.compute_units_consumed,
            LIMIT,
        );
        meta.compute_units_consumed
    };
    let repair = Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let withdrawal = |amount: u64| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: env.control_sequences(asset).authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    };
    let payments = [withdrawal(37), withdrawal(entitlement - 37)];
    assert!(payments
        .iter()
        .flat_map(|ix| &ix.accounts)
        .all(|meta| !meta.is_signer));
    let mut peak = land(
        env,
        &[repair.clone(), payments[0].clone(), close.clone()],
        true,
        &[],
        0,
        Some((4, PercolatorError::EngineLockActive)),
    );
    assert_eq!(env.market_state(), before);
    assert_eq!(env.svm.get_account(&destination), missing_destination);

    let changed = [env.market, env.vault, destination];
    let mut paid = 0;
    for (index, amount) in [37, entitlement - 37].into_iter().enumerate() {
        let instructions = if index == 0 {
            vec![repair.clone(), payments[index].clone()]
        } else {
            vec![payments[index].clone()]
        };
        peak = peak.max(land(
            env,
            &instructions,
            false,
            &changed,
            if index == 0 { rent } else { 0 },
            None,
        ));
        let rank_before = entitlement - paid;
        paid += amount;
        let remaining = entitlement - paid;
        assert!(remaining < rank_before);
        let mut expected = before.clone();
        expected.1.vault -= u128::from(paid);
        expected.1.insurance = remaining.into();
        expected.1.insurance_domain_budget[domain] = remaining.into();
        expected.1.insurance_domain_spent[domain] = 0;
        expected.1.insurance_domain_budget_remaining_total = remaining.into();
        assert_eq!(
            env.market_state(),
            expected,
            "exact implicit recredit and claim debit"
        );
        let mut expected_vault = vault_frame.clone();
        let mut vault = TokenAccount::unpack(&expected_vault.data).unwrap();
        vault.amount -= paid;
        TokenAccount::pack(vault, &mut expected_vault.data).unwrap();
        assert_eq!(env.svm.get_account(&env.vault), Some(expected_vault));
        let custody = env.svm.get_account(&destination).unwrap();
        assert_eq!(custody.lamports, rent);
        assert_eq!(custody.owner, spl_token::ID);
        assert_eq!(custody.data.len(), TokenAccount::LEN);
        assert!(!custody.executable);
        assert_eq!(
            TokenAccount::unpack(&custody.data).unwrap(),
            TokenAccount {
                mint: env.mint,
                owner: absent[0],
                amount: paid,
                delegate: COption::None,
                state: spl_token::state::AccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 0,
                close_authority: COption::None,
            }
        );
        let market = env.svm.get_account(&env.market).unwrap();
        for index in [0, 1] {
            assert_eq!(
                state::read_asset_oracle_profile(&market.data, index).unwrap(),
                profiles[index]
            );
            assert_eq!(env.control_sequences(index), sequences[index]);
        }
        assert_market_stock_census(
            "absent-wallet recredit",
            &expected.1,
            &market.data,
            &[],
            env.token_amount(env.vault).into(),
        )
        .unwrap();
        assert_reservation_encumbrance_census("absent-wallet recredit", &expected.1, &[]).unwrap();
        state::market_view_mut(&mut market.data.clone())
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
        assert_eq!(env.svm.get_account(&env.mint), Some(mint_frame.clone()));
    }
    let market_rent = env.svm.get_account(&env.market).unwrap().lamports;
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    expected_admin.lamports += market_rent + vault_frame.lamports - tombstone_rent;
    let changed = [env.market, env.vault, env.mint, admin.pubkey()];
    peak = peak.max(land(env, &[close.clone()], true, &changed, 0, None));
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert!(env
        .svm
        .get_account(&env.vault)
        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
    let mut expected_mint = mint_frame;
    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
    mint.supply -= burned;
    Mint::pack(mint, &mut expected_mint.data).unwrap();
    assert_eq!(env.svm.get_account(&env.mint), Some(expected_mint));
    assert_eq!(env.token_amount(destination), entitlement);
    eprintln!("row421 absent-wallet suffix: asset={asset}, insurance={entitlement}, keeper_payments=2, slab_calls=1, exact_rollbacks=1, peak={peak} CU");
    peak
}
