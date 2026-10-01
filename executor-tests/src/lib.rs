//! A one-node LEZ v0.3 chain in memory, for the committed vesting, curve and
//! LBP binaries.
//!
//! Transactions are signed `PublicTransaction`s and go through
//! `ValidatedStateDiff::from_public_transaction_with_cycle_budget`, the call
//! the sequencer's settlement makes, then `V03State::apply_state_diff`. Nothing
//! is mocked between the message and the state: the plan and every apply run in
//! the RISC0 executor, the chained native and token transfers run the real
//! v0.3.0 programs, and the block timestamp is checked against the windows the
//! plan sets.

use antumbra_vesting_core::{Instruction, VestingSchedule};
use borsh::BorshSerialize;
use lee::{
    public_transaction::{Message, WitnessSet},
    Account, AccountId, PrivateKey, ProgramShardSelector, PublicKey, PublicTransaction, ShardData,
    V03State, ValidatedStateDiff,
};
use lee_core::program::TransactionEvent;
use token_core::{TokenDefinition, TokenHolding};

/// `fee_core::market::MAX_GAS_EXEC`: the most execution gas one transaction
/// may declare, and gas is cycles one for one.
pub const GAS_CAP: u64 = 10_000_000;

/// The header account the tests deploy the program at. On a real chain this is
/// whatever account the deployer claims; the program derives every PDA from it.
pub const VESTING: AccountId = AccountId::new(*b"antumbra/vesting/test-header/v03");

pub const BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../artifacts/programs/v0.3/antumbra_vesting.bin");

/// The header accounts the curve and LBP programs are deployed at in tests.
pub const CURVE: AccountId = AccountId::new(*b"antumbra/curve/test-header/v0.3/");
pub const LBP: AccountId = AccountId::new(*b"antumbra/lbp/test-header/v0.3/__");

pub const CURVE_BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../artifacts/programs/v0.3/antumbra_curve.bin");
pub const LBP_BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../artifacts/programs/v0.3/antumbra_lbp.bin");

fn load(path: &str) -> lee::program::Program {
    let bin = std::fs::read(path).unwrap_or_else(|_| panic!("build the guest first: {path}"));
    lee::program::Program::new(bin.into()).expect("a RISC0 program binary")
}

pub struct Key {
    pub sk: PrivateKey,
    pub id: AccountId,
}

impl Key {
    #[must_use]
    pub fn new(seed: u8) -> Self {
        let sk = PrivateKey::try_new([seed; 32]).expect("a valid secp256k1 scalar");
        let id = AccountId::from(&PublicKey::new_from_private_key(&sk));
        Self { sk, id }
    }
}

pub struct Chain {
    pub state: V03State,
    pub block: u64,
    /// The block timestamp, milliseconds.
    pub now: u64,
    /// Cycles the last transaction used, landed or not.
    pub last_cycles: u64,
    pub events: Vec<TransactionEvent>,
}

/// A token holding shard, as the token program writes it.
#[must_use]
pub fn holding_shard(definition_id: AccountId, balance: u128) -> ShardData {
    ShardData::from(&TokenHolding::Fungible { definition_id, balance })
}

impl Chain {
    /// `native`: accounts funded with native balance. `tokens`: (holder,
    /// definition, balance). Every definition named is created too.
    #[must_use]
    pub fn new(native: &[(AccountId, u128)], tokens: &[(AccountId, AccountId, u128)]) -> Self {
        let token = programs::token_account_id();
        let mut accounts: Vec<(AccountId, Account)> =
            native.iter().map(|(id, b)| (*id, Account::funded(*b))).collect();
        for (holder, def, bal) in tokens {
            let acc = accounts.iter_mut().find(|(id, _)| id == holder);
            let shard = holding_shard(*def, *bal);
            match acc {
                Some((_, a)) => a.data.set_shard(token, shard),
                None => accounts.push((*holder, Account::default().with_shard(token, shard))),
            }
            if !accounts.iter().any(|(id, _)| id == def) {
                let d = TokenDefinition::Fungible {
                    name: "TEST".into(),
                    total_supply: 1 << 100,
                    metadata_id: None,
                };
                accounts.push((*def, Account::default().with_shard(token, ShardData::from(&d))));
            }
        }
        let state = V03State::new()
            .with_public_accounts(accounts)
            .with_named_programs([
                (VESTING, load(BIN)),
                (CURVE, load(CURVE_BIN)),
                (LBP, load(LBP_BIN)),
                (token, programs::token()),
            ]);
        Self {
            state,
            block: 10,
            now: 1_700_000_000_000,
            last_cycles: 0,
            events: vec![],
        }
    }

