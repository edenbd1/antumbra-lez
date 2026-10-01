//! antumbra_lbp on LEZ v0.3: the wire types and the whole program logic.
//!
//! The guest binary is two lines that hand [`plan`] and [`apply`] to
//! `lee_core::program::run_program`. Everything else lives here so the CLI and
//! the host tests use the same types, seeds and checks as the program.
//!
//! WHAT THE PROGRAM DOES
//!
//! A liquidity bootstrapping pool (RFP-016). `create_pool` writes the reserves
//! and a weight schedule; `execute_buy` takes the buyer's collateral into the
//! pool's holding and prices it with `antumbra::binfixed::weighted_buy` at the
//! weight the schedule gives for the buy's time; `withdraw` pays the creator
//! the collateral raised, net of the at-close fee, once the schedule has ended;
//! `set_paused` halts buying. No current weight is stored: it is recomputed
//! from the schedule on every buy, so there is no poke and no stale weight.
//!
//! THE SPLIT v0.3 FORCES
//!
//! A v0.3 program runs twice per call. `plan` sees account ids, signer flags
//! and which shard each account row selects, never data; `apply` sees one
//! shard's bytes and may replace them if the shard is its own. So the plan
//! moves exactly `collateral_in` from the buyer into the holding, and `apply`
//! prices it against the stored pool, refusing on slippage, a pause or a time
//! the pool has moved past; a refusal fails the whole transaction, payment
//! included. A withdrawal names the amount and the fee, the plan pays exactly
//! those, and `apply` refuses unless they are what the stored pool says.
//!
//! WHERE TIME COMES FROM
//!
//! v0.2.4 took `now` from the caller and only refused one earlier than a time
//! the pool had already seen. On v0.3 a plan bounds the block timestamp at
//! which the transaction is valid and the sequencer enforces it: a buy at
//! `now` is valid only in `[now, now + BUY_WINDOW_MS)`, so a buyer cannot price
//! at a future weight, nor at one more than two minutes old. A withdrawal at
//! `now` is valid from `now` on, and `apply` requires `now >= t_end`, so the
//! creator cannot withdraw before the schedule has ended in block time.
//!
//! WHERE THE MONEY SITS
//!
//! v0.3 has no program-owned accounts. A pool's holding is a PDA of this
//! program, `[pool_id, "holding"]`, whose native or token shard holds the
//! collateral; only a chained call carrying its PDA seed, which only this
//! program can attach, debits it.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    native_token::{self, NATIVE_TOKEN_PROGRAM_ID},
    program::{write_once, AccountMeta, ChainedCall, PdaSeed, Plan, PlanInput, ProgramEvent, TimestampValidityWindow},
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use token_core::{TokenDescriptor, TokenKind};

pub mod errors;
use errors::*;

/// How long after the time it names a buy may still land.
pub const BUY_WINDOW_MS: u64 = 120_000;

/// The token program's account id on every LEZ v0.3 chain, derived from its
/// builtin name rather than read from anything the caller passes.
#[must_use]
pub fn token_program() -> AccountId {
    token_core::token_account_id()
}

