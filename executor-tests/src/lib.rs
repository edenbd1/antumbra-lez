//! Drive the committed `antumbra_vesting` binary the way the LEZ sequencer does.
//!
//! `lee/state_machine/src/program/mod.rs` executes a public transaction's program
//! by writing four inputs — program id, caller, pre-states, instruction words —
//! into a RISC0 executor with a 32M-cycle session limit, and decoding the journal
//! as a `ProgramOutput`. This does exactly that, against
//! `artifacts/programs/antumbra_vesting.bin`, so an acceptance or a refusal here
//! is the one the chain would give. It does not prove: proving costs minutes and
//! establishes nothing extra about which inputs a program accepts.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{Account, AccountId, AccountWithMetadata, Data, Nonce};
use lee_core::program::{ChainedCall, ProgramId, ProgramOutput};
use risc0_zkvm::{default_executor, ExecutorEnv};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MAX_NUM_CYCLES_PUBLIC_EXECUTION: u64 = 1024 * 1024 * 32;

pub const AUTH_TRANSFER: ProgramId = [
    583309054, 2344528779, 3806558405, 2890696795, 2257354672, 3978764116, 2273929063, 1518858078,
];
pub const TOKEN: ProgramId = [
    1047643340, 4291649067, 2093396023, 4016657193, 3904308476, 481382041, 2987082047, 2603530278,
];
pub const CLOCK: ProgramId = [
    96247601, 2082502477, 822865082, 1048693993, 3544189898, 772921104, 1694408900, 4234239033,
];
pub const CLOCK_ACCOUNT: [u8; 32] = *b"/LEZ/ClockProgramAccount/0000001";
pub const Z: [u8; 32] = [0; 32];

/// The instruction enum `#[lez_program]` generates: one variant per
/// `#[instruction]`, in declaration order, fields in argument order. risc0's
/// serde writes the variant index first, so this order is the ABI.
#[derive(Serialize, Clone)]
pub enum Ix {
    CreateSchedule {
        schedule_id: [u8; 32], kind: u8, start: u64, cliff: u64, end: u64, total: u128,
        beneficiary: [u8; 32], cancelable: u8, transferable: u8, tranches: u32,
        cancel_authority: [u8; 32], milestone_authority: [u8; 32], refund_to: [u8; 32],
    },
    CreateTokenSchedule {
        schedule_id: [u8; 32], kind: u8, start: u64, cliff: u64, end: u64, total: u128,
        beneficiary: [u8; 32], cancelable: u8, transferable: u8, tranches: u32,
        cancel_authority: [u8; 32], milestone_authority: [u8; 32],
    },
    FundSchedule { schedule_id: [u8; 32], amount: u128 },
    FundTokenSchedule { schedule_id: [u8; 32], amount: u128 },
    Cancel { schedule_id: [u8; 32] },
    MakeNonCancelable { schedule_id: [u8; 32] },
    TransferBeneficiary { schedule_id: [u8; 32], new_beneficiary: [u8; 32] },
    SignalMilestone { schedule_id: [u8; 32], index: u32 },
    Claim { schedule_id: [u8; 32] },
    CreateScheduleBatch {
        batch_id: [u8; 32], kind: u8, start: u64, cliff: u64, end: u64, total_each: u128,
        beneficiaries: Vec<[u8; 32]>, cancelable: u8, transferable: u8, tranches: u32,
        cancel_authority: [u8; 32], milestone_authority: [u8; 32], refund_to: [u8; 32],
    },
    FundBatch { batch_id: [u8; 32], amount: u128 },
    ClaimBatch { schedule_id: [u8; 32], batch_id: [u8; 32] },
    CancelBatch { schedule_id: [u8; 32], batch_id: [u8; 32] },
}

impl Ix {
    pub fn name(&self) -> &'static str {
        match self {
            Ix::CreateSchedule { .. } => "create_schedule",
            Ix::CreateTokenSchedule { .. } => "create_token_schedule",
            Ix::FundSchedule { .. } => "fund_schedule",
            Ix::FundTokenSchedule { .. } => "fund_token_schedule",
            Ix::Cancel { .. } => "cancel",
            Ix::MakeNonCancelable { .. } => "make_non_cancelable",
            Ix::TransferBeneficiary { .. } => "transfer_beneficiary",
            Ix::SignalMilestone { .. } => "signal_milestone",
            Ix::Claim { .. } => "claim",
            Ix::CreateScheduleBatch { .. } => "create_schedule_batch",
            Ix::FundBatch { .. } => "fund_batch",
            Ix::ClaimBatch { .. } => "claim_batch",
            Ix::CancelBatch { .. } => "cancel_batch",
        }
    }
}

