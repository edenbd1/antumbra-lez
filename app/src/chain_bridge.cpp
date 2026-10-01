// SPDX-License-Identifier: MIT OR Apache-2.0
#include "chain_bridge.h"

#include <QDateTime>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkReply>
#include <QNetworkProxy>
#include <QNetworkRequest>
#include <QClipboard>
#include <QDir>
#include <QGuiApplication>
#include <QFileInfo>
#include <QSettings>
#include <QStandardPaths>
#include <QUrl>

namespace {

// LEZ v0.3 accounts have no single owner and no single data field: an account
// is a map of shards keyed by program account id (base58), and native balance
// is the shard at the all-zero id. Every read below picks the one shard it
// means, so an account another program also writes to cannot be misread.
const char* kNativeShard  = "11111111111111111111111111111111";
// Builtin program ids are SHA-256("/LEE-BuiltinProgram/v1/AccountId" || name),
// the same on every v0.3 chain.
const char* kClockProgram = "2hwaRW8sMnyKLLRbDRw8k68tiv9bnfq98oxcdtcp157Z";
const char* kTokenProgram = "AxDdLwqkWgR9qaSctvABV1ZtvWJJyzXB8199xZueifPj";
// The per-block clock account; its clock shard is { block_id: u64, timestamp: u64 }.
const char* kClockAccount = "4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU";

// The vesting program's header account and the schedule the panel follows.
// On v0.3 a program lives at whatever account it was deployed to, so these
// come from the environment or the module's settings rather than being
// compiled in. The public testnet v0.3 deployment has not happened yet; until
// it does the program id is empty and the panel says "not deployed yet"
// instead of showing a v0.2.4 address that no longer resolves.
const char* kDefaultRpc = "https://testnet.lez.logos.co";

// Basecamp passes --user-dir to its modules as LOGOS_USER_DIR. Settings live
// under that directory's module_data, next to every other module's state, so
// a throwaway Basecamp instance never reads or writes the default one's.
QString settingsPath() {
    const QByteArray user = qgetenv("LOGOS_USER_DIR");
    const QString base = user.isEmpty()
        ? QStandardPaths::writableLocation(QStandardPaths::AppDataLocation)
        : QString::fromUtf8(user);
    return QDir(base).filePath(QStringLiteral("module_data/antumbra_lez/settings.ini"));
}

// The launchpad programs (RFP-015/016) are still v0.2.4 programs, and that
// chain was reset; their accounts are not on v0.3.
const char* kSaleAccount     = "4AjdDDLLpyumGxnLki51cQPt5hbvQoUvG81KMerGqmBh";
const char* kSaleHolding     = "3kXqDQxkWMK13ZDCFkfamfktPXbMGndr5d1N5FVB8VrV";
const char* kPoolAccount     = "25ekuB2nQ84WLvoVjWejf63Z714X9vvjnb7Jz4R3Kkdg";

QByteArray shardOf(const QJsonObject& acc, const QString& program) {
    const QJsonArray raw = acc.value(QStringLiteral("data")).toObject()
                              .value(QStringLiteral("shards")).toObject()
                              .value(program).toArray();
    QByteArray out;
    out.reserve(raw.size());
    for (const QJsonValue& v : raw) out.append(static_cast<char>(v.toInt()));
    return out;
}

// The two holdings are plain balances rather than decoded state: what they hold
// is the value actually escrowed, which is the number a reader of an analytics
// panel most wants and the one a stale cache would most misrepresent.

// Borsh reader over the raw account bytes. Every read is bounds-checked and
// sets `ok` false rather than returning a plausible-looking zero, because an
// account that is one byte short would otherwise render as a real balance.
struct Reader {
    const QByteArray& b;
    int pos = 0;
    bool ok = true;

