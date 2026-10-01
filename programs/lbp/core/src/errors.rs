//! Refusal codes. A v0.3 program refuses by panicking, which fails the whole
//! transaction; the code opens the panic message (`E6005: ...`), so a refusal
//! reads the same in the sequencer log, the executor tests and the CLI.
//!
//! The numbers are the v0.2.4 ones where the meaning survived. 6007 (buyer
//! unowned) and 6008 (insufficient balance) are retired: v0.3 accounts have no
//! owner field, a plan cannot read a balance, and a buyer who cannot pay is
//! refused by the transfer program itself, which fails the whole transaction.

pub const E_BAD_POOL: u32 = 6001;
pub const E_NOT_ANCHORED: u32 = 6002;
pub const E_PRICING_REFUSED: u32 = 6003;
pub const E_SLIPPAGE: u32 = 6004;
pub const E_PAUSED: u32 = 6005;
pub const E_TIME_WENT_BACKWARDS: u32 = 6006;
pub const E_TREASURY_OVERFLOW: u32 = 6009;
pub const E_TREASURY_MISMATCH: u32 = 6010;
pub const E_NOT_CREATOR: u32 = 6011;
pub const E_EXISTS: u32 = 6012;
pub const E_ACCOUNTS: u32 = 6013;
pub const E_NOT_SIGNED: u32 = 6014;
pub const E_WRONG_ASSET: u32 = 6015;
pub const E_AMOUNT: u32 = 6016;
pub const E_NOT_ENDED: u32 = 6017;

/// Every code with its name, in the order the IDL lists them.
pub const ALL: [(u32, &str); 15] = [
    (E_BAD_POOL, "BadPool"),
    (E_NOT_ANCHORED, "NotAnchored"),
    (E_PRICING_REFUSED, "PricingRefused"),
    (E_SLIPPAGE, "Slippage"),
    (E_PAUSED, "Paused"),
    (E_TIME_WENT_BACKWARDS, "TimeWentBackwards"),
    (E_TREASURY_OVERFLOW, "TreasuryOverflow"),
    (E_TREASURY_MISMATCH, "TreasuryMismatch"),
    (E_NOT_CREATOR, "NotCreator"),
    (E_EXISTS, "AlreadyExists"),
    (E_ACCOUNTS, "Accounts"),
    (E_NOT_SIGNED, "NotSigned"),
    (E_WRONG_ASSET, "WrongAsset"),
    (E_AMOUNT, "Amount"),
    (E_NOT_ENDED, "NotEnded"),
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