/// `VestingSchedule` as the program writes it, field for field.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct Schedule {
    pub kind: u8,
    pub start: u64,
    pub cliff: u64,
    pub end: u64,
    pub total: u128,
    pub claimed: u128,
    pub last_seen: u64,
    pub beneficiary: [u8; 32],
    pub escrow: [u8; 32],
    pub creator: [u8; 32],
    pub cancelable: u8,
    pub transferable: u8,
    pub cancelled_at: u64,
    pub signalled: u64,
    pub tranches: u32,
    pub asset: u8,
    pub token_definition: [u8; 32],
    pub refund_to: [u8; 32],
    pub cancel_authority: [u8; 32],
    pub milestone_authority: [u8; 32],
}

pub fn elf() -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../artifacts/programs/antumbra_vesting.bin");
    std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

pub fn program_id(elf: &[u8]) -> ProgramId {
    risc0_binfmt::ProgramBinary::decode(elf).expect("decode").compute_image_id().expect("image id").into()
}

/// SPEL's public PDA: one seed used as is, several combined by SHA-256, then
/// `/LEE/v0.2/AccountId/PDA/` ‖ program id ‖ seed, hashed.
pub fn pda(program: &ProgramId, seeds: &[[u8; 32]]) -> AccountId {
    let combined: [u8; 32] = if seeds.len() == 1 {
        seeds[0]
    } else {
        let mut h = Sha256::new();
        for s in seeds {
            h.update(s);
        }
        h.finalize().into()
    };
    let mut bytes = [0u8; 96];
    bytes[0..32].copy_from_slice(b"/LEE/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00\x00");
    bytes[32..64].copy_from_slice(bytemuck::cast_slice(program));
    bytes[64..96].copy_from_slice(&combined);
    AccountId::new(Sha256::digest(bytes).into())
}

pub fn lit(s: &str) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[..s.len()].copy_from_slice(s.as_bytes());
    b
}

pub fn acc(id: [u8; 32], owner: ProgramId, balance: u128, data: Vec<u8>, signer: bool) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: owner,
            balance,
            data: Data::try_from(data).expect("data fits"),
            nonce: Nonce(0),
        },
        is_authorized: signer,
        account_id: AccountId::new(id),
    }
}

pub fn clock(ms: u64) -> AccountWithMetadata {
    let mut d = 1u64.to_le_bytes().to_vec();
    d.extend_from_slice(&ms.to_le_bytes());
    acc(CLOCK_ACCOUNT, CLOCK, 0, d, false)
}

/// A fungible token holding: `TokenHolding::Fungible { definition, balance }`.
pub fn token_holding(id: [u8; 32], definition: [u8; 32], balance: u128, signer: bool) -> AccountWithMetadata {
    let mut d = vec![0u8];
    d.extend_from_slice(&definition);
    d.extend_from_slice(&balance.to_le_bytes());
    acc(id, TOKEN, 0, d, signer)
}

pub struct Run {
    pub output: ProgramOutput,
    pub cycles: u64,
}

impl Run {
    /// The account the program wrote for `id`, by pairing post-states with the
    /// pre-states they answer.
    pub fn post(&self, id: &AccountWithMetadata) -> &Account {
        let i = self
            .output
            .pre_states
            .iter()
            .position(|p| p.account_id == id.account_id)
            .expect("account not in the output");
        self.output.post_states[i].account()
    }
    pub fn schedule(&self, id: &AccountWithMetadata) -> Schedule {
        let d: Vec<u8> = self.post(id).data.clone().into_inner();
        Schedule::try_from_slice(&d).expect("schedule decodes")
    }
    pub fn calls(&self) -> &[ChainedCall] {
        &self.output.chained_calls
    }
}

/// Execute one instruction. `Err` carries the program's own message, which
/// starts `Program error [<code>]`.
pub fn run(elf: &[u8], pid: &ProgramId, ix: &Ix, pre: Vec<AccountWithMetadata>) -> Result<Run, String> {
    let data = risc0_zkvm::serde::to_vec(ix).map_err(|e| e.to_string())?;
    let caller: Option<ProgramId> = None;
    let mut b = ExecutorEnv::builder();
    b.session_limit(Some(MAX_NUM_CYCLES_PUBLIC_EXECUTION));
    b.write(pid).unwrap();
    b.write(&caller).unwrap();
    b.write(&pre).unwrap();
    b.write(&data).unwrap();
    let env = b.build().map_err(|e| e.to_string())?;
    let info = default_executor().execute(env, elf).map_err(|e| format!("{e:#}"))?;
    let cycles = info.segments.iter().map(|s| u64::from(s.cycles)).sum();
    let output: ProgramOutput = info.journal.decode().map_err(|e| e.to_string())?;
    Ok(Run { output, cycles })
}

