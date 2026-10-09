//! `antumbra-vesting`: create, claim (public or private), cancel, transfer and
//! inspect antumbra_vesting schedules on LEZ v0.3.
//!
//! The commands that only read (`show`, `ids`, `image-id`, `now`) need no
//! wallet: they ask the sequencer at `--rpc` (`ANTUMBRA_RPC`, by default the
//! public testnet) or compute locally. The commands that sign take the keys,
//! the sequencer URL and the fee settings from the v0.3 wallet home
//! (`LEE_WALLET_HOME_DIR`). The program's header account comes from
//! `--program` or `ANTUMBRA_PROGRAM`, by default the one deployed on the
//! public testnet v0.3.
//!
//! Every transaction prints one JSON line on stdout: the operation, its hash,
//! the block it was included in, and a verdict read back from the chain.
//! `applied` means the schedule shard changed as the instruction intends;
//! `reverted` means the transaction was included (and, if public, charged) but
//! the program refused it; `rejected` means the sequencer never took it.

use std::{collections::HashMap, path::PathBuf, str::FromStr, time::Duration};

use antumbra_vesting_core::{
    self as core, batch_schedule_id, claimable, holding_account, schedule_account, unvested, Asset, Instruction,
    Terms, VestingSchedule,
};
use anyhow::{anyhow, bail, ensure, Context as _, Result};
use clap::{Parser, Subcommand, ValueEnum};
use lee::{
    privacy_preserving_transaction::circuit::ProgramWithDependencies, program::Program, AccountId,
    ProgramShardSelector,
};
use sequencer_service_rpc::{RpcClient as _, SequencerClient, SequencerClientBuilder};
use wallet::{program_facades::program_loader::ProgramLoader, AccountIdentity, AccountMention, WalletCore};

/// The program deployed on the public testnet v0.3 (see DEPLOYMENTS.md).
const DEPLOYED_PROGRAM: &str = "FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X";
/// The public testnet v0.3 sequencer.
const TESTNET_RPC: &str = "https://testnet.lez.logos.co";
/// More members than any batch can hold under the 10M gas cap (450).
const BATCH_SCAN_LIMIT: u32 = 4096;

