//! INV-073: recovered insurance survives native redemption, wallet recreation and
//! quote-rail changes after both insurance signing keys have disappeared.
//! INV-018/021/024/025/063/067/069/070/071/078/081/082 receive bounded related
//! evidence. SPL close authority is granted before departure; it authorizes only
//! custody redemption. Administrative expiry/retirement remains a prerequisite.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};

const LIQUIDITY: u64 = 211;
const VAULT_DONATION: u64 = 19;
const RECIPIENT_DONATION: u64 = 17;
const NATIVE_PAYMENTS: [u64; 2] = [23, 31];
const BURN: u64 = 207;
const LIMIT: u64 = 300_000;

#[test]
fn v16_program_recredited_insurance_recreates_native_custody_without_role_signatures() {
    for native_first in [false, true] {
        for sync in [false, true] {
            absent_reserve_progress(
                ProviderHistory::FreshNativeCustody { native_first, sync },
                None,
            );
        }
    }
}

pub(super) fn world(params: V16CuMarketParams) -> V16CuEnv {
    use inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_create_public_spl_mint;
    use inv_081_success_state_validity_over_complete_public_routes::inv081_public_native_market_with_params;

    let mut env =
        inv081_public_native_market_with_params(params.max_portfolio_assets as usize, params);
    let admin = env.admin.insecure_clone();
    let primary = inv018_create_public_spl_mint(
        &mut env.svm,
        &env.payer,
        admin.pubkey(),
        spl_token::native_mint::DECIMALS,
    );
    env.send(
        ProgInstruction::UpdateBaseUnitMints {
            primary_mint: primary.to_bytes(),
            secondary_mint: env.mint.to_bytes(),
            authority_epoch: env.control_sequences(0).authority_epoch,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(primary, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new_readonly(env.vault, false),
        ],
        &[&admin],
    )
    .unwrap();
    // Update host handles only after the public configuration succeeds.
    env.mint = primary;
    env.vault = create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, primary);
    env
}

pub(super) struct NativeCustody {
    pub(super) vault: Pubkey,
    recipient: Pubkey,
    pub(super) admin_token: Pubkey,
}

impl NativeCustody {
    pub(super) fn keys(&self) -> [Pubkey; 4] {
        [
            spl_token::native_mint::ID,
            self.vault,
            self.recipient,
            self.admin_token,
        ]
    }

    pub(super) fn create(env: &mut V16CuEnv, admin: &Keypair, beneficiary: &Keypair) -> Self {
        let mint = spl_token::native_mint::ID;
        let vault = canonical_vault_ata(env.vault_authority, mint);
        assert_eq!(env.token_amount(vault), 0);
        let recipient = create_ata_for_test(&mut env.svm, &env.payer, beneficiary.pubkey(), mint);
        let admin_token = create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint);
        let market = env.svm.get_account(&env.market);
        send_raw_ixs(
            &mut env.svm,
            &env.payer,
            vec![
                system_instruction::transfer(&admin.pubkey(), &vault, LIQUIDITY),
                spl_token::instruction::sync_native(&spl_token::ID, &vault).unwrap(),
                system_instruction::transfer(&admin.pubkey(), &vault, VAULT_DONATION),
                system_instruction::transfer(&admin.pubkey(), &recipient, RECIPIENT_DONATION),
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &recipient,
                    Some(&env.payer.pubkey()),
                    spl_token::instruction::AuthorityType::CloseAccount,
                    &beneficiary.pubkey(),
                    &[],
                )
                .unwrap(),
            ],
            &[admin, beneficiary],
        )
        .unwrap();
        assert_eq!(env.svm.get_account(&env.market), market);
        assert_eq!(env.market_state().1.insurance, 0);
        Self {
            vault,
            recipient,
            admin_token,
        }
    }
}

fn token_image(frame: &Account, amount: u64, raw: u64) -> Account {
    // Independent expected Account image; never installed into LiteSVM.
    let mut expected = frame.clone();
    let mut token = TokenAccount::unpack(&frame.data).unwrap();
    assert_eq!(token.state, AccountState::Initialized);
    assert_eq!(token.delegate, COption::None);
    token.amount = amount;
    if let COption::Some(rent) = token.is_native {
        expected.lamports = rent + amount + raw;
    } else {
        assert_eq!(raw, 0);
    }
    TokenAccount::pack(token, &mut expected.data).unwrap();
    expected
}

