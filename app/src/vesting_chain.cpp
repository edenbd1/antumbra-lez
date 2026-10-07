// SPDX-License-Identifier: MIT OR Apache-2.0
#include "vesting_chain.h"

#include <algorithm>
#include <cstring>

#include <QCryptographicHash>
#include <QJsonDocument>
#include <QNetworkProxy>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QSet>
#include <QUrl>
#include <QUrlQuery>

namespace av {

namespace {

const char* kAlphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
// Builtin program ids are the same on every v0.3 chain.
const char* kNativeShard = "11111111111111111111111111111111";
const char* kTokenProgram = "AxDdLwqkWgR9qaSctvABV1ZtvWJJyzXB8199xZueifPj";
const char* kClockProgram = "2hwaRW8sMnyKLLRbDRw8k68tiv9bnfq98oxcdtcp157Z";
// The per-block clock account: its clock shard is { block_id: u64, timestamp: u64 }.
const char* kClockAccount = "4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU";
// The explorer's server function for an account's transactions. Its suffix is
// a hash of the function's path; if a redeploy changes it, it is read back
// out of the explorer's own WebAssembly bundle.
const char* kExplorerPath = "/api/get_transactions_by_account3022937127152978530";
constexpr int kScheduleLen = 312;
constexpr int kBatchMax = 500;

// Borsh reader, bounds-checked: a short account sets `ok` false rather than
// reading as a plausible zero.
struct Reader {
    const QByteArray& b;
    int pos = 0;
    bool ok = true;
    bool need(int n) { if (pos + n > b.size()) { ok = false; return false; } return true; }
    quint8 u8() { if (!need(1)) return 0; return quint8(b[pos++]); }
    quint32 u32() { if (!need(4)) return 0; quint32 v = 0; for (int i = 3; i >= 0; --i) v = (v << 8) | quint8(b[pos + i]); pos += 4; return v; }
    quint64 u64() { if (!need(8)) return 0; quint64 v = 0; for (int i = 7; i >= 0; --i) v = (v << 8) | quint8(b[pos + i]); pos += 8; return v; }
    u128 u128v() { if (!need(16)) return 0; u128 v = 0; for (int i = 15; i >= 0; --i) v = (v << 8) | quint8(b[pos + i]); pos += 16; return v; }
    QByteArray bytes(int n) { if (!need(n)) return {}; QByteArray o = b.mid(pos, n); pos += n; return o; }
};

QByteArray shard(const QJsonObject& acc, const QString& program) {
    const QJsonArray raw = acc.value("data").toObject().value("shards").toObject().value(program).toArray();
    QByteArray out;
    out.reserve(raw.size());
    for (const QJsonValue& v : raw) out.append(char(v.toInt()));
    return out;
}
QByteArray bytesOf(const QJsonArray& raw) {
    QByteArray out;
    out.reserve(raw.size());
    for (const QJsonValue& v : raw) out.append(char(v.toInt()));
    return out;
}

QString unitOf(const Schedule& s) { return s.asset == 1 ? QStringLiteral("tokens") : QStringLiteral("LGO"); }
QString shortKey(const QString& k) { return k.size() > 10 ? k.left(4) + QChar(0x2026) + k.right(4) : k; }
QString fmt(u128 v) {
    QString d = dec(v), out;
    for (int i = 0; i < d.size(); ++i) {
        if (i && (d.size() - i) % 3 == 0) out += ',';
        out += d[i];
    }
    return out;
}
QJsonObject chip(const QString& text, const char* tone) { return {{"text", text}, {"tone", tone}}; }

// Callbacks that finish after N parallel reads.
struct Join {
    int pending = 0;
    std::function<void()> then;
    void done() { if (--pending == 0 && then) then(); }
};

}  // namespace

// ── Encodings ────────────────────────────────────────────────────────────────

QByteArray sha256(const QByteArray& in) { return QCryptographicHash::hash(in, QCryptographicHash::Sha256); }

QString base58(const QByteArray& in) {
    QByteArray digits;  // base-58 digits, little end first
    for (unsigned char c : in) {
        int carry = c;
        for (char& d : digits) { carry += (quint8(d) << 8); d = char(carry % 58); carry /= 58; }
        while (carry) { digits.append(char(carry % 58)); carry /= 58; }
    }
    QString out;
    for (unsigned char c : in) { if (c) break; out.append('1'); }
    for (int i = digits.size() - 1; i >= 0; --i) out.append(QChar(kAlphabet[quint8(digits[i])]));
    return out;
}

QByteArray unbase58(const QString& s) {
    if (s.isEmpty() || s.size() > 44) return {};
    QByteArray bytes;  // big-endian magnitude, little end first while building
    for (QChar ch : s) {
        const char* p = std::strchr(kAlphabet, ch.toLatin1());
        if (!p || ch.unicode() > 127 || ch.toLatin1() == 0) return {};
        int carry = int(p - kAlphabet);
        for (char& b : bytes) { carry += quint8(b) * 58; b = char(carry & 0xff); carry >>= 8; }
        while (carry) { bytes.append(char(carry & 0xff)); carry >>= 8; }
    }
    for (QChar ch : s) { if (ch != '1') break; bytes.append('\0'); }
    std::reverse(bytes.begin(), bytes.end());
    return bytes.size() == 32 ? bytes : QByteArray();
}

QString dec(u128 v) {
    if (v == 0) return QStringLiteral("0");
    QString out;
    while (v > 0) { out.prepend(QChar('0' + int(v % 10))); v /= 10; }
    return out;
}

u128 parseDec(const QString& s, bool* ok) {
    u128 v = 0;
    bool good = !s.isEmpty();
    for (QChar c : s) {
        if (!c.isDigit()) { good = false; break; }
        const u128 n = v * 10 + u128(c.digitValue());
        if (n / 10 != v) { good = false; break; }
        v = n;
    }
    if (ok) *ok = good;
    return good ? v : 0;
}

QByteArray idBytes(const QString& raw, QString* err) {
    const QString s = raw.trimmed();
    bool hex = s.size() == 64;
    for (QChar c : s) if (!(c.isDigit() || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'))) { hex = false; break; }
    if (hex) return QByteArray::fromHex(s.toLatin1());
    const QByteArray utf8 = s.toUtf8();
    if (utf8.isEmpty() || utf8.size() > 32) {
        if (err) *err = QStringLiteral("An id is 64 hex digits, or text of at most 32 bytes.");
        return {};
    }
    return utf8 + QByteArray(32 - utf8.size(), '\0');
}

QString idText(const QByteArray& id) {
    int n = id.size();
    while (n > 0 && id[n - 1] == '\0') --n;
    bool printable = n > 0;
    for (int i = 0; i < n; ++i) {
        const quint8 c = quint8(id[i]);
        if (c < 0x21 || c > 0x7e) { printable = false; break; }
    }
    // Text that is itself 64 hex digits would be read back as hex: keep hex.
    if (printable && n == 64) printable = false;
    return printable ? QString::fromLatin1(id.left(n)) : QString::fromLatin1(id.toHex());
}

QByteArray textSeed(const char* s) {
    QByteArray b(s);
    return b + QByteArray(32 - b.size(), '\0');
}

QByteArray combineSeeds(const QList<QByteArray>& seeds) {
    if (seeds.size() == 1) return seeds.first();
    QByteArray all;
    for (const auto& s : seeds) all += s;
    return sha256(all);
}

QByteArray pda(const QByteArray& program, const QByteArray& seed) {
    QByteArray prefix("/LEE/v0.2/AccountId/PDA/");
    prefix += QByteArray(32 - prefix.size(), '\0');
    return sha256(prefix + program + seed);
}

QByteArray scheduleAccount(const QByteArray& program, const QByteArray& id) { return pda(program, combineSeeds({id})); }
QByteArray holdingAccount(const QByteArray& program, const QByteArray& id) {
    return pda(program, combineSeeds({id, textSeed("holding")}));
}
QByteArray batchScheduleId(const QByteArray& batchId, quint32 i) {
    QByteArray idx(32, '\0');
    for (int k = 0; k < 4; ++k) idx[k] = char((i >> (8 * k)) & 0xff);
    return combineSeeds({batchId, idx});
}

// ── The schedule ─────────────────────────────────────────────────────────────

bool decodeSchedule(const QByteArray& b, Schedule& s) {
    if (b.size() != kScheduleLen) return false;
    Reader r{b};
    s.kind = r.u8();
    s.start = r.u64(); s.cliff = r.u64(); s.end = r.u64();
    s.total = r.u128v(); s.claimed = r.u128v();
    s.lastSeen = r.u64();
    s.beneficiary = r.bytes(32); s.escrow = r.bytes(32); s.creator = r.bytes(32);
    s.cancelable = r.u8(); s.transferable = r.u8();
    s.cancelledAt = r.u64(); s.signalled = r.u64();
    s.tranches = r.u32();
    s.asset = r.u8();
    s.tokenDefinition = r.bytes(32); s.refundTo = r.bytes(32);
    s.cancelAuthority = r.bytes(32); s.milestoneAuthority = r.bytes(32);
    return r.ok && r.pos == b.size() && s.kind <= 2;
}

namespace {
// floor(a * n / d) with n < d, exact without a 256-bit product.
u128 mulDivFloor(u128 a, u128 n, u128 d) { return (a / d) * n + ((a % d) * n) / d; }
}

u128 vestedAt(const Schedule& s, quint64 now) {
    if (s.kind == 2) {
        if (s.tranches == 0) return 0;
        const int done = __builtin_popcountll(s.signalled);
        return done >= int(s.tranches) ? s.total : mulDivFloor(s.total, u128(done), u128(s.tranches));
    }
    const quint64 t = s.cancelledAt ? std::min(now, s.cancelledAt) : now;
    if (s.kind == 0 && t < s.cliff) return 0;
    if (t <= s.start) return 0;
    if (t >= s.end) return s.total;
    return mulDivFloor(s.total, u128(t - s.start), u128(s.end - s.start));
}

QJsonObject scheduleJson(const Schedule& s, quint64 now) {
    const u128 vested = vestedAt(s, now);
    const u128 claimable = vested > s.claimed ? vested - s.claimed : 0;
    int signalledCount = 0;
    QJsonArray tranches;
    for (quint32 i = 0; i < s.tranches && i < 64; ++i) {
        const bool lit = s.signalled >> i & 1;
        signalledCount += lit;
        tranches.append(lit);
    }
    QString state; const char* tone = "dim";
    if (s.cancelledAt) { state = QStringLiteral("Cancelled"); tone = "bad"; }
    else if (s.total > 0 && s.claimed == s.total) { state = QStringLiteral("Fully claimed"); tone = "dim"; }
    else if (s.kind == 2) {
        if (signalledCount == int(s.tranches)) { state = QStringLiteral("All milestones signalled"); tone = "ok"; }
        else { state = QStringLiteral("%1 of %2 signalled").arg(signalledCount).arg(s.tranches); tone = "accent"; }
    }
    else if (now < s.start) { state = QStringLiteral("Not started"); tone = "dim"; }
    else if (s.kind == 0 && now < s.cliff) { state = QStringLiteral("Before the cliff"); tone = "warn"; }
    else if (now < s.end) { state = QStringLiteral("Vesting"); tone = "accent"; }
    else { state = QStringLiteral("Fully vested"); tone = "ok"; }

    const QString token = base58(s.tokenDefinition);
    QJsonArray chips{chip(state, tone)};
    chips.append(chip(s.kind == 0 ? "Cliff + linear" : s.kind == 1 ? "Linear" : "Milestones", "dim"));
    chips.append(chip(s.asset == 1 ? "Token " + shortKey(token) : QStringLiteral("Native LGO"), "dim"));
    if (!s.cancelledAt) chips.append(s.cancelable ? chip("Cancelable", "warn") : chip("Non-cancelable", "ok"));
    chips.append(chip(s.transferable ? "Transferable" : "Not transferable", "dim"));

    const double total = double(s.total);
    return QJsonObject{
        {"kind", int(s.kind)},
        {"kindLabel", s.kind == 0 ? "Cliff + linear" : s.kind == 1 ? "Linear" : "Milestones"},
        {"start", double(s.start)}, {"cliff", double(s.cliff)}, {"end", double(s.end)},
        {"cancelledAt", double(s.cancelledAt)}, {"lastSeen", double(s.lastSeen)},
        {"total", dec(s.total)}, {"claimed", dec(s.claimed)}, {"vested", dec(vested)},
        {"claimable", dec(claimable)}, {"unvested", dec(s.total - vested)},
        {"totalF", total}, {"claimedF", double(s.claimed)}, {"vestedF", double(vested)},
        {"perTranche", s.tranches ? dec(s.total / s.tranches) : QStringLiteral("0")},
        {"tranches", tranches}, {"signalledCount", signalledCount},
        {"cancelable", bool(s.cancelable)}, {"transferable", bool(s.transferable)},
        {"asset", s.asset == 1 ? "token" : "native"}, {"unit", unitOf(s)},
        {"tokenDefinition", s.asset == 1 ? token : QString()},
        {"beneficiary", base58(s.beneficiary)}, {"creator", base58(s.creator)},
        {"escrow", base58(s.escrow)}, {"refundTo", base58(s.refundTo)},
        {"cancelAuthority", base58(s.cancelAuthority)}, {"milestoneAuthority", base58(s.milestoneAuthority)},
        {"state", state}, {"tone", tone}, {"chips", chips}, {"now", double(now)},
    };
}

// ── Replaying a schedule's transactions ─────────────────────────────────────

namespace {

struct Ix {
    int tag = -1;
    QByteArray sid, batch, beneficiary, newBeneficiary;
    QList<QByteArray> beneficiaries;
    Schedule terms;  // kind, start, cliff, end, total, tranches, flags, authorities, refund, asset
    u128 amount = 0, refund = 0;
    quint64 at = 0;
    quint32 index = 0;
    bool ok = false;
};

void readAsset(Reader& r, Schedule& s) {
    s.asset = r.u8();
    s.tokenDefinition = s.asset == 1 ? r.bytes(32) : QByteArray(32, '\0');
}
void readTerms(Reader& r, Schedule& s) {
    s.kind = r.u8(); s.start = r.u64(); s.cliff = r.u64(); s.end = r.u64();
    s.total = r.u128v(); s.tranches = r.u32();
    s.cancelable = r.u8(); s.transferable = r.u8();
    s.cancelAuthority = r.bytes(32); s.milestoneAuthority = r.bytes(32); s.refundTo = r.bytes(32);
}

Ix decodeIx(const QByteArray& d) {
    Ix x;
    Reader r{d};
    x.tag = r.u8();
    switch (x.tag) {
    case 0: x.sid = r.bytes(32); x.beneficiary = r.bytes(32); readAsset(r, x.terms); readTerms(r, x.terms); break;
    case 1: {
        x.batch = r.bytes(32);
        const quint32 n = r.u32();
        for (quint32 i = 0; i < n && r.ok && i <= kBatchMax; ++i) x.beneficiaries.append(r.bytes(32));
        readAsset(r, x.terms); readTerms(r, x.terms);
        break;
    }
    case 2: case 3: {
        x.sid = r.bytes(32);
        if (r.u8() == 1) x.batch = r.bytes(32);
        readAsset(r, x.terms);
        if (x.tag == 2) { x.amount = r.u128v(); x.at = r.u64(); }
        else { x.at = r.u64(); x.refund = r.u128v(); }
        break;
    }
    case 4: x.sid = r.bytes(32); break;
    case 5: x.sid = r.bytes(32); x.newBeneficiary = r.bytes(32); break;
    case 6: x.sid = r.bytes(32); x.index = r.u32(); break;
    default: r.ok = false;
    }
    x.ok = r.ok;
    return x;
}

QString who(const QByteArray& a, const Schedule& s) {
    if (a == s.beneficiary) return QStringLiteral("the beneficiary");
    if (a == s.creator) return QStringLiteral("the creator");
    return shortKey(base58(a));
}

}  // namespace

QJsonObject replayActivity(const QJsonArray& txs, const QByteArray& program, const QByteArray& scheduleAcc,
                           const Schedule& fin) {
    const QString prog = base58(program), self = base58(scheduleAcc);
    const QString unit = unitOf(fin);
    Schedule st;          // the schedule as replayed
    bool created = false;
    bool privateSeen = false;
    QByteArray foundSid, foundBatch;
    QJsonArray events;

    for (const QJsonValue& v : txs) {
        const QJsonObject o = v.toObject();
        const bool isPublic = o.contains("Public");
        const QJsonObject t = o.value(isPublic ? "Public" : "PrivacyPreserving").toObject();
        const QString hash = t.value("hash").toString();
        const QJsonObject msg = t.value("message").toObject();
        QJsonObject ev{{"tx", hash}, {"at", 0.0}, {"private", !isPublic}};

        if (!isPublic) {
            // A privacy-preserving transaction shows its effects on public
            // accounts: here, the claim recorded on the schedule. It is only
            // ever included once its proof checked, so it applied.
            for (const QJsonValue& a : msg.value("public_actions").toArray()) {
                const QJsonObject ao = a.toObject();
                if (ao.value("account_id").toString() != self) continue;
                for (const QJsonValue& e : ao.value("effects").toArray()) {
                    const QJsonObject eo = e.toObject();
                    if (eo.value("program_account_id").toString() != prog) continue;
                    const QByteArray data = bytesOf(eo.value("data").toArray());
                    Reader r{data};
                    if (r.u8() != 1) continue;  // Effect::Claim
                    r.bytes(32); r.bytes(32);
                    Schedule tmp; readAsset(r, tmp);
                    const u128 amount = r.u128v();
                    const quint64 at = r.u64();
                    if (!r.ok) continue;
                    st.claimed += amount;
                    st.lastSeen = std::max(st.lastSeen, at);
                    privateSeen = true;
                    ev["what"] = QStringLiteral("Claimed privately");
                    ev["detail"] = fmt(amount) + " " + unit + " into a shielded account";
                    ev["ok"] = true; ev["at"] = double(at);
                }
            }
            if (!ev.contains("what")) continue;
            events.append(ev);
            continue;
        }

        if (msg.value("program_account_id").toString() != prog) continue;
        const QByteArray data = bytesOf(msg.value("instruction_data").toArray());
        const Ix x = decodeIx(data);
        if (!x.ok) continue;
        // Rows naming an account with this program's shard: the schedule
        // itself, or a signer handle.
        QList<QByteArray> rows, handles;
        for (const QJsonValue& rv : msg.value("shard_selectors").toArray()) {
            const QJsonObject ro = rv.toObject();
            const QByteArray id = unbase58(ro.value("account_id").toString());
            rows.append(id);
            if (ro.value("program_account_id").toString() == prog && id != scheduleAcc) handles.append(id);
        }
        const QByteArray signer = handles.isEmpty() ? QByteArray() : handles.last();
        bool ok = false;
        QString what, detail;

        if (x.tag == 0 || x.tag == 1) {
            QByteArray ben;
            if (x.tag == 0) { ben = x.beneficiary; foundSid = x.sid; }
            else {
                for (int i = 0; i < x.beneficiaries.size(); ++i)
                    if (scheduleAccount(program, batchScheduleId(x.batch, quint32(i))) == scheduleAcc) {
                        ben = x.beneficiaries[i]; foundSid = batchScheduleId(x.batch, quint32(i)); foundBatch = x.batch;
                    }
            }
            const QByteArray creator = rows.value(x.tag == 0 ? 2 : 1);
            what = created ? QStringLiteral("Create refused") : QStringLiteral("Created");
            if (created) detail = QStringLiteral("this schedule id already exists");
            else {
                created = ok = true;
                st = x.terms;
                st.beneficiary = ben; st.creator = creator; st.claimed = 0; st.cancelledAt = 0; st.signalled = 0; st.lastSeen = 0;
                const QByteArray zero(32, '\0');
                if (st.cancelAuthority == zero) st.cancelAuthority = creator;
                if (st.milestoneAuthority == zero) st.milestoneAuthority = creator;
                const quint64 mins = (st.end - st.start) / 60000;
                detail = fmt(st.total) + " " + unit + ", "
                       + (st.kind == 2 ? QStringLiteral("in %1 milestones").arg(st.tranches)
                                       : (st.kind == 0 ? QStringLiteral("cliff + linear over ") : QStringLiteral("linear over "))
                                         + (mins >= 2880 ? QString::number(mins / 1440) + " days"
                                            : mins >= 120 ? QString::number(mins / 60) + " h" : QString::number(mins) + " min"));
                if (x.tag == 1) detail += QStringLiteral(", in a batch of %1").arg(x.beneficiaries.size());
            }
        } else {
            if (!x.sid.isEmpty()) foundSid = x.sid;
            if (!x.batch.isEmpty()) foundBatch = x.batch;
            if (!created) {
                // The creation is not in the index: replay from the chain's
                // state, which the consistency check below will then question.
                st = fin; st.claimed = 0; st.cancelledAt = 0; st.signalled = 0; st.lastSeen = 0;
                created = true;
            }
            switch (x.tag) {
            case 2: {
                const u128 vested = vestedAt(st, x.at);
                const u128 owed = vested > st.claimed ? vested - st.claimed : 0;
                ev["at"] = double(x.at);
                if (signer != st.beneficiary) { what = "Claim refused"; detail = "signed by " + who(signer, st) + ", not the beneficiary"; }
                else if (x.amount == 0 || x.amount > owed) { what = "Claim refused"; detail = fmt(x.amount) + " asked, more than had vested"; }
                else if (x.at > fin.lastSeen) {
                    // An applied claim records its time in last_seen; one
                    // later than the chain's last_seen never applied.
                    what = "Claim refused"; detail = "for a time the chain had not reached";
                } else {
                    ok = true; st.claimed += x.amount; st.lastSeen = std::max(st.lastSeen, x.at);
                    what = "Claimed";
                    detail = fmt(x.amount) + " " + unit + " to " + shortKey(base58(rows.value(2)));
                }
                break;
            }
            case 3: {
                ev["at"] = double(x.at);
                const u128 unvested = st.total - vestedAt(st, x.at);
                if (signer != st.cancelAuthority) { what = "Cancel refused"; detail = "signed by " + who(signer, st) + ", not the cancel authority"; }
                else if (!st.cancelable) { what = "Cancel refused"; detail = "the schedule is not cancelable"; }
                else if (st.cancelledAt) { what = "Cancel refused"; detail = "already cancelled"; }
                else if (x.refund != unvested) { what = "Cancel refused"; detail = "refund of " + fmt(x.refund) + " is not the unvested " + fmt(unvested); }
                else {
                    ok = true; st.cancelledAt = x.at; st.lastSeen = std::max(st.lastSeen, x.at);
                    what = "Cancelled"; detail = fmt(x.refund) + " " + unit + " refunded to " + shortKey(base58(st.refundTo));
                }
                break;
            }
            case 4:
                if (signer != st.creator) { what = "Lock refused"; detail = "signed by " + who(signer, st) + ", not the creator"; }
                else { ok = true; st.cancelable = 0; what = "Made non-cancelable"; detail = "by the creator, for good"; }
                break;
            case 5:
                if (signer != st.beneficiary) { what = "Transfer refused"; detail = "signed by " + who(signer, st) + ", not the beneficiary"; }
                else if (!st.transferable) { what = "Transfer refused"; detail = "the schedule is not transferable"; }
                else { ok = true; what = "Transferred"; detail = "to " + shortKey(base58(x.newBeneficiary)); st.beneficiary = x.newBeneficiary; }
                break;
            case 6:
                if (signer != st.milestoneAuthority) { what = "Signal refused"; detail = "signed by " + who(signer, st) + ", not the milestone authority"; }
                else if (x.index >= st.tranches || x.index >= 64) { what = "Signal refused"; detail = QStringLiteral("there is no milestone %1").arg(x.index + 1); }
                else if (st.signalled >> x.index & 1) { what = "Signal refused"; detail = QStringLiteral("milestone %1 was already signalled").arg(x.index + 1); }
                else { ok = true; st.signalled |= quint64(1) << x.index; what = QStringLiteral("Milestone %1 signalled").arg(x.index + 1); detail = "by " + shortKey(base58(signer)); }
                break;
            default: continue;
            }
        }
        ev["what"] = what; ev["detail"] = detail; ev["ok"] = ok;
        events.append(ev);
    }

    const bool consistent = created && st.claimed == fin.claimed && st.cancelledAt == fin.cancelledAt
                            && st.signalled == fin.signalled && st.beneficiary == fin.beneficiary
                            && st.cancelable == fin.cancelable;
    QString note;
    if (events.isEmpty()) note = QStringLiteral("The explorer has no transactions for this schedule yet. Its index can trail the chain by up to two hours.");
    else if (consistent) note = QStringLiteral("Replayed against the schedule's rules; the result matches what the chain holds.");
    else note = QStringLiteral("Replaying these against the schedule's rules does not give what the chain holds, so some verdicts may be wrong. The explorer's index can trail the chain by up to two hours.");
    QJsonObject out{{"events", events}, {"consistent", consistent && !events.isEmpty()}, {"note", note}, {"private", privateSeen}};
    if (!foundSid.isEmpty()) out["scheduleId"] = idText(foundSid);
    if (!foundBatch.isEmpty()) out["batchId"] = idText(foundBatch);
    return out;
}

// ── Examples ─────────────────────────────────────────────────────────────────

const QList<Example>& examples() {
    static const QList<Example> list{
        {"testnet4-lin", "", "Linear, 60 of 600 claimed"},
        {"testnet4-nc", "", "Made non-cancelable"},
        {"testnet4-mil", "", "Milestones, two of four signalled"},
        {"testnet4-tok", "", "Token escrow, cancelled"},
        {"testnet4-auth", "", "Cancelled by a nominated authority"},
        {"testnet4-tr", "", "Transferred to a new beneficiary"},
        {"testnet4-priv", "", "Claimed into a shielded account"},
        {"testnet4-batch8", "batch", "Eight schedules over one escrow"},
    };
    return list;
}

// ── The reader ───────────────────────────────────────────────────────────────

Chain::Chain(QObject* parent) : QObject(parent), explorerPath_(QString::fromLatin1(kExplorerPath)) {
    // Qt's macOS system-proxy lookup JIT-compiles a regular expression, which
    // traps under Basecamp's hardened runtime. Direct connections only.
    QNetworkProxyFactory::setUseSystemConfiguration(false);
    net_.setProxy(QNetworkProxy::NoProxy);
    net_.setTransferTimeout(20000);
}

void Chain::getAccount(const QString& id, std::function<void(const QJsonObject&, const QString&)> cb) {
    const QJsonObject body{{"jsonrpc", "2.0"}, {"id", 1}, {"method", "getAccount"}, {"params", QJsonArray{id}}};
    QNetworkRequest req{QUrl(rpc)};
    req.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/json"));
    QNetworkReply* reply = net_.post(req, QJsonDocument(body).toJson(QJsonDocument::Compact));
    connect(reply, &QNetworkReply::finished, this, [reply, cb]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) { cb({}, reply->errorString()); return; }
        const QJsonObject root = QJsonDocument::fromJson(reply->readAll()).object();
        if (root.contains("error")) { cb({}, root.value("error").toObject().value("message").toString("the node returned an error")); return; }
        if (!root.contains("result")) { cb({}, QStringLiteral("the node's answer is not JSON-RPC")); return; }
        cb(root.value("result").toObject(), QString());
    });
}

void Chain::clock(std::function<void(quint64, quint64, const QString&)> cb) {
    getAccount(QString::fromLatin1(kClockAccount), [cb](const QJsonObject& acc, const QString& err) {
        if (!err.isEmpty()) { cb(0, 0, err); return; }
        const QByteArray d = shard(acc, QString::fromLatin1(kClockProgram));
        Reader r{d};
        const quint64 block = r.u64(), ms = r.u64();
        if (!r.ok) { cb(0, 0, QStringLiteral("the clock account is not a v0.3 clock")); return; }
        cb(block, ms, QString());
    });
}

void Chain::status(Done done) {
    auto out = std::make_shared<QJsonObject>();
    auto j = std::make_shared<Join>();
    j->pending = 2;
    j->then = [out, done]() { done(*out); };
    clock([out, j](quint64 block, quint64 ms, const QString& err) {
        if (!err.isEmpty()) (*out)["error"] = err;
        else { (*out)["block"] = double(block); (*out)["clock"] = double(ms); }
        j->done();
    });
    getAccount(base58(program), [out, j](const QJsonObject& acc, const QString& err) {
        if (err.isEmpty()) (*out)["program"] = acc.value("data").toObject().value("shards").toObject().isEmpty() ? "missing" : "found";
        j->done();
    });
}

void Chain::readSchedules(const QList<QByteArray>& accounts,
                          std::function<void(const QList<QJsonObject>&, quint64, const QString&)> cb) {
    auto results = std::make_shared<QList<QJsonObject>>();
    results->resize(accounts.size());
    auto now = std::make_shared<quint64>(0);
    auto error = std::make_shared<QString>();
    auto j = std::make_shared<Join>();
    j->pending = int(accounts.size()) + 1;
    j->then = [results, now, error, cb]() {
        // Times are computed once the clock is known, for every schedule alike.
        for (QJsonObject& o : *results) {
            if (!o.contains("_raw")) continue;
            Schedule s;
            decodeSchedule(QByteArray::fromBase64(o.value("_raw").toString().toLatin1()), s);
            const QString acct = o.value("account").toString();
            o = scheduleJson(s, *now);
            o["account"] = acct;
        }
        cb(*results, *now, *error);
    };
    clock([now, error, j](quint64, quint64 ms, const QString& err) {
        if (!err.isEmpty()) *error = err;
        *now = ms;
        j->done();
    });
    const QString prog = base58(program);
    for (int i = 0; i < accounts.size(); ++i) {
        const QString acct = base58(accounts[i]);
        getAccount(acct, [results, error, j, i, prog, acct](const QJsonObject& acc, const QString& err) {
            if (!err.isEmpty()) *error = err;
            const QByteArray d = shard(acc, prog);
            Schedule s;
            if (err.isEmpty() && decodeSchedule(d, s))
                (*results)[i] = QJsonObject{{"_raw", QString::fromLatin1(d.toBase64())}, {"account", acct}};
            j->done();
        });
    }
    if (accounts.isEmpty()) { /* the clock read still finishes the join */ }
}

void Chain::loadDetail(const QByteArray& account, const QByteArray& scheduleId, const QByteArray& batchId, Done done) {
    readSchedules({account}, [this, account, scheduleId, batchId, done](const QList<QJsonObject>& rs, quint64 now, const QString& err) {
        if (!err.isEmpty()) { done({{"kind", "error"}, {"text", "Could not read the chain: " + err}}); return; }
        QJsonObject s = rs.value(0);
        if (s.isEmpty()) { done({{"kind", "none"}, {"text", "No schedule of this program at " + base58(account) + "."}}); return; }
        if (!scheduleId.isEmpty()) s["scheduleId"] = idText(scheduleId);
        if (!batchId.isEmpty()) s["batchId"] = idText(batchId);
        // The escrow, read for what it actually holds.
        getAccount(s.value("escrow").toString(), [this, s, now, batchId, account, done](const QJsonObject& acc, const QString& err) mutable {
            if (!err.isEmpty()) s["escrowError"] = err;
            else {
                u128 held = 0;
                if (s.value("asset").toString() == "token") {
                    const QByteArray t = shard(acc, QString::fromLatin1(kTokenProgram));
                    if (t.size() >= 49 && t[0] == 0) { Reader r{t}; r.bytes(33); held = r.u128v(); }
                } else {
                    const QByteArray n = shard(acc, QString::fromLatin1(kNativeShard));
                    if (!n.isEmpty()) { Reader r{n}; held = r.u128v(); }
                }
                s["escrowHeld"] = dec(held);
                // What this schedule alone still needs from its escrow.
                const u128 total = parseDec(s.value("total").toString()), claimed = parseDec(s.value("claimed").toString());
                const u128 vested = parseDec(s.value("vested").toString());
                const u128 owed = s.value("cancelledAt").toDouble() > 0 ? (vested > claimed ? vested - claimed : 0) : total - claimed;
                s["escrowOwed"] = dec(owed);
                // A batch shares one escrow, so one schedule cannot judge it.
                bool shared = !batchId.isEmpty();
                if (!shared && s.contains("scheduleId"))
                    shared = base58(holdingAccount(program, idBytes(s.value("scheduleId").toString()))) != s.value("escrow").toString();
                s["escrowShared"] = shared;
                s["escrowCovers"] = held >= owed;
            }
            s["account"] = base58(account);
            done({{"kind", "schedule"}, {"schedule", s}});
            Q_UNUSED(now);
        });
    });
}

void Chain::openSchedule(const QByteArray& account, const QByteArray& batchId, Done done) {
    loadDetail(account, {}, batchId, done);
}

void Chain::listBatch(const QByteArray& batchId, Done done) {
    // Schedules of a batch are numbered from 0; read them in chunks until one
    // is missing.
    auto items = std::make_shared<QJsonArray>();
    auto step = std::make_shared<std::function<void(quint32)>>();
    *step = [this, batchId, items, step, done](quint32 from) {
        QList<QByteArray> accts;
        for (quint32 i = from; i < from + 25 && i < kBatchMax; ++i) accts.append(scheduleAccount(program, batchScheduleId(batchId, i)));
        readSchedules(accts, [this, batchId, items, step, done, from](const QList<QJsonObject>& rs, quint64, const QString& err) {
            if (!err.isEmpty()) { done({{"kind", "error"}, {"text", "Could not read the chain: " + err}}); *step = nullptr; return; }
            bool ended = false;
            for (int k = 0; k < rs.size(); ++k) {
                if (rs[k].isEmpty()) { ended = true; break; }
                QJsonObject o = rs[k];
                o["scheduleId"] = idText(batchScheduleId(batchId, from + quint32(k)));
                o["batchId"] = idText(batchId);
                o["index"] = int(from + quint32(k));
                o["label"] = QStringLiteral("No. %1 of %2").arg(from + quint32(k) + 1);
                items->append(o);
            }
            if (!ended && from + 25 < kBatchMax) { (*step)(from + 25); return; }
            const int n = int(items->size());
            QJsonArray fixed;
            u128 total = 0, claimable = 0; int cancelled = 0;
            for (QJsonValue v : *items) {
                QJsonObject o = v.toObject();
                o["label"] = QStringLiteral("No. %1 of %2").arg(o.value("index").toInt() + 1).arg(n);
                total += parseDec(o.value("total").toString());
                claimable += parseDec(o.value("claimable").toString());
                cancelled += o.value("cancelledAt").toDouble() > 0;
                fixed.append(o);
            }
            const QString unit = n ? fixed.first().toObject().value("unit").toString() : QStringLiteral("LGO");
            done({{"kind", "list"}, {"title", "Batch " + idText(batchId)},
                  {"subtitle", QStringLiteral("%1 schedules over one escrow · %2 %3 in all · %4 claimable now")
                                   .arg(n).arg(fmt(total), unit, fmt(claimable))},
                  {"batchId", idText(batchId)}, {"count", n}, {"total", dec(total)}, {"claimable", dec(claimable)},
                  {"cancelled", cancelled}, {"items", fixed}});
            *step = nullptr;
        });
    };
    (*step)(0);
}

void Chain::explorerTxs(const QString& account, std::function<void(const QJsonArray&, const QString&)> cb, bool rediscovered) {
    QUrl url(explorer + explorerPath_);
    QNetworkRequest req{url};
    req.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/x-www-form-urlencoded"));
    QUrlQuery form;
    form.addQueryItem("account_id", account);
    form.addQueryItem("offset", "0");
    form.addQueryItem("limit", "200");
    QNetworkReply* reply = net_.post(req, form.toString(QUrl::FullyEncoded).toLatin1());
    connect(reply, &QNetworkReply::finished, this, [this, reply, account, cb, rediscovered]() {
        reply->deleteLater();
        const int code = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
        const QByteArray body = reply->readAll();
        const QJsonDocument doc = QJsonDocument::fromJson(body);
        if (reply->error() == QNetworkReply::NoError && doc.isArray()) { cb(doc.array(), QString()); return; }
        if (rediscovered || (code != 404 && reply->error() != QNetworkReply::ContentNotFoundError && doc.isNull() == false)) {
            cb({}, reply->error() != QNetworkReply::NoError ? reply->errorString() : QStringLiteral("the explorer's answer is not a list"));
            return;
        }
        // The server function moved: find its new name in the explorer's bundle.
        QNetworkReply* w = net_.get(QNetworkRequest(QUrl(explorer + "/pkg/explorer_service.wasm")));
        connect(w, &QNetworkReply::finished, this, [this, w, account, cb]() {
            w->deleteLater();
            const QByteArray wasm = w->readAll();
            const QByteArray name("/api/get_transactions_by_account");
            const int at = wasm.indexOf(name);
            int end = at + name.size();
            while (at >= 0 && end < wasm.size() && wasm[end] >= '0' && wasm[end] <= '9') ++end;
            if (at < 0 || end == at + name.size()) { cb({}, QStringLiteral("the explorer no longer lists an account's transactions")); return; }
            explorerPath_ = QString::fromLatin1(wasm.mid(at, end - at));
            explorerTxs(account, cb, true);
        });
    });
}

void Chain::activity(const QByteArray& account, Done done) {
    explorerTxs(base58(account), [this, account, done](const QJsonArray& txs, const QString& err) {
        if (!err.isEmpty()) { done({{"kind", "activity"}, {"unavailable", true}, {"note", "The explorer could not be read: " + err + "."}, {"events", QJsonArray()}}); return; }
        getAccount(base58(account), [this, account, txs, done](const QJsonObject& acc, const QString& err) {
            Schedule fin;
            if (!err.isEmpty() || !decodeSchedule(shard(acc, base58(program)), fin)) {
                done({{"kind", "activity"}, {"unavailable", true}, {"note", QStringLiteral("The schedule could not be read to replay its transactions.")}, {"events", QJsonArray()}});
                return;
            }
            QJsonObject out = replayActivity(txs, program, account, fin);
            out["kind"] = "activity";
            done(out);
        });
    });
}

void Chain::summaries(const QList<QPair<QString, QString>>& ids, Done done) {
    // Singles in one read; each batch listed on its own; then in the given order.
    auto items = std::make_shared<QList<QJsonObject>>();
    items->resize(ids.size());
    auto error = std::make_shared<QString>();
    auto j = std::make_shared<Join>();
    j->pending = 1;
    j->then = [items, error, done]() {
        QJsonArray out;
        for (const QJsonObject& o : *items) if (!o.isEmpty()) out.append(o);
        if (out.isEmpty() && !error->isEmpty()) {
            done({{"kind", "error"}, {"text", error->startsWith("Could not") ? *error : "Could not read the chain: " + *error}});
            return;
        }
        done({{"kind", "list"}, {"items", out}});
    };
    QList<QByteArray> singles; QList<int> where;
    for (int i = 0; i < ids.size(); ++i) {
        const QByteArray id = idBytes(ids[i].first);
        if (ids[i].second == "batch") {
            ++j->pending;
            listBatch(id, [items, error, j, i, id](const QJsonObject& r) {
                if (r.value("kind").toString() == "error") *error = r.value("text").toString();
                if (r.value("kind").toString() == "list") {
                    const int n = r.value("count").toInt(), c = r.value("cancelled").toInt();
                    QJsonArray chips{QJsonObject{{"text", c ? QStringLiteral("%1 of %2 cancelled").arg(c).arg(n) : QStringLiteral("Batch of %1").arg(n)}, {"tone", c ? "warn" : "dim"}}};
                    const QJsonObject first = r.value("items").toArray().first().toObject();
                    (*items)[i] = QJsonObject{{"isBatch", true}, {"scheduleId", idText(id)}, {"batchId", idText(id)},
                                              {"kindLabel", QStringLiteral("Batch of %1").arg(n)}, {"total", r.value("total")},
                                              {"claimable", r.value("claimable")}, {"unit", first.value("unit")},
                                              {"state", chips.first().toObject().value("text")}, {"tone", c ? "warn" : "dim"}};
                }
                j->done();
            });
        } else { singles.append(scheduleAccount(program, id)); where.append(i); }
    }
    ++j->pending;
    readSchedules(singles, [items, error, where, ids, j](const QList<QJsonObject>& rs, quint64, const QString& err) {
        if (!err.isEmpty()) *error = err;
        for (int k = 0; k < rs.size(); ++k) {
            if (rs[k].isEmpty()) continue;
            QJsonObject o = rs[k];
            o["scheduleId"] = ids[where[k]].first;
            (*items)[where[k]] = o;
        }
        j->done();
    });
    j->done();
}

void Chain::searchAccount(const QByteArray& account, const QJsonObject& acc, Done done) {
    Q_UNUSED(acc);
    const QString who = base58(account);
    explorerTxs(who, [this, account, who, done](const QJsonArray& txs, const QString& explorerErr) {
        // Candidates: every schedule this account's transactions name, and the
        // examples. ids[account] = (schedule id, batch id).
        QHash<QByteArray, QPair<QByteArray, QByteArray>> ids;
        const QString prog = base58(program);
        for (const QJsonValue& v : txs) {
            const QJsonObject o = v.toObject();
            if (o.contains("PrivacyPreserving")) {
                for (const QJsonValue& a : o.value("PrivacyPreserving").toObject().value("message").toObject().value("public_actions").toArray())
                    for (const QJsonValue& e : a.toObject().value("effects").toArray())
                        if (e.toObject().value("program_account_id").toString() == prog) {
                            const QByteArray sa = unbase58(a.toObject().value("account_id").toString());
                            if (!sa.isEmpty() && !ids.contains(sa)) ids.insert(sa, {});
                        }
                continue;
            }
            const QJsonObject msg = o.value("Public").toObject().value("message").toObject();
            if (msg.value("program_account_id").toString() != prog) continue;
            const Ix x = decodeIx(bytesOf(msg.value("instruction_data").toArray()));
            if (!x.ok) continue;
            if (x.tag == 1) {
                for (int i = 0; i < x.beneficiaries.size(); ++i) {
                    const QByteArray sid = batchScheduleId(x.batch, quint32(i));
                    ids.insert(scheduleAccount(program, sid), {sid, x.batch});
                }
            } else if (!x.sid.isEmpty()) {
                ids.insert(scheduleAccount(program, x.sid), {x.sid, x.batch});
            }
        }
        for (const Example& e : examples()) {
            if (QByteArray(e.batch) == "batch") continue;
            const QByteArray sid = idBytes(QString::fromLatin1(e.id));
            ids.insert(scheduleAccount(program, sid), {sid, {}});
        }
        QList<QByteArray> accts = ids.keys();
        if (accts.size() > kBatchMax) accts = accts.mid(0, kBatchMax);
        readSchedules(accts, [account, who, ids, accts, explorerErr, done](const QList<QJsonObject>& rs, quint64, const QString& err) {
            if (!err.isEmpty() && rs.isEmpty()) { done({{"kind", "error"}, {"text", "Could not read the chain: " + err}}); return; }
            QList<QJsonObject> found;
            for (int k = 0; k < rs.size(); ++k) {
                QJsonObject o = rs[k];
                if (o.isEmpty()) continue;
                QStringList roles;
                if (o.value("beneficiary").toString() == who) roles << "beneficiary";
                if (o.value("creator").toString() == who) roles << "creator";
                if (o.value("cancelAuthority").toString() == who && !roles.contains("creator")) roles << "cancel authority";
                if (o.value("milestoneAuthority").toString() == who && !roles.contains("creator")) roles << "milestone authority";
                if (o.value("refundTo").toString() == who) roles << "refund account";
                if (roles.isEmpty()) continue;
                const auto pair = ids.value(accts[k]);
                if (!pair.first.isEmpty()) o["scheduleId"] = idText(pair.first);
                if (!pair.second.isEmpty()) {
                    o["batchId"] = idText(pair.second);
                    for (quint32 i = 0; i < kBatchMax; ++i)
                        if (batchScheduleId(pair.second, i) == pair.first) { o["label"] = QStringLiteral("%1 · No. %2").arg(idText(pair.second)).arg(i + 1); break; }
                }
                if (pair.first.isEmpty()) o["label"] = "Schedule " + shortKey(o.value("account").toString());
                o["roles"] = roles.join(", ");
                found.append(o);
            }
            // Something to claim first, then by name.
            std::sort(found.begin(), found.end(), [](const QJsonObject& a, const QJsonObject& b) {
                const bool ca = a.value("claimable").toString() != "0", cb = b.value("claimable").toString() != "0";
                if (ca != cb) return ca;
                auto name = [](const QJsonObject& o) { return o.value("label").toString(o.value("scheduleId").toString()); };
                return name(a) < name(b);
            });
            QJsonArray items;
            for (const QJsonObject& o : found) items.append(o);
            const QString limit = QStringLiteral(
                "Found from this account's transactions on the explorer and from the examples. The chain cannot list "
                "schedules by beneficiary, and creating a schedule is not recorded under the beneficiary's account, so a "
                "schedule this account has never claimed from or been transferred cannot be found from the account alone. "
                "Look it up by its id.");
            done({{"kind", "list"}, {"title", "Account " + who.left(4) + QChar(0x2026) + who.right(4)},
                  {"account", who},
                  {"subtitle", items.isEmpty() ? QStringLiteral("No schedule found for this account.")
                                               : QStringLiteral("%1 schedule%2 where this account has a role").arg(items.size()).arg(items.size() == 1 ? "" : "s")},
                  {"note", explorerErr.isEmpty() ? limit : "The explorer could not be read (" + explorerErr + "), so only the examples were searched. " + limit},
                  {"items", items}});
        });
    });
}

void Chain::lookup(const QString& raw, Done done) {
    const QString q = raw.trimmed();
    if (q.isEmpty()) { done({{"kind", "error"}, {"text", QStringLiteral("Type a schedule id, a batch id or an account.")}}); return; }
    const QByteArray asAccount = q.size() >= 32 ? unbase58(q) : QByteArray();
    if (!asAccount.isEmpty()) {
        getAccount(q, [this, asAccount, done](const QJsonObject& acc, const QString& err) {
            if (!err.isEmpty()) { done({{"kind", "error"}, {"text", "Could not read the chain: " + err}}); return; }
            Schedule s;
            if (decodeSchedule(shard(acc, base58(program)), s)) { loadDetail(asAccount, {}, {}, done); return; }
            searchAccount(asAccount, acc, done);
        });
        return;
    }
    QString err;
    const QByteArray id = idBytes(q, &err);
    if (id.isEmpty()) { done({{"kind", "error"}, {"text", err}}); return; }
    const QByteArray sAcc = scheduleAccount(program, id), b0 = scheduleAccount(program, batchScheduleId(id, 0));
    readSchedules({sAcc, b0}, [this, q, id, sAcc, done](const QList<QJsonObject>& rs, quint64, const QString& e) {
        if (!e.isEmpty()) { done({{"kind", "error"}, {"text", "Could not read the chain: " + e}}); return; }
        if (!rs.value(0).isEmpty()) { loadDetail(sAcc, id, {}, done); return; }
        if (!rs.value(1).isEmpty()) { listBatch(id, done); return; }
        done({{"kind", "none"}, {"text", "No schedule or batch with the id “" + q + "” on this program."}});
    });
}

}  // namespace av