#[derive(Parser)]
#[command(name = "antumbra-vesting", version, about = "antumbra_vesting on LEZ v0.3")]
struct Cli {
    /// The program's header account (base58). Default: the one deployed on
    /// the public testnet v0.3.
    #[arg(long, env = "ANTUMBRA_PROGRAM", global = true, default_value = DEPLOYED_PROGRAM)]
    program: AccountId,
    /// The sequencer the read-only commands ask. Commands that sign use the
    /// wallet's own sequencer instead.
    #[arg(long, env = "ANTUMBRA_RPC", global = true, default_value = TESTNET_RPC)]
    rpc: String,
    /// The guest binary, needed to prove a private claim locally.
    #[arg(long, env = "ANTUMBRA_ELF", global = true, default_value = "artifacts/programs/v0.3/antumbra_vesting.bin")]
    elf: PathBuf,
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
enum KindArg {
    Cliff,
    Linear,
    Milestones,
}

#[derive(clap::Args, Clone)]
struct TermsArgs {
    #[arg(long, value_enum)]
    kind: KindArg,
    /// Times: milliseconds since the epoch, `now`, or `now+90s`, `now-5m`, `now+2h`, `now+30d`,
    /// relative to the chain's clock.
    #[arg(long, default_value = "now")]
    start: String,
    #[arg(long)]
    cliff: Option<String>,
    #[arg(long, default_value = "now+30m")]
    end: String,
    /// Per schedule.
    #[arg(long)]
    total: u128,
    #[arg(long, default_value_t = 0)]
    tranches: u32,
    #[arg(long)]
    non_cancelable: bool,
    #[arg(long)]
    transferable: bool,
    #[arg(long)]
    cancel_authority: Option<AccountId>,
    #[arg(long)]
    milestone_authority: Option<AccountId>,
    /// The account a cancellation refunds into (its native or token shard).
    #[arg(long)]
    refund_to: AccountId,
    /// A token definition; omit for native balance.
    #[arg(long)]
    token: Option<AccountId>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Deploy the guest through program_loader into fresh wallet accounts; prints the header.
    Deploy {
        #[arg(long)]
        payer: AccountId,
        /// Deploy into this header account, already in the wallet, instead of
        /// a fresh one. After a testnet reset this keeps the program id, and so
        /// every schedule and escrow PDA derived from it. Needs `--segment`.
        #[arg(long, requires = "segment")]
        header: Option<AccountId>,
        /// A segment account already in the wallet, in order; with `--header`.
        #[arg(long, requires = "header")]
        segment: Vec<AccountId>,
    },
    /// Print the chain clock (ms). Reads `--rpc`; needs no wallet.
    Now,
    /// Print the ImageID of `--elf` as the 32 bytes a program header stores,
    /// in hex. Computed locally; needs no wallet and no network.
    ImageId,
    /// Print the schedule and holding PDAs of an id. Computed locally; needs
    /// no wallet and no network.
    Ids {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        batch_id: Option<String>,
    },
    /// Decode a schedule and what is claimable now, or with `--batch-id`
    /// every schedule of a batch and their totals. Reads `--rpc`; needs no
    /// wallet.
    Show {
        /// The schedule id (same as `--schedule-id`).
        #[arg(conflicts_with_all = ["schedule_id", "batch_id"])]
        id: Option<String>,
        #[arg(long, conflicts_with = "batch_id")]
        schedule_id: Option<String>,
        /// List the batch's schedules, numbered from 0, until one is missing.
        #[arg(long)]
        batch_id: Option<String>,
    },
    Create {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        beneficiary: AccountId,
        #[arg(long)]
        creator: AccountId,
        #[command(flatten)]
        terms: TermsArgs,
    },
    Batch {
        #[arg(long)]
        batch_id: String,
        /// Comma-separated beneficiaries; `--repeat N` repeats the list.
        #[arg(long, value_delimiter = ',')]
        beneficiaries: Vec<AccountId>,
        #[arg(long, default_value_t = 1)]
        repeat: u32,
        #[arg(long)]
        creator: AccountId,
        #[command(flatten)]
        terms: TermsArgs,
    },
    /// Claim into `--to Public/<id>` or `--to Private/<id>` (proved locally).
    Claim {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        batch_id: Option<String>,
        #[arg(long)]
        beneficiary: AccountId,
        #[arg(long)]
        to: String,
        /// Default: everything claimable at `--at`.
        #[arg(long)]
        amount: Option<u128>,
        #[arg(long, default_value = "now")]
        at: String,
    },
    Cancel {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        batch_id: Option<String>,
        #[arg(long)]
        authority: AccountId,
        #[arg(long, default_value = "now")]
        at: String,
        /// Default: exactly the unvested part at `--at`.
        #[arg(long)]
        refund: Option<u128>,
    },
    MakeNonCancelable {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        creator: AccountId,
    },
    Transfer {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        beneficiary: AccountId,
        #[arg(long)]
        to: AccountId,
    },
    Signal {
        #[arg(long)]
        schedule_id: String,
        #[arg(long)]
        authority: AccountId,
        #[arg(long)]
        index: u32,
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
    Ok(core::seed_from_str(s))
}

struct Ctx {
    /// Only the commands that sign open the wallet.
    wallet: Option<WalletCore>,
    rpc: SequencerClient,
    program: AccountId,
    elf: PathBuf,
    payer: Option<AccountId>,
    force: bool,
    label: String,
}

impl Ctx {
    fn program(&self) -> Result<AccountId> {
        Ok(self.program)
    }

    fn wallet(&self) -> &WalletCore {
        self.wallet.as_ref().expect("a signing command opens the wallet")
    }

    /// A public account, from the wallet's sequencer when the command signs
    /// and from `--rpc` otherwise.
    async fn account(&self, id: AccountId) -> Result<lee::Account> {
        match &self.wallet {
            Some(w) => w.get_account_public(id).await,
            None => self.rpc.get_account(id).await.map_err(|e| anyhow!("reading {id} from the sequencer: {e}")),
        }
    }