pub(super) fn finish(
    env: &mut V16CuEnv,
    admin: &Keypair,
    asset: usize,
    absent: [Pubkey; 3],
    primary_recipient: Pubkey,
    native: &NativeCustody,
    entitlement: u64,
    native_first: bool,
    sync: bool,
    close: &Instruction,
    tracked: &[Pubkey],
) -> u64 {
    let domain = 2 * asset;
    let initial = env.market_state();
    let market_frame = env.svm.get_account(&env.market).unwrap();
    let vaults = [env.vault, native.vault];
    let recipients = [primary_recipient, native.recipient];
    let admin_tokens = [close.accounts[4].pubkey, native.admin_token];
    let mints = [env.mint, spl_token::native_mint::ID];
    let mint_frames = mints.map(|key| env.svm.get_account(&key).unwrap());
    let vault_frames = vaults.map(|key| env.svm.get_account(&key).unwrap());
    let admin_frames = admin_tokens.map(|key| env.svm.get_account(&key).unwrap());
    let absent_frames = absent.map(|key| env.svm.get_account(&key));
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let sequences = [0, 1].map(|index| env.control_sequences(index));
    let profiles =
        [0, 1].map(|index| state::read_asset_oracle_profile(&market_frame.data, index).unwrap());
    assert!(matches!(entitlement, 100 | 101));
    assert_eq!(initial.1.insurance_domain_spent[domain], 100);
    assert_eq!(initial.1.insurance, u128::from(entitlement - 100));
    assert_eq!(initial.1.vault, u128::from(BURN + entitlement));
    assert_eq!(initial.1.materialized_portfolio_count, 0);
    assert_eq!((initial.1.c_tot, initial.1.pnl_pos_tot), (0, 0));
    assert_eq!(
        initial.1.source_backing_buckets[domain].status,
        BackingBucketStatusV16::Expired
    );
    for key in [absent[0], absent[1], primary_recipient] {
        assert!(env
            .svm
            .get_account(&key)
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
    }
    assert_eq!(env.token_amount(vaults[0]), BURN + entitlement);
    assert_eq!(env.token_amount(vaults[1]), LIQUIDITY);
    assert_eq!(vault_frames[1].lamports, rent + LIQUIDITY + VAULT_DONATION);
    assert_eq!(
        Mint::unpack(&mint_frames[0].data).unwrap().supply,
        1_237 + entitlement + 307
    );
    assert_eq!(
        Mint::unpack(&mint_frames[0].data).unwrap().mint_authority,
        COption::None
    );
    assert_eq!(Mint::unpack(&mint_frames[1].data).unwrap().supply, 0);

    let original_native = env.svm.get_account(&native.recipient).unwrap();
    let token = TokenAccount::unpack(&original_native.data).unwrap();
    assert_eq!((token.owner, token.amount), (absent[0], 0));
    assert_eq!(token.close_authority, COption::Some(env.payer.pubkey()));
    assert_eq!(original_native.lamports, rent + RECIPIENT_DONATION);
    let mut recipient_frames = admin_frames.clone();
    for (rail, frame) in recipient_frames.iter_mut().enumerate() {
        let mut token = TokenAccount::unpack(&frame.data).unwrap();
        token.owner = absent[0];
        token.close_authority = COption::None;
        token.amount = 0;
        frame.lamports = rent;
        TokenAccount::pack(token, &mut frame.data).unwrap();
        assert_eq!(token.mint, mints[rail]);
    }

    let land = |env: &mut V16CuEnv,
                instructions: &[Instruction],
                signed_close: bool,
                changed: &[Pubkey],
                creation_rent: u64,
                reject: bool| {
        env.svm.expire_blockhash();
        let mut batch = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32),
        ];
        batch.extend_from_slice(instructions);
        if reject {
            let mut denied = close.clone();
            denied.accounts[0].is_signer = false;
            batch.push(denied);
        }
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
        let before: Vec<_> = keys.iter().map(|key| env.svm.get_account(key)).collect();
        let fee = required as u64 * FeeStructure::default().lamports_per_signature;
        let result = env.svm.send_transaction(tx);
        let meta = if reject {
            let failure = result.expect_err("administrative suffix requires its signer");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    (instructions.len() + 2) as u8,
                    InstructionError::Custom(PercolatorError::ExpectedSigner as u32)
                )
            );
            for program in [env.program_id, associated_token_program_id()] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    instructions
                        .iter()
                        .filter(|ix| ix.program_id == program)
                        .count()
                );
            }
            failure.meta
        } else {
            result.expect("bounded recredit, redemption and custody recreation")
        };
        let before_lamports: u128 = before
            .iter()
            .flatten()
            .map(|a| u128::from(a.lamports))
            .sum();
        let after_lamports: u128 = keys
            .iter()
            .filter_map(|key| env.svm.get_account(key))
            .map(|a| u128::from(a.lamports))
            .sum();
        assert_eq!(after_lamports + u128::from(fee), before_lamports);
        for (key, mut frame) in keys.into_iter().zip(before) {
            if key == env.payer.pubkey() {
                frame.as_mut().unwrap().lamports -= fee + if reject { 0 } else { creation_rent };
            }
            if reject || !changed.contains(&key) {
                assert_eq!(
                    env.svm.get_account(&key),
                    frame,
                    "complete Account frame {key}"
                );
            }
        }
        assert_cu_within(
            "INV-073 native recredit custody",
            meta.compute_units_consumed,
            LIMIT,
        );
        meta.compute_units_consumed
    };
    let repair = |rail: usize| Instruction {
        program_id: associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(recipients[rail], false),
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new_readonly(mints[rail], false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: vec![1],
    };
    let payment = |rail: usize, amount: u64| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(absent[0], false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(recipients[rail], false),
            AccountMeta::new(vaults[rail], false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset as u16,
            market_id: env.asset_market_id(asset as u16),
            authority_epoch: sequences[asset].authority_epoch,
            amount: amount.into(),
        }
        .encode(),
    };
    let native_total = NATIVE_PAYMENTS.iter().sum::<u64>();
    let classic_amount = entitlement - native_total;
    let classic = vec![repair(0), payment(0, classic_amount)];
    let mut native_prefix = Vec::new();
    if sync {
        for key in [native.vault, native.recipient] {
            native_prefix.push(spl_token::instruction::sync_native(&spl_token::ID, &key).unwrap());
        }
    }
    // The existing close authority redeems only the donated old custody. Recreate
    // unencumbered custody before the wrapper pays the beneficiary's recovered claim.
    native_prefix.push(
        spl_token::instruction::close_account(
            &spl_token::ID,
            &native.recipient,
            &absent[0],
            &env.payer.pubkey(),
            &[],
        )
        .unwrap(),
    );
    native_prefix.push(repair(1));
    native_prefix.push(payment(1, NATIVE_PAYMENTS[0]));
    let native_tail = vec![payment(1, NATIVE_PAYMENTS[1])];
    let events = if native_first {
        [
            (1, NATIVE_PAYMENTS[0], native_prefix),
            (0, classic_amount, classic),
            (1, NATIVE_PAYMENTS[1], native_tail),
        ]
    } else {
        [
            (0, classic_amount, classic),
            (1, NATIVE_PAYMENTS[0], native_prefix),
            (1, NATIVE_PAYMENTS[1], native_tail),
        ]
    };
    let mut paid = [0u64; 2];
    let mut recreated = false;
    let mut peak = 0;
    for (rail, amount, prefix) in events {
        let rank_before = entitlement - paid.iter().sum::<u64>();
        let creates = prefix
            .iter()
            .any(|ix| ix.program_id == associated_token_program_id());
        peak = peak.max(land(env, &prefix, false, &[], 0, true));
        let changed = [env.market, vaults[rail], recipients[rail], absent[0]];
        peak = peak.max(land(
            env,
            &prefix,
            false,
            &changed,
            if creates { rent } else { 0 },
            false,
        ));
        paid[rail] += amount;
        if rail == 1 {
            recreated = true;
        }
        let remaining = entitlement - paid.iter().sum::<u64>();
        assert_eq!(rank_before - remaining, amount);
        assert!(remaining < rank_before);
        let mut expected = initial.clone();
        expected.1.vault -= u128::from(entitlement - remaining);
        expected.1.insurance = remaining.into();
        expected.1.insurance_domain_budget[domain] = remaining.into();
        expected.1.insurance_domain_spent[domain] = 0;
        expected.1.insurance_domain_budget_remaining_total = remaining.into();
        assert_eq!(
            env.market_state(),
            expected,
            "one shared recredited claim across rails and custody incarnations"
        );
        let market = env.svm.get_account(&env.market).unwrap();
        let mut metadata = market.clone();
        metadata.data.clone_from(&market_frame.data);
        assert_eq!(metadata, market_frame);
        for index in 0..2 {
            assert_eq!(env.control_sequences(index), sequences[index]);
            assert_eq!(
                state::read_asset_oracle_profile(&market.data, index).unwrap(),
                profiles[index]
            );
            assert_eq!(
                env.svm.get_account(&mints[index]),
                Some(mint_frames[index].clone())
            );
            assert_eq!(
                env.svm.get_account(&admin_tokens[index]),
                Some(admin_frames[index].clone())
            );
        }
        assert_eq!(
            env.svm.get_account(&vaults[0]),
            Some(token_image(
                &vault_frames[0],
                BURN + entitlement - paid[0],
                0
            ))
        );
        let synced = sync && recreated;
        assert_eq!(
            env.svm.get_account(&vaults[1]),
            Some(token_image(
                &vault_frames[1],
                LIQUIDITY - paid[1] + if synced { VAULT_DONATION } else { 0 },
                if synced { 0 } else { VAULT_DONATION }
            ))
        );
        if paid[0] != 0 {
            assert_eq!(
                env.svm.get_account(&recipients[0]),
                Some(token_image(&recipient_frames[0], paid[0], 0))
            );
        } else {
            assert!(env
                .svm
                .get_account(&recipients[0])
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        }
        if recreated {
            assert_eq!(
                env.svm.get_account(&recipients[1]),
                Some(token_image(&recipient_frames[1], paid[1], 0))
            );
        } else {
            assert_eq!(
                env.svm.get_account(&recipients[1]),
                Some(original_native.clone())
            );
        }
        let wallet = if recreated {
            Some(Account {
                lamports: rent + RECIPIENT_DONATION,
                data: vec![],
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            })
        } else {
            absent_frames[0].clone()
        };
        assert_eq!(
            env.svm.get_account(&absent[0]),
            wallet,
            "SPL redemption alone recreates the missing owner wallet"
        );
        for index in 1..3 {
            assert_eq!(env.svm.get_account(&absent[index]), absent_frames[index]);
        }
        assert_eq!(
            env.token_amount(vaults[0]) - u64::try_from(expected.1.vault).unwrap(),
            paid[1]
        );
        assert_eq!(
            env.token_amount(vaults[0]) + paid[0] + 1_337,
            Mint::unpack(&mint_frames[0].data).unwrap().supply
        );
        assert_eq!(
            env.svm.get_account(&vaults[1]).unwrap().lamports - rent + paid[1],
            LIQUIDITY + VAULT_DONATION
        );
        assert_market_stock_census(
            "native recredit custody",
            &expected.1,
            &market.data,
            &[],
            expected.1.vault,
        )
        .unwrap();
        assert_reservation_encumbrance_census("native recredit custody", &expected.1, &[]).unwrap();
        state::market_view_mut(&mut market.data.clone())
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    }
    assert_eq!(paid, [classic_amount, native_total]);
    assert!(recreated);
    assert_eq!(env.market_state().1.vault, BURN.into());
    assert_eq!(env.market_state().1.insurance, 0);
    let tombstone_rent = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);
    let mut expected_admin = env.svm.get_account(&admin.pubkey()).unwrap();
    expected_admin.lamports +=
        market_frame.lamports + 2 * rent - tombstone_rent + if sync { 0 } else { VAULT_DONATION };
    let changed = [
        env.market,
        vaults[0],
        vaults[1],
        admin_tokens[0],
        admin_tokens[1],
        env.mint,
        admin.pubkey(),
    ];
    peak = peak.max(land(env, &[close.clone()], true, &changed, 0, false));
    let tombstone = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&tombstone);
    assert_eq!(tombstone.lamports, tombstone_rent);
    assert_eq!(
        env.svm.get_account(&admin.pubkey()),
        Some(expected_admin.clone())
    );
    for vault in vaults {
        assert!(env
            .svm
            .get_account(&vault)
            .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
    }
    assert_eq!(
        env.svm.get_account(&admin_tokens[0]),
        Some(token_image(&admin_frames[0], native_total, 0))
    );
    let native_surplus = LIQUIDITY - native_total + if sync { VAULT_DONATION } else { 0 };
    assert_eq!(
        env.svm.get_account(&admin_tokens[1]),
        Some(token_image(&admin_frames[1], native_surplus, 0))
    );
    let mut expected_mint = mint_frames[0].clone();
    let mut mint = Mint::unpack(&expected_mint.data).unwrap();
    mint.supply -= BURN;
    Mint::pack(mint, &mut expected_mint.data).unwrap();
    assert_eq!(env.svm.get_account(&mints[0]), Some(expected_mint));
    assert_eq!(mint.supply, 1_337 + paid[0] + native_total);
    assert_eq!(env.svm.get_account(&mints[1]), Some(mint_frames[1].clone()));

    let redeem_surplus = spl_token::instruction::close_account(
        &spl_token::ID,
        &native.admin_token,
        &admin.pubkey(),
        &admin.pubkey(),
        &[],
    )
    .unwrap();
    peak = peak.max(land(
        env,
        &[redeem_surplus],
        true,
        &[native.admin_token, admin.pubkey()],
        0,
        false,
    ));
    expected_admin.lamports += rent + native_surplus;
    assert_eq!(env.svm.get_account(&admin.pubkey()), Some(expected_admin));
    assert!(env
        .svm
        .get_account(&native.admin_token)
        .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
    assert_eq!(env.svm.get_account(&env.market), Some(tombstone));
    println!("native recredit custody: asset={asset}, entitlement={entitlement}, native_first={native_first}, sync={sync}, payments=3, rollbacks=3, repairs=2, wallet_recreations=1, burned={BURN}, peak={peak} CU");
    peak
}
