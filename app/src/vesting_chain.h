// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Reading antumbra_vesting schedules from a LEZ v0.3 chain, with QtCore and
// QtNetwork only, so it is tested without Basecamp (tests/chain_test.cpp).
//
// On v0.3 an account is a map of shards keyed by program id. A schedule is the
// vesting program's shard of the PDA [schedule_id] of the program's header; its
// escrow is the native or token shard of the PDA [id, "holding"], where id is
// the schedule's own id or its batch's. Native balance is the shard at the
// all-zero id; a token balance is the token program's shard.
//
// Nothing here signs. The reader turns chain bytes into JSON for the view, and
// computes what has vested against the chain's own clock account, never the
// machine's clock.
#pragma once

#include <functional>
#include <memory>

#include <QByteArray>
#include <QJsonArray>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QObject>
#include <QString>

namespace av {

using u128 = unsigned __int128;

// ── Encodings and addresses ──────────────────────────────────────────────────
QByteArray sha256(const QByteArray& in);
QString base58(const QByteArray& in);
// 32 bytes, or empty when `s` is not base58 for exactly 32 bytes.
QByteArray unbase58(const QString& s);
QString dec(u128 v);
// "1234567" from a decimal string, or 0 on overflow/garbage.
u128 parseDec(const QString& s, bool* ok = nullptr);

// A 32-byte id: 64 hex digits, or text of at most 32 bytes, zero-padded, the
// way the CLI reads `--schedule-id` and `--batch-id`. Empty and `err` set when
// it is neither.
QByteArray idBytes(const QString& s, QString* err = nullptr);
// How the CLI would be given this id back: the text when it is printable text
// followed by zero padding, else 64 hex digits.
QString idText(const QByteArray& id);
QByteArray textSeed(const char* s);
QByteArray combineSeeds(const QList<QByteArray>& seeds);
QByteArray pda(const QByteArray& program, const QByteArray& seed);
QByteArray scheduleAccount(const QByteArray& program, const QByteArray& scheduleId);
QByteArray holdingAccount(const QByteArray& program, const QByteArray& id);
QByteArray batchScheduleId(const QByteArray& batchId, quint32 i);

// ── The schedule ─────────────────────────────────────────────────────────────
struct Schedule {
    quint8 kind = 0;  // 0 cliff + linear, 1 linear, 2 milestones
    quint64 start = 0, cliff = 0, end = 0;
    u128 total = 0, claimed = 0;
    quint64 lastSeen = 0;
    QByteArray beneficiary, escrow, creator;
    quint8 cancelable = 0, transferable = 0;
    quint64 cancelledAt = 0, signalled = 0;
    quint32 tranches = 0;
    quint8 asset = 0;  // 0 native, 1 token
    QByteArray tokenDefinition, refundTo, cancelAuthority, milestoneAuthority;
};
// VestingSchedule's borsh layout, 312 bytes. False when the bytes are not one.
bool decodeSchedule(const QByteArray& b, Schedule& out);
// The program's own accrual rule (core::vested).
u128 vestedAt(const Schedule& s, quint64 now);

// The view's JSON for a schedule: amounts as decimal strings, times as
// milliseconds, accounts as base58, plus derived state and status chips.
QJsonObject scheduleJson(const Schedule& s, quint64 now);

// ── What a public claim costs (RFP-017 U4) ──────────────────────────────────
// LEZ v0.3 reserves gas_limit x base_fee_exec + data_bytes x base_fee_stor from
// the payer when it takes a public transaction, and charges
// min(cycles, gas_limit) x base_fee_exec + data_bytes x base_fee_stor
// (lez/programs/fee/core/src/assess.rs:75 and :90 at db66590a). A private
// transaction is exempt (lez/chain_state/src/classify.rs:53).
constexpr quint64 kClaimGasLimit = 300000;      // what the CLI's runbook declares
constexpr quint64 kClaimDataBytes = 700;        // claims on testnet measured 653 (native) and 685 (token) bytes
constexpr quint64 kClaimCyclesNative = 49024;   // executor-tests/CYCLES.md, claim (native)
constexpr quint64 kClaimCyclesToken = 87913;    // executor-tests/CYCLES.md, claim (token)
constexpr quint64 kBaseFeeMin = 8;              // BASE_FEE_EXEC_MIN and _STOR_MIN, lez/programs/fee/core/src/market.rs:17 and :29
// {gasLimit, dataBytes, cycles, baseFeeExec, baseFeeStor, reserve, estimate}, amounts as decimal strings.
QJsonObject claimFee(quint64 baseFeeExec, quint64 baseFeeStor, bool token);

// ── Transactions on a schedule, as the explorer's index lists them ──────────
// Each Public transaction of `program` is decoded and replayed against the
// schedule's rules, so a refused attempt is told apart from an applied one.
// `final` is the schedule as the chain holds it now; the replay is checked
// against it and `consistent` says whether they agree.
QJsonObject replayActivity(const QJsonArray& txs, const QByteArray& program, const QByteArray& scheduleAcc,
                           const Schedule& final);

// ── The reader ───────────────────────────────────────────────────────────────
class Chain : public QObject {
    Q_OBJECT
public:
    explicit Chain(QObject* parent = nullptr);

    QString rpc, explorer;
    QByteArray program;  // 32 bytes

    using Done = std::function<void(const QJsonObject&)>;

    // {block, clock} or {error}
    void status(Done done);
    void lookup(const QString& query, Done done);
    void openSchedule(const QByteArray& account, const QByteArray& batchId, Done done);
    void summaries(const QList<QPair<QString, QString>>& idsAndBatches, Done done);
    void activity(const QByteArray& account, Done done);
    // What the pre-claim confirmation needs: the fee market (getFeeState) and
    // the beneficiary's account (it signs, and by default pays).
    // {kind: "claimContext", fee: claimFee(…) + {source: "node"|"minimum", height, error},
    //  beneficiary: {read, error, nonce, balance, initialised, covers}}
    void claimContext(const QByteArray& beneficiary, bool token, Done done);

    // Raw access, exposed for the tests.
    void getAccount(const QString& id, std::function<void(const QJsonObject& acc, const QString& err)> cb);
    void clock(std::function<void(quint64 block, quint64 ms, const QString& err)> cb);

private:
    void rpcCall(const QString& method, const QJsonArray& params, std::function<void(const QJsonValue&, const QString& err)> cb);
    void explorerTxs(const QString& account, std::function<void(const QJsonArray&, const QString& err)> cb,
                     bool rediscovered = false);
    void readSchedules(const QList<QByteArray>& accounts, std::function<void(const QList<QJsonObject>&, quint64 now, const QString& err)> cb);
    void loadDetail(const QByteArray& account, const QByteArray& scheduleId, const QByteArray& batchId, Done done);
    void listBatch(const QByteArray& batchId, Done done);
    void searchAccount(const QByteArray& account, const QJsonObject& acc, Done done);

    QNetworkAccessManager net_;
    QString explorerPath_;  // the explorer's server function, found once
};

// The examples the app offers: schedules the end-to-end run of 2 Oct 2026
// created on the public testnet (evidence/v03/testnet.tsv), with a word on
// what each one shows.
struct Example { const char* id; const char* batch; const char* note; };
const QList<Example>& examples();

}  // namespace av
