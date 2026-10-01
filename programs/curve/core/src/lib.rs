//! antumbra_curve on LEZ v0.3: the wire types and the whole program logic.
//!
//! The guest binary is two lines that hand [`plan`] and [`apply`] to
//! `lee_core::program::run_program`. Everything else lives here so the CLI and
//! the host tests use the same types, seeds and checks as the program.
//!
//! WHAT THE PROGRAM DOES
//!
//! A bonding-curve sale (RFP-015). `create_sale` writes the curve; `execute_buy`
//! takes the buyer's collateral into the sale's holding and advances the curve
//! by exactly what `antumbra::Curve::buy` computes after the per-swap fee;
//! `collect_fees` sweeps the accrued fee to the fee treasury fixed at creation;
//! `withdraw` pays the creator the collateral raised once the sale reserve is
//! exhausted. The arithmetic is the `antumbra` crate, unchanged, the same code
//! the host tests and the Kani proofs cover; `k = Vt * Vc` is never formed.
//!
//! THE SPLIT v0.3 FORCES
//!
//! A v0.3 program runs twice per call. `plan` sees account ids, signer flags
//! and which shard each account row selects, never data. It decides the
//! effects, the chained calls, the validity windows and the events. Then each
//! effect is applied by the same program, one shard at a time: `apply` sees
//! that shard's current bytes and may replace them if the shard is its own.
//!
//! So a buy is planned without the curve: the plan moves exactly
//! `collateral_in` from the buyer into the holding, and `apply` prices it
//! against the stored curve, refusing on slippage, a closed sale or a size the
//! reserve cannot serve. A refusal in `apply` fails the whole transaction,
//! payment included. The same pattern covers every payout: the caller names
//! the amount (the accrued fee, the collateral raised), the plan pays exactly
//! that, and `apply` refuses unless it equals what the stored sale says.
//!
//! WHERE THE MONEY SITS
//!
//! v0.3 has no program-owned accounts. Native balance is the shard at
//! `[0; 32]` of any account, and a token balance is the token program's shard.
//! A sale's holding is a PDA of this program, `[sale_id, "holding"]`; nobody
//! holds a key for it, and the only way to debit it is a chained call carrying
//! its PDA seed, which only this program can attach. The collateral can be the
//! native balance or a token of the builtin token program.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    native_token::{self, NATIVE_TOKEN_PROGRAM_ID},
    program::{write_once, AccountMeta, ChainedCall, PdaSeed, Plan, PlanInput, ProgramEvent},
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use token_core::{TokenDescriptor, TokenKind};

pub mod errors;
use errors::*;

/// The token program's account id on every LEZ v0.3 chain, derived from its
/// builtin name rather than read from anything the caller passes, so a
/// transfer can never be routed through a lookalike program.
#[must_use]
pub fn token_program() -> AccountId {
    token_core::token_account_id()
}

/// What a sale takes as collateral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Asset {
    /// The chain's native balance (LGO on testnet).
    Native,
    /// A fungible token of the builtin token program.
    Token { definition_id: AccountId },
}

impl Asset {
    /// The program whose shard holds this asset on any account.
    #[must_use]
    pub fn shard_program(&self) -> AccountId {
        match self {
            Self::Native => NATIVE_TOKEN_PROGRAM_ID,
            Self::Token { .. } => token_program(),
        }
    }

    fn code(&self) -> (u8, [u8; 32]) {
        match self {
            Self::Native => (0, [0; 32]),
            Self::Token { definition_id } => (1, *definition_id.value()),
        }
    }

    /// The asset a stored sale records.
    #[must_use]
    pub fn of(code: u8, definition: [u8; 32]) -> Self {
        if code == 0 {
            Self::Native
        } else {
            Self::Token {
                definition_id: AccountId::new(definition),
            }
        }
    }
}

