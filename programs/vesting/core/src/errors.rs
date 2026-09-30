//! Refusal codes. A v0.3 program refuses by panicking, which fails the whole
//! transaction; the code opens the panic message (`E7004: ...`), so a refusal
//! reads the same in the sequencer log, the executor tests and the CLI.
//!
//! The numbers are the v0.2.4 ones where the meaning survived. 7014 (bad clock)
//! is retired: the program no longer reads a clock account.

pub const E_BAD_SCHEDULE: u32 = 7001;
pub const E_NOT_ANCHORED: u32 = 7002;
pub const E_NOTHING_CLAIMABLE: u32 = 7003;
pub const E_NOT_BENEFICIARY: u32 = 7004;
pub const E_TIME_WENT_BACKWARDS: u32 = 7005;
pub const E_ESCROW_MISMATCH: u32 = 7006;
pub const E_NOT_CREATOR: u32 = 7009;
pub const E_NOT_CANCELABLE: u32 = 7010;
pub const E_ALREADY_CANCELLED: u32 = 7011;
pub const E_NOT_TRANSFERABLE: u32 = 7012;
pub const E_MILESTONE_BAD: u32 = 7013;
pub const E_NOT_AUTHORITY: u32 = 7015;
pub const E_WRONG_ASSET: u32 = 7016;
pub const E_WRONG_REFUND: u32 = 7017;
pub const E_BATCH: u32 = 7018;
pub const E_EXISTS: u32 = 7019;
pub const E_REFUND_AMOUNT: u32 = 7020;
pub const E_ACCOUNTS: u32 = 7021;
pub const E_NOT_SIGNED: u32 = 7022;

/// Every code with its name, in the order the IDL lists them.
pub const ALL: [(u32, &str); 19] = [
    (E_BAD_SCHEDULE, "BadSchedule"),
    (E_NOT_ANCHORED, "NotAnchored"),
    (E_NOTHING_CLAIMABLE, "NothingClaimable"),
    (E_NOT_BENEFICIARY, "NotBeneficiary"),
    (E_TIME_WENT_BACKWARDS, "TimeWentBackwards"),
    (E_ESCROW_MISMATCH, "EscrowMismatch"),
    (E_NOT_CREATOR, "NotCreator"),
    (E_NOT_CANCELABLE, "NotCancelable"),
    (E_ALREADY_CANCELLED, "AlreadyCancelled"),
    (E_NOT_TRANSFERABLE, "NotTransferable"),
    (E_MILESTONE_BAD, "MilestoneBad"),
    (E_NOT_AUTHORITY, "NotAuthority"),
    (E_WRONG_ASSET, "WrongAsset"),
    (E_WRONG_REFUND, "WrongRefund"),
    (E_BATCH, "Batch"),
    (E_EXISTS, "AlreadyExists"),
    (E_REFUND_AMOUNT, "RefundAmount"),
    (E_ACCOUNTS, "Accounts"),
    (E_NOT_SIGNED, "NotSigned"),
];

/// Refuse the transaction with `code`.
#[track_caller]
pub fn refuse(code: u32, why: &str) -> ! {
    panic!("E{code}: {why}")
}

#[track_caller]
pub fn refuse_unless(ok: bool, code: u32, why: &str) {
    if !ok {
        refuse(code, why);
    }
}
