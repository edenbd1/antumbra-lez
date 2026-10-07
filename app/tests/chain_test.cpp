// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The app's chain reader, without Basecamp.
//
//   chain_test <fixtures-dir>          offline: addresses, decoding, replay
//   chain_test <fixtures-dir> --live   also reads the public testnet
//
// Offline, every check runs against bytes saved from the public testnet v0.3
// (tests/fixtures): the schedules of the end-to-end run of 2 Oct 2026 and the
// explorer's transactions for them. The verdicts the replay gives must be the
// ones the run's manifest (evidence/v03/testnet.tsv) recorded for each step.
#include <QCoreApplication>
#include <QFile>
#include <QJsonDocument>
#include <QTimer>
#include <cstdio>

#include "vesting_chain.h"

using namespace av;

static int checks = 0, failures = 0;
static void check(bool ok, const QString& what) {
    ++checks;
    if (!ok) { ++failures; std::printf("FAIL %s\n", qPrintable(what)); }
}
static void eq(const QString& got, const QString& want, const QString& what) {
    check(got == want, what + ": got " + got + ", want " + want);
}

static QJsonDocument load(const QString& dir, const QString& name) {
    QFile f(dir + "/" + name);
    if (!f.open(QIODevice::ReadOnly)) { std::printf("FAIL missing fixture %s\n", qPrintable(name)); ++failures; return {}; }
    return QJsonDocument::fromJson(f.readAll());
}
static QByteArray shardOf(const QJsonObject& acc, const QString& prog) {
    QByteArray b;
    for (const QJsonValue& v : acc.value("data").toObject().value("shards").toObject().value(prog).toArray()) b.append(char(v.toInt()));
    return b;
}

