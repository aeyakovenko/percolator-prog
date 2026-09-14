//! INV-073 / row421: recovered insurance has one claim across both quote rails.
//! Public loss settlement spends 100 primary atoms after both insurance keys are
//! dropped. Backing expiry exposes a 100-atom recovery; unsigned payouts switch
//! rails without counting donated secondary liquidity as another entitlement.
//! Recredit/payment rollback and paid-prefix rollback precede exact dual-vault
//! retirement. This finite history requires portfolio owners and the close admin.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const SECONDARY_STOCK: u64 = 211;
const FIRST: u64 = 37;
const BURN: u64 = 207;
const LIMIT: u64 = 300_000;

#[test]
fn v16_program_recredited_insurance_switches_quote_rails_without_operator_signatures() {
    for secondary_first in [false, true] {
        absent_reserve_progress(ProviderHistory::FreshDualQuote(secondary_first), None);
    }
}

pub(super) struct Secondary {
    mint: Pubkey,
    pub(super) vault: Pubkey,
    recipient: Pubkey,
    pub(super) admin_token: Pubkey,
}

impl Secondary {
    pub(super) fn keys(&self) -> [Pubkey; 4] {
        [self.mint, self.vault, self.recipient, self.admin_token]
    }

    pub(super) fn create(env: &mut V16CuEnv, admin: &Keypair, beneficiary: Pubkey) -> Self {
        use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;

        let mint = inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 0);
        let vault = create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, mint);
        let recipient = create_ata_for_test(&mut env.svm, &env.payer, beneficiary, mint);
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint);
        env.send(
            ProgInstruction::UpdateBaseUnitMints {
                primary_mint: env.mint.to_bytes(),
                secondary_mint: mint.to_bytes(),
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(env.mint, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(env.vault, false),
            ],
            &[admin],
        )
        .unwrap();
        let market = env.svm.get_account(&env.market);
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &mint,
                    &vault,
                    &admin.pubkey(),
                    &[],
                    SECONDARY_STOCK,
                )
                .unwrap(),
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
            ],
            &[admin],
        )
        .unwrap();
        assert_eq!(env.svm.get_account(&env.market), market);
        assert_eq!(env.market_state().1.insurance, 0);
        Self {
            mint,
            vault,
            recipient,
            admin_token,
        }
    }
}

