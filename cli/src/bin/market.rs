//! `antumbra-market`: deploy and drive antumbra_curve (RFP-015) and
//! antumbra_lbp (RFP-016) on LEZ v0.3.
//!
//! Keys, the sequencer URL and the fee settings come from the v0.3 wallet home
//! (`LEE_WALLET_HOME_DIR`), as for `antumbra-vesting`. The curve program's
//! header comes from `--program` or `ANTUMBRA_CURVE`, the LBP's from
//! `--program` or `ANTUMBRA_LBP`.
//!
//! Every transaction prints one JSON line on stdout: the operation, its hash,
//! the block it was included in, and a verdict read back from the chain.
//! `applied` means the sale or pool shard changed; `reverted` means the
//! transaction was included (and charged) but the program refused it;
//! `rejected` means the sequencer never took it. `quote` and `show` compute on
//! the host, with the `antumbra` crate, what a buy does to the state the chain
//! holds now, which is how the on-chain values are compared with the host.

use std::{path::PathBuf, time::Duration};

use antumbra_curve_core as curve;
use antumbra_lbp_core as lbp;
use anyhow::{anyhow, bail, Context as _, Result};
use borsh::BorshSerialize;
use clap::{Parser, Subcommand, ValueEnum};
use lee::{program::Program, AccountId, ProgramShardSelector};
use serde_json::{json, Value};
use wallet::{program_facades::program_loader::ProgramLoader, AccountIdentity, AccountMention, WalletCore};

#[derive(Parser)]
#[command(name = "antumbra-market", version, about = "antumbra_curve and antumbra_lbp on LEZ v0.3")]
struct Cli {
    /// The account that pays a public transaction's fee. Default: the first
    /// funded signer.
    #[arg(long, env = "ANTUMBRA_PAYER", global = true)]
    payer: Option<AccountId>,
    /// Declared execution gas for a public transaction (at most 10,000,000).
    #[arg(long, global = true)]
    gas_limit: Option<u64>,
    /// Send even if the local pre-check says the program will refuse. Used to
    /// put refusals on chain on purpose.
    #[arg(long, global = true)]
    force: bool,
    /// A label for the JSON line.
    #[arg(long, global = true, default_value = "")]
    label: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Which {
    Curve,
    Lbp,
}

impl Which {
    fn default_elf(self) -> PathBuf {
        PathBuf::from(match self {
            Self::Curve => "artifacts/programs/v0.3/antumbra_curve.bin",
            Self::Lbp => "artifacts/programs/v0.3/antumbra_lbp.bin",
        })
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Deploy a guest through program_loader into fresh wallet accounts; prints the header.
    Deploy {
        #[arg(value_enum)]
        which: Which,
        #[arg(long)]
        payer: AccountId,
        /// Default: the committed artifact.
        #[arg(long)]
        elf: Option<PathBuf>,
    },
    /// Print the ImageID of a guest as the 32 bytes a program header stores, in hex.
    ImageId {
        #[arg(value_enum)]
        which: Which,
        #[arg(long)]
        elf: Option<PathBuf>,
    },
    /// Print the chain clock (ms).
    Now,
    /// antumbra_curve, the bonding-curve sale.
    Curve {
        #[arg(long, env = "ANTUMBRA_CURVE")]
        program: AccountId,
        #[command(subcommand)]
        cmd: CurveCmd,
    },
    /// antumbra_lbp, the liquidity bootstrapping pool.
    Lbp {
        #[arg(long, env = "ANTUMBRA_LBP")]
        program: AccountId,
        #[command(subcommand)]
        cmd: LbpCmd,
    },
}

#[derive(clap::Args, Clone)]
struct Collateral {
    /// A token definition; omit for native balance.
    #[arg(long)]
    token: Option<AccountId>,
}

impl Collateral {
    fn curve(&self) -> curve::Asset {
        self.token.map_or(curve::Asset::Native, |definition_id| curve::Asset::Token { definition_id })
    }

