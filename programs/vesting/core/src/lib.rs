//! antumbra_vesting on LEZ v0.3: the wire types and the whole program logic.
//!
//! The guest binary is two lines that hand [`plan`] and [`apply`] to
//! `lee_core::program::run_program`. Everything else lives here so the CLI and
//! the host tests use the same types, seeds and checks as the program.
//!
//! THE SPLIT v0.3 FORCES
//!
//! A v0.3 program runs twice per call. `plan` sees account ids, signer flags
//! and which shard each account row selects, never data. It decides the
//! effects, the chained calls, the validity windows and the events. Then each
//! effect is applied by the same program, one shard at a time: `apply` sees
//! that shard's current bytes and may replace them if the shard is its own.
//!
//! So nothing stored can be read while the payout is being planned. The claim
//! therefore carries the amount and the time the caller wants, the plan pays
//! exactly that amount out of the escrow, and `apply` checks the amount against
//! what the stored schedule has vested by that time. A claim for more than has
//! vested panics in `apply`, which fails the whole transaction, payout
//! included. The same pattern covers every check against stored state: the
//! signer's id travels in the effect and `apply` compares it with the field.
//!
//! WHERE TIME COMES FROM
//!
//! A plan cannot read the clock either. It can bound the block timestamp at
//! which the transaction is valid, and the sequencer enforces that bound. A
//! claim at time `t` is valid from `t` onward, so a caller can only claim
//! against a time that has already happened. A cancellation at `t` is valid
//! only in `[t, t + CANCEL_WINDOW_MS)`, so an authority cannot freeze accrual
//! more than that far in the past.
//!
//! WHERE THE MONEY SITS
//!
//! v0.3 has no program-owned accounts. Native balance is the shard at
//! `[0; 32]` of any account, and a token balance is the token program's shard.
//! A schedule's escrow is a PDA of this program, `[schedule_id, "holding"]`
//! (a batch shares `[batch_id, "holding"]`); nobody holds a key for it, and
//! the only way to debit it is a chained call carrying its PDA seed, which only
//! this program can attach.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    native_token::{self, NATIVE_TOKEN_PROGRAM_ID},
    program::{AccountMeta, ChainedCall, PdaSeed, Plan, PlanInput, ProgramEvent},
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use token_core::{TokenDescriptor, TokenKind};

pub mod errors;
use errors::*;

/// How far past the time it names a cancellation may still land.
pub const CANCEL_WINDOW_MS: u64 = 120_000;

/// Milestone schedules keep one bit per tranche in a `u64`.
pub const MAX_TRANCHES: u32 = 64;

/// The token program's account id on every LEZ v0.3 chain, derived from its
/// builtin name rather than read from anything the caller passes, so a payout
/// can never be routed through a lookalike program.
#[must_use]
pub fn token_program() -> AccountId {
    token_core::token_account_id()
}

/// What a schedule escrows.
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
}

/// The terms a creator chooses. `total` is per schedule, also in a batch.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Terms {
    /// 0 = cliff + linear, 1 = linear, 2 = milestones.
    pub kind: u8,
    /// Milliseconds since the epoch, the unit of the block timestamp.
    pub start: u64,
    pub cliff: u64,
    pub end: u64,
    pub total: u128,
    /// Equal tranches of a milestone schedule; zero for the time-based kinds.
    pub tranches: u32,
    pub cancelable: bool,
    pub transferable: bool,
    /// Zero means the creator.
    pub cancel_authority: AccountId,
    /// Zero means the creator.
    pub milestone_authority: AccountId,
    /// The account whose native or token shard a cancellation refunds into.
    pub refund_to: AccountId,
}

