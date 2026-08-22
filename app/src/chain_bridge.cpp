// SPDX-License-Identifier: MIT OR Apache-2.0
#include "chain_bridge.h"

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
const char* kScheduleAccount = "DnFqQZChzEkKRtzENwUPQYed25ni1qztTJ33iJ2PGqnE";
const char* kScheduleHolding = "FF8dXEEyCoWuH3vanrFpEZXe2xon8Xze44Xk55MVzXzS";

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
    fetch(QStringLiteral("sale"), QString::fromLatin1(kSaleAccount));
    fetch(QStringLiteral("pool"), QString::fromLatin1(kPoolAccount));
    fetch(QStringLiteral("schedule"), QString::fromLatin1(kScheduleAccount));
    fetch(QStringLiteral("sale-escrow"), QString::fromLatin1(kSaleHolding));
    fetch(QStringLiteral("schedule-escrow"), QString::fromLatin1(kScheduleHolding));
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
        if (label.endsWith(QLatin1String("-escrow"))) {
            const QString which = label.left(label.size() - 7);
            emit escrowUpdated(which,
                               QString::number(acc.value(QStringLiteral("balance")).toDouble(), 'f', 0));
            emit statusChanged(QStringLiteral("%1 escrow read from chain").arg(which));
            return;
        }

        const QJsonArray raw = acc.value(QStringLiteral("data")).toArray();
        QByteArray data;
        data.reserve(raw.size());
        for (const QJsonValue& v : raw) data.append(static_cast<char>(v.toInt()));

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
            r.u64(); r.u64(); r.u64();              // start, cliff, end
            const QString total = r.u128(), claimed = r.u128();
            const quint64 last = r.u64();
            if (!r.ok) { emit failed(label, QStringLiteral("account data is short")); return; }
            emit scheduleUpdated(total, claimed, QString::number(last),
                                 kind == 0 ? QStringLiteral("cliff + linear")
                                           : QStringLiteral("linear"));
        }
        emit statusChanged(QStringLiteral("%1 read from chain").arg(label));
    });
}