fn token_image(frame: &Account, amount: u64) -> Account {
    // Expected image only; never installed into LiteSVM.
    let mut expected = frame.clone();
    let mut token = TokenAccount::unpack(&frame.data).unwrap();
    assert_eq!(token.is_native, COption::None);
    assert_eq!(token.state, AccountState::Initialized);
    assert_eq!(
        (token.delegate, token.close_authority),
        (COption::None, COption::None)
    );
    token.amount = amount;
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

pub(super) fn finish(
    env: &mut V16CuEnv,
    admin: &Keypair,
    asset: usize,
    absent: [Pubkey; 3],
    primary_recipient: Pubkey,
    secondary: &Secondary,
    entitlement: u64,
    secondary_first: bool,
    close: &Instruction,
    tracked: &[Pubkey],
) -> u64 {
    let domain = 2 * asset;
    let initial = env.market_state();
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let vaults = [env.vault, secondary.vault];
    let recipients = [primary_recipient, secondary.recipient];
    let admin_tokens = [close.accounts[4].pubkey, secondary.admin_token];
    let mints = [env.mint, secondary.mint];
    let vault_frames = vaults.map(|key| env.svm.get_account(&key).unwrap());
    let recipient_frames = recipients.map(|key| env.svm.get_account(&key).unwrap());
    let admin_token_frames = admin_tokens.map(|key| env.svm.get_account(&key).unwrap());
    let mint_frames = mints.map(|key| env.svm.get_account(&key).unwrap());
    let profiles =
        [0, 1].map(|index| state::read_asset_oracle_profile(&market_frame.data, index).unwrap());
    let sequences = [0, 1].map(|index| env.control_sequences(index));
    let absent_frames = absent.map(|key| env.svm.get_account(&key));
    assert!(matches!(entitlement, 100 | 101));
    assert!(!absent.contains(&env.payer.pubkey()));
    assert_ne!(env.payer.pubkey(), admin.pubkey());
    assert_eq!(initial.1.insurance_domain_spent[domain], 100);
    assert_eq!(initial.1.insurance, u128::from(entitlement - 100));
    assert_eq!(
        initial.1.insurance_domain_budget[domain],
        entitlement.into()
    );
    assert_eq!(
        initial.1.insurance_domain_budget_remaining_total,
        u128::from(entitlement - 100)
    );
    assert_eq!(initial.1.materialized_portfolio_count, 0);
    assert_eq!((initial.1.c_tot, initial.1.pnl_pos_tot), (0, 0));
    assert_eq!(initial.1.vault, u128::from(BURN + entitlement));
    assert_eq!(env.token_amount(vaults[0]), BURN + entitlement);
    assert_eq!(env.token_amount(vaults[1]), SECONDARY_STOCK);
    assert_eq!(
        initial.1.source_backing_buckets[domain].status,
        BackingBucketStatusV16::Expired
    );
    for (index, mint) in mint_frames.iter().enumerate() {
        let mint = Mint::unpack(&mint.data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(
            mint.supply,
            if index == 0 {
                1_237 + entitlement + 307
            } else {
                SECONDARY_STOCK
            }
        );
    }
    for (index, frame) in recipient_frames.iter().enumerate() {
        let token = TokenAccount::unpack(&frame.data).unwrap();
        assert_eq!(
            (token.owner, token.mint, token.amount),
            (absent[0], mints[index], 0)
        );
    }

    let land = |env: &mut V16CuEnv,
                instructions: &[Instruction],
                signed_close: bool,
                changed: &[Pubkey],
                rejection: Option<u8>| {
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
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        let required = usize::from(tx.message.header.num_required_signatures);
        assert_eq!(required, 1 + usize::from(signed_close));
        assert!(absent
            .iter()
            .all(|key| !tx.message.account_keys[..required].contains(key)));
        let mut keys = tx.message.account_keys.clone();
        keys.extend_from_slice(tracked);
        keys.sort_unstable();
        keys.dedup();
        let frames: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let result = env.svm.send_transaction(tx);
        let meta = if let Some(index) = rejection {
            let failure =
                result.expect_err("shared recovered claim still limits both liquid rails");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    index,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                )
            );
            for program in [env.program_id, spl_token::ID] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    usize::from(index - 2),
                    "real recredit/payout prefix must succeed before rejection"
                );
            }
            failure.meta
        } else {
            result.expect("unsigned recovered insurance continuation")
        };
        for (key, mut frame) in keys.into_iter().zip(frames) {
            if key == env.payer.pubkey() {
                frame.as_mut().unwrap().lamports -=
                    required as u64 * FeeStructure::default().lamports_per_signature;
            }
            if rejection.is_some() || !changed.contains(&key) {
                assert_eq!(
                    env.svm.get_account(&key),
                    frame,
                    "complete Account frame {key}"
                );
            }
        }
        assert_eq!(absent.map(|key| env.svm.get_account(&key)), absent_frames);
        assert_cu_within(
            "row421 recredited quote rails",
            meta.compute_units_consumed,
            LIMIT,
        );
        meta.compute_units_consumed
    };
    let program_id = env.program_id;
    let market_key = env.market;
    let market_id = env.asset_market_id(asset as u16);
    let vault_authority = env.vault_authority;
    let withdrawal_at = |rail: usize, amount: u64, authority_epoch: u64| Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new(market_key, false),
            AccountMeta::new(recipients[rail], false),
            AccountMeta::new(vaults[rail], false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id,
            authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    };
    let withdrawal = |env: &V16CuEnv, rail: usize, amount: u64| {
        withdrawal_at(rail, amount, env.control_sequences(asset).authority_epoch)
    };
    let first_rail = usize::from(secondary_first);
    let last_rail = 1 - first_rail;
    for ix in [
        withdrawal(env, first_rail, FIRST),
        withdrawal(env, last_rail, entitlement - FIRST),
    ] {
        assert!(ix.accounts.iter().all(|meta| !meta.is_signer));
        assert!(!ix
            .accounts
            .iter()
            .any(|meta| meta.pubkey == absent[1] || meta.pubkey == admin.pubkey()));
    }

    let check = |env: &V16CuEnv, paid: [u64; 2]| {
        let total = paid.iter().sum::<u64>();
        let remaining = entitlement - total;
        let mut expected = initial.clone();
        expected.1.vault -= u128::from(total);
        expected.1.insurance = remaining.into();
        expected.1.insurance_domain_budget[domain] = remaining.into();
        expected.1.insurance_domain_spent[domain] = 0;
        expected.1.insurance_domain_budget_remaining_total = remaining.into();
        assert_eq!(
            env.market_state(),
            expected,
            "exact shared-claim recredit/debit, including peer domains"
        );
        let market = env.svm.get_account(&env.market).unwrap();
        let mut metadata = market.clone();
        metadata.data.clone_from(&market_frame.data);
        assert_eq!(metadata, market_frame, "complete market metadata frame");
        for index in 0..2 {
            assert_eq!(
                state::read_asset_oracle_profile(&market.data, index).unwrap(),
                profiles[index]
            );
            let mut expected_sequences = sequences[index];
            if index == asset {
                expected_sequences.authority_epoch +=
                    u64::from(paid[0] != 0) + u64::from(paid[1] != 0);
            }
            assert_eq!(env.control_sequences(index), expected_sequences);
            assert_eq!(
                env.svm.get_account(&recipients[index]),
                Some(token_image(&recipient_frames[index], paid[index]))
            );
            assert_eq!(
                env.svm.get_account(&vaults[index]),
                Some(token_image(
                    &vault_frames[index],
                    if index == 0 {
                        BURN + entitlement - paid[0]
                    } else {
                        SECONDARY_STOCK - paid[1]
                    }
                ))
            );
            assert_eq!(
                env.svm.get_account(&mints[index]),
                Some(mint_frames[index].clone())
            );
            assert_eq!(
                env.svm.get_account(&admin_tokens[index]),
                Some(admin_token_frames[index].clone())
            );
        }
        assert_eq!(
            env.token_amount(vaults[0]) - u64::try_from(expected.1.vault).unwrap(),
            paid[1],
            "secondary recovery payouts displace exactly their primary backing into raw surplus"
        );
        assert_eq!(
            env.token_amount(vaults[0]) + paid[0] + 1_337,
            Mint::unpack(&mint_frames[0].data).unwrap().supply
        );
        assert_eq!(env.token_amount(vaults[1]) + paid[1], SECONDARY_STOCK);
        // The stock census uses booked custody; both raw rails are reconciled above.
        assert_market_stock_census(
            "recredited quote rails",
            &expected.1,
            &market.data,
            &[],
            expected.1.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("recredited quote rails", &expected.1, &[]).unwrap();
        state::market_view_mut(&mut market.data.clone())
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    };
    let mut peak = [0u64; 3];
    // Each rejecting suffix has enough raw custody; the shared claim is its limit.
    assert!(env.token_amount(vaults[last_rail]) >= entitlement - FIRST + 1);
    let epoch = env.control_sequences(asset).authority_epoch;
    let first = withdrawal_at(first_rail, FIRST, epoch);
    let excess = withdrawal_at(last_rail, entitlement - FIRST + 1, epoch + 1);
    peak[0] = land(env, &[first.clone(), excess], false, &[], Some(3));
    assert_eq!(
        env.market_state(),
        initial,
        "failed other-rail suffix restores unbooked recovery"
    );
    let changes = [env.market, vaults[first_rail], recipients[first_rail]];
    let first = withdrawal(env, first_rail, FIRST);
    peak[1] = land(env, &[first], false, &changes, None);
    let mut paid = [0u64; 2];
    paid[first_rail] = FIRST;
    check(env, paid);

    assert!(env.token_amount(vaults[first_rail]) >= 2);
    let epoch = env.control_sequences(asset).authority_epoch;
    let almost_last = withdrawal_at(last_rail, entitlement - FIRST - 1, epoch);
    let two = withdrawal_at(first_rail, 2, epoch + 1);
    peak[0] = peak[0].max(land(env, &[almost_last, two], false, &[], Some(3)));
    check(env, paid);
    let before = entitlement - paid.iter().sum::<u64>();
    let changes = [env.market, vaults[last_rail], recipients[last_rail]];
    let last = withdrawal(env, last_rail, entitlement - FIRST);
    peak[1] = peak[1].max(land(env, &[last], false, &changes, None));
    paid[last_rail] = entitlement - FIRST;
    assert_eq!(before, paid[last_rail]);
    check(env, paid);
    assert_eq!(env.market_state().1.insurance, 0);
    assert_eq!(env.market_state().1.vault, BURN.into());
    assert!(env.token_amount(vaults[first_rail]) >= 1);
    let replay = withdrawal(env, first_rail, 1);
    peak[0] = peak[0].max(land(env, &[replay], false, &[], Some(2)));
    check(env, paid);

    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    expected_admin.lamports += market_frame.lamports
        + vault_frames.iter().map(|frame| frame.lamports).sum::<u64>()
        - tombstone_rent;
    let changes = [
        env.market,
        vaults[0],
        vaults[1],
        admin_tokens[0],
        admin_tokens[1],
        env.mint,
        admin.pubkey(),
    ];
    let mut final_close = close.clone();
    final_close.data = ProgInstruction::CloseSlab {
        authority_epoch: env.control_sequences(0).authority_epoch,
    }
    .encode();
    peak[2] = land(env, &[final_close], true, &changes, None);
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    for index in 0..2 {
        assert!(env
            .svm
            .get_account(&vaults[index])
            .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
        assert_eq!(
            env.svm.get_account(&recipients[index]),
            Some(token_image(&recipient_frames[index], paid[index]))
        );
        let admin_amount = if index == 0 {
            paid[1]
        } else {
            SECONDARY_STOCK - paid[1]
        };
        assert_eq!(
            env.svm.get_account(&admin_tokens[index]),
            Some(token_image(&admin_token_frames[index], admin_amount))
        );
        let mut expected_mint = mint_frames[index].clone();
        let mut mint = Mint::unpack(&expected_mint.data).unwrap();
        if index == 0 {
            mint.supply -= BURN;
        }
        Mint::pack(mint, &mut expected_mint.data).unwrap();
        assert_eq!(env.svm.get_account(&mints[index]), Some(expected_mint));
        assert_eq!(
            mint.supply,
            paid[index] + admin_amount + if index == 0 { 1_337 } else { 0 }
        );
    }
    eprintln!("row421 recredited rails: asset={asset}, entitlement={entitlement}, secondary_first={secondary_first}, paid={paid:?}, rollbacks=3, payouts=2, close=1, peak_CU(rejection,payment,close)={peak:?}");
    *peak.iter().max().unwrap()
}