/// What a pool takes as collateral.
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

    /// The asset a stored pool records.
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
    /// Open a pool with a weight schedule. `w_start == w_end` is a fixed-weight
    /// pool through the same code path. Weights are 18-decimal fractions of
    /// `antumbra::weighted::ONE`; times are milliseconds, the block timestamp's
    /// unit.
    ///
    /// Accounts: `[pool (this program's shard), creator (signer handle)]`.
    CreatePool {
        pool_id: [u8; 32],
        collateral: Asset,
        reserve_token: u128,
        reserve_collateral: u128,
        w_start: u128,
        w_end: u128,
        t_start: u64,
        t_end: u64,
        fee_treasury: AccountId,
        /// Millionths; at most `antumbra::fees::CAP_AT_CLOSE` (5%).
        fee_rate: u128,
    },
    /// Pay `collateral_in` into the holding and buy at the weight for `now`.
    /// Valid only while the block timestamp is in `[now, now + BUY_WINDOW_MS)`.
    ///
    /// Accounts: `[pool, buyer (asset shard, signs), holding (asset shard)]`.
    ExecuteBuy {
        pool_id: [u8; 32],
        collateral: Asset,
        now: u64,
        collateral_in: u128,
        min_tokens_out: u128,
    },
    /// After `t_end`, pay `amount - fee` to `destination` and `fee` to the fee
    /// treasury fixed at creation. `amount` must be the collateral raised and
    /// not yet withdrawn, `fee` exactly `close_fee(fee_rate, amount)`. Valid
    /// once the block timestamp reaches `now`.
    ///
    /// Accounts: `[pool, holding (asset shard), destination (asset shard),
    /// fee_treasury (asset shard), creator (signer handle)]`.
    Withdraw {
        pool_id: [u8; 32],
        collateral: Asset,
        now: u64,
        amount: u128,
        fee: u128,
    },
    /// Emergency stop: buying halts while `paused != 0`. The weight schedule
    /// does not, because it is a function of the clock.
    ///
    /// Accounts: `[pool, creator (signer handle)]`.
    SetPaused { pool_id: [u8; 32], paused: u8 },
}

/// On-chain pool state, this program's shard of the pool PDA. The first
/// twelve fields are the v0.2.4 layout unchanged; the last four are new on
/// v0.3. There is no current weight: the schedule is stored, the weight is
/// derived.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Pool {
    pub reserve_token: u128,
    pub reserve_collateral: u128,
    pub w_start: u128,
    pub w_end: u128,
    pub t_start: u64,
    pub t_end: u64,
    pub last_seen: u64,
    pub paused: u8,
    pub creator: AccountId,
    /// The holding PDA, fixed at creation, so a buy cannot name another.
    pub treasury: AccountId,
    /// Where the at-close fee goes.
    pub fee_treasury: AccountId,
    /// Millionths, capped in the program at 5%.
    pub fee_rate: u128,
    /// Collateral paid in by buys. v0.2.4 read the holding's balance at close;
    /// a v0.3 apply cannot read another program's shard, so the pool keeps the
    /// count itself.
    pub raised: u128,
    /// Of `raised`, what a withdrawal already paid out (fee included).
    pub withdrawn: u128,
    /// 0 = native, 1 = token.
    pub collateral: u8,
    pub collateral_definition: [u8; 32],
}

impl Pool {
    #[must_use]
    pub fn asset(&self) -> Asset {
        Asset::of(self.collateral, self.collateral_definition)
    }
}

/// What `plan` asks `apply` to do to a pool shard.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    Create(Pool),
    Buy {
        holding: AccountId,
        collateral: Asset,
        now: u64,
        collateral_in: u128,
        min_tokens_out: u128,
    },
    Withdraw {
        creator: AccountId,
        holding: AccountId,
        fee_treasury: AccountId,
        collateral: Asset,
        now: u64,
        amount: u128,
        fee: u128,
    },
    SetPaused {
        creator: AccountId,
        paused: u8,
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
pub fn pool_seed(pool_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[pool_id])
}

/// `[pool_id, "holding"]`, the v0.2.4 seeds.
#[must_use]
pub fn holding_seed(pool_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[pool_id, &seed_from_str("holding")])
}

#[must_use]
pub fn pool_account(program: &AccountId, pool_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &pool_seed(pool_id))
}

#[must_use]
pub fn holding_account(program: &AccountId, pool_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &holding_seed(pool_id))
}

// ------------------------------------------------------------------- events