    fn lbp(&self) -> lbp::Asset {
        self.token.map_or(lbp::Asset::Native, |definition_id| lbp::Asset::Token { definition_id })
    }
}

#[derive(Subcommand)]
enum CurveCmd {
    /// Print the sale and holding PDAs of an id.
    Ids {
        #[arg(long)]
        sale_id: String,
    },
    /// Decode a sale and what the creator may withdraw now.
    Show {
        #[arg(long)]
        sale_id: String,
    },
    /// What a buy of `--collateral-in` does to the sale as the chain holds it
    /// now, computed on the host with `antumbra::Curve`.
    Quote {
        #[arg(long)]
        sale_id: String,
        #[arg(long)]
        collateral_in: u128,
    },
    Create {
        #[arg(long)]
        sale_id: String,
        #[arg(long)]
        creator: AccountId,
        #[arg(long)]
        vt: u128,
        #[arg(long)]
        vc: u128,
        #[arg(long)]
        sale_reserve: u128,
        #[arg(long, default_value_t = 0)]
        seed_reserve: u128,
        #[arg(long)]
        fee_treasury: AccountId,
        /// Millionths; at most 10000 (1%).
        #[arg(long, default_value_t = 0)]
        fee_rate: u128,
        #[command(flatten)]
        collateral: Collateral,
    },
    Buy {
        #[arg(long)]
        sale_id: String,
        #[arg(long)]
        buyer: AccountId,
        #[arg(long)]
        collateral_in: u128,
        #[arg(long, default_value_t = 0)]
        min_tokens_out: u128,
    },
    /// Sweep the accrued fee to the fee treasury. Permissionless; needs `--payer`.
    CollectFees {
        #[arg(long)]
        sale_id: String,
        /// Default: exactly the accrued fee.
        #[arg(long)]
        amount: Option<u128>,
    },
    Withdraw {
        #[arg(long)]
        sale_id: String,
        #[arg(long)]
        creator: AccountId,
        /// Default: the creator.
        #[arg(long)]
        to: Option<AccountId>,
        /// Default: exactly what may be withdrawn now.
        #[arg(long)]
        amount: Option<u128>,
    },
}

#[derive(Subcommand)]
enum LbpCmd {
    /// Print the pool and holding PDAs of an id.
    Ids {
        #[arg(long)]
        pool_id: String,
    },
    /// Decode a pool, its weight at the chain clock, and what a withdrawal pays.
    Show {
        #[arg(long)]
        pool_id: String,
    },
    /// What a buy of `--collateral-in` at `--at` does to the pool as the chain
    /// holds it now, computed on the host with `antumbra::binfixed`.
    Quote {
        #[arg(long)]
        pool_id: String,
        #[arg(long)]
        collateral_in: u128,
        #[arg(long, default_value = "now")]
        at: String,
    },
    Create {
        #[arg(long)]
        pool_id: String,
        #[arg(long)]
        creator: AccountId,
        #[arg(long)]
        reserve_token: u128,
        #[arg(long)]
        reserve_collateral: u128,
        /// 18-decimal fraction of 1e18.
        #[arg(long)]
        w_start: u128,
        #[arg(long)]
        w_end: u128,
        /// Milliseconds since the epoch, `now`, or `now+90s`, `now-5m`, `now+2h`, `now+30d`.
        #[arg(long, default_value = "now")]
        t_start: String,
        #[arg(long)]
        t_end: String,
        #[arg(long)]
        fee_treasury: AccountId,
        /// Millionths; at most 50000 (5%).
        #[arg(long, default_value_t = 0)]
        fee_rate: u128,
        #[command(flatten)]
        collateral: Collateral,
    },
    /// Buy at the weight for `--at`. A time ahead of the chain clock is waited for.
    Buy {
        #[arg(long)]
        pool_id: String,
        #[arg(long)]
        buyer: AccountId,
        #[arg(long)]
        collateral_in: u128,
        #[arg(long, default_value_t = 0)]
        min_tokens_out: u128,
        #[arg(long, default_value = "now")]
        at: String,
    },
    Withdraw {
        #[arg(long)]
        pool_id: String,
        #[arg(long)]
        creator: AccountId,
        /// Default: the creator.
        #[arg(long)]
        to: Option<AccountId>,
        #[arg(long, default_value = "now")]
        at: String,
        /// Default: exactly what may be withdrawn.
        #[arg(long)]
        amount: Option<u128>,
        /// Default: exactly the at-close fee on the amount.
        #[arg(long)]
        fee: Option<u128>,
    },
    SetPaused {
        #[arg(long)]
        pool_id: String,
        #[arg(long)]
        creator: AccountId,
        #[arg(long)]
        paused: u8,
    },
}

/// A 32-byte id: 64 hex digits, or text of at most 32 bytes, zero-padded.
fn id32(s: &str) -> Result<[u8; 32]> {
    if s.len() == 64 {
        if let Ok(v) = hex::decode(s) {
            return Ok(v.try_into().expect("32 bytes"));
        }
    }
    if s.len() > 32 {
        bail!("an id is 64 hex digits or at most 32 bytes of text: {s}");
    }
    Ok(curve::seed_from_str(s))
}

/// The ImageID as a program header stores it: eight u32 words, little-endian.
fn image_hex(program: &Program) -> String {
    program.id().iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

struct Ctx {
    wallet: WalletCore,
    payer: Option<AccountId>,
    force: bool,
    label: String,
}

impl Ctx {
    async fn now(&self) -> Result<u64> {
        let clock = clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
        let acc = self.wallet.get_account_public(clock).await?;
        let shard = acc.data.shard(clock_core::clock_account_id());
        Ok(clock_core::ClockAccountData::from_bytes(shard).timestamp)
    }

    /// `now`, `now+90s`, `now-5m`, or plain milliseconds.
    async fn time(&self, s: &str) -> Result<u64> {
        let Some(rest) = s.strip_prefix("now") else {
            return s.parse().with_context(|| format!("not a time: {s}"));
        };
        let now = self.now().await?;
        if rest.is_empty() {
            return Ok(now);
        }
        let (sign, num) = rest.split_at(1);
        let (digits, unit) = num.split_at(num.len() - 1);
        let n: u64 = digits.parse().with_context(|| format!("not a time: {s}"))?;
        let ms = n * match unit {
            "s" => 1_000,
            "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            _ => bail!("unit is s, m, h or d: {s}"),
        };
        Ok(match sign {
            "+" => now + ms,
            "-" => now - ms,
            _ => bail!("not a time: {s}"),
        })
    }

    /// Block until the chain clock reaches `t`, at most five minutes.
    async fn wait_until(&self, t: u64) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
        while self.now().await? < t {
            if std::time::Instant::now() > deadline {
                bail!("the chain clock did not reach {t} within five minutes");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    async fn shard(&self, account: AccountId, program: AccountId) -> Result<Vec<u8>> {
        Ok(self.wallet.get_account_public(account).await?.data.shard(program).to_vec())
    }

    async fn state<T: borsh::BorshDeserialize>(&self, account: AccountId, program: AccountId, what: &str) -> Result<T> {
        let bytes = self.shard(account, program).await?;
        if bytes.is_empty() {
            bail!("no {what} at {account}");
        }
        Ok(borsh::from_slice(&bytes)?)
    }

    /// Mentions for `rows`, where `signers` sign and the rest do not. An account
    /// named twice is named with one identity, as the wallet requires.
    fn mentions(rows: Vec<ProgramShardSelector>, signers: &[AccountId]) -> Vec<AccountMention> {
        rows.into_iter()
            .map(|r| {
                if signers.contains(&r.account_id) {
                    AccountIdentity::Public(r.account_id).select_program_shard(r.program_account_id)
                } else {
                    AccountIdentity::PublicNoSign(r.account_id).select_program_shard(r.program_account_id)
                }
            })
            .collect()
    }

    /// Send `ix` to `program` and report whether `watch`'s shard of `program` changed.
    async fn send<I: BorshSerialize + Clone>(
        &self,
        op: &str,
        program: AccountId,
        ix: &I,
        rows: Vec<ProgramShardSelector>,
        signers: &[AccountId],
        watch: AccountId,
    ) -> Result<()> {
        let before = self.shard(watch, program).await?;
        let data = Program::serialize_instruction(ix.clone())?;
        let sent = self
            .wallet
            .send_pub_tx_paid_by(Self::mentions(rows, signers), data, program, self.payer)
            .await
            .map_err(|e| anyhow!("{e:?}"));
        let hash = match sent {
            Ok(h) => h,
            Err(e) => {
                println!("{}", json!({"op": op, "label": self.label, "verdict": "rejected", "error": e.to_string()}));
                return Ok(());
            }
        };
        // A transaction outside its validity window is held and never
        // included; three minutes is dozens of blocks.
        let polled = tokio::time::timeout(Duration::from_secs(180), self.wallet.poll_transaction(hash)).await;
        let block = match polled {
            Ok(Ok((_, block))) => Some(block),
            _ => None,
        };
        let after = self.shard(watch, program).await?;
        let verdict = match block {
            None => "not-included",
            Some(_) if after != before => "applied",
            Some(_) => "reverted",
        };
        println!("{}", json!({"op": op, "label": self.label, "tx": hash.to_string(), "block": block, "verdict": verdict}));
        Ok(())
    }

    fn refuse_unless_forced(&self, ok: bool, why: String) -> Result<()> {
        if !ok && !self.force {
            bail!("{why} (use --force to send anyway)");
        }
        Ok(())
    }
}

fn s(x: u128) -> Value {
    json!(x.to_string())
}

fn sale_json(x: &curve::Sale) -> Value {
    json!({
        "vt": s(x.vt), "vc": s(x.vc), "sale_reserve": s(x.sale_reserve), "real_collateral": s(x.real_collateral),
        "seed_reserve": s(x.seed_reserve), "creator": x.creator.to_string(), "treasury": x.treasury.to_string(),
        "fee_treasury": x.fee_treasury.to_string(), "fee_rate": s(x.fee_rate), "fees_accrued": s(x.fees_accrued),
        "withdrawn": s(x.withdrawn), "collateral": x.collateral,
        "collateral_definition": AccountId::new(x.collateral_definition).to_string(),
    })
}

fn pool_json(x: &lbp::Pool) -> Value {
    json!({
        "reserve_token": s(x.reserve_token), "reserve_collateral": s(x.reserve_collateral),
        "w_start": s(x.w_start), "w_end": s(x.w_end), "t_start": x.t_start, "t_end": x.t_end,
        "last_seen": x.last_seen, "paused": x.paused, "creator": x.creator.to_string(),
        "treasury": x.treasury.to_string(), "fee_treasury": x.fee_treasury.to_string(), "fee_rate": s(x.fee_rate),
        "raised": s(x.raised), "withdrawn": s(x.withdrawn), "collateral": x.collateral,
        "collateral_definition": AccountId::new(x.collateral_definition).to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut wallet = WalletCore::from_env().await?;
    if let Some(g) = cli.gas_limit {
        let mut cfg = wallet.config().clone();
        cfg.gas_limit = g;
        wallet.set_config(cfg);
    }
    let ctx = Ctx {
        wallet,
        payer: cli.payer,
        force: cli.force,
        label: cli.label.clone(),
    };
    match cli.cmd {
        Cmd::Deploy { which, payer, elf } => deploy(ctx, which, payer, elf).await,
        Cmd::ImageId { which, elf } => {
            let path = elf.unwrap_or_else(|| which.default_elf());
            let bytecode = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            println!("{}", image_hex(&Program::new(bytecode.into())?));
            Ok(())
        }
        Cmd::Now => {
            println!("{}", ctx.now().await?);
            Ok(())
        }
        Cmd::Curve { program, cmd } => run_curve(&ctx, program, cmd).await,
        Cmd::Lbp { program, cmd } => run_lbp(&ctx, program, cmd).await,
    }
}

async fn deploy(mut ctx: Ctx, which: Which, payer: AccountId, elf: Option<PathBuf>) -> Result<()> {
    let path = elf.unwrap_or_else(|| which.default_elf());
    let bytecode = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let program = Program::new(bytecode.clone().into())?;
    // The loader chunks the user ELF inside the program binary, not the binary itself.
    let user_elf = risc0_binfmt::ProgramBinary::decode(&bytecode).map_err(|e| anyhow!("{e}"))?.user_elf.len();
    let segments = user_elf.div_ceil(program_loader_core::MAX_SEGMENT_DATA_LEN);
    let header = ctx.wallet.create_new_account_public(None).0;
    let segs: Vec<AccountId> = std::iter::repeat_with(|| ctx.wallet.create_new_account_public(None).0).take(segments).collect();
    ctx.wallet.store_persistent_data()?;
    ProgramLoader(&ctx.wallet).deploy(header, &segs, bytecode, false, Some(payer)).await?;
    println!(
        "{}",
        json!({"op": "deploy", "program": path.file_stem().and_then(|s| s.to_str()), "header": header.to_string(), "image_id": image_hex(&program), "segments": segs.len()})
    );
    Ok(())
}

async fn run_curve(ctx: &Ctx, p: AccountId, cmd: CurveCmd) -> Result<()> {
    let sale_of = |sid: &[u8; 32]| curve::sale_account(&p, sid);
    match cmd {
        CurveCmd::Ids { sale_id } => {
            let sid = id32(&sale_id)?;
            println!(
                "{}",
                json!({"sale_id": hex::encode(sid), "sale": sale_of(&sid).to_string(), "holding": curve::holding_account(&p, &sid).to_string()})
            );
        }
        CurveCmd::Show { sale_id } => {
            let sid = id32(&sale_id)?;
            let x: curve::Sale = ctx.state(sale_of(&sid), p, "sale").await?;
            let mut v = sale_json(&x);
            v["closed"] = json!(x.curve().is_closed());
            v["withdrawable"] = s(curve::withdrawable(&x));
            println!("{v}");
        }
        CurveCmd::Quote { sale_id, collateral_in } => {
            let sid = id32(&sale_id)?;
            let x: curve::Sale = ctx.state(sale_of(&sid), p, "sale").await?;
            match curve::quote_buy(&x, collateral_in, 0) {
                Ok((fee, out, after)) => println!("{}", json!({"fee": s(fee), "tokens_out": s(out), "after": sale_json(&after)})),
                Err(code) => println!("{}", json!({"refused": code})),
            }
        }
        CurveCmd::Create { sale_id, creator, vt, vc, sale_reserve, seed_reserve, fee_treasury, fee_rate, collateral } => {
            let sid = id32(&sale_id)?;
            let ix = curve::Instruction::CreateSale {
                sale_id: sid,
                collateral: collateral.curve(),
                vt,
                vc,
                sale_reserve,
                seed_reserve,
                fee_treasury,
                fee_rate,
            };
            ctx.send("curve-create", p, &ix, curve::rows::create(&p, &sid, creator), &[creator], sale_of(&sid)).await?;
        }
        CurveCmd::Buy { sale_id, buyer, collateral_in, min_tokens_out } => {
            let sid = id32(&sale_id)?;
            let x: curve::Sale = ctx.state(sale_of(&sid), p, "sale").await?;
            let quote = curve::quote_buy(&x, collateral_in, min_tokens_out);
            ctx.refuse_unless_forced(quote.is_ok(), format!("the program will refuse this buy: E{}", quote.as_ref().err().unwrap_or(&0)))?;
            let asset = x.asset();
            let ix = curve::Instruction::ExecuteBuy { sale_id: sid, collateral: asset, collateral_in, min_tokens_out };
            ctx.send("curve-buy", p, &ix, curve::rows::buy(&p, &sid, &asset, buyer), &[buyer], sale_of(&sid)).await?;
        }
        CurveCmd::CollectFees { sale_id, amount } => {
            let sid = id32(&sale_id)?;
            let x: curve::Sale = ctx.state(sale_of(&sid), p, "sale").await?;
            let amount = amount.unwrap_or(x.fees_accrued);
            ctx.refuse_unless_forced(
                amount > 0 && amount == x.fees_accrued,
                format!("the accrued fee is {}; refusing to sweep {amount}", x.fees_accrued),
            )?;
            let asset = x.asset();
            let ix = curve::Instruction::CollectFees { sale_id: sid, collateral: asset, amount };
            let rows = curve::rows::collect_fees(&p, &sid, &asset, x.fee_treasury);
            ctx.send("curve-collect-fees", p, &ix, rows, &[], sale_of(&sid)).await?;
        }
        CurveCmd::Withdraw { sale_id, creator, to, amount } => {
            let sid = id32(&sale_id)?;
            let x: curve::Sale = ctx.state(sale_of(&sid), p, "sale").await?;
            let owed = curve::withdrawable(&x);
            let amount = amount.unwrap_or(owed);
            ctx.refuse_unless_forced(amount > 0 && amount == owed, format!("withdrawable now is {owed}; refusing to send {amount}"))?;
            let asset = x.asset();
            let ix = curve::Instruction::Withdraw { sale_id: sid, collateral: asset, amount };
            let rows = curve::rows::withdraw(&p, &sid, &asset, to.unwrap_or(creator), creator);
            ctx.send("curve-withdraw", p, &ix, rows, &[creator], sale_of(&sid)).await?;
        }
    }
    Ok(())
}

async fn run_lbp(ctx: &Ctx, p: AccountId, cmd: LbpCmd) -> Result<()> {
    let pool_of = |pid: &[u8; 32]| lbp::pool_account(&p, pid);
    match cmd {
        LbpCmd::Ids { pool_id } => {
            let pid = id32(&pool_id)?;
            println!(
                "{}",
                json!({"pool_id": hex::encode(pid), "pool": pool_of(&pid).to_string(), "holding": lbp::holding_account(&p, &pid).to_string()})
            );
        }
        LbpCmd::Show { pool_id } => {
            let pid = id32(&pool_id)?;
            let x: lbp::Pool = ctx.state(pool_of(&pid), p, "pool").await?;
            let now = ctx.now().await?;
            let mut v = pool_json(&x);
            v["clock"] = json!(now);
            v["weight_now"] = lbp::weight(&x, now).map_or(Value::Null, s);
            v["withdrawal"] = match lbp::withdrawal(&x) {
                Ok((amount, fee, to_creator)) => json!({"amount": s(amount), "fee": s(fee), "to_creator": s(to_creator)}),
                Err(_) => Value::Null,
            };
            println!("{v}");
        }
        LbpCmd::Quote { pool_id, collateral_in, at } => {
            let pid = id32(&pool_id)?;
            let x: lbp::Pool = ctx.state(pool_of(&pid), p, "pool").await?;
            let at = ctx.time(&at).await?;
            match lbp::quote_buy(&x, at, collateral_in, 0) {
                Ok((out, after)) => {
                    let w = lbp::weight(&x, at).map_err(|c| anyhow!("E{c}"))?;
                    println!("{}", json!({"at": at, "weight": s(w), "tokens_out": s(out), "after": pool_json(&after)}));
                }
                Err(code) => println!("{}", json!({"refused": code})),
            }
        }
        LbpCmd::Create {
            pool_id,
            creator,
            reserve_token,
            reserve_collateral,
            w_start,
            w_end,
            t_start,
            t_end,
            fee_treasury,
            fee_rate,
            collateral,
        } => {
            let pid = id32(&pool_id)?;
            let ix = lbp::Instruction::CreatePool {
                pool_id: pid,
                collateral: collateral.lbp(),
                reserve_token,
                reserve_collateral,
                w_start,
                w_end,
                t_start: ctx.time(&t_start).await?,
                t_end: ctx.time(&t_end).await?,
                fee_treasury,
                fee_rate,
            };
            ctx.send("lbp-create", p, &ix, lbp::rows::handle(&p, &pid, creator), &[creator], pool_of(&pid)).await?;
        }
        LbpCmd::Buy { pool_id, buyer, collateral_in, min_tokens_out, at } => {
            let pid = id32(&pool_id)?;
            let at = ctx.time(&at).await?;
            ctx.wait_until(at).await?;
            let x: lbp::Pool = ctx.state(pool_of(&pid), p, "pool").await?;
            let quote = lbp::quote_buy(&x, at, collateral_in, min_tokens_out);
            ctx.refuse_unless_forced(quote.is_ok(), format!("the program will refuse this buy: E{}", quote.as_ref().err().unwrap_or(&0)))?;
            let asset = x.asset();
            let ix = lbp::Instruction::ExecuteBuy { pool_id: pid, collateral: asset, now: at, collateral_in, min_tokens_out };
            ctx.send("lbp-buy", p, &ix, lbp::rows::buy(&p, &pid, &asset, buyer), &[buyer], pool_of(&pid)).await?;
        }
        LbpCmd::Withdraw { pool_id, creator, to, at, amount, fee } => {
            let pid = id32(&pool_id)?;
            let at = ctx.time(&at).await?;
            ctx.wait_until(at).await?;
            let x: lbp::Pool = ctx.state(pool_of(&pid), p, "pool").await?;
            let (owed, owed_fee, _) = lbp::withdrawal(&x).unwrap_or((0, 0, 0));
            let amount = amount.unwrap_or(owed);
            let fee = fee.unwrap_or(owed_fee);
            ctx.refuse_unless_forced(
                at >= x.t_end && amount > 0 && amount == owed && fee == owed_fee,
                format!("at {at} (t_end {}) the withdrawal is {owed} with fee {owed_fee}; refusing {amount} with fee {fee}", x.t_end),
            )?;
            let asset = x.asset();
            let ix = lbp::Instruction::Withdraw { pool_id: pid, collateral: asset, now: at, amount, fee };
            let rows = lbp::rows::withdraw(&p, &pid, &asset, to.unwrap_or(creator), x.fee_treasury, creator);
            ctx.send("lbp-withdraw", p, &ix, rows, &[creator], pool_of(&pid)).await?;
        }
        LbpCmd::SetPaused { pool_id, creator, paused } => {
            let pid = id32(&pool_id)?;
            let ix = lbp::Instruction::SetPaused { pool_id: pid, paused };
            ctx.send("lbp-set-paused", p, &ix, lbp::rows::handle(&p, &pid, creator), &[creator], pool_of(&pid)).await?;
        }
    }
    Ok(())
}
