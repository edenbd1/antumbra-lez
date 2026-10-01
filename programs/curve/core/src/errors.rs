//! Refusal codes. A v0.3 program refuses by panicking, which fails the whole
//! transaction; the code opens the panic message (`E5004: ...`), so a refusal
//! reads the same in the sequencer log, the executor tests and the CLI.
//!
//! The numbers are the v0.2.4 ones where the meaning survived. 5005 (buyer
//! unowned) and 5006 (insufficient balance) are retired: v0.3 accounts have no
//! owner field, a plan cannot read a balance, and a buyer who cannot pay is
//! refused by the transfer program itself, which fails the whole transaction.

pub const E_BAD_SALE: u32 = 5001;
pub const E_NOT_ANCHORED: u32 = 5002;
pub const E_PRICING_REFUSED: u32 = 5003;
pub const E_CLOSED: u32 = 5004;
pub const E_TREASURY_OVERFLOW: u32 = 5007;
pub const E_TREASURY_MISMATCH: u32 = 5008;
pub const E_FEE_TREASURY_MISMATCH: u32 = 5009;
pub const E_FEE_RATE_ABOVE_CAP: u32 = 5010;
pub const E_NOT_CREATOR: u32 = 5011;
pub const E_EXISTS: u32 = 5012;
pub const E_ACCOUNTS: u32 = 5013;
pub const E_NOT_SIGNED: u32 = 5014;
pub const E_WRONG_ASSET: u32 = 5015;
pub const E_AMOUNT: u32 = 5016;

/// Every code with its name, in the order the IDL lists them.
pub const ALL: [(u32, &str); 14] = [
    (E_BAD_SALE, "BadSale"),
    (E_NOT_ANCHORED, "NotAnchored"),
    (E_PRICING_REFUSED, "PricingRefused"),
    (E_CLOSED, "Closed"),
    (E_TREASURY_OVERFLOW, "TreasuryOverflow"),
    (E_TREASURY_MISMATCH, "TreasuryMismatch"),
    (E_FEE_TREASURY_MISMATCH, "FeeTreasuryMismatch"),
    (E_FEE_RATE_ABOVE_CAP, "FeeRateAboveCap"),
    (E_NOT_CREATOR, "NotCreator"),
    (E_EXISTS, "AlreadyExists"),
    (E_ACCOUNTS, "Accounts"),
    (E_NOT_SIGNED, "NotSigned"),
    (E_WRONG_ASSET, "WrongAsset"),
    (E_AMOUNT, "Amount"),
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