    void skip(int n) {
        if (pos + n > b.size()) { ok = false; return; }
        pos += n;
    }
    quint8 u8() {
        if (pos + 1 > b.size()) { ok = false; return 0; }
        return static_cast<quint8>(b[pos++]);
    }
    quint64 u64() {
        if (pos + 8 > b.size()) { ok = false; return 0; }
        quint64 v = 0;
        for (int i = 7; i >= 0; --i) v = (v << 8) | static_cast<quint8>(b[pos + i]);
        pos += 8;
        return v;
    }
    unsigned __int128 u128v() {
        if (pos + 16 > b.size()) { ok = false; return 0; }
        unsigned __int128 v = 0;
        for (int i = 15; i >= 0; --i) v = (v << 8) | static_cast<quint8>(b[pos + i]);
        pos += 16;
        return v;
    }
    quint32 u32() {
        if (pos + 4 > b.size()) { ok = false; return 0; }
        quint32 v = 0;
        for (int i = 3; i >= 0; --i) v = (v << 8) | static_cast<quint8>(b[pos + i]);
        pos += 4;
        return v;
    }
    QByteArray bytes(int n) {
        if (pos + n > b.size()) { ok = false; return {}; }
        QByteArray out = b.mid(pos, n);
        pos += n;
        return out;
    }
    // No portable 128-bit integer in the standard, and these values genuinely
    // exceed 64 bits — a token total at 18 decimals passes 2^64 at 18.4 units —
    // so the digits are accumulated in decimal instead of being truncated.
    QString u128() {
        if (pos + 16 > b.size()) { ok = false; return QStringLiteral("0"); }
        QString out = QStringLiteral("0");
        for (int i = 15; i >= 0; --i) {
            out = mulAdd(out, 256, static_cast<quint8>(b[pos + i]));
        }
        pos += 16;
        return out;
    }

private:
    // Long multiplication on a decimal string: out = out * m + add.
    static QString mulAdd(const QString& in, int m, int add) {
        QString rev;
        int carry = add;
        for (int i = in.size() - 1; i >= 0; --i) {
            int d = in[i].digitValue() * m + carry;
            rev.append(QChar('0' + d % 10));
            carry = d / 10;
        }
        while (carry > 0) { rev.append(QChar('0' + carry % 10)); carry /= 10; }
        std::reverse(rev.begin(), rev.end());
        while (rev.size() > 1 && rev[0] == QChar('0')) rev.remove(0, 1);
        return rev.isEmpty() ? QStringLiteral("0") : rev;
    }
};

QString dec(unsigned __int128 v) {
    if (v == 0) return QStringLiteral("0");
    QString out;
    while (v > 0) { out.prepend(QChar('0' + int(v % 10))); v /= 10; }
    return out;
}

QString base58(const QByteArray& in) {
    static const char* A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    QByteArray digits;                       // base-58 digits, little end first
    for (unsigned char c : in) {
        int carry = c;
        for (char& d : digits) { carry += (static_cast<unsigned char>(d) << 8); d = char(carry % 58); carry /= 58; }
        while (carry) { digits.append(char(carry % 58)); carry /= 58; }
    }
    QString out;
    for (unsigned char c : in) { if (c) break; out.append(QChar('1')); }
    for (int i = digits.size() - 1; i >= 0; --i) out.append(QChar(A[static_cast<unsigned char>(digits[i])]));
    return out;
}

// The program's own accrual rule, restated: nothing before the cliff, then
// everything accrued since `start` at once and linear to `end`; a cancelled
// schedule stops accruing at the cancellation; milestones release
// total × signalled / tranches.
unsigned __int128 vestedAt(quint8 kind, quint64 start, quint64 cliff, quint64 end,
                           unsigned __int128 total, quint64 cancelledAt,
                           quint64 signalled, quint32 tranches, quint64 now) {
    if (kind == 2) {
        if (tranches == 0) return 0;
        return total * static_cast<unsigned __int128>(__builtin_popcountll(signalled)) / tranches;
    }
    const quint64 t = cancelledAt ? qMin(now, cancelledAt) : now;
    if (kind == 0 && t < cliff) return 0;
    if (t <= start) return 0;
    if (t >= end) return total;
    return total * static_cast<unsigned __int128>(t - start) / static_cast<unsigned __int128>(end - start);
}

} // namespace