int main(int argc, char** argv) {
    QCoreApplication app(argc, argv);
    if (argc < 2) { std::printf("usage: chain_test <fixtures-dir> [--live]\n"); return 2; }
    const QString dir = QString::fromLocal8Bit(argv[1]);
    const bool live = argc > 2 && QByteArray(argv[2]) == "--live";

    const QString progB58 = "FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X";
    const QByteArray prog = unbase58(progB58);
    check(prog.size() == 32, "the program id decodes to 32 bytes");
    eq(base58(prog), progB58, "base58 round trip");
    eq(base58(unbase58("11111111111111111111111111111111")), "11111111111111111111111111111111", "all-zero id round trip");
    check(unbase58("testnet4-lin").isEmpty(), "a schedule id is not mistaken for an account");

    // Addresses, as `antumbra-vesting ids` derives them and the manifest lists them.
    eq(base58(scheduleAccount(prog, idBytes("testnet4-lin"))), "ERiBqLX9VnL326D3hyfVKfMVg1qyYu7gN7U2L1DnWvL9", "schedule PDA of testnet4-lin");
    eq(base58(holdingAccount(prog, idBytes("testnet4-lin"))), "46jcRC8aGtyLmHqHhbTUWEiGjMFby1HWVgt9QtFMmQW4", "escrow PDA of testnet4-lin");
    eq(QString::fromLatin1(batchScheduleId(idBytes("testnet4-batch8"), 0).toHex()),
       "ed655cf9ae7f4d3efb4baae2412e82b32e26ba83a7f4d94232dcead54a6c8837", "schedule 0 of testnet4-batch8");
    eq(base58(holdingAccount(prog, idBytes("testnet4-batch8"))), "2PGcDdbvicrt4gewMf6BofQBbkEM3XZoRvx6Ve4aDsDW", "shared escrow of testnet4-batch8");
    eq(idText(idBytes("testnet4-lin")), "testnet4-lin", "a text id reads back as text");
    eq(idText(batchScheduleId(idBytes("testnet4-batch8"), 0)), "ed655cf9ae7f4d3efb4baae2412e82b32e26ba83a7f4d94232dcead54a6c8837", "a hash id reads back as hex");
    QString err;
    check(idBytes(QString(33, 'x'), &err).isEmpty() && !err.isEmpty(), "an id longer than 32 bytes is refused");
    eq(dec(parseDec("340282366920938463463374607431768211455")), "340282366920938463463374607431768211455", "u128 max round trip");

    // Decoding and accrual, at the chain clock 2026-10-06 17:20:26.791 UTC.
    const quint64 now = 1791307226791ULL;
    struct Want { const char* name; const char* vested; const char* claimed; const char* claimable; const char* state; const char* verdicts; };
    const Want wants[] = {
        {"lin", "600", "60", "540", "Fully vested", "ARRARR"},
        {"can", "50", "50", "0", "Cancelled", "ARRARA"},
        {"tok", "8333", "6666", "1667", "Cancelled", "AAA"},
        {"mil", "500", "500", "0", "2 of 4 signalled", "ARAAA"},
        {"tr", "300", "300", "0", "Fully claimed", "ARARA"},
        {"nc", "300", "0", "300", "Fully vested", "ARAR"},
        {"auth", "60", "0", "60", "Cancelled", "ARA"},
        {"priv", "250", "250", "0", "Fully claimed", "AA"},
        {"b1", "240", "0", "240", "Cancelled", "AA"},
    };
    for (const Want& w : wants) {
        const QString n = QString::fromLatin1(w.name);
        const QJsonObject acc = load(dir, "testnet4-" + n + ".account.json").object();
        Schedule s;
        check(decodeSchedule(shardOf(acc, progB58), s), "testnet4-" + n + " decodes");
        const QJsonObject j = scheduleJson(s, now);
        eq(j.value("vested").toString(), w.vested, n + " vested");
        eq(j.value("claimed").toString(), w.claimed, n + " claimed");
        eq(j.value("claimable").toString(), w.claimable, n + " claimable");
        eq(j.value("state").toString(), w.state, n + " state");
        // The replay of its transactions gives the manifest's verdicts.
        const QJsonArray txs = load(dir, "testnet4-" + n + ".txs.json").array();
        const QByteArray self = unbase58(acc.value("account_id").toString().isEmpty() ? QString() : acc.value("account_id").toString());
        Q_UNUSED(self);
        QByteArray account;
        if (n == "b1") account = scheduleAccount(prog, batchScheduleId(idBytes("testnet4-batch8"), 1));
        else account = scheduleAccount(prog, idBytes("testnet4-" + n));
        const QJsonObject r = replayActivity(txs, prog, account, s);
        QString got;
        for (const QJsonValue& e : r.value("events").toArray()) got += e.toObject().value("ok").toBool() ? 'A' : 'R';
        eq(got, w.verdicts, n + " verdicts");
        check(r.value("consistent").toBool(), n + " replay matches the chain: " + r.value("note").toString());
        if (n == "b1") { eq(r.value("batchId").toString(), "testnet4-batch8", "b1 batch found"); }
        else eq(r.value("scheduleId").toString(), "testnet4-" + n, n + " schedule id found");
        if (n == "lin") {
            const QJsonArray ev = r.value("events").toArray();
            eq(ev[1].toObject().value("detail").toString(), "601 asked, more than had vested", "lin overclaim reason");
            eq(ev[2].toObject().value("detail").toString(), "signed by the creator, not the beneficiary", "lin wrong signer reason");
            eq(ev[4].toObject().value("detail").toString(), "this schedule id already exists", "lin duplicate reason");
            eq(ev[5].toObject().value("detail").toString(), "for a time the chain had not reached", "lin future claim reason");
        }
        if (n == "priv") eq(r.value("events").toArray()[1].toObject().value("what").toString(), "Claimed privately", "priv claim seen");
    }

    if (!live) {
        std::printf("%d checks, %d failed\n", checks, failures);
        return failures ? 1 : 0;
    }

    // Live: the same questions to the public testnet.
    Chain c;
    c.rpc = "https://testnet.lez.logos.co";
    c.explorer = "https://explorer.testnet.lez.logos.co";
    c.program = prog;
    int pending = 4;
    auto fin = [&]() { if (--pending == 0) { std::printf("%d checks, %d failed\n", checks, failures); app.exit(failures ? 1 : 0); } };
    c.lookup("testnet4-lin", [&](const QJsonObject& o) {
        const QJsonObject s = o.value("schedule").toObject();
        eq(o.value("kind").toString(), "schedule", "live lookup testnet4-lin");
        eq(s.value("claimed").toString(), "60", "live claimed");
        eq(s.value("escrowHeld").toString(), "540", "live escrow");
        std::printf("live testnet4-lin: claimable %s, escrow %s, clock %.0f\n", qPrintable(s.value("claimable").toString()),
                    qPrintable(s.value("escrowHeld").toString()), s.value("now").toDouble());
        fin();
    });
    c.lookup("testnet4-batch8", [&](const QJsonObject& o) {
        eq(o.value("kind").toString(), "list", "live batch");
        eq(QString::number(o.value("count").toInt()), "8", "live batch count");
        eq(o.value("claimable").toString(), "4221", "live batch claimable");
        fin();
    });
    c.lookup("2xub3k7XNpfsPk44Gt52dZtadG4zXrqqdPKKgjFZuKfn", [&](const QJsonObject& o) {
        eq(o.value("kind").toString(), "list", "live account search");
        std::printf("live account search: %s\n", qPrintable(o.value("subtitle").toString()));
        check(o.value("items").toArray().size() >= 8, "the beneficiary has at least the examples");
        fin();
    });
    c.activity(scheduleAccount(prog, idBytes("testnet4-lin")), [&](const QJsonObject& o) {
        check(o.value("consistent").toBool(), "live activity replay is consistent: " + o.value("note").toString());
        fin();
    });
    QTimer::singleShot(90000, &app, [&]() { std::printf("FAIL live reads timed out\n"); app.exit(1); });
    return app.exec();
}