    /// Send `ix` over `rows`, signed by `signers`, in a block at `self.now`.
    /// `Err` carries the refusal as the state machine reports it.
    pub fn send(&mut self, ix: &Instruction, rows: Vec<ProgramShardSelector>, signers: &[&Key]) -> Result<(), String> {
        self.send_with_budget(ix, rows, signers, GAS_CAP)
    }

    pub fn send_with_budget(
        &mut self,
        ix: &Instruction,
        rows: Vec<ProgramShardSelector>,
        signers: &[&Key],
        budget: u64,
    ) -> Result<(), String> {
        self.send_to(VESTING, ix, rows, signers, budget)
    }

    /// Send any program's instruction to the program at header `program`.
    pub fn send_to<I: BorshSerialize>(
        &mut self,
        program: AccountId,
        ix: &I,
        rows: Vec<ProgramShardSelector>,
        signers: &[&Key],
        budget: u64,
    ) -> Result<(), String> {
        let nonces = signers.iter().map(|k| self.state.get_account_by_id(k.id).nonce).collect();
        let message = Message::try_new(program, rows, nonces, ix).map_err(|e| e.to_string())?;
        let keys: Vec<&PrivateKey> = signers.iter().map(|k| &k.sk).collect();
        let tx = PublicTransaction::new(message.clone(), WitnessSet::for_message(&message, &keys));
        let r = ValidatedStateDiff::from_public_transaction_with_cycle_budget(&tx, &self.state, self.block, self.now, budget);
        self.block += 1;
        match r {
            Ok((diff, charge)) => {
                self.last_cycles = charge.cycles;
                self.events = self.state.apply_state_diff(diff);
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }

    #[must_use]
    pub fn native(&self, id: AccountId) -> u128 {
        self.state.get_account_by_id(id).data.native_balance().expect("canonical balance")
    }

    #[must_use]
    pub fn token(&self, id: AccountId) -> u128 {
        let acc = self.state.get_account_by_id(id);
        let shard = acc.data.shard(programs::token_account_id());
        if shard.is_empty() {
            return 0;
        }
        match TokenHolding::try_from(shard).expect("a token holding") {
            TokenHolding::Fungible { balance, .. } => balance,
            other => panic!("not fungible: {other:?}"),
        }
    }

    #[must_use]
    pub fn schedule(&self, schedule_id: &[u8; 32]) -> VestingSchedule {
        let id = antumbra_vesting_core::schedule_account(&VESTING, schedule_id);
        antumbra_vesting_core::load(self.state.get_account_by_id(id).data.shard(VESTING))
    }

    /// The raw bytes of `program`'s own shard on `account`.
    #[must_use]
    pub fn shard(&self, account: AccountId, program: AccountId) -> Vec<u8> {
        self.state.get_account_by_id(account).data.shard(program).to_vec()
    }

    #[must_use]
    pub fn has_schedule(&self, schedule_id: &[u8; 32]) -> bool {
        let id = antumbra_vesting_core::schedule_account(&VESTING, schedule_id);
        !self.state.get_account_by_id(id).data.shard(VESTING).is_empty()
    }
}

/// The refusal code a failed send carries, e.g. `Some(7004)`.
#[must_use]
pub fn code(err: &str) -> Option<u32> {
    let at = err.find(": E")? + 3;
    err[at..at + 4].parse().ok().filter(|_| err.contains("Guest panicked"))
}