ChainBridge::ChainBridge(QObject* parent)
    : QObject(parent), m_settingsFile(settingsPath()) {
    // Environment first (a scripted run pins what it reads), then what the
    // user saved, then the defaults.
    QSettings saved(m_settingsFile, QSettings::IniFormat);
    auto pick = [&](const char* env, const char* key, const QString& fallback) {
        const QByteArray v = qgetenv(env);
        if (!v.isEmpty()) return QString::fromUtf8(v).trimmed();
        return saved.value(QLatin1String(key), fallback).toString().trimmed();
    };
    m_rpc = pick("ANTUMBRA_RPC", "rpc", QString::fromLatin1(kDefaultRpc));
    if (m_rpc.isEmpty()) m_rpc = QString::fromLatin1(kDefaultRpc);
    m_program = pick("ANTUMBRA_PROGRAM", "program", QString());
    m_schedule = pick("ANTUMBRA_SCHEDULE", "schedule", QString());
    m_holding = pick("ANTUMBRA_HOLDING", "holding", QString());

    // Qt's macOS system-proxy lookup builds a QRegularExpression, PCRE2 tries to
    // JIT-compile it, and pthread_jit_write_protect_np traps: Basecamp runs under
    // the hardened runtime without com.apple.security.cs.allow-jit, so the first
    // HTTP request took the whole host process down with SIGTRAP. The module is
    // not the one deciding to JIT and cannot add the entitlement to someone
    // else's binary, so it declines the lookup instead. Direct connection only,
    // which is what talking to a sequencer over its public URL wants anyway.
    QNetworkProxyFactory::setUseSystemConfiguration(false);
    m_net.setProxy(QNetworkProxy::NoProxy);
}

QString ChainBridge::defaultEndpoint() const { return QString::fromLatin1(kDefaultRpc); }

QStringList ChainBridge::envOverrides() const {
    QStringList out;
    for (const char* e : {"ANTUMBRA_RPC", "ANTUMBRA_PROGRAM", "ANTUMBRA_SCHEDULE", "ANTUMBRA_HOLDING"})
        if (!qgetenv(e).isEmpty()) out << QString::fromLatin1(e);
    return out;
}

QString ChainBridge::clipboardText() const {
    const QClipboard* c = QGuiApplication::clipboard();
    return c ? c->text().trimmed() : QString();
}

void ChainBridge::setEndpoint(const QString& url) {
    if (!url.trimmed().isEmpty()) m_rpc = url.trimmed();
}

void ChainBridge::setVestingTarget(const QString& program, const QString& schedule,
                                   const QString& holding) {
    m_program = program.trimmed();
    m_schedule = schedule.trimmed();
    m_holding = holding.trimmed();
}

void ChainBridge::saveSettings(const QString& rpc, const QString& program,
                               const QString& schedule, const QString& holding) {
    setEndpoint(rpc.trimmed().isEmpty() ? QString::fromLatin1(kDefaultRpc) : rpc);
    setVestingTarget(program, schedule, holding);
    QDir().mkpath(QFileInfo(m_settingsFile).absolutePath());
    QSettings saved(m_settingsFile, QSettings::IniFormat);
    saved.setValue(QStringLiteral("rpc"), m_rpc);
    saved.setValue(QStringLiteral("program"), m_program);
    saved.setValue(QStringLiteral("schedule"), m_schedule);
    saved.setValue(QStringLiteral("holding"), m_holding);
    saved.sync();
    emit statusChanged(QStringLiteral("settings saved to %1").arg(m_settingsFile));
}

void ChainBridge::refresh() {
    if (m_program.isEmpty()) {
        emit notDeployed(QStringLiteral("The v0.3 vesting program is not deployed on %1 yet. "
                                        "Once it is, put its program id in Settings.").arg(m_rpc));
        return;
    }
    if (m_schedule.isEmpty() || m_holding.isEmpty()) {
        emit failed(QStringLiteral("schedule"),
                    QStringLiteral("set the schedule and holding accounts to follow in Settings "
                                   "(`antumbra-vesting ids --schedule-id <id>` prints both)"));
        return;
    }
    emit statusChanged(QStringLiteral("reading %1…").arg(m_rpc));
    // The program first: an id that holds no account on this sequencer means
    // the program is not deployed here, which is a state worth naming rather
    // than a pile of "no account" errors. Then the clock, against which the
    // schedule is computed, and its answer triggers the two vesting reads.
    fetch(QStringLiteral("program"), m_program);
}

