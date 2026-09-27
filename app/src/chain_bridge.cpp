// SPDX-License-Identifier: MIT OR Apache-2.0
#include "chain_bridge.h"

#include <QDateTime>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkReply>
#include <QNetworkProxy>
#include <QNetworkRequest>
#include <QUrl>

namespace {

// The three PDAs the deployed programs write, from DEPLOYMENTS.md. They are
// derived from the program id and the sale/pool/schedule id, so they are stable
// and safe to compile in: a wrong address reads as an uninitialised account
// rather than as someone else's state.
const char* kSaleAccount     = "4AjdDDLLpyumGxnLki51cQPt5hbvQoUvG81KMerGqmBh";
const char* kSaleHolding     = "3kXqDQxkWMK13ZDCFkfamfktPXbMGndr5d1N5FVB8VrV";
const char* kPoolAccount     = "25ekuB2nQ84WLvoVjWejf63Z714X9vvjnb7Jz4R3Kkdg";
// The vesting schedule the panel follows: a year-long token position left
// accruing on purpose, written by scripts/replay-vesting.sh (section 9), which
// prints these two addresses.
const char* kScheduleAccount = "tEnLNnoHheKEqyUpZpWLQ59ECDwTe7rm3MDttpXxPL2";
const char* kScheduleHolding = "6RDFqaQnosx1Nh2E17wY7wz8Zc5MDxGQARBPzfnh7U51";

// The sequencer-written clock the vesting program itself reads, so "claimable
// now" here is computed against the same time a claim would be.
const char* kClockAccount    = "4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU";

// ProgramId word 0 of the token program; an escrow owned by it holds a token
// balance in its data rather than a native balance.
const qint64 kTokenProgramWord0 = 1047643340;

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
    : QObject(parent), m_rpc(QStringLiteral("https://testnet.lez.logos.co")) {
    // Qt's macOS system-proxy lookup builds a QRegularExpression, PCRE2 tries to
    // JIT-compile it, and pthread_jit_write_protect_np traps: Basecamp runs under
    // the hardened runtime without com.apple.security.cs.allow-jit, so the first
    // HTTP request took the whole host process down with SIGTRAP. The module is
    // not the one deciding to JIT and cannot add the entitlement to someone
    // else's binary, so it declines the lookup instead. Direct connection only —
    // which is what talking to a sequencer over its public URL wants anyway.
    QNetworkProxyFactory::setUseSystemConfiguration(false);
    m_net.setProxy(QNetworkProxy::NoProxy);
}

void ChainBridge::setEndpoint(const QString& url) {
    if (!url.isEmpty()) m_rpc = url;
}

void ChainBridge::refresh() {
    emit statusChanged(QStringLiteral("reading %1…").arg(m_rpc));
    // The clock first: the schedule is only meaningful against it, so its
    // answer triggers the two vesting reads.
    fetch(QStringLiteral("clock"), QString::fromLatin1(kClockAccount));
}

void ChainBridge::refreshLaunchpad() {
    emit statusChanged(QStringLiteral("reading %1…").arg(m_rpc));
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
        if (!result.isObject()) {
            // A null result is an uninitialised account, which is a real answer
            // and not an error — but it is not state either, so say which.
            emit failed(label, QStringLiteral("no account at that address"));
            return;
        }
        const QJsonObject acc = result.toObject();

        // The holdings are read for their balance alone: what a program has
        // actually escrowed is the number an analytics panel exists to show,
        // and the one a stale copy would most misrepresent.
        const QJsonArray raw = acc.value(QStringLiteral("data")).toArray();
        QByteArray data;
        data.reserve(raw.size());
        for (const QJsonValue& v : raw) data.append(static_cast<char>(v.toInt()));

        if (label == QLatin1String("clock")) {
            Reader r{data};
            r.u64();                                   // block_id
            m_nowMs = r.u64();                         // timestamp, ms
            if (!r.ok) { emit failed(label, QStringLiteral("clock account is short")); return; }
            fetch(QStringLiteral("schedule"), QString::fromLatin1(kScheduleAccount));
            fetch(QStringLiteral("schedule-escrow"), QString::fromLatin1(kScheduleHolding));
            return;
        }

        // The holdings are read for what they actually escrow: a native balance,
        // or for a token-program holding the balance inside its data.
        if (label.endsWith(QLatin1String("-escrow"))) {
            const QString which = label.left(label.size() - 7);
            const QJsonArray owner = acc.value(QStringLiteral("program_owner")).toArray();
            QString held;
            if (!owner.isEmpty() && owner.at(0).toInteger() == kTokenProgramWord0 && data.size() >= 49) {
                Reader r{data};
                r.skip(33);                            // tag, definition id
                held = dec(r.u128v()) + QStringLiteral("  (token balance held by the escrow)");
            } else {
                held = QString::number(acc.value(QStringLiteral("balance")).toDouble(), 'f', 0)
                       + QStringLiteral("  (native balance held by the escrow)");
            }
            emit escrowUpdated(which, held);
            emit statusChanged(QStringLiteral("%1 escrow read from chain").arg(which));
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
            if (data.size() > r.pos) {                  // the second layout appends these
                asset = r.u8();
                definition = r.bytes(32);
            }
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