/// The program's instruction. The variant order is the ABI.
///
/// A "signer handle" is a row naming the signing account with this program's
/// own shard selected. The program never writes it; the row exists so the
/// signature marks the account authorised, and so the same account can also
/// appear with its native or token shard as a destination.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Create one schedule and fund its escrow with exactly `terms.total` in the
    /// same transaction.
    ///
    /// Accounts: `[schedule (this program's shard), holding (asset shard),
    /// creator (asset shard, signs)]`.
    CreateSchedule {
        schedule_id: [u8; 32],
        beneficiary: AccountId,
        asset: Asset,
        terms: Terms,
    },
    /// Create one schedule per beneficiary on shared terms over one shared
    /// escrow, funded with exactly `terms.total × n` by one transfer.
    ///
    /// Accounts: `[holding (asset shard), creator (asset shard, signs),
    /// schedule_0 .. schedule_{n-1} (this program's shard)]`, where schedule
    /// `i` has id [`batch_schedule_id`]`(batch_id, i)`.
    CreateScheduleBatch {
        batch_id: [u8; 32],
        beneficiaries: Vec<AccountId>,
        asset: Asset,
        terms: Terms,
    },
    /// Pay `amount` into `destination`, which the beneficiary names. Valid only
    /// once the block timestamp reaches `at`; `apply` refuses it unless
    /// `amount <= vested(at) - claimed`. `batch_id` is set for a batch schedule,
    /// whose escrow derives from the batch.
    ///
    /// Accounts: `[schedule, holding (asset shard), destination (asset shard),
    /// beneficiary (signer handle)]`.
    Claim {
        schedule_id: [u8; 32],
        batch_id: Option<[u8; 32]>,
        asset: Asset,
        amount: u128,
        at: u64,
    },
    /// Freeze accrual at `at` and return `refund`, which must equal
    /// `total - vested(at)` exactly, to the refund account fixed at creation.
    /// Valid only while the block timestamp is in `[at, at + CANCEL_WINDOW_MS)`.
    ///
    /// Accounts: `[schedule, holding (asset shard), refund (asset shard),
    /// authority (signer handle)]`.
    Cancel {
        schedule_id: [u8; 32],
        batch_id: Option<[u8; 32]>,
        asset: Asset,
        at: u64,
        refund: u128,
    },
    /// One-way. Accounts: `[schedule, creator (signer handle)]`.
    MakeNonCancelable { schedule_id: [u8; 32] },
    /// Only the current beneficiary, only if the schedule was created
    /// transferable. Accounts: `[schedule, beneficiary (signer handle)]`.
    TransferBeneficiary {
        schedule_id: [u8; 32],
        new_beneficiary: AccountId,
    },
    /// Sets one bit; a set bit is refused. Accounts: `[schedule, authority
    /// (signer handle)]`.
    SignalMilestone { schedule_id: [u8; 32], index: u32 },
}

/// On-chain schedule state, this program's shard of the schedule PDA. The
/// field order is the v0.2.4 layout unchanged, which the Basecamp panel decodes.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct VestingSchedule {
    pub kind: u8,
    pub start: u64,
    pub cliff: u64,
    pub end: u64,
    pub total: u128,
    pub claimed: u128,
    /// The largest time any claim or the cancellation named.
    pub last_seen: u64,
    pub beneficiary: AccountId,
    /// The escrow PDA, fixed at creation, so no instruction can name another.
    pub escrow: AccountId,
    pub creator: AccountId,
    /// 1 while cancelable; the move to 0 is one-way.
    pub cancelable: u8,
    /// Fixed at creation (F4).
    pub transferable: u8,
    /// The instant accrual stopped, or 0.
    pub cancelled_at: u64,
    /// Bit `i` set means milestone `i` has been signalled.
    pub signalled: u64,
    pub tranches: u32,
    /// 0 = native, 1 = token.
    pub asset: u8,
    pub token_definition: [u8; 32],
    pub refund_to: AccountId,
    pub cancel_authority: AccountId,
    pub milestone_authority: AccountId,
}