void ChainBridge::refreshLaunchpad() {
    emit statusChanged(QStringLiteral("reading %1…").arg(m_rpc));
    // v0.2.4 accounts: on a v0.3 sequencer these read as uninitialised.
    fetch(QStringLiteral("sale"), QString::fromLatin1(kSaleAccount));
    fetch(QStringLiteral("pool"), QString::fromLatin1(kPoolAccount));
    fetch(QStringLiteral("sale-escrow"), QString::fromLatin1(kSaleHolding));
}

void ChainBridge::fetch(const QString& label, const QString& accountId) {
    QJsonObject body{
        {QStringLiteral("jsonrpc"), QStringLiteral("2.0")},
        {QStringLiteral("id"), 1},
        {QStringLiteral("method"), QStringLiteral("getAccount")},
        {QStringLiteral("params"), QJsonArray{accountId}},
    };

    QNetworkRequest req{QUrl(m_rpc)};
    req.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/json"));

    QNetworkReply* reply = m_net.post(req, QJsonDocument(body).toJson(QJsonDocument::Compact));
    connect(reply, &QNetworkReply::finished, this, [this, reply, label]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            emit failed(label, reply->errorString());
            return;
        }
        const QJsonObject root = QJsonDocument::fromJson(reply->readAll()).object();
        const QJsonValue result = root.value(QStringLiteral("result"));
        if (label == QLatin1String("program")) {
            // A v0.3 sequencer answers every id, with an empty shard map for
            // an account nothing ever wrote: that is "no program here".
            const QJsonObject shards = result.toObject().value(QStringLiteral("data")).toObject()
                                           .value(QStringLiteral("shards")).toObject();
            if (!result.isObject() || shards.isEmpty()) {
                emit notDeployed(QStringLiteral("No program at %1 on %2: the v0.3 vesting program "
                                                "is not deployed on this chain (yet).")
                                     .arg(m_program, m_rpc));
                return;
            }
            fetch(QStringLiteral("clock"), QString::fromLatin1(kClockAccount));
            return;
        }
        if (!result.isObject()) {
            // A null result is an uninitialised account, which is a real answer
            // and not an error — but it is not state either, so say which.
            emit failed(label, QStringLiteral("no account at that address"));
            return;
        }
        const QJsonObject acc = result.toObject();

        if (label == QLatin1String("clock")) {
            const QByteArray data = shardOf(acc, QString::fromLatin1(kClockProgram));
            Reader r{data};
            r.u64();                                   // block_id
            m_nowMs = r.u64();                         // timestamp, ms
            if (!r.ok) { emit failed(label, QStringLiteral("clock account is short")); return; }
            fetch(QStringLiteral("schedule"), m_schedule);
            fetch(QStringLiteral("schedule-escrow"), m_holding);
            return;
        }

        // The holdings are read for what they actually escrow: the token
        // program's shard if it holds one (tag 0 = fungible, then the
        // definition, then a u128 balance), else the native shard.
        if (label.endsWith(QLatin1String("-escrow"))) {
            const QString which = label.left(label.size() - 7);
            const QByteArray token = shardOf(acc, QString::fromLatin1(kTokenProgram));
            QString held;
            if (token.size() >= 49 && token[0] == 0) {
                Reader r{token};
                r.skip(33);                            // tag, definition id
                held = dec(r.u128v()) + QStringLiteral("  (token balance held by the escrow)");
            } else {
                const QByteArray native = shardOf(acc, QString::fromLatin1(kNativeShard));
                Reader r{native};
                // An empty native shard is a zero balance, not an error.
                held = (native.isEmpty() ? QStringLiteral("0") : dec(r.u128v()))
                       + QStringLiteral("  (native balance held by the escrow)");
            }
            emit escrowUpdated(which, held);
            emit statusChanged(QStringLiteral("%1 escrow read from chain").arg(which));
            return;
        }

        // Program state is the program's own shard: the vesting schedule sits
        // at the vesting program's header id. The launchpad accounts are
        // v0.2.4 ones and carry no shard map.
        const QByteArray data = label == QLatin1String("schedule")
                                    ? shardOf(acc, m_program)
                                    : QByteArray();
        if (label == QLatin1String("schedule") && data.isEmpty()) {
            emit failed(label, QStringLiteral("no schedule of this program at %1").arg(m_schedule));
            return;
        }
        Reader r{data};
        if (label == QLatin1String("sale")) {
            const QString vt = r.u128(), vc = r.u128(), sr = r.u128(),
                          rc = r.u128(), seed = r.u128();
            r.skip(32 * 3);                       // creator, holding, fee treasury
            r.u128();                             // fee rate
            const QString accrued = r.u128();
            if (!r.ok) { emit failed(label, QStringLiteral("account data is short")); return; }
            emit saleUpdated(vt, vc, sr, rc, seed, accrued);
        } else if (label == QLatin1String("pool")) {
            const QString rt = r.u128(), rcol = r.u128(), ws = r.u128(), we = r.u128();
            r.u64(); r.u64();                       // t_start, t_end
            const quint64 last = r.u64();
            if (!r.ok) { emit failed(label, QStringLiteral("account data is short")); return; }
            emit poolUpdated(rt, rcol, ws, we, QString::number(last));
        } else {
            const quint8 kind = r.u8();
            const quint64 start = r.u64(), cliff = r.u64(), end = r.u64();
            const unsigned __int128 total = r.u128v(), claimed = r.u128v();
            r.u64();                                    // last_seen
            r.skip(32 * 3);                             // beneficiary, escrow, creator
            const quint8 cancelable = r.u8();
            r.u8();                                     // transferable
            const quint64 cancelledAt = r.u64(), signalled = r.u64();
            const quint32 tranches = r.u32();
            quint8 asset = 0;
            QByteArray definition;
            asset = r.u8();
            definition = r.bytes(32);
            if (!r.ok) { emit failed(label, QStringLiteral("account data is short")); return; }

            const unsigned __int128 vested = vestedAt(kind, start, cliff, end, total, cancelledAt,
                                                      signalled, tranches, m_nowMs);
            const unsigned __int128 claimable = vested > claimed ? vested - claimed : 0;
            auto when = [](quint64 ms) {
                return QDateTime::fromMSecsSinceEpoch(qint64(ms), Qt::UTC).toString(QStringLiteral("yyyy-MM-dd HH:mm 'UTC'"));
            };
            QString next;
            if (cancelledAt) next = QStringLiteral("none — cancelled at ") + when(cancelledAt);
            else if (kind == 2) next = QStringLiteral("milestone %1 of %2, when signalled")
                                           .arg(__builtin_popcountll(signalled) + 1).arg(tranches);
            else if (kind == 0 && m_nowMs < cliff) next = QStringLiteral("cliff lump at ") + when(cliff);
            else if (m_nowMs < end) next = QStringLiteral("continuous, fully vested at ") + when(end);
            else next = QStringLiteral("none — fully vested");

            QVariantMap m;
            m[QStringLiteral("kind")] = kind == 0 ? QStringLiteral("cliff + linear")
                                      : kind == 1 ? QStringLiteral("linear") : QStringLiteral("milestones");
            m[QStringLiteral("asset")] = asset == 1 ? QStringLiteral("token ") + base58(definition)
                                                    : QStringLiteral("native balance");
            m[QStringLiteral("total")] = dec(total);
            m[QStringLiteral("claimed")] = dec(claimed);
            m[QStringLiteral("vested")] = dec(vested);
            m[QStringLiteral("claimable")] = dec(claimable);
            m[QStringLiteral("next")] = next;
            m[QStringLiteral("cancelable")] = cancelledAt ? QStringLiteral("cancelled")
                                            : cancelable ? QStringLiteral("yes") : QStringLiteral("no (one-way)");
            m[QStringLiteral("clock")] = when(m_nowMs);
            emit scheduleUpdated(m);
        }
        emit statusChanged(QStringLiteral("%1 read from chain").arg(label));
    });
}