/// The program's instruction. The variant order is the ABI.
///
/// A "signer handle" is a row naming the signing account with this program's
/// own shard selected. The program never writes it; the row exists so the
/// signature marks the account authorised, and so the same account can also
/// appear with its native or token shard as a destination.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Open a sale. The curve is validated now, through `antumbra::Curve::new`
    /// and the 1% fee cap, rather than by the first buyer.
    ///
    /// Accounts: `[sale (this program's shard), creator (signer handle)]`.
    CreateSale {
        sale_id: [u8; 32],
        collateral: Asset,
        vt: u128,
        vc: u128,
        sale_reserve: u128,
        seed_reserve: u128,
        fee_treasury: AccountId,
        /// Millionths; at most `antumbra::fees::CAP_PER_SWAP` (1%).
        fee_rate: u128,
    },
    /// Pay `collateral_in` into the sale's holding and buy at the curve.
    /// `apply` refuses unless `Curve::buy(collateral_in - fee, min_tokens_out)`
    /// succeeds against the stored curve.
    ///
    /// Accounts: `[sale, buyer (asset shard, signs), holding (asset shard)]`.
    ExecuteBuy {
        sale_id: [u8; 32],
        collateral: Asset,
        collateral_in: u128,
        min_tokens_out: u128,
    },
    /// Sweep `amount`, which must equal the accrued fee exactly, from the
    /// holding to the fee treasury fixed at creation. Permissionless.
    ///
    /// Accounts: `[sale, holding (asset shard), fee_treasury (asset shard)]`.
    CollectFees {
        sale_id: [u8; 32],
        collateral: Asset,
        amount: u128,
    },
    /// Once the sale reserve is exhausted, pay `amount`, which must equal the
    /// collateral raised and not yet withdrawn, into `destination`. The accrued
    /// fee is not part of it.
    ///
    /// Accounts: `[sale, holding (asset shard), destination (asset shard),
    /// creator (signer handle)]`.
    Withdraw {
        sale_id: [u8; 32],
        collateral: Asset,
        amount: u128,
    },
}

/// On-chain sale state, this program's shard of the sale PDA. The first ten
/// fields are the v0.2.4 layout unchanged; the last three are new on v0.3.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Sale {
    pub vt: u128,
    pub vc: u128,
    pub sale_reserve: u128,
    pub real_collateral: u128,
    pub seed_reserve: u128,
    pub creator: AccountId,
    /// The holding PDA, fixed at creation, so a buy cannot name another.
    pub treasury: AccountId,
    /// Where the protocol fee goes. Separate from the holding, because the two
    /// belong to different parties.
    pub fee_treasury: AccountId,
    /// Millionths: 1_000_000 is 100%. Capped in the program at 1%.
    pub fee_rate: u128,
    /// Fee taken on buys and not yet swept to the fee treasury.
    pub fees_accrued: u128,
    /// Collateral already paid to the creator. v0.2.4 read the holding's
    /// balance to know what was left; a v0.3 apply cannot read another
    /// program's shard, so the sale keeps the count itself.
    pub withdrawn: u128,
    /// 0 = native, 1 = token.
    pub collateral: u8,
    pub collateral_definition: [u8; 32],
}

impl Sale {
    #[must_use]
    pub fn curve(&self) -> antumbra::Curve {
        antumbra::Curve {
            vt: self.vt,
            vc: self.vc,
            sale_reserve: self.sale_reserve,
            real_collateral: self.real_collateral,
        }
    }

    #[must_use]
    pub fn asset(&self) -> Asset {
        Asset::of(self.collateral, self.collateral_definition)
    }
}

/// What `plan` asks `apply` to do to a sale shard. Every field is either
/// something the plan knew (an account id, a signer) or something the caller
/// claimed (an amount), and `apply` checks it against the state.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    Create(Sale),
    Buy {
        holding: AccountId,
        collateral: Asset,
        collateral_in: u128,
        min_tokens_out: u128,
    },
    CollectFees {
        holding: AccountId,
        fee_treasury: AccountId,
        collateral: Asset,
        amount: u128,
    },
    Withdraw {
        creator: AccountId,
        holding: AccountId,
        collateral: Asset,
        amount: u128,
    },
}

