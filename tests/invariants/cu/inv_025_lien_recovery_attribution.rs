//! INV-024/025/026: stock movement, late rollback, and Recovery with a live lien.
//! All expected value comes from public funding, quantity, price and margin inputs.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ENDOWMENTS: [u64; 4] = [313, 1_000, 211, 500];
const SCALE: u128 = BOUND_SCALE;

#[derive(Clone, Debug, Default)]
struct Ledger {
    capital: [u128; 3],
    claims: [[u128; 4]; 3],
    liens: [[u128; 4]; 3],
    positions: [[i128; 2]; 3],
    cash: [u64; 4],
    backing: [u128; 4],
    insurance: [u128; 4],
    // Loss-funded source backing is attributed separately from provider SPL
    // deposits. Neither claim faces nor IM labels are additional token stock.
    loss_backing: [u128; 4],
}

struct World {
    env: V16CuEnv,
    owners: [Keypair; 3],
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 4],
    mint_frame: Account,
    authority_frames: [Account; 4],
}

impl World {
    fn new() -> Self {
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                h_max: 4,
                initial_margin_bps: 1_000,
                maintenance_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        let owners = std::array::from_fn(|_| Keypair::new());
        let portfolios = std::array::from_fn(|actor| {
            env.svm
                .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                .unwrap();
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let authorities = [
            owners[0].pubkey(),
            owners[1].pubkey(),
            owners[2].pubkey(),
            env.admin.pubkey(),
        ];
        let tokens =
            authorities.map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner, env.mint));
        let mut funding: Vec<_> = tokens
            .into_iter()
            .zip(ENDOWMENTS)
            .map(|(token, amount)| {
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    amount,
                )
                .unwrap()
            })
            .collect();
        funding.push(
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
        );
        send_raw_ixs(&mut env.svm, &env.payer, funding, &[&env.admin]).unwrap();
        let mint_frame = env.svm.get_account(&env.mint).unwrap();
        let authority_frames = authorities.map(|key| env.svm.get_account(&key).unwrap());
        Self {
            env,
            owners,
            portfolios,
            tokens,
            mint_frame,
            authority_frames,
        }
    }

    fn check(&self, e: &Ledger, label: &str) {
        let env = &self.env;
        let market = env.svm.get_account(&env.market).unwrap();
        let header = market_group_header_bytes(&market.data);
        let group = env.market_state().1;
        let mut owner_claims = [0; 4];
        let mut owner_liens = [0; 4];
        for actor in 0..3 {
            let raw = env.svm.get_account(&self.portfolios[actor]).unwrap();
            assert_eq!(raw.owner, env.program_id);
            let (identity, owner) = state::read_portfolio_owner_preflight(&raw.data).unwrap();
            assert_eq!(owner, self.owners[actor].pubkey().to_bytes());
            assert_eq!(identity.market_group_id, env.market.to_bytes());
            let account = env.portfolio_state(self.portfolios[actor]);
            assert_eq!(
                account.capital.get(),
                e.capital[actor],
                "{label}: owner {actor} capital"
            );
            assert_eq!(
                account.pnl.get(),
                e.claims[actor].iter().sum::<u128>() as i128,
                "{label}: owner {actor} PnL"
            );
            assert_eq!(account.cancel_deposit_escrow.get(), 0);
            assert_eq!(account.reserved_pnl.get(), 0);
            assert_eq!(account.fee_credits.get(), 0);
            assert!(!resolved_receipt(&account).present);
            let mut positions = [0; 2];
            for leg in account
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
            {
                positions[leg.asset_index as usize] += leg.basis_pos_q;
            }
            assert_eq!(
                positions,
                e.positions[actor].map(|lots| lots * POS_SCALE as i128),
                "{label}: owner {actor} exposure"
            );
            let mut claims = [0; 4];
            let mut liens = [0; 4];
            let mut occupied = [false; 4];
            for source in account
                .source_domains
                .iter()
                .filter(|source| source.is_occupied())
            {
                let domain = source.domain.get() as usize;
                assert!(!occupied[domain], "one owner/domain attribution");
                occupied[domain] = true;
                assert_eq!(
                    source.source_claim_market_id.get(),
                    group.assets[domain / 2].market_id
                );
                claims[domain] = source.source_claim_bound_num.get();
                liens[domain] = source.source_lien_counterparty_backing_num.get();
                assert_eq!(
                    source.source_lien_effective_reserved.get() * SCALE,
                    liens[domain]
                );
                assert_eq!(source.source_claim_liened_num.get(), liens[domain]);
                assert_eq!(
                    source.source_claim_counterparty_liened_num.get(),
                    liens[domain]
                );
                for zero in [
                    source.source_claim_insurance_liened_num.get(),
                    source.source_lien_insurance_backing_num.get(),
                    source.source_claim_impaired_num.get(),
                    source.source_lien_impaired_effective_reserved.get(),
                ] {
                    assert_eq!(
                        zero, 0,
                        "{label}: no insurance-backed or impaired owner claim"
                    );
                }
            }
            assert_eq!(
                claims,
                e.claims[actor].map(|v| v * SCALE),
                "{label}: owner {actor} domains"
            );
            assert_eq!(
                liens,
                e.liens[actor].map(|v| v * SCALE),
                "{label}: owner {actor} liens"
            );
            for domain in 0..4 {
                owner_claims[domain] += claims[domain];
                owner_liens[domain] += liens[domain];
            }
        }
        for domain in 0..4 {
            let source = group.source_credit[domain];
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(
                source.positive_claim_bound_num, owner_claims[domain],
                "{label}: domain {domain} bound"
            );
            assert_eq!(source.exact_positive_claim_num, owner_claims[domain]);
            if owner_claims[domain] != 0 {
                assert_eq!(source.credit_rate_num, percolator::CREDIT_RATE_SCALE);
            }
            let fresh = (e.backing[domain] + e.loss_backing[domain]) * SCALE;
            assert_eq!(
                source.fresh_reserved_backing_num, fresh,
                "{label}: domain {domain} backing origins"
            );
            assert_eq!(source.valid_liened_backing_num, owner_liens[domain]);
            assert_eq!(bucket.valid_liened_backing_num, owner_liens[domain]);
            assert_eq!(
                bucket.fresh_unliened_backing_num,
                fresh - owner_liens[domain]
            );
            for zero in [
                source.spent_backing_num,
                source.provider_receivable_num,
                source.impaired_liened_backing_num,
                source.insurance_credit_reserved_num,
                source.valid_liened_insurance_num,
                source.impaired_liened_insurance_num,
                bucket.consumed_liened_backing_num,
                bucket.impaired_liened_backing_num,
                bucket.utilization_fee_earnings,
            ] {
                assert_eq!(zero, 0, "{label}: domain {domain} zero class");
            }
        }
        // Inspect every raw asset slot as well as the decoded aggregate view.
        for asset in 0..state::market_slot_capacity(&market.data).unwrap() {
            let slot = bytemuck::pod_read_unaligned::<percolator::EngineAssetSlotV16Account>(
                market_engine_slot_bytes(&market.data, asset),
            );
            assert_eq!(
                [
                    slot.insurance_domain_budget_long.get(),
                    slot.insurance_domain_budget_short.get()
                ],
                [e.insurance[2 * asset], e.insurance[2 * asset + 1]]
            );
            assert_eq!(slot.insurance_domain_spent_long.get(), 0);
            assert_eq!(slot.insurance_domain_spent_short.get(), 0);
        }
        let capital = e.capital.iter().sum::<u128>();
        let backing = e.backing.iter().sum::<u128>() + e.loss_backing.iter().sum::<u128>();
        let insurance = e.insurance.iter().sum::<u128>();
        let claims = e.claims.iter().flatten().sum::<u128>();
        let vault = capital + backing + insurance;
        assert_eq!(header.materialized_portfolio_count.get(), 3);
        assert_eq!(header.c_tot.get(), capital);
        assert_eq!(header.pnl_pos_tot.get(), claims);
        assert_eq!(header.source_claim_bound_total_num.get(), claims * SCALE);
        assert_eq!(header.source_fresh_backing_total_num.get(), backing * SCALE);
        assert_eq!(header.insurance.get(), insurance);
        assert_eq!(
            header.insurance_domain_budget_remaining_total.get(),
            insurance
        );
        assert_eq!(header.source_insurance_credit_reserved_total_atoms.get(), 0);
        assert_eq!(header.backing_provider_earnings_total.get(), 0);
        assert_eq!(
            header.vault.get(),
            vault,
            "{label}: exact input-derived stocks, zero residual"
        );
        assert_eq!(group.vault, vault);
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), self.mint_frame);
        let mint = Mint::unpack(&self.mint_frame.data).unwrap();
        assert_eq!(mint.supply, ENDOWMENTS.iter().sum::<u64>());
        assert_eq!(mint.mint_authority, COption::None);
        let authorities = [
            self.owners[0].pubkey(),
            self.owners[1].pubkey(),
            self.owners[2].pubkey(),
            env.admin.pubkey(),
        ];
        for (key, frame) in authorities.into_iter().zip(&self.authority_frames) {
            assert_eq!(env.svm.get_account(&key).as_ref(), Some(frame));
        }
        for (key, authority, amount) in self
            .tokens
            .into_iter()
            .zip(authorities)
            .zip(e.cash)
            .map(|((k, a), n)| (k, a, n))
            .chain([(env.vault, env.vault_authority, vault as u64)])
        {
            let raw = env.svm.get_account(&key).unwrap();
            assert_eq!(raw.owner, spl_token::ID);
            let token = TokenAccount::unpack(&raw.data).unwrap();
            assert_eq!(
                (token.mint, token.owner, token.amount),
                (env.mint, authority, amount),
                "{label}: exact custody owner {authority}"
            );
            assert_eq!(token.state, AccountState::Initialized);
            assert_eq!(token.delegate, COption::None);
            assert_eq!(token.close_authority, COption::None);
        }
        assert_eq!(
            vault + e.cash.iter().map(|v| u128::from(*v)).sum::<u128>(),
            u128::from(mint.supply)
        );
    }

    fn deposit(&self, actor: usize, amount: u128) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            data: self.env.deposit_ix(self.portfolios[actor], amount).encode(),
            accounts: vec![
                AccountMeta::new(self.owners[actor].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        }
    }

    fn trade(&self, asset: u16, lots: i128, price: u64) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            data: self
                .env
                .trade_no_cpi_ix(
                    self.portfolios[0],
                    self.portfolios[1],
                    asset,
                    lots * POS_SCALE as i128,
                    price,
                    0,
                )
                .encode(),
            accounts: vec![
                AccountMeta::new(self.owners[0].pubkey(), true),
                AccountMeta::new(self.owners[1].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[0], false),
                AccountMeta::new(self.portfolios[1], false),
            ],
        }
    }

    fn withdraw(&self, actor: usize, amount: u128) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            data: self
                .env
                .withdraw_ix(self.portfolios[actor], amount)
                .encode(),
            accounts: vec![
                AccountMeta::new(self.owners[actor].pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        }
    }

    fn reserve(&self, domain: u16, amount: u128, backing: bool) -> Instruction {
        let market_id = self.env.asset_market_id(domain / 2);
        let sequences = self.env.control_sequences(domain as usize / 2);
        let intent_id = next_control_sequence(if backing {
            sequences.backing_top_up
        } else {
            sequences.insurance_top_up
        });
        let ix = if backing {
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: sequences.authority_epoch,
                intent_id,
                market_id,
                domain,
                backing_fee_bps: 0,
                insurance_share_bps: 0,
                amount,
                expiry_slot: 100,
            }
        } else {
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: sequences.authority_epoch,
                intent_id,
                market_id,
                domain,
                amount,
            }
        };
        Instruction {
            program_id: self.env.program_id,
            data: ix.encode(),
            accounts: vec![
                AccountMeta::new(self.env.admin.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.tokens[3], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        }
    }

    fn send(&mut self, ix: Instruction) -> u64 {
        let signers: Vec<_> = self
            .owners
            .iter()
            .chain([&self.env.admin])
            .filter(|key| {
                ix.accounts
                    .iter()
                    .any(|meta| meta.is_signer && meta.pubkey == key.pubkey())
            })
            .collect();
        send_raw_tx(&mut self.env.svm, &self.env.payer, ix, &signers).unwrap()
    }
}