/// What `plan` asks `apply` to do to a schedule shard. Every field is either
/// something the plan knew (an account id, a signer) or something the caller
/// claimed (an amount, a time), and `apply` checks it against the state.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Effect {
    Create(VestingSchedule),
    Claim {
        beneficiary: AccountId,
        holding: AccountId,
        asset: Asset,
        amount: u128,
        at: u64,
    },
    Cancel {
        authority: AccountId,
        holding: AccountId,
        refund_to: AccountId,
        asset: Asset,
        at: u64,
        refund: u128,
    },
    MakeNonCancelable {
        creator: AccountId,
    },
    TransferBeneficiary {
        beneficiary: AccountId,
        new_beneficiary: AccountId,
    },
    SignalMilestone {
        authority: AccountId,
        index: u32,
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
pub fn schedule_seed(schedule_id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[schedule_id])
}

/// The escrow seed of a schedule, or of a batch: `[id, "holding"]`.
#[must_use]
pub fn holding_seed(id: &[u8; 32]) -> PdaSeed {
    combine_seeds(&[id, &seed_from_str("holding")])
}

/// Schedule `i` of a batch: `SHA-256(batch_id ‖ u32_le(i) padded to 32)`.
#[must_use]
pub fn batch_schedule_id(batch_id: &[u8; 32], i: u32) -> [u8; 32] {
    let mut idx = [0u8; 32];
    idx[..4].copy_from_slice(&i.to_le_bytes());
    *combine_seeds(&[batch_id, &idx]).as_bytes()
}

#[must_use]
pub fn schedule_account(program: &AccountId, schedule_id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &schedule_seed(schedule_id))
}

#[must_use]
pub fn holding_account(program: &AccountId, id: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &holding_seed(id))
}

// ------------------------------------------------------------------- events