/// The program's own error code, read from the guest's panic message. SPEL
/// writes `Program error [<framework code>]: Program error <ours>: <message>`;
/// the framework code is ours offset by 6000, so the second one is read.
pub fn code(err: &str) -> Option<u32> {
    let tail = &err[err.find("]: Program error ")? + "]: Program error ".len()..];
    tail.split(':').next()?.trim().parse().ok()
}

/// A world with one schedule in it, in the state a test needs.
pub struct World {
    pub elf: Vec<u8>,
    pub pid: ProgramId,
    pub id: [u8; 32],
    pub schedule: AccountWithMetadata,
    pub holding: AccountWithMetadata,
}

pub const CREATOR: [u8; 32] = [0xC0; 32];
pub const BENEFICIARY: [u8; 32] = [0xBE; 32];
pub const DEST: [u8; 32] = [0xDE; 32];
pub const REFUND: [u8; 32] = [0xAF; 32];
pub const STRANGER: [u8; 32] = [0x55; 32];
pub const DEFINITION: [u8; 32] = [0xD0; 32];

impl World {
    pub fn new(schedule_id: &str) -> Self {
        let elf = elf();
        let pid = program_id(&elf);
        let id = lit(schedule_id);
        let s_id = pda(&pid, &[id]);
        let h_id = pda(&pid, &[id, lit("holding")]);
        World {
            elf,
            pid,
            id,
            schedule: acc(*s_id.value(), ProgramId::default(), 0, vec![], false),
            holding: acc(*h_id.value(), ProgramId::default(), 0, vec![], false),
        }
    }

    /// Put `state` on chain as if created, with `escrowed` in the holding.
    pub fn with(&mut self, state: Schedule, escrowed: u128) {
        self.schedule.account.program_owner = self.pid;
        self.schedule.account.data = Data::try_from(borsh::to_vec(&state).unwrap()).unwrap();
        if state.asset == 0 {
            self.holding.account.program_owner = self.pid;
            self.holding.account.balance = escrowed;
        } else {
            self.holding = token_holding(*self.holding.account_id.value(), state.token_definition, escrowed, false);
        }
    }

    pub fn linear(&self, start: u64, end: u64, total: u128) -> Schedule {
        Schedule {
            kind: 1, start, cliff: start, end, total, claimed: 0, last_seen: 0,
            beneficiary: BENEFICIARY, escrow: *self.holding.account_id.value(), creator: CREATOR,
            cancelable: 1, transferable: 1, cancelled_at: 0, signalled: 0, tranches: 0,
            asset: 0, token_definition: Z, refund_to: REFUND, cancel_authority: CREATOR,
            milestone_authority: CREATOR,
        }
    }

    pub fn claim(&self, now: u64, dest: AccountWithMetadata, signer: [u8; 32]) -> Result<Run, String> {
        run(&self.elf, &self.pid, &Ix::Claim { schedule_id: self.id }, vec![
            self.schedule.clone(),
            self.holding.clone(),
            dest,
            acc(signer, AUTH_TRANSFER, 0, vec![], true),
            clock(now),
        ])
    }

    pub fn cancel(&self, now: u64, refund: AccountWithMetadata, authority: [u8; 32]) -> Result<Run, String> {
        run(&self.elf, &self.pid, &Ix::Cancel { schedule_id: self.id }, vec![
            self.schedule.clone(),
            self.holding.clone(),
            refund,
            acc(authority, AUTH_TRANSFER, 0, vec![], true),
            clock(now),
        ])
    }
}

pub fn native(id: [u8; 32], balance: u128) -> AccountWithMetadata {
    acc(id, AUTH_TRANSFER, balance, vec![], false)
}

/// Schedule `i` of a batch: SHA-256(batch_id ‖ i as 32 little-endian bytes).
pub fn batch_schedule_id(batch_id: &[u8; 32], i: u32) -> [u8; 32] {
    let mut idx = [0u8; 32];
    idx[..4].copy_from_slice(&i.to_le_bytes());
    let mut h = Sha256::new();
    h.update(batch_id);
    h.update(idx);
    h.finalize().into()
}