#[test]
fn v16_program_lien_recovery_preserves_attributed_stocks_and_late_rollback() {
    let mut total_steps = 0;
    let mut peak_cu = 0;
    let mut worlds = 0;
    for (direction, reverse_prefix, reverse_forfeit) in
        [-1i128, 1].into_iter().flat_map(|direction| {
            [false, true]
                .into_iter()
                .flat_map(move |prefix| [false, true].map(|forfeit| (direction, prefix, forfeit)))
        })
    {
        let winning_mark = (100 + 5 * direction) as u64;
        let adverse_mark = (100 - 5 * direction) as u64;
        let winning_domain = usize::from(direction > 0);
        let adverse_domain = 2 + usize::from(direction < 0);
        let spare_backing = winning_domain ^ 1;
        let spare_insurance = adverse_domain ^ 1;
        let mut w = World::new();
        let mut e = Ledger {
            cash: ENDOWMENTS,
            ..Ledger::default()
        };
        let mut steps = 0;
        macro_rules! check {
            ($label:expr, $cu:expr) => {{
                let cu = $cu;
                assert!(cu <= 1_400_000, "public transaction compute bound");
                peak_cu = peak_cu.max(cu);
                steps += 1;
                w.check(&e, $label);
            }};
        }
        w.check(&e, "funded public setup");
        for asset in 0..2 {
            check!(
                "configure mark",
                w.env.configure_auth_mark_for_asset_as_admin(asset, 1, 100)
            );
        }
        for (actor, amount) in [(0, 313), (1, 1_000), (2, 101)] {
            e.capital[actor] += amount;
            e.cash[actor] -= amount as u64;
            let ix = w.deposit(actor, amount);
            check!("initial deposit", w.send(ix));
        }
        for (domain, amount, backing) in [
            (winning_domain, 150, true),
            (adverse_domain, 79, true),
            (spare_backing, 43, false),
        ] {
            if backing {
                e.backing[domain] += amount;
            } else {
                e.insurance[domain] += amount;
            }
            e.cash[3] -= amount as u64;
            let ix = w.reserve(domain as u16, amount, backing);
            check!("initial reserve", w.send(ix));
        }
        for (asset, lots) in [(0, 20), (1, 10)] {
            let lots = lots * direction;
            e.positions[0][asset as usize] = lots;
            e.positions[1][asset as usize] = -lots;
            let ix = w.trade(asset, lots, 100);
            check!("open", w.send(ix));
        }
        w.env.svm.warp_to_slot(2);
        check!(
            "winning mark",
            w.env.push_auth_mark_for_asset_as_admin(0, 2, winning_mark)
        );
        check!(
            "adverse mark",
            w.env.push_auth_mark_for_asset_as_admin(1, 2, adverse_mark)
        );
        // Each account-local settlement is checked, including the temporarily
        // unpaired gain/loss prefix. Public quantities give 20*5 and 10*5 atoms.
        for (actor, first_asset) in [(1, 0), (0, 0), (1, 1)] {
            if actor == 0 {
                e.claims[0][winning_domain] = 20 * u128::from(winning_mark.abs_diff(100));
                e.capital[0] -= 10 * u128::from(adverse_mark.abs_diff(100));
                e.loss_backing[adverse_domain] += 50;
            } else if first_asset == 0 {
                e.capital[1] -= 20 * u128::from(winning_mark.abs_diff(100));
                e.loss_backing[winning_domain] += 100;
                e.claims[1][adverse_domain] = 10 * u128::from(adverse_mark.abs_diff(100));
            }
            check!(
                "settle account",
                w.env.crank(
                    w.portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations_for_assets(&[
                            first_asset,
                            1 - first_asset
                        ]),
                    }
                )
            );
        }

        let prefix = [
            w.trade(1, 2 * direction, adverse_mark),
            w.deposit(2, 17),
            w.reserve(spare_backing as u16, 23, true),
            w.reserve(spare_insurance as u16, 19, false),
        ];
        let order = if reverse_prefix {
            [3, 2, 1, 0]
        } else {
            [0, 1, 2, 3]
        };
        let mut failing_deposit = w.deposit(2, 94);
        failing_deposit.data = ProgInstruction::Deposit {
            portfolio_id: w.env.portfolio_id(w.portfolios[2]),
            expected_sequence: w.env.portfolio_matcher_sequence(w.portfolios[2]) + 1,
            amount: 94,
        }
        .encode();
        let mut instructions = vec![heap_ix(), cu_ix()];
        instructions.extend(order.map(|i| prefix[i].clone()));
        instructions.push(failing_deposit);
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&w.env.payer.pubkey()),
            &vec![
                &w.env.payer,
                &w.owners[0],
                &w.owners[1],
                &w.owners[2],
                &w.env.admin,
            ],
            w.env.svm.latest_blockhash(),
        );
        let fee = FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let mut keys = tx.message.account_keys.clone();
        keys.extend(w.portfolios);
        keys.extend(w.tokens);
        keys.extend([
            w.env.mint,
            w.env.vault_authority,
            solana_sdk::sysvar::clock::ID,
        ]);
        keys.sort_unstable();
        keys.dedup();
        let frame: Vec<_> = keys
            .into_iter()
            .map(|key| (key, w.env.svm.get_account(&key)))
            .collect();
        let failure = w
            .env
            .svm
            .send_transaction(tx)
            .expect_err("94-atom deposit has only 93 external atoms after valid prefix");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                6,
                InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
            )
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", w.env.program_id))
                .count(),
            4,
            "all four stock/lien prefix instructions executed"
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", spl_token::ID))
                .count(),
            3,
            "all three prefix token transfers executed"
        );
        for (key, mut before) in frame {
            if key == w.env.payer.pubkey() {
                before.as_mut().unwrap().lamports -= fee;
            }
            assert_eq!(
                w.env.svm.get_account(&key),
                before,
                "late failure full account rollback: {key}"
            );
        }
        check!("late rollback", failure.meta.compute_units_consumed);
        for i in order {
            match i {
                // Full-rate backing covers only the independently computed IM
                // shortfall: 53 atoms when short, 61 when long.
                0 => {
                    e.liens[0][winning_domain] = (20 * u128::from(winning_mark)
                        + 12 * u128::from(adverse_mark))
                    .div_ceil(10)
                        - (313 - 50);
                    e.positions[0][1] += 2 * direction;
                    e.positions[1][1] -= 2 * direction;
                }
                1 => {
                    e.capital[2] += 17;
                    e.cash[2] -= 17;
                }
                2 => {
                    e.backing[spare_backing] += 23;
                    e.cash[3] -= 23;
                }
                3 => {
                    e.insurance[spare_insurance] += 19;
                    e.cash[3] -= 19;
                }
                _ => unreachable!(),
            }
            check!("identical retained prefix retry", w.send(prefix[i].clone()));
        }
        assert!(e.liens[0][winning_domain] > 0);

        let sibling_frames = [
            w.env.svm.get_account(&w.portfolios[0]),
            w.env.svm.get_account(&w.portfolios[1]),
        ];
        e.capital[2] -= 31;
        e.cash[2] += 31;
        let ix = w.withdraw(2, 31);
        check!("unrelated owner withdrawal with live lien", w.send(ix));
        e.backing[spare_backing] -= 11;
        e.cash[3] += 11;
        check!(
            "unrelated backing withdrawal with live lien",
            w.env.withdraw_backing_bucket_to_admin_token_with_cu(
                w.tokens[3],
                spare_backing as u16,
                11
            )
        );
        e.insurance[spare_backing] -= 7;
        e.cash[3] += 7;
        check!(
            "insurance withdrawal with live lien",
            w.env
                .withdraw_insurance_domain_to_admin_token_with_cu(w.tokens[3], 0, 7)
        );
        assert_eq!(
            [
                w.env.svm.get_account(&w.portfolios[0]),
                w.env.svm.get_account(&w.portfolios[1])
            ],
            sibling_frames
        );

        check!(
            "Recovery policy",
            w.env.configure_permissionless_resolve_with_cu(1_000, 1)
        );
        w.env.svm.warp_to_slot(3);
        let admin = w.env.admin.insecure_clone();
        check!(
            "adverse asset shutdown",
            w.env
                .try_shutdown_asset_with_authority(&admin, 1, 3)
                .unwrap()
        );
        assert_eq!(
            w.env.market_state().1.assets[1].lifecycle,
            AssetLifecycleV16::Recovery
        );
        for actor in if reverse_forfeit { [1, 0] } else { [0, 1] } {
            e.positions[actor][1] = 0;
            check!(
                "Recovery leg forfeit",
                w.env.forfeit_recovery_leg_with_cu(
                    &w.owners[actor],
                    w.portfolios[actor],
                    1,
                    u128::MAX
                )
            );
        }
        for actor in [0, 1] {
            check!(
                "post-forfeit refresh",
                w.env.crank(
                    w.portfolios[actor],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 3,
                        observations: vec![]
                    }
                )
            );
        }
        let ix = w.trade(0, -20 * direction, winning_mark);
        e.positions[0][0] = 0;
        e.positions[1][0] = 0;
        check!("close surviving live leg", w.send(ix));
        e.liens[0][winning_domain] = 0;
        check!(
            "release flat owner lien",
            w.env.crank(
                w.portfolios[0],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 3,
                    observations: vec![]
                }
            )
        );
        for actor in 0..3 {
            let amount = e.capital[actor];
            e.capital[actor] = 0;
            e.cash[actor] += amount as u64;
            let ix = w.withdraw(actor, amount);
            check!("senior owner exit", w.send(ix));
        }
        total_steps += steps;
        assert_eq!(e.cash, [263, 900, 211, 204]);
        assert_eq!(w.env.token_amount(w.env.vault), 446);
        worlds += 1;
    }
    assert_eq!((worlds, total_steps), (8, 272));
    println!(
        "lien/stock attribution: {worlds} worlds, {total_steps} checked transactions, {worlds} late rollbacks, peak {peak_cu} CU"
    );
}