// ---------------------------------------------------------------- addresses

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        buf.extend_from_slice(p);
    }
    Impl::hash_bytes(&buf)
        .as_bytes()
        .try_into()
        .expect("a SHA-256 digest is 32 bytes")
}

/// A string seed, zero-padded to 32 bytes: SPEL's `seed_from_str`.
#[must_use]
pub fn seed_from_str(s: &str) -> [u8; 32] {
    let src = s.as_bytes();
    assert!(src.len() <= 32, "seed string exceeds 32 bytes");
    let mut out = [0u8; 32];
    out[..src.len()].copy_from_slice(src);
    out
}

/// SPEL's multi-seed rule, kept so the seeds read the same as on v0.2.4: one
/// seed is used as is, several are hashed together with SHA-256.
#[must_use]
pub fn combine_seeds(seeds: &[&[u8; 32]]) -> PdaSeed {
    match seeds {
        [one] => PdaSeed::new(**one),
        many => {
            let parts: Vec<&[u8]> = many.iter().map(|s| s.as_slice()).collect();
            PdaSeed::new(sha256(&parts))
        }
    }
}

#[must_use]
pub fn sale_seed(sale_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[sale_id])
}

/// `[sale_id, "holding"]`, the v0.2.4 seeds.
#[must_use]
pub fn holding_seed(sale_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[sale_id, &seed_from_str("holding")])
}

#[must_use]
pub fn sale_account(program: &AccountId, sale_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &sale_seed(sale_id))
}

#[must_use]
pub fn holding_account(program: &AccountId, sale_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &holding_seed(sale_id))
}

// ------------------------------------------------------------------- events