/// `sha256("antumbra_lbp::<Name>")[..8]`, the convention `ProgramEvent`
/// documents.
#[must_use]
pub fn event_selector(name: &str) -> [u8; 8] {
    let full = sha256(&[b"antumbra_lbp::", name.as_bytes()]);
    full[..8].try_into().expect("8 of 32 bytes")
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PoolCreated {
    pub pool_id: [u8; 32],
    pub creator: AccountId,
    pub holding: AccountId,
    pub collateral: Asset,
    pub reserve_token: u128,
    pub reserve_collateral: u128,
    pub w_start: u128,
    pub w_end: u128,
    pub t_start: u64,
    pub t_end: u64,
    pub fee_rate: u128,
}

/// What the buyer paid, at what time, and the least it accepted. The tokens
/// are priced in `apply`, after the plan has emitted its events, so they are
/// read from the pool state rather than from the event.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BuyPaid {
    pub pool_id: [u8; 32],
    pub buyer: AccountId,
    pub now: u64,
    pub collateral_in: u128,
    pub min_tokens_out: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Withdrawn {
    pub pool_id: [u8; 32],
    pub to_creator: u128,
    pub fee: u128,
    pub destination: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PauseSet {
    pub pool_id: [u8; 32],
    pub paused: u8,
}

/// Every event name, in the order the IDL lists them.
pub const EVENT_NAMES: [&str; 4] = ["PoolCreated", "BuyPaid", "Withdrawn", "PauseSet"];

fn event<T: BorshSerialize>(name: &str, body: &T) -> ProgramEvent {
    ProgramEvent {
        selector: event_selector(name),
        data: borsh::to_vec(body).expect("borsh serialization is infallible"),
    }
}

// --------------------------------------------------------------- arithmetic

fn fee_config(rate: u128) -> Result<antumbra::fees::FeeConfig, u32> {
    antumbra::fees::FeeConfig::new(rate, antumbra::fees::CAP_AT_CLOSE).map_err(|_| E_BAD_POOL)
}

/// The state a creation writes, validated now rather than at the first buy.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn new_pool(
    creator: AccountId,
    treasury: AccountId,
    collateral: &Asset,
    reserve_token: u128,
    reserve_collateral: u128,
    w_start: u128,
    w_end: u128,
    t_start: u64,
    t_end: u64,
    fee_treasury: AccountId,
    fee_rate: u128,
) -> Pool {
    fee_config(fee_rate).unwrap_or_else(|_| refuse(E_BAD_POOL, "fee rate exceeds the 5% cap"));
    // weight_at refuses an inverted or zero-length schedule, so ask it now.
    antumbra::weighted::weight_at(w_start, w_end, t_start, t_end, t_start)
        .unwrap_or_else(|_| refuse(E_BAD_POOL, "weight schedule is degenerate"));
    refuse_unless(reserve_token != 0 && reserve_collateral != 0, E_BAD_POOL, "a reserve is zero");
    let (code, definition) = collateral.code();
    Pool {
        reserve_token,
        reserve_collateral,
        w_start,
        w_end,
        t_start,
        t_end,
        last_seen: t_start,
        paused: 0,
        creator,
        treasury,
        fee_treasury,
        fee_rate,
        raised: 0,
        withdrawn: 0,
        collateral: code,
        collateral_definition: definition,
    }
}

/// The token weight the schedule gives at `now`. Derived, never stored.
pub fn weight(p: &Pool, now: u64) -> Result<u128, u32> {
    antumbra::weighted::weight_at(p.w_start, p.w_end, p.t_start, p.t_end, now).map_err(|_| E_BAD_POOL)
}

/// What a buy of `collateral_in` at `now` does to `p`: `(tokens_out, after)`.
/// The same function `apply` runs; clients call it to quote.
pub fn quote_buy(p: &Pool, now: u64, collateral_in: u128, min_tokens_out: u128) -> Result<(u128, Pool), u32> {
    if p.paused != 0 {
        return Err(E_PAUSED);
    }
    // The weight schedule is monotone in time: a buy at a time earlier than one
    // already honoured would re-price at a weight the pool has moved past.
    if now < p.last_seen {
        return Err(E_TIME_WENT_BACKWARDS);
    }
    let w_token = weight(p, now)?;
    let w_collateral = antumbra::weighted::ONE.checked_sub(w_token).ok_or(E_BAD_POOL)?;
    let out = antumbra::binfixed::weighted_buy(p.reserve_token, p.reserve_collateral, collateral_in, w_token, w_collateral)
        .map_err(|_| E_PRICING_REFUSED)?;
    // Checked before any field moves, so a refused buy leaves the pool as it was.
    if out < min_tokens_out {
        return Err(E_SLIPPAGE);
    }
    let mut after = p.clone();
    after.reserve_token = p.reserve_token.checked_sub(out).ok_or(E_PRICING_REFUSED)?;
    after.reserve_collateral = p.reserve_collateral.checked_add(collateral_in).ok_or(E_PRICING_REFUSED)?;
    after.raised = p.raised.checked_add(collateral_in).ok_or(E_TREASURY_OVERFLOW)?;
    after.last_seen = now;
    Ok((out, after))
}

/// What a withdrawal pays now: `(amount, fee, to_creator)`. `amount` is the
/// collateral raised and not yet withdrawn; the fee is RFP-016's at-close fee,
/// rounded up against the creator.
pub fn withdrawal(p: &Pool) -> Result<(u128, u128, u128), u32> {
    let amount = p.raised.checked_sub(p.withdrawn).ok_or(E_TREASURY_OVERFLOW)?;
    if amount == 0 {
        return Err(E_PRICING_REFUSED);
    }
    let cfg = fee_config(p.fee_rate)?;
    let (fee, to_creator) = antumbra::fees::close_fee(&cfg, amount).map_err(|_| E_PRICING_REFUSED)?;
    Ok((amount, fee, to_creator))
}

// --------------------------------------------------------------------- plan

fn rows<const N: usize>(input: &PlanInput) -> [AccountMeta; N] {
    <[AccountMeta; N]>::try_from(input.accounts.clone())
        .unwrap_or_else(|_| refuse(E_ACCOUNTS, "wrong number of accounts for this instruction"))
}

fn expect_pool(row: &AccountMeta, me: &AccountId, pool_id: &[u8; 32]) {
    refuse_unless(
        row.account_id == pool_account(me, pool_id) && row.program_account_id == *me,
        E_NOT_ANCHORED,
        "the pool row is not this program's shard of the pool PDA",
    );
}

fn expect_holding(row: &AccountMeta, me: &AccountId, pool_id: &[u8; 32], asset: &Asset) {
    refuse_unless(
        row.account_id == holding_account(me, pool_id),
        E_TREASURY_MISMATCH,
        "the holding row is not this pool's holding PDA",
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
fn payout(asset: &Asset, holding: &AccountMeta, pool_id: &[u8; 32], to: &AccountMeta, amount: u128) -> ChainedCall {
    match asset {
        Asset::Native => native_token::custody_transfer(holding.account_id, holding_seed(pool_id), to.account_id, amount),
        Asset::Token { .. } => transfer_call(asset, holding, to, amount).with_pda_seeds(vec![holding_seed(pool_id)]),
    }
}

/// The plan entry point.
pub fn plan(input: &PlanInput, ix: Instruction) -> Plan {
    let me = input.self_account_id;
    let mut plan = Plan::new(input);
    match ix {
        Instruction::CreatePool {
            pool_id,
            collateral,
            reserve_token,
            reserve_collateral,
            w_start,
            w_end,
            t_start,
            t_end,
            fee_treasury,
            fee_rate,
        } => {
            let [pool, creator] = rows::<2>(input);
            expect_pool(&pool, &me, &pool_id);
            expect_signer_handle(&creator, &me);
            let holding = holding_account(&me, &pool_id);
            let state = new_pool(
                creator.account_id,
                holding,
                &collateral,
                reserve_token,
                reserve_collateral,
                w_start,
                w_end,
                t_start,
                t_end,
                fee_treasury,
                fee_rate,
            );
            plan.effect(&pool, &Effect::Create(state));
            plan.event(event(
                "PoolCreated",
                &PoolCreated {
                    pool_id,
                    creator: creator.account_id,
                    holding,
                    collateral,
                    reserve_token,
                    reserve_collateral,
                    w_start,
                    w_end,
                    t_start,
                    t_end,
                    fee_rate,
                },
            ));
        }
        Instruction::ExecuteBuy {
            pool_id,
            collateral,
            now,
            collateral_in,
            min_tokens_out,
        } => {
            let [pool, buyer, holding] = rows::<3>(input);
            expect_pool(&pool, &me, &pool_id);
            expect_asset_row(&buyer, &collateral);
            refuse_unless(buyer.is_authorized, E_NOT_SIGNED, "the buyer must sign");
            expect_holding(&holding, &me, &pool_id, &collateral);
            refuse_unless(collateral_in > 0, E_PRICING_REFUSED, "a buy pays a positive amount");
            let until = now
                .checked_add(BUY_WINDOW_MS)
                .unwrap_or_else(|| refuse(E_TIME_WENT_BACKWARDS, "buy time overflows"));
            plan.timestamp_window(
                TimestampValidityWindow::try_from(now..until).expect("a positive window is not empty"),
            );
            plan.effect(
                &pool,
                &Effect::Buy {
                    holding: holding.account_id,
                    collateral,
                    now,
                    collateral_in,
                    min_tokens_out,
                },
            );
            plan.call(transfer_call(&collateral, &buyer, &holding, collateral_in));
            plan.event(event(
                "BuyPaid",
                &BuyPaid {
                    pool_id,
                    buyer: buyer.account_id,
                    now,
                    collateral_in,
                    min_tokens_out,
                },
            ));
        }
        Instruction::Withdraw {
            pool_id,
            collateral,
            now,
            amount,
            fee,
        } => {
            let [pool, holding, destination, fee_treasury, creator] = rows::<5>(input);
            expect_pool(&pool, &me, &pool_id);
            expect_holding(&holding, &me, &pool_id, &collateral);
            expect_asset_row(&destination, &collateral);
            expect_asset_row(&fee_treasury, &collateral);
            expect_signer_handle(&creator, &me);
            refuse_unless(amount > 0, E_PRICING_REFUSED, "nothing to withdraw");
            let to_creator = amount
                .checked_sub(fee)
                .unwrap_or_else(|| refuse(E_AMOUNT, "the fee exceeds the amount"));
            plan.timestamp_window(now..);
            plan.effect(
                &pool,
                &Effect::Withdraw {
                    creator: creator.account_id,
                    holding: holding.account_id,
                    fee_treasury: fee_treasury.account_id,
                    collateral,
                    now,
                    amount,
                    fee,
                },
            );
            if to_creator > 0 {
                plan.call(payout(&collateral, &holding, &pool_id, &destination, to_creator));
            }
            if fee > 0 {
                plan.call(payout(&collateral, &holding, &pool_id, &fee_treasury, fee));
            }
            plan.event(event(
                "Withdrawn",
                &Withdrawn {
                    pool_id,
                    to_creator,
                    fee,
                    destination: destination.account_id,
                },
            ));
        }
        Instruction::SetPaused { pool_id, paused } => {
            let [pool, creator] = rows::<2>(input);
            expect_pool(&pool, &me, &pool_id);
            expect_signer_handle(&creator, &me);
            plan.effect(
                &pool,
                &Effect::SetPaused {
                    creator: creator.account_id,
                    paused,
                },
            );
            plan.event(event("PauseSet", &PauseSet { pool_id, paused }));
        }
    }
    plan
}

// -------------------------------------------------------------------- apply

/// Decode a stored pool, refusing an empty shard: nothing was created there.
#[must_use]
pub fn load(pre: &[u8]) -> Pool {
    refuse_unless(!pre.is_empty(), E_NOT_ANCHORED, "no pool is committed at this id");
    Pool::try_from_slice(pre).unwrap_or_else(|_| refuse(E_BAD_POOL, "pool failed to deserialize"))
}

fn store(p: &Pool) -> Vec<u8> {
    borsh::to_vec(p).expect("borsh serialization is infallible")
}

fn expect_stored(p: &Pool, holding: &AccountId, asset: &Asset) {
    refuse_unless(
        p.treasury == *holding,
        E_TREASURY_MISMATCH,
        "holding is not the account this pool was created with",
    );
    refuse_unless(
        asset.code() == (p.collateral, p.collateral_definition),
        E_WRONG_ASSET,
        "the instruction names a different collateral from the pool's",
    );
}

/// The apply entry point: one effect against one pool shard.
#[must_use]
pub fn apply(effect: Effect, pre: &[u8]) -> Option<Vec<u8>> {
    match effect {
        Effect::Create(state) => {
            // A pool id is used once. Refusing any existing bytes, not only
            // different ones, keeps a replayed creation from resetting a pool.
            refuse_unless(pre.is_empty(), E_EXISTS, "a pool already exists at this id");
            Some(write_once(pre, store(&state)))
        }
        Effect::Buy {
            holding,
            collateral,
            now,
            collateral_in,
            min_tokens_out,
        } => {
            let p = load(pre);
            expect_stored(&p, &holding, &collateral);
            match quote_buy(&p, now, collateral_in, min_tokens_out) {
                Ok((_, after)) => Some(store(&after)),
                Err(E_PAUSED) => refuse(E_PAUSED, "buying is paused"),
                Err(E_TIME_WENT_BACKWARDS) => refuse(
                    E_TIME_WENT_BACKWARDS,
                    "now is earlier than a timestamp this pool has already seen",
                ),
                Err(E_SLIPPAGE) => refuse(E_SLIPPAGE, "execution price is worse than the minimum accepted"),
                Err(E_BAD_POOL) => refuse(E_BAD_POOL, "weight schedule is degenerate"),
                Err(code) => refuse(code, "buy refused: size or reserve"),
            }
        }
        Effect::Withdraw {
            creator,
            holding,
            fee_treasury,
            collateral,
            now,
            amount,
            fee,
        } => {
            let mut p = load(pre);
            expect_stored(&p, &holding, &collateral);
            refuse_unless(p.creator == creator, E_NOT_CREATOR, "signer is not the creator");
            refuse_unless(
                p.fee_treasury == fee_treasury,
                E_TREASURY_MISMATCH,
                "fee treasury is not the account this pool was created with",
            );
            // A creator withdrawing mid-sale would be taking collateral that
            // still backs a pool people are trading against.
            refuse_unless(now >= p.t_end, E_NOT_ENDED, "the sale has not reached its end timestamp");
            let (owed, owed_fee, _) = withdrawal(&p).unwrap_or_else(|code| refuse(code, "nothing to withdraw"));
            refuse_unless(
                amount == owed,
                E_AMOUNT,
                "the amount is not exactly the collateral raised and not yet withdrawn",
            );
            refuse_unless(fee == owed_fee, E_AMOUNT, "the fee is not exactly the at-close fee on the amount");
            p.withdrawn = p
                .withdrawn
                .checked_add(amount)
                .unwrap_or_else(|| refuse(E_TREASURY_OVERFLOW, "withdrawn overflows"));
            p.last_seen = p.last_seen.max(now);
            Some(store(&p))
        }
        Effect::SetPaused { creator, paused } => {
            let mut p = load(pre);
            refuse_unless(p.creator == creator, E_NOT_CREATOR, "signer is not the creator");
            p.paused = paused;
            Some(store(&p))
        }
    }
}

/// The account rows each instruction takes, in order, for clients.
pub mod rows {
    use super::*;

    fn sel(account: AccountId, program: AccountId) -> ProgramShardSelector {
        ProgramShardSelector::new(account, program)
    }

    /// `[pool, signer handle]`: create_pool and set_paused.
    #[must_use]
    pub fn handle(program: &AccountId, pool_id: &[u8; 32], signer: AccountId) -> Vec<ProgramShardSelector> {
        vec![sel(pool_account(program, pool_id), *program), sel(signer, *program)]
    }

    #[must_use]
    pub fn buy(program: &AccountId, pool_id: &[u8; 32], asset: &Asset, buyer: AccountId) -> Vec<ProgramShardSelector> {
        vec![
            sel(pool_account(program, pool_id), *program),
            sel(buyer, asset.shard_program()),
            sel(holding_account(program, pool_id), asset.shard_program()),
        ]
    }

    #[must_use]
    pub fn withdraw(
        program: &AccountId,
        pool_id: &[u8; 32],
        asset: &Asset,
        destination: AccountId,
        fee_treasury: AccountId,
        creator: AccountId,
    ) -> Vec<ProgramShardSelector> {
        vec![
            sel(pool_account(program, pool_id), *program),
            sel(holding_account(program, pool_id), asset.shard_program()),
            sel(destination, asset.shard_program()),
            sel(fee_treasury, asset.shard_program()),
            sel(creator, *program),
        ]
    }
}

#[cfg(test)]
mod tests;