/// `sha256("antumbra_vesting::<Name>")[..8]`, the convention `ProgramEvent`
/// documents.
#[must_use]
pub fn event_selector(name: &str) -> [u8; 8] {
    let full = sha256(&[b"antumbra_vesting::", name.as_bytes()]);
    full[..8].try_into().expect("8 of 32 bytes")
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ScheduleCreated {
    pub schedule_id: [u8; 32],
    pub beneficiary: AccountId,
    pub holding: AccountId,
    pub asset: Asset,
    pub kind: u8,
    pub total: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BatchCreated {
    pub batch_id: [u8; 32],
    pub holding: AccountId,
    pub asset: Asset,
    pub count: u32,
    pub total_each: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Claimed {
    pub schedule_id: [u8; 32],
    pub amount: u128,
    pub at: u64,
    pub destination: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Cancelled {
    pub schedule_id: [u8; 32],
    pub at: u64,
    pub refund: u128,
    pub refund_to: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct MadeNonCancelable {
    pub schedule_id: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BeneficiaryTransferred {
    pub schedule_id: [u8; 32],
    pub from: AccountId,
    pub to: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct MilestoneSignalled {
    pub schedule_id: [u8; 32],
    pub index: u32,
}

/// Every event name, in the order the IDL lists them.
pub const EVENT_NAMES: [&str; 7] = [
    "ScheduleCreated",
    "BatchCreated",
    "Claimed",
    "Cancelled",
    "MadeNonCancelable",
    "BeneficiaryTransferred",
    "MilestoneSignalled",
];

fn event<T: BorshSerialize>(name: &str, body: &T) -> ProgramEvent {
    ProgramEvent {
        selector: event_selector(name),
        data: borsh::to_vec(body).expect("borsh serialization is infallible"),
    }
}

// --------------------------------------------------------------- arithmetic

fn milestone_vested(s: &VestingSchedule) -> u128 {
    refuse_unless(
        s.tranches != 0 && s.tranches <= MAX_TRANCHES,
        E_MILESTONE_BAD,
        "tranche count out of range",
    );
    let done = u128::from(s.signalled.count_ones());
    antumbra::mul_div_floor(s.total, done, u128::from(s.tranches))
        .unwrap_or_else(|_| refuse(E_MILESTONE_BAD, "milestone arithmetic overflowed"))
}

fn rebuild(s: &VestingSchedule) -> antumbra::vesting::Schedule {
    let built = match s.kind {
        0 => antumbra::vesting::Schedule::cliff_linear(s.start, s.cliff, s.end, s.total),
        1 => antumbra::vesting::Schedule::linear(s.start, s.end, s.total),
        _ => refuse(E_BAD_SCHEDULE, "unknown schedule kind"),
    };
    let mut sched =
        built.unwrap_or_else(|_| refuse(E_BAD_SCHEDULE, "schedule parameters are degenerate"));
    sched.claimed = s.claimed;
    if s.cancelled_at != 0 {
        sched.cancelled_at = Some(s.cancelled_at);
    }
    sched
}

/// What has vested by `t`, frozen at a cancellation. The same
/// `antumbra::vesting` code the host tests and the Kani proofs cover.
#[must_use]
pub fn vested(s: &VestingSchedule, t: u64) -> u128 {
    if s.kind == 2 {
        milestone_vested(s)
    } else {
        rebuild(s).vested_at(t)
    }
}

/// Vested by `t` and not yet claimed: the largest `amount` a claim at `t` may name.
#[must_use]
pub fn claimable(s: &VestingSchedule, t: u64) -> u128 {
    vested(s, t).saturating_sub(s.claimed)
}

/// The exact `refund` a cancellation at `t` must name.
#[must_use]
pub fn unvested(s: &VestingSchedule, t: u64) -> u128 {
    let live = VestingSchedule {
        cancelled_at: 0,
        ..s.clone()
    };
    s.total
        .checked_sub(vested(&live, t))
        .unwrap_or_else(|| refuse(E_BAD_SCHEDULE, "vested exceeds total"))
}

fn or_creator(a: AccountId, creator: AccountId) -> AccountId {
    if a == AccountId::default() {
        creator
    } else {
        a
    }
}

/// The state a creation writes, validated now rather than at the first claim.
#[must_use]
pub fn new_state(
    creator: AccountId,
    escrow: AccountId,
    beneficiary: AccountId,
    asset: &Asset,
    terms: &Terms,
) -> VestingSchedule {
    let (asset_code, token_definition) = asset.code();
    let state = VestingSchedule {
        kind: terms.kind,
        start: terms.start,
        cliff: terms.cliff,
        end: terms.end,
        total: terms.total,
        claimed: 0,
        last_seen: 0,
        beneficiary,
        escrow,
        creator,
        cancelable: u8::from(terms.cancelable),
        transferable: u8::from(terms.transferable),
        cancelled_at: 0,
        signalled: 0,
        tranches: terms.tranches,
        asset: asset_code,
        token_definition,
        refund_to: terms.refund_to,
        cancel_authority: or_creator(terms.cancel_authority, creator),
        milestone_authority: or_creator(terms.milestone_authority, creator),
    };
    if state.kind == 2 {
        let _ = milestone_vested(&state);
        refuse_unless(state.total > 0, E_BAD_SCHEDULE, "zero total");
    } else {
        refuse_unless(state.tranches == 0, E_BAD_SCHEDULE, "tranches on a time schedule");
        let _ = rebuild(&state);
    }
    refuse_unless(
        state.refund_to != AccountId::default(),
        E_WRONG_REFUND,
        "no refund account",
    );
    state
}

// --------------------------------------------------------------------- plan

fn rows<const N: usize>(input: &PlanInput) -> [AccountMeta; N] {
    <[AccountMeta; N]>::try_from(input.accounts.clone())
        .unwrap_or_else(|_| refuse(E_ACCOUNTS, "wrong number of accounts for this instruction"))
}

fn expect_schedule(row: &AccountMeta, me: &AccountId, schedule_id: &[u8; 32]) {
    refuse_unless(
        row.account_id == schedule_account(me, schedule_id) && row.program_account_id == *me,
        E_NOT_ANCHORED,
        "the schedule row is not this program's shard of the schedule PDA",
    );
}

fn expect_holding(row: &AccountMeta, me: &AccountId, seed_id: &[u8; 32], asset: &Asset) {
    refuse_unless(
        row.account_id == holding_account(me, seed_id),
        E_ESCROW_MISMATCH,
        "the holding row is not the escrow PDA",
    );
    refuse_unless(
        row.program_account_id == asset.shard_program(),
        E_WRONG_ASSET,
        "the holding row selects the wrong shard for this asset",
    );
}

fn expect_asset_row(row: &AccountMeta, asset: &Asset) {
    refuse_unless(
        row.program_account_id == asset.shard_program(),
        E_WRONG_ASSET,
        "this row must select the asset's shard",
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

/// A payout out of an escrow: the transfer with the escrow's PDA seed
/// attached, which is the only thing that authorises a debit of it.
fn payout(asset: &Asset, holding: &AccountMeta, seed_id: &[u8; 32], to: &AccountMeta, amount: u128) -> ChainedCall {
    match asset {
        Asset::Native => native_token::custody_transfer(
            holding.account_id,
            holding_seed(seed_id),
            to.account_id,
            amount,
        ),
        Asset::Token { .. } => {
            transfer_call(asset, holding, to, amount).with_pda_seeds(vec![holding_seed(seed_id)])
        }
    }
}

/// The plan entry point.
#[must_use]
pub fn plan(input: &PlanInput, ix: Instruction) -> Plan {
    let me = input.self_account_id;
    let mut plan = Plan::new(input);
    match ix {
        Instruction::CreateSchedule {
            schedule_id,
            beneficiary,
            asset,
            terms,
        } => {
            let [schedule, holding, creator] = rows::<3>(input);
            expect_schedule(&schedule, &me, &schedule_id);
            expect_holding(&holding, &me, &schedule_id, &asset);
            expect_asset_row(&creator, &asset);
            refuse_unless(creator.is_authorized, E_NOT_SIGNED, "the creator must sign");
            let state = new_state(creator.account_id, holding.account_id, beneficiary, &asset, &terms);
            plan.effect(&schedule, &Effect::Create(state));
            plan.call(transfer_call(&asset, &creator, &holding, terms.total));
            plan.event(event(
                "ScheduleCreated",
                &ScheduleCreated {
                    schedule_id,
                    beneficiary,
                    holding: holding.account_id,
                    asset,
                    kind: terms.kind,
                    total: terms.total,
                },
            ));
        }
        Instruction::CreateScheduleBatch {
            batch_id,
            beneficiaries,
            asset,
            terms,
        } => {
            let n = beneficiaries.len();
            refuse_unless(
                n >= 1 && input.accounts.len() == n + 2,
                E_BATCH,
                "one schedule row per beneficiary, at least one",
            );
            let holding = input.accounts[0].clone();
            let creator = input.accounts[1].clone();
            expect_holding(&holding, &me, &batch_id, &asset);
            expect_asset_row(&creator, &asset);
            refuse_unless(creator.is_authorized, E_NOT_SIGNED, "the creator must sign");
            let count = u32::try_from(n).unwrap_or_else(|_| refuse(E_BATCH, "too many schedules"));
            let sum = terms
                .total
                .checked_mul(u128::from(count))
                .unwrap_or_else(|| refuse(E_BATCH, "batch total overflows"));
            for (i, (row, who)) in input.accounts[2..].iter().zip(&beneficiaries).enumerate() {
                let sid = batch_schedule_id(&batch_id, i as u32);
                refuse_unless(
                    row.account_id == schedule_account(&me, &sid) && row.program_account_id == me,
                    E_BATCH,
                    "a schedule row is not the next PDA of this batch",
                );
                let state = new_state(creator.account_id, holding.account_id, *who, &asset, &terms);
                plan.effect(row, &Effect::Create(state));
            }
            plan.call(transfer_call(&asset, &creator, &holding, sum));
            plan.event(event(
                "BatchCreated",
                &BatchCreated {
                    batch_id,
                    holding: holding.account_id,
                    asset,
                    count,
                    total_each: terms.total,
                },
            ));
        }
        Instruction::Claim {
            schedule_id,
            batch_id,
            asset,
            amount,
            at,
        } => {
            let [schedule, holding, destination, beneficiary] = rows::<4>(input);
            let seed_id = batch_id.unwrap_or(schedule_id);
            expect_schedule(&schedule, &me, &schedule_id);
            expect_holding(&holding, &me, &seed_id, &asset);
            expect_asset_row(&destination, &asset);
            expect_signer_handle(&beneficiary, &me);
            refuse_unless(amount > 0, E_NOTHING_CLAIMABLE, "a claim names a positive amount");
            plan.timestamp_window(at..);
            plan.effect(
                &schedule,
                &Effect::Claim {
                    beneficiary: beneficiary.account_id,
                    holding: holding.account_id,
                    asset,
                    amount,
                    at,
                },
            );
            plan.call(payout(&asset, &holding, &seed_id, &destination, amount));
            plan.event(event(
                "Claimed",
                &Claimed {
                    schedule_id,
                    amount,
                    at,
                    destination: destination.account_id,
                },
            ));
        }
        Instruction::Cancel {
            schedule_id,
            batch_id,
            asset,
            at,
            refund,
        } => {
            let [schedule, holding, refund_row, authority] = rows::<4>(input);
            let seed_id = batch_id.unwrap_or(schedule_id);
            expect_schedule(&schedule, &me, &schedule_id);
            expect_holding(&holding, &me, &seed_id, &asset);
            expect_asset_row(&refund_row, &asset);
            expect_signer_handle(&authority, &me);
            let until = at
                .checked_add(CANCEL_WINDOW_MS)
                .unwrap_or_else(|| refuse(E_TIME_WENT_BACKWARDS, "cancellation time overflows"));
            // `at..until` is never empty: CANCEL_WINDOW_MS is positive.
            plan.timestamp_window(
                lee_core::program::TimestampValidityWindow::try_from(at..until)
                    .expect("a positive window is not empty"),
            );
            plan.effect(
                &schedule,
                &Effect::Cancel {
                    authority: authority.account_id,
                    holding: holding.account_id,
                    refund_to: refund_row.account_id,
                    asset,
                    at,
                    refund,
                },
            );
            if refund > 0 {
                plan.call(payout(&asset, &holding, &seed_id, &refund_row, refund));
            }
            plan.event(event(
                "Cancelled",
                &Cancelled {
                    schedule_id,
                    at,
                    refund,
                    refund_to: refund_row.account_id,
                },
            ));
        }
        Instruction::MakeNonCancelable { schedule_id } => {
            let [schedule, creator] = rows::<2>(input);
            expect_schedule(&schedule, &me, &schedule_id);
            expect_signer_handle(&creator, &me);
            plan.effect(
                &schedule,
                &Effect::MakeNonCancelable {
                    creator: creator.account_id,
                },
            );
            plan.event(event("MadeNonCancelable", &MadeNonCancelable { schedule_id }));
        }
        Instruction::TransferBeneficiary {
            schedule_id,
            new_beneficiary,
        } => {
            let [schedule, beneficiary] = rows::<2>(input);
            expect_schedule(&schedule, &me, &schedule_id);
            expect_signer_handle(&beneficiary, &me);
            plan.effect(
                &schedule,
                &Effect::TransferBeneficiary {
                    beneficiary: beneficiary.account_id,
                    new_beneficiary,
                },
            );
            plan.event(event(
                "BeneficiaryTransferred",
                &BeneficiaryTransferred {
                    schedule_id,
                    from: beneficiary.account_id,
                    to: new_beneficiary,
                },
            ));
        }
        Instruction::SignalMilestone { schedule_id, index } => {
            let [schedule, authority] = rows::<2>(input);
            expect_schedule(&schedule, &me, &schedule_id);
            expect_signer_handle(&authority, &me);
            plan.effect(
                &schedule,
                &Effect::SignalMilestone {
                    authority: authority.account_id,
                    index,
                },
            );
            plan.event(event("MilestoneSignalled", &MilestoneSignalled { schedule_id, index }));
        }
    }
    plan
}

// -------------------------------------------------------------------- apply

/// Decode a stored schedule, refusing an empty shard: nothing was created there.
#[must_use]
pub fn load(pre: &[u8]) -> VestingSchedule {
    refuse_unless(!pre.is_empty(), E_NOT_ANCHORED, "no schedule is committed at this id");
    VestingSchedule::try_from_slice(pre)
        .unwrap_or_else(|_| refuse(E_BAD_SCHEDULE, "schedule failed to deserialize"))
}

fn store(s: &VestingSchedule) -> Vec<u8> {
    borsh::to_vec(s).expect("borsh serialization is infallible")
}

fn expect_asset(s: &VestingSchedule, asset: &Asset) {
    refuse_unless(
        asset.code() == (s.asset, s.token_definition),
        E_WRONG_ASSET,
        "the instruction names a different asset from the schedule's",
    );
}

/// The apply entry point: one effect against one schedule shard.
#[must_use]
pub fn apply(effect: Effect, pre: &[u8]) -> Option<Vec<u8>> {
    match effect {
        Effect::Create(state) => {
            // A schedule id is used once. Refusing any existing bytes, rather
            // than only different ones, keeps a replayed creation from landing
            // and paying into the escrow a second time.
            refuse_unless(pre.is_empty(), E_EXISTS, "a schedule already exists at this id");
            Some(store(&state))
        }
        Effect::Claim {
            beneficiary,
            holding,
            asset,
            amount,
            at,
        } => {
            let mut s = load(pre);
            refuse_unless(
                s.beneficiary == beneficiary,
                E_NOT_BENEFICIARY,
                "signer is not the beneficiary this schedule names",
            );
            refuse_unless(s.escrow == holding, E_ESCROW_MISMATCH, "holding is not this schedule's");
            expect_asset(&s, &asset);
            let owed = claimable(&s, at);
            refuse_unless(
                amount <= owed,
                E_NOTHING_CLAIMABLE,
                "the amount exceeds what has vested by that time and is unclaimed",
            );
            s.claimed = s
                .claimed
                .checked_add(amount)
                .unwrap_or_else(|| refuse(E_BAD_SCHEDULE, "claimed overflows"));
            s.last_seen = s.last_seen.max(at);
            Some(store(&s))
        }
        Effect::Cancel {
            authority,
            holding,
            refund_to,
            asset,
            at,
            refund,
        } => {
            let mut s = load(pre);
            refuse_unless(
                s.cancel_authority == authority,
                E_NOT_AUTHORITY,
                "signer is not this schedule's cancel authority",
            );
            refuse_unless(s.cancelable != 0, E_NOT_CANCELABLE, "this schedule was made non-cancelable");
            refuse_unless(s.cancelled_at == 0, E_ALREADY_CANCELLED, "already cancelled");
            refuse_unless(
                s.refund_to == refund_to,
                E_WRONG_REFUND,
                "refund account is not the one fixed at creation",
            );
            refuse_unless(s.escrow == holding, E_ESCROW_MISMATCH, "holding is not this schedule's");
            expect_asset(&s, &asset);
            // A claim already paid what had vested by `last_seen`; freezing
            // earlier would refund some of that a second time.
            refuse_unless(
                at >= s.last_seen,
                E_TIME_WENT_BACKWARDS,
                "the cancellation names a time before this schedule's last claim",
            );
            refuse_unless(
                refund == unvested(&s, at),
                E_REFUND_AMOUNT,
                "the refund is not exactly the unvested part at that time",
            );
            // A time of zero would leave the schedule looking uncancelled.
            s.cancelled_at = at.max(1);
            s.last_seen = at;
            Some(store(&s))
        }
        Effect::MakeNonCancelable { creator } => {
            let mut s = load(pre);
            refuse_unless(s.creator == creator, E_NOT_CREATOR, "signer is not the creator");
            refuse_unless(s.cancelable != 0, E_NOT_CANCELABLE, "already non-cancelable");
            s.cancelable = 0;
            Some(store(&s))
        }
        Effect::TransferBeneficiary {
            beneficiary,
            new_beneficiary,
        } => {
            let mut s = load(pre);
            refuse_unless(
                s.transferable != 0,
                E_NOT_TRANSFERABLE,
                "this position was created non-transferable",
            );
            // The holder moves it, never the creator.
            refuse_unless(
                s.beneficiary == beneficiary,
                E_NOT_BENEFICIARY,
                "signer is not the current beneficiary",
            );
            s.beneficiary = new_beneficiary;
            Some(store(&s))
        }
        Effect::SignalMilestone { authority, index } => {
            let mut s = load(pre);
            refuse_unless(
                s.milestone_authority == authority,
                E_NOT_AUTHORITY,
                "signer is not this schedule's milestone authority",
            );
            refuse_unless(s.kind == 2, E_MILESTONE_BAD, "not a milestone schedule");
            refuse_unless(s.cancelled_at == 0, E_ALREADY_CANCELLED, "the schedule was cancelled");
            refuse_unless(
                index < s.tranches.min(MAX_TRANCHES),
                E_MILESTONE_BAD,
                "milestone index out of range",
            );
            let bit = 1u64 << index;
            refuse_unless(s.signalled & bit == 0, E_MILESTONE_BAD, "already signalled");
            s.signalled |= bit;
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
    pub fn create(program: &AccountId, schedule_id: &[u8; 32], asset: &Asset, creator: AccountId) -> Vec<ProgramShardSelector> {
        vec![
            sel(schedule_account(program, schedule_id), *program),
            sel(holding_account(program, schedule_id), asset.shard_program()),
            sel(creator, asset.shard_program()),
        ]
    }

    #[must_use]
    pub fn batch(program: &AccountId, batch_id: &[u8; 32], n: u32, asset: &Asset, creator: AccountId) -> Vec<ProgramShardSelector> {
        let mut out = vec![
            sel(holding_account(program, batch_id), asset.shard_program()),
            sel(creator, asset.shard_program()),
        ];
        out.extend((0..n).map(|i| sel(schedule_account(program, &batch_schedule_id(batch_id, i)), *program)));
        out
    }

    #[must_use]
    pub fn claim(
        program: &AccountId,
        schedule_id: &[u8; 32],
        batch_id: Option<&[u8; 32]>,
        asset: &Asset,
        destination: AccountId,
        beneficiary: AccountId,
    ) -> Vec<ProgramShardSelector> {
        vec![
            sel(schedule_account(program, schedule_id), *program),
            sel(holding_account(program, batch_id.unwrap_or(schedule_id)), asset.shard_program()),
            sel(destination, asset.shard_program()),
            sel(beneficiary, *program),
        ]
    }

    #[must_use]
    pub fn cancel(
        program: &AccountId,
        schedule_id: &[u8; 32],
        batch_id: Option<&[u8; 32]>,
        asset: &Asset,
        refund_to: AccountId,
        authority: AccountId,
    ) -> Vec<ProgramShardSelector> {
        claim(program, schedule_id, batch_id, asset, refund_to, authority)
    }

    /// `[schedule, signer handle]`: make_non_cancelable, transfer_beneficiary,
    /// signal_milestone.
    #[must_use]
    pub fn handle(program: &AccountId, schedule_id: &[u8; 32], signer: AccountId) -> Vec<ProgramShardSelector> {
        vec![sel(schedule_account(program, schedule_id), *program), sel(signer, *program)]
    }
}

#[cfg(test)]
mod tests;