/// `sha256("antumbra_curve::<Name>")[..8]`, the convention `ProgramEvent`
/// documents.
#[must_use]
pub fn event_selector(name: &str) -> [u8; 8] {
    let full = sha256(&[b"antumbra_curve::", name.as_bytes()]);
    full[..8].try_into().expect("8 of 32 bytes")
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SaleCreated {
    pub sale_id: [u8; 32],
    pub creator: AccountId,
    pub holding: AccountId,
    pub collateral: Asset,
    pub vt: u128,
    pub vc: u128,
    pub sale_reserve: u128,
    pub seed_reserve: u128,
    pub fee_rate: u128,
}

/// What the buyer paid and the least it accepted. The tokens it received are
/// priced in `apply`, after the plan has emitted its events, so they are read
/// from the sale state rather than from the event.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BuyPaid {
    pub sale_id: [u8; 32],
    pub buyer: AccountId,
    pub collateral_in: u128,
    pub min_tokens_out: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FeesCollected {
    pub sale_id: [u8; 32],
    pub amount: u128,
    pub fee_treasury: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Withdrawn {
    pub sale_id: [u8; 32],
    pub amount: u128,
    pub destination: AccountId,
}

/// Every event name, in the order the IDL lists them.
pub const EVENT_NAMES: [&str; 4] = ["SaleCreated", "BuyPaid", "FeesCollected", "Withdrawn"];

fn event<T: BorshSerialize>(name: &str, body: &T) -> ProgramEvent {
    ProgramEvent {
        selector: event_selector(name),
        data: borsh::to_vec(body).expect("borsh serialization is infallible"),
    }
}

// --------------------------------------------------------------- arithmetic

fn fee_config(rate: u128) -> antumbra::fees::FeeConfig {
    // The cap lives here rather than in policy, and an over-cap rate is refused
    // by name rather than clamped, because clamping hides a misconfiguration.
    antumbra::fees::FeeConfig::new(rate, antumbra::fees::CAP_PER_SWAP)
        .unwrap_or_else(|_| refuse(E_FEE_RATE_ABOVE_CAP, "fee rate exceeds the 1% cap"))
}

/// The state a creation writes, validated now rather than at the first buy.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn new_sale(
    creator: AccountId,
    treasury: AccountId,
    collateral: &Asset,
    vt: u128,
    vc: u128,
    sale_reserve: u128,
    seed_reserve: u128,
    fee_treasury: AccountId,
    fee_rate: u128,
) -> Sale {
    // Through the audited constructor, so a virtual reserve that cannot serve
    // the sale quantity is refused here rather than discovered by a buyer.
    antumbra::Curve::new(vt, vc, sale_reserve)
        .unwrap_or_else(|_| refuse(E_BAD_SALE, "curve parameters are degenerate"));
    let _ = fee_config(fee_rate);
    let (code, definition) = collateral.code();
    Sale {
        vt,
        vc,
        sale_reserve,
        real_collateral: 0,
        seed_reserve,
        creator,
        treasury,
        fee_treasury,
        fee_rate,
        fees_accrued: 0,
        withdrawn: 0,
        collateral: code,
        collateral_definition: definition,
    }
}

/// What a buy of `collateral_in` does to `s`: `(fee, tokens_out, after)`. The
/// fee comes off before pricing, rounded up, so the constant product sees what
/// the pool actually keeps. The same function `apply` runs; clients call it to
/// quote.
pub fn quote_buy(s: &Sale, collateral_in: u128, min_tokens_out: u128) -> Result<(u128, u128, Sale), u32> {
    let mut curve = s.curve();
    if curve.is_closed() {
        return Err(E_CLOSED);
    }
    let cfg = antumbra::fees::FeeConfig::new(s.fee_rate, antumbra::fees::CAP_PER_SWAP)
        .map_err(|_| E_FEE_RATE_ABOVE_CAP)?;
    let (fee, effective) = antumbra::fees::buy_fee(&cfg, collateral_in).map_err(|_| E_PRICING_REFUSED)?;
    // On refusal the curve is left untouched; the copy below is only reached
    // on success.
    let out = curve.buy(effective, min_tokens_out).map_err(|_| E_PRICING_REFUSED)?;
    let mut after = s.clone();
    after.fees_accrued = s.fees_accrued.checked_add(fee).ok_or(E_TREASURY_OVERFLOW)?;
    after.vt = curve.vt;
    after.vc = curve.vc;
    after.sale_reserve = curve.sale_reserve;
    after.real_collateral = curve.real_collateral;
    Ok((fee, out, after))
}

/// The collateral the creator may withdraw now: everything the curve took in,
/// less what was already withdrawn. Zero until the sale reserve is exhausted.
#[must_use]
pub fn withdrawable(s: &Sale) -> u128 {
    if s.sale_reserve != 0 {
        return 0;
    }
    s.real_collateral.saturating_sub(s.withdrawn)
}

// --------------------------------------------------------------------- plan

fn rows<const N: usize>(input: &PlanInput) -> [AccountMeta; N] {
    <[AccountMeta; N]>::try_from(input.accounts.clone())
        .unwrap_or_else(|_| refuse(E_ACCOUNTS, "wrong number of accounts for this instruction"))
}

fn expect_sale(row: &AccountMeta, me: &AccountId, sale_id: &[u8; 32]) {
    refuse_unless(
        row.account_id == sale_account(me, sale_id) && row.program_account_id == *me,
        E_NOT_ANCHORED,
        "the sale row is not this program's shard of the sale PDA",
    );
}

fn expect_holding(row: &AccountMeta, me: &AccountId, sale_id: &[u8; 32], asset: &Asset) {
    refuse_unless(
        row.account_id == holding_account(me, sale_id),
        E_TREASURY_MISMATCH,
        "the holding row is not this sale's holding PDA",
    );
    expect_asset_row(row, asset);
}

fn expect_asset_row(row: &AccountMeta, asset: &Asset) {
    refuse_unless(
        row.program_account_id == asset.shard_program(),
        E_WRONG_ASSET,
        "this row must select the collateral's shard",
    );
}

fn expect_signer_handle(row: &AccountMeta, me: &AccountId) {
    refuse_unless(row.is_authorized, E_NOT_SIGNED, "the signer row carries no signature");
    refuse_unless(
        row.program_account_id == *me,
        E_ACCOUNTS,
        "a signer handle selects this program's shard",
    );
}

fn transfer_call(asset: &Asset, from: &AccountMeta, to: &AccountMeta, amount: u128) -> ChainedCall {
    match asset {
        Asset::Native => ChainedCall::new(
            NATIVE_TOKEN_PROGRAM_ID,
            vec![ProgramShardSelector::from(from), ProgramShardSelector::from(to)],
            &native_token::Instruction::Transfer { amount },
        ),
        Asset::Token { definition_id } => ChainedCall::new(
            token_program(),
            vec![ProgramShardSelector::from(from), ProgramShardSelector::from(to)],
            &token_core::Instruction::Transfer {
                amount_to_transfer: amount,
                descriptor: TokenDescriptor {
                    definition_id: *definition_id,
                    kind: TokenKind::Fungible,
                },
            },
        ),
    }
}

/// A payout out of the holding: the transfer with the holding's PDA seed
/// attached, which is the only thing that authorises a debit of it.
fn payout(asset: &Asset, holding: &AccountMeta, sale_id: &[u8; 32], to: &AccountMeta, amount: u128) -> ChainedCall {
    match asset {
        Asset::Native => native_token::custody_transfer(holding.account_id, holding_seed(sale_id), to.account_id, amount),
        Asset::Token { .. } => transfer_call(asset, holding, to, amount).with_pda_seeds(vec![holding_seed(sale_id)]),
    }
}

/// The plan entry point.
pub fn plan(input: &PlanInput, ix: Instruction) -> Plan {
    let me = input.self_account_id;
    let mut plan = Plan::new(input);
    match ix {
        Instruction::CreateSale {
            sale_id,
            collateral,
            vt,
            vc,
            sale_reserve,
            seed_reserve,
            fee_treasury,
            fee_rate,
        } => {
            let [sale, creator] = rows::<2>(input);
            expect_sale(&sale, &me, &sale_id);
            expect_signer_handle(&creator, &me);
            let holding = holding_account(&me, &sale_id);
            let state = new_sale(
                creator.account_id,
                holding,
                &collateral,
                vt,
                vc,
                sale_reserve,
                seed_reserve,
                fee_treasury,
                fee_rate,
            );
            plan.effect(&sale, &Effect::Create(state));
            plan.event(event(
                "SaleCreated",
                &SaleCreated {
                    sale_id,
                    creator: creator.account_id,
                    holding,
                    collateral,
                    vt,
                    vc,
                    sale_reserve,
                    seed_reserve,
                    fee_rate,
                },
            ));
        }
        Instruction::ExecuteBuy {
            sale_id,
            collateral,
            collateral_in,
            min_tokens_out,
        } => {
            let [sale, buyer, holding] = rows::<3>(input);
            expect_sale(&sale, &me, &sale_id);
            expect_asset_row(&buyer, &collateral);
            refuse_unless(buyer.is_authorized, E_NOT_SIGNED, "the buyer must sign");
            expect_holding(&holding, &me, &sale_id, &collateral);
            refuse_unless(collateral_in > 0, E_PRICING_REFUSED, "a buy pays a positive amount");
            plan.effect(
                &sale,
                &Effect::Buy {
                    holding: holding.account_id,
                    collateral,
                    collateral_in,
                    min_tokens_out,
                },
            );
            // ONE transfer of the whole input into the holding; the fee is
            // accrued in the sale state and swept by collect_fees, as on v0.2.4.
            plan.call(transfer_call(&collateral, &buyer, &holding, collateral_in));
            plan.event(event(
                "BuyPaid",
                &BuyPaid {
                    sale_id,
                    buyer: buyer.account_id,
                    collateral_in,
                    min_tokens_out,
                },
            ));
        }
        Instruction::CollectFees {
            sale_id,
            collateral,
            amount,
        } => {
            let [sale, holding, fee_treasury] = rows::<3>(input);
            expect_sale(&sale, &me, &sale_id);
            expect_holding(&holding, &me, &sale_id, &collateral);
            expect_asset_row(&fee_treasury, &collateral);
            refuse_unless(amount > 0, E_PRICING_REFUSED, "no fees accrued");
            plan.effect(
                &sale,
                &Effect::CollectFees {
                    holding: holding.account_id,
                    fee_treasury: fee_treasury.account_id,
                    collateral,
                    amount,
                },
            );
            plan.call(payout(&collateral, &holding, &sale_id, &fee_treasury, amount));
            plan.event(event(
                "FeesCollected",
                &FeesCollected {
                    sale_id,
                    amount,
                    fee_treasury: fee_treasury.account_id,
                },
            ));
        }
        Instruction::Withdraw {
            sale_id,
            collateral,
            amount,
        } => {
            let [sale, holding, destination, creator] = rows::<4>(input);
            expect_sale(&sale, &me, &sale_id);
            expect_holding(&holding, &me, &sale_id, &collateral);
            expect_asset_row(&destination, &collateral);
            expect_signer_handle(&creator, &me);
            refuse_unless(amount > 0, E_PRICING_REFUSED, "nothing to withdraw");
            plan.effect(
                &sale,
                &Effect::Withdraw {
                    creator: creator.account_id,
                    holding: holding.account_id,
                    collateral,
                    amount,
                },
            );
            plan.call(payout(&collateral, &holding, &sale_id, &destination, amount));
            plan.event(event(
                "Withdrawn",
                &Withdrawn {
                    sale_id,
                    amount,
                    destination: destination.account_id,
                },
            ));
        }
    }
    plan
}

// -------------------------------------------------------------------- apply

/// Decode a stored sale, refusing an empty shard: nothing was created there.
#[must_use]
pub fn load(pre: &[u8]) -> Sale {
    refuse_unless(!pre.is_empty(), E_NOT_ANCHORED, "no sale is committed at this id");
    Sale::try_from_slice(pre).unwrap_or_else(|_| refuse(E_BAD_SALE, "sale failed to deserialize"))
}

fn store(s: &Sale) -> Vec<u8> {
    borsh::to_vec(s).expect("borsh serialization is infallible")
}

fn expect_stored(s: &Sale, holding: &AccountId, asset: &Asset) {
    refuse_unless(
        s.treasury == *holding,
        E_TREASURY_MISMATCH,
        "holding is not the account this sale was created with",
    );
    refuse_unless(
        asset.code() == (s.collateral, s.collateral_definition),
        E_WRONG_ASSET,
        "the instruction names a different collateral from the sale's",
    );
}

/// The apply entry point: one effect against one sale shard.
#[must_use]
pub fn apply(effect: Effect, pre: &[u8]) -> Option<Vec<u8>> {
    match effect {
        Effect::Create(state) => {
            // A sale id is used once. Refusing any existing bytes, not only
            // different ones, keeps a replayed creation from resetting a sale.
            refuse_unless(pre.is_empty(), E_EXISTS, "a sale already exists at this id");
            Some(write_once(pre, store(&state)))
        }
        Effect::Buy {
            holding,
            collateral,
            collateral_in,
            min_tokens_out,
        } => {
            let s = load(pre);
            expect_stored(&s, &holding, &collateral);
            match quote_buy(&s, collateral_in, min_tokens_out) {
                Ok((_, _, after)) => Some(store(&after)),
                Err(E_CLOSED) => refuse(E_CLOSED, "the sale reserve is exhausted; this sale is closed"),
                Err(E_FEE_RATE_ABOVE_CAP) => refuse(E_FEE_RATE_ABOVE_CAP, "stored fee rate exceeds the cap"),
                Err(E_TREASURY_OVERFLOW) => refuse(E_TREASURY_OVERFLOW, "accrued fees overflow"),
                Err(code) => refuse(code, "buy refused: slippage, size or reserve"),
            }
        }
        Effect::CollectFees {
            holding,
            fee_treasury,
            collateral,
            amount,
        } => {
            let mut s = load(pre);
            expect_stored(&s, &holding, &collateral);
            refuse_unless(
                s.fee_treasury == fee_treasury,
                E_FEE_TREASURY_MISMATCH,
                "fee treasury is not the account this sale was created with",
            );
            refuse_unless(s.fees_accrued > 0, E_PRICING_REFUSED, "no fees accrued");
            refuse_unless(
                amount == s.fees_accrued,
                E_AMOUNT,
                "the amount is not exactly the accrued fee",
            );
            // Zeroed in the same transaction as the payout, so a repeated
            // sweep finds nothing rather than paying twice.
            s.fees_accrued = 0;
            Some(store(&s))
        }
        Effect::Withdraw {
            creator,
            holding,
            collateral,
            amount,
        } => {
            let mut s = load(pre);
            expect_stored(&s, &holding, &collateral);
            refuse_unless(s.creator == creator, E_NOT_CREATOR, "signer is not the creator of this sale");
            // F4's close condition: a creator who could withdraw mid-sale
            // would be taking collateral that still backs unsold tokens.
            refuse_unless(
                s.sale_reserve == 0,
                E_CLOSED,
                "the sale has not closed: tokens remain unsold",
            );
            let payable = withdrawable(&s);
            refuse_unless(payable > 0, E_PRICING_REFUSED, "nothing to withdraw");
            // The accrued fee is the protocol's and is never part of this.
            refuse_unless(
                amount == payable,
                E_AMOUNT,
                "the amount is not exactly the collateral raised and not yet withdrawn",
            );
            s.withdrawn = s
                .withdrawn
                .checked_add(amount)
                .unwrap_or_else(|| refuse(E_TREASURY_OVERFLOW, "withdrawn overflows"));
            Some(store(&s))
        }
    }
}

/// The account rows each instruction takes, in order, for clients. The plan
/// checks exactly these; building them here keeps the CLI, the tests and the
/// program from disagreeing about an order.
pub mod rows {
    use super::*;

    fn sel(account: AccountId, program: AccountId) -> ProgramShardSelector {
        ProgramShardSelector::new(account, program)
    }

    #[must_use]
    pub fn create(program: &AccountId, sale_id: &[u8; 32], creator: AccountId) -> Vec<ProgramShardSelector> {
        vec![sel(sale_account(program, sale_id), *program), sel(creator, *program)]
    }

    #[must_use]
    pub fn buy(program: &AccountId, sale_id: &[u8; 32], asset: &Asset, buyer: AccountId) -> Vec<ProgramShardSelector> {
        vec![
            sel(sale_account(program, sale_id), *program),
            sel(buyer, asset.shard_program()),
            sel(holding_account(program, sale_id), asset.shard_program()),
        ]
    }

    #[must_use]
    pub fn collect_fees(
        program: &AccountId,
        sale_id: &[u8; 32],
        asset: &Asset,
        fee_treasury: AccountId,
    ) -> Vec<ProgramShardSelector> {
        vec![
            sel(sale_account(program, sale_id), *program),
            sel(holding_account(program, sale_id), asset.shard_program()),
            sel(fee_treasury, asset.shard_program()),
        ]
    }

    #[must_use]
    pub fn withdraw(
        program: &AccountId,
        sale_id: &[u8; 32],
        asset: &Asset,
        destination: AccountId,
        creator: AccountId,
    ) -> Vec<ProgramShardSelector> {
        vec![
            sel(sale_account(program, sale_id), *program),
            sel(holding_account(program, sale_id), asset.shard_program()),
            sel(destination, asset.shard_program()),
            sel(creator, *program),
        ]
    }
}

#[cfg(test)]
mod tests;