    async fn now(&self) -> Result<u64> {
        let clock = clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
        let acc = self.account(clock).await?;
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

    async fn schedule_shard(&self, sid: &[u8; 32]) -> Result<Vec<u8>> {
        let program = self.program()?;
        let acc = self.account(schedule_account(&program, sid)).await?;
        Ok(acc.data.shard(program).to_vec())
    }

    async fn schedule(&self, sid: &[u8; 32]) -> Result<VestingSchedule> {
        let bytes = self.schedule_shard(sid).await?;
        if bytes.is_empty() {
            bail!("no schedule at id {}", hex::encode(sid));
        }
        Ok(borsh::from_slice(&bytes)?)
    }

    fn mention(&self, id: AccountId, shard: AccountId, signs: bool) -> AccountMention {
        if signs {
            AccountIdentity::Public(id).select_program_shard(shard)
        } else {
            AccountIdentity::PublicNoSign(id).select_program_shard(shard)
        }
    }

    /// Mentions for `rows`, where `signers` sign and the rest do not. An account
    /// named twice is named with one identity, as the wallet requires.
    fn mentions(&self, rows: Vec<ProgramShardSelector>, signers: &[AccountId]) -> Vec<AccountMention> {
        rows.into_iter()
            .map(|r| self.mention(r.account_id, r.program_account_id, signers.contains(&r.account_id)))
            .collect()
    }

    async fn send_public(&self, op: &str, ix: &Instruction, mentions: Vec<AccountMention>, watch: [u8; 32]) -> Result<()> {
        let program = self.program()?;
        let before = self.schedule_shard(&watch).await?;
        let data = Program::serialize_instruction(ix.clone())?;
        let sent = self.wallet().send_pub_tx_paid_by(mentions, data, program, self.payer).await;
        self.report(op, sent.map_err(|e| anyhow!("{e:?}")), &watch, before).await
    }

    async fn report(&self, op: &str, sent: Result<common::HashType>, watch: &[u8; 32], before: Vec<u8>) -> Result<()> {
        let hash = match sent {
            Ok(h) => h,
            Err(e) => {
                let line = serde_json::json!({"op": op, "label": self.label, "verdict": "rejected", "error": e.to_string()});
                println!("{line}");
                return Ok(());
            }
        };
        // A transaction outside its validity window is held and never
        // included; three minutes is dozens of blocks.
        let polled = tokio::time::timeout(Duration::from_secs(180), self.wallet().poll_transaction(hash)).await;
        let block = match polled {
            Ok(Ok((_, block))) => Some(block),
            _ => None,
        };
        let after = self.schedule_shard(watch).await?;
        let verdict = match block {
            None => "not-included",
            Some(_) if after != before => "applied",
            Some(_) => "reverted",
        };
        let line = serde_json::json!({
            "op": op, "label": self.label, "tx": hash.to_string(), "block": block, "verdict": verdict,
        });
        println!("{line}");
        Ok(())
    }
}

/// The ImageID as a program header stores it: eight u32 words, little-endian.
fn image_hex(program: &Program) -> String {
    program.id().iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

fn asset_of(s: &VestingSchedule) -> Asset {
    if s.asset == 0 {
        Asset::Native
    } else {
        Asset::Token { definition_id: AccountId::new(s.token_definition) }
    }
}

async fn terms(ctx: &Ctx, t: &TermsArgs) -> Result<(Terms, Asset)> {
    let kind = match t.kind {
        KindArg::Cliff => 0,
        KindArg::Linear => 1,
        KindArg::Milestones => 2,
    };
    let start = ctx.time(&t.start).await?;
    let cliff = match &t.cliff {
        Some(c) => ctx.time(c).await?,
        None => start,
    };
    let terms = Terms {
        kind,
        start,
        cliff,
        end: ctx.time(&t.end).await?,
        total: t.total,
        tranches: t.tranches,
        cancelable: !t.non_cancelable,
        transferable: t.transferable,
        cancel_authority: t.cancel_authority.unwrap_or_default(),
        milestone_authority: t.milestone_authority.unwrap_or_default(),
        refund_to: t.refund_to,
    };
    let asset = t.token.map_or(Asset::Native, |definition_id| Asset::Token { definition_id });
    Ok((terms, asset))
}

fn schedule_json(s: &VestingSchedule, now: u64) -> serde_json::Value {
    serde_json::json!({
        "kind": s.kind, "start": s.start, "cliff": s.cliff, "end": s.end,
        "total": s.total.to_string(), "claimed": s.claimed.to_string(),
        "last_seen": s.last_seen, "beneficiary": s.beneficiary.to_string(),
        "escrow": s.escrow.to_string(), "creator": s.creator.to_string(),
        "cancelable": s.cancelable, "transferable": s.transferable,
        "cancelled_at": s.cancelled_at, "signalled": s.signalled, "tranches": s.tranches,
        "asset": s.asset, "token_definition": AccountId::new(s.token_definition).to_string(),
        "refund_to": s.refund_to.to_string(), "cancel_authority": s.cancel_authority.to_string(),
        "milestone_authority": s.milestone_authority.to_string(),
        "clock": now, "vested": core::vested(s, now).to_string(),
        "claimable": claimable(s, now).to_string(),
    })
}

/// Every schedule of a batch: member `i` has id `batch_schedule_id(batch, i)`,
/// the scan stops at the first id with no schedule. One line per member, then
/// one line with the batch's totals.
async fn show_batch(ctx: &Ctx, batch: &str, now: u64) -> Result<()> {
    let p = ctx.program()?;
    let bid = id32(batch)?;
    let (mut n, mut total, mut claimed, mut owed) = (0u32, 0u128, 0u128, 0u128);
    while n < BATCH_SCAN_LIMIT {
        let sid = batch_schedule_id(&bid, n);
        let bytes = ctx.schedule_shard(&sid).await?;
        if bytes.is_empty() {
            break;
        }
        let s: VestingSchedule = borsh::from_slice(&bytes)?;
        let c = claimable(&s, now);
        println!(
            "{}",
            serde_json::json!({
                "index": n, "schedule_id": hex::encode(sid),
                "schedule": schedule_account(&p, &sid).to_string(),
                "beneficiary": s.beneficiary.to_string(),
                "total": s.total.to_string(), "claimed": s.claimed.to_string(),
                "vested": core::vested(&s, now).to_string(), "claimable": c.to_string(),
                "cancelled_at": s.cancelled_at,
            })
        );
        total += s.total;
        claimed += s.claimed;
        owed += c;
        n += 1;
    }
    if n == 0 {
        bail!("no schedule in batch {} (its schedule 0 is not on chain)", hex::encode(bid));
    }
    println!(
        "{}",
        serde_json::json!({
            "batch_id": hex::encode(bid), "holding": holding_account(&p, &bid).to_string(),
            "schedules": n, "total": total.to_string(), "claimed": claimed.to_string(),
            "claimable": owed.to_string(), "clock": now,
        })
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let rpc = SequencerClientBuilder::default()
        .build(&cli.rpc)
        .with_context(|| format!("not a sequencer URL: {}", cli.rpc))?;
    let signs = !matches!(cli.cmd, Cmd::Now | Cmd::ImageId | Cmd::Ids { .. } | Cmd::Show { .. });
    let wallet = if signs {
        let mut wallet = WalletCore::from_env()
            .await
            .context("this command signs, so it needs a v0.3 wallet home (LEE_WALLET_HOME_DIR); show, ids, image-id and now do not")?;
        if let Some(g) = cli.gas_limit {
            let mut cfg = wallet.config().clone();
            cfg.gas_limit = g;
            wallet.set_config(cfg);
        }
        Some(wallet)
    } else {
        None
    };
    let ctx = Ctx {
        wallet,
        rpc,
        program: cli.program,
        elf: cli.elf.clone(),
        payer: cli.payer,
        force: cli.force,
        label: cli.label.clone(),
    };
    run(ctx, cli.cmd).await
}

async fn run(mut ctx: Ctx, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Deploy { payer, header, segment } => {
            let bytecode = std::fs::read(&ctx.elf).with_context(|| format!("reading {}", ctx.elf.display()))?;
            let program = Program::new(bytecode.clone().into())?;
            // The loader chunks the user ELF inside the program binary, not the binary itself.
            let user_elf = risc0_binfmt::ProgramBinary::decode(&bytecode).map_err(|e| anyhow!("{e}"))?.user_elf.len();
            let segments = user_elf.div_ceil(program_loader_core::MAX_SEGMENT_DATA_LEN);
            let wallet = ctx.wallet.as_mut().expect("deploy opens the wallet");
            let (header, segs) = match header {
                Some(h) => {
                    ensure!(segment.len() == segments, "this binary needs {segments} segment accounts, got {}", segment.len());
                    (h, segment)
                }
                None => {
                    let h = wallet.create_new_account_public(None).0;
                    let s: Vec<AccountId> =
                        std::iter::repeat_with(|| wallet.create_new_account_public(None).0).take(segments).collect();
                    wallet.store_persistent_data()?;
                    (h, s)
                }
            };
            ProgramLoader(&*wallet).deploy(header, &segs, bytecode, false, Some(payer)).await?;
            println!("{}", serde_json::json!({"op": "deploy", "header": header.to_string(), "image_id": image_hex(&program), "segments": segs.len()}));
        }
        Cmd::Now => println!("{}", ctx.now().await?),
        Cmd::ImageId => {
            let bytecode = std::fs::read(&ctx.elf).with_context(|| format!("reading {}", ctx.elf.display()))?;
            println!("{}", image_hex(&Program::new(bytecode.into())?));
        }
        Cmd::Ids { schedule_id, batch_id } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let seed = match &batch_id {
                Some(b) => id32(b)?,
                None => sid,
            };
            println!(
                "{}",
                serde_json::json!({
                    "schedule_id": hex::encode(sid),
                    "schedule": schedule_account(&p, &sid).to_string(),
                    "holding": holding_account(&p, &seed).to_string(),
                })
            );
        }
        Cmd::Show { id, schedule_id, batch_id } => {
            let now = ctx.now().await?;
            if let Some(batch) = batch_id {
                return show_batch(&ctx, &batch, now).await;
            }
            let Some(schedule_id) = id.or(schedule_id) else {
                bail!("show takes a schedule id, or --batch-id <id>");
            };
            let sid = id32(&schedule_id)?;
            let s = ctx.schedule(&sid).await?;
            println!("{}", schedule_json(&s, now));
        }
        Cmd::Create { schedule_id, beneficiary, creator, terms: t } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let (terms, asset) = terms(&ctx, &t).await?;
            let rows = core::rows::create(&p, &sid, &asset, creator);
            let ix = Instruction::CreateSchedule { schedule_id: sid, beneficiary, asset, terms };
            let m = ctx.mentions(rows, &[creator]);
            ctx.send_public("create", &ix, m, sid).await?;
        }
        Cmd::Batch { batch_id, beneficiaries, repeat, creator, terms: t } => {
            let p = ctx.program()?;
            let bid = id32(&batch_id)?;
            let (terms, asset) = terms(&ctx, &t).await?;
            let bens: Vec<AccountId> =
                (0..repeat).flat_map(|_| beneficiaries.iter().copied()).collect();
            let n = u32::try_from(bens.len())?;
            let rows = core::rows::batch(&p, &bid, n, &asset, creator);
            let ix = Instruction::CreateScheduleBatch { batch_id: bid, beneficiaries: bens, asset, terms };
            let m = ctx.mentions(rows, &[creator]);
            ctx.send_public(&format!("batch[{n}]"), &ix, m, batch_schedule_id(&bid, 0)).await?;
        }
        Cmd::Claim { schedule_id, batch_id, beneficiary, to, amount, at } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let bid = batch_id.as_deref().map(id32).transpose()?;
            let s = ctx.schedule(&sid).await?;
            let asset = asset_of(&s);
            let at = ctx.time(&at).await?;
            let owed = claimable(&s, at);
            let amount = amount.unwrap_or(owed);
            if !ctx.force && (amount == 0 || amount > owed) {
                bail!("claimable at {at} is {owed}; refusing to send {amount} (use --force to send anyway)");
            }
            let ix = Instruction::Claim { schedule_id: sid, batch_id: bid, asset, amount, at };
            let (private, dest) = match to.split_once('/') {
                Some(("Private", id)) => (true, AccountId::from_str(id)?),
                Some(("Public", id)) => (false, AccountId::from_str(id)?),
                _ => bail!("--to is Public/<id> or Private/<id>"),
            };
            let rows = core::rows::claim(&p, &sid, bid.as_ref(), &asset, dest, beneficiary);
            if !private {
                let m = ctx.mentions(rows, &[beneficiary]);
                return ctx.send_public("claim", &ix, m, sid).await;
            }
            // Private: the destination is a shielded account the wallet owns.
            // The schedule, the escrow and the beneficiary's signature are
            // public; the destination and the amount it received are not.
            let mut m = ctx.mentions(rows[..2].to_vec(), &[]);
            m.push(AccountIdentity::PrivateOwned(dest).select_program_shard(asset.shard_program()));
            m.push(ctx.mention(beneficiary, p, true));
            let bytecode = std::fs::read(&ctx.elf).with_context(|| format!("reading {}", ctx.elf.display()))?;
            let mut deps = HashMap::new();
            if matches!(asset, Asset::Token { .. }) {
                deps.insert(programs::token_account_id(), programs::token());
            }
            let pwd = ProgramWithDependencies::new(Program::new(bytecode.into())?, p, deps);
            let before = ctx.schedule_shard(&sid).await?;
            eprintln!("proving the private claim locally; this takes minutes");
            let sent = ctx
                .wallet()
                .send_privacy_preserving_tx(m, Program::serialize_instruction(ix)?, &pwd)
                .await
                .map(|(h, _)| h)
                .map_err(|e| anyhow!("{e:?}"));
            ctx.report("claim-private", sent, &sid, before).await?;
            // Pick up the new note so `wallet account get` shows the balance.
            ctx.wallet.as_mut().expect("claim opens the wallet").sync_to_latest_block().await?;
        }
        Cmd::Cancel { schedule_id, batch_id, authority, at, refund } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let bid = batch_id.as_deref().map(id32).transpose()?;
            let s = ctx.schedule(&sid).await?;
            let asset = asset_of(&s);
            let at = ctx.time(&at).await?;
            let exact = unvested(&s, at);
            let refund = refund.unwrap_or(exact);
            if !ctx.force && refund != exact {
                bail!("the unvested part at {at} is {exact}; refusing to send {refund}");
            }
            let ix = Instruction::Cancel { schedule_id: sid, batch_id: bid, asset, at, refund };
            let rows = core::rows::cancel(&p, &sid, bid.as_ref(), &asset, s.refund_to, authority);
            let m = ctx.mentions(rows, &[authority]);
            ctx.send_public("cancel", &ix, m, sid).await?;
        }
        Cmd::MakeNonCancelable { schedule_id, creator } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let ix = Instruction::MakeNonCancelable { schedule_id: sid };
            let m = ctx.mentions(core::rows::handle(&p, &sid, creator), &[creator]);
            ctx.send_public("make-non-cancelable", &ix, m, sid).await?;
        }
        Cmd::Transfer { schedule_id, beneficiary, to } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let ix = Instruction::TransferBeneficiary { schedule_id: sid, new_beneficiary: to };
            let m = ctx.mentions(core::rows::handle(&p, &sid, beneficiary), &[beneficiary]);
            ctx.send_public("transfer", &ix, m, sid).await?;
        }
        Cmd::Signal { schedule_id, authority, index } => {
            let p = ctx.program()?;
            let sid = id32(&schedule_id)?;
            let ix = Instruction::SignalMilestone { schedule_id: sid, index };
            let m = ctx.mentions(core::rows::handle(&p, &sid, authority), &[authority]);
            ctx.send_public("signal", &ix, m, sid).await?;
        }
    }
    Ok(())
}
