// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Runs the app's real view (src/qml/Main.qml) outside Basecamp, against the
// real chain reader, with a stand-in for what Basecamp gives a view: a
// `logos` object whose module() is the backend and whose watch() resolves at
// once. For layout checks at every width and for screenshots; the app itself
// is verified in Basecamp.
//
//   qml_host <Main.qml> <width> <height> <out.png|-> [query] [--offline] [--sheet] [--walkthrough DIR]
//
// With a query, it is looked up (and the first list item opened) before the
// screenshot. --offline points the reader at an address that refuses
// connections, to see the error states; the run fails on any QML warning.
//
// The pre-claim sheet (RFP-017 U4, U5, Privacy 2) is then checked: offline,
// on testnet4-lin as saved in tests/fixtures (next to this file, found from
// Main.qml's path); with --sheet, on the schedule the query opened. Both
// paths are opened; the amount is checked at 0, over the claimable amount
// and not a number; the public path shows the command at once, the private
// path shows the disclosure and hides the command until it is acknowledged,
// and the disclosure sits above the command. <out>-sheet-public.png and
// <out>-sheet-private.png are saved. Any failed check fails the run.
//
// --walkthrough DIR (with a query) saves numbered frames of a scripted tour,
// 15 per second of film: the schedule, its curve, escrow and activity, then
// the sheet on the public path, the private path, the disclosure
// acknowledged and the command. `ffmpeg -framerate 15 -i DIR/f%05d.png` makes
// the clip.
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QJsonDocument>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QTimer>
#include <QDateTime>
#include <algorithm>
#include <cstdio>
#include <functional>
#include <memory>
#include <cstdlib>

#include "vesting_chain.h"

class Host : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString appVersion READ appVersion CONSTANT)
    Q_PROPERTY(QString rpc MEMBER rpc_ CONSTANT)
    Q_PROPERTY(QString program MEMBER program_ CONSTANT)
    Q_PROPERTY(QString explorer MEMBER explorer_ CONSTANT)
    Q_PROPERTY(QString defaultsJson MEMBER defaults_ CONSTANT)
    Q_PROPERTY(QString envOverrides MEMBER env_ CONSTANT)
    Q_PROPERTY(QString statusJson MEMBER status_ NOTIFY statusJsonChanged)
public:
    explicit Host(bool offline) {
        rpc_ = offline ? "https://127.0.0.1:9" : "https://testnet.lez.logos.co";
        explorer_ = offline ? "https://127.0.0.1:9" : "https://explorer.testnet.lez.logos.co";
        program_ = "FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X";
        defaults_ = "{\"rpc\":\"https://testnet.lez.logos.co\",\"program\":\"FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X\",\"explorer\":\"https://explorer.testnet.lez.logos.co\"}";
        chain_.rpc = rpc_; chain_.explorer = explorer_; chain_.program = av::unbase58(program_);
        status_ = "{\"state\":\"connecting\"}";
    }
    QString appVersion() const { return QStringLiteral("0.4.1"); }

    Q_INVOKABLE QString lookup(QString t, QString q) { chain_.lookup(q, [=](const QJsonObject& o) { send(t, o); }); return {}; }
    Q_INVOKABLE QString openSchedule(QString t, QString a, QString b) {
        chain_.openSchedule(av::unbase58(a), b.isEmpty() ? QByteArray() : av::idBytes(b), [=](const QJsonObject& o) { send(t, o); });
        return {};
    }
    Q_INVOKABLE QString examples(QString t) {
        QList<QPair<QString, QString>> ids;
        QJsonObject notes;
        for (const auto& x : av::examples()) { ids.append(qMakePair(QString::fromLatin1(x.id), QString::fromLatin1(x.batch))); notes[QString::fromLatin1(x.id)] = QString::fromLatin1(x.note); }
        chain_.summaries(ids, [=](QJsonObject o) {
            QJsonArray items;
            for (QJsonValue v : o.value("items").toArray()) { QJsonObject i = v.toObject(); i["note"] = notes.value(i.value("scheduleId").toString()); items.append(i); }
            o["items"] = items; send(t, o);
        });
        return {};
    }
    Q_INVOKABLE QString activity(QString t, QString a) { chain_.activity(av::unbase58(a), [=](const QJsonObject& o) { send(t, o); }); return {}; }
    Q_INVOKABLE QString claimContext(QString t, QString b, QString asset) {
        chain_.claimContext(av::unbase58(b), asset == "token", [=](const QJsonObject& o) { send(t, o); });
        return {};
    }
    Q_INVOKABLE QString refreshStatus() {
        chain_.status([this](const QJsonObject& s) {
            QJsonObject out = s;
            out["state"] = s.contains("error") ? "error" : s.value("program").toString() == "missing" ? "warn" : "ok";
            if (s.contains("error")) out["text"] = rpc_ + " did not answer: " + s.value("error").toString();
            status_ = QString::fromUtf8(QJsonDocument(out).toJson(QJsonDocument::Compact));
            emit statusJsonChanged();
        });
        return {};
    }
    Q_INVOKABLE QString saveSettings(QString, QString, QString) { return QStringLiteral("error: not in the test host"); }
    Q_INVOKABLE QString openLink(QString url) { std::printf("openLink %s\n", qPrintable(url)); return {}; }
signals:
    void answered(QString token, QString json);
    void statusJsonChanged();
private:
    void send(const QString& t, const QJsonObject& o) { emit answered(t, QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact))); }
    av::Chain chain_;
    QString rpc_, program_, explorer_, defaults_, env_, status_;
};

// What Basecamp gives a view.
class Logos : public QObject {
    Q_OBJECT
public:
    explicit Logos(Host* h) : host_(h) {}
    Q_INVOKABLE QObject* module(const QString&) { return host_; }
    Q_INVOKABLE bool isViewModuleReady(const QString&) { return true; }
    Q_INVOKABLE void watch(const QVariant& pending, const QJSValue& ok, const QJSValue&) { QJSValue f = ok; f.call({QJSValue(pending.toString())}); }
signals:
    void viewModuleReadyChanged(QString name, bool isReady);
private:
    Host* host_;
};

static QVariant toVariant(const QJsonObject& o) { return QVariant(o.toVariantMap()); }

int main(int argc, char** argv) {
    QGuiApplication app(argc, argv);
    if (argc < 5) { std::printf("usage: qml_host <Main.qml> <width> <height> <out.png|-> [query] [--offline] [--sheet] [--walkthrough DIR]\n"); return 2; }
    const QString mainQml = QString::fromLocal8Bit(argv[1]);
    const int w = atoi(argv[2]), h = atoi(argv[3]);
    if (w < 1 || h < 1) { std::printf("width and height are positive integers\n"); return 2; }
    const QString out = QString::fromLocal8Bit(argv[4]);
    QString query, walkDir; bool offline = false, sheetLive = false;
    for (int i = 5; i < argc; ++i) {
        const QByteArray a(argv[i]);
        if (a == "--offline") offline = true;
        else if (a == "--sheet") sheetLive = true;
        else if (a == "--walkthrough" && i + 1 < argc) walkDir = QString::fromLocal8Bit(argv[++i]);
        else query = QString::fromLocal8Bit(argv[i]);
    }

    int warnings = 0;
    static int* warn = &warnings;
    qInstallMessageHandler([](QtMsgType t, const QMessageLogContext&, const QString& m) {
        if (t != QtDebugMsg && t != QtInfoMsg && !m.contains("font family") && !m.contains("OpenGL")) { ++*warn; if (*warn <= 20) std::fprintf(stderr, "QML: %s\n", qPrintable(m));
            if (*warn > 2000) { std::fprintf(stderr, "QML: too many warnings, giving up\n"); std::_Exit(1); } }
    });

    Host host(offline);
    Logos logos(&host);
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty("logos", &logos);
    QQuickWindow window;
    window.resize(w, h);
    window.setColor(QColor("#101114"));
    QQmlComponent comp(&engine, QUrl::fromLocalFile(mainQml));
    QObject* obj = comp.create(engine.rootContext());
    if (!obj) { std::fprintf(stderr, "%s\n", qPrintable(comp.errorString())); return 1; }
    auto* root = qobject_cast<QQuickItem*>(obj);
    root->setParentItem(window.contentItem());
    root->setSize(QSizeF(w, h));
    window.show();

    int failures = 0, sheetChecks = 0;
    auto check = [&](bool ok, const QString& what) { ++sheetChecks; if (!ok) { ++failures; std::fprintf(stderr, "FAIL sheet: %s\n", qPrintable(what)); } };
    auto leave = [&]() {
        std::fflush(stdout); std::fflush(stderr);
        // Leave at once: the run's answer is in, and tearing the scene down
        // under the offscreen platform is not what is being tested.
        std::_Exit(warnings || failures ? 1 : 0);
    };
    auto item = [&](const char* prop) { return qvariant_cast<QQuickItem*>(root->property(prop)); };
    auto shown = [&](const char* prop) { QQuickItem* i = item(prop); return i && i->isVisible(); };
    auto derived = [&](const QString& suffix) {
        QString b = out; if (b.endsWith(".png")) b.chop(4); return b + suffix + ".png";
    };
    auto saveWindow = [&](const QString& path) {
        if (out == "-" && walkDir.isEmpty()) return;
        const QImage img = window.grabWindow();
        if (img.isNull() || !img.save(path)) { std::fprintf(stderr, "could not save %s\n", qPrintable(path)); ++failures; return; }
        std::printf("saved %s\n", qPrintable(path));
    };
    // Waits (on the event loop) for `ready`, then runs `next`.
    std::function<void(std::function<bool()>, std::function<void()>, int)> waitFor;
    waitFor = [&](std::function<bool()> ready, std::function<void()> next, int left) {
        if (ready() || left <= 0) { QTimer::singleShot(250, next); return; }
        QTimer::singleShot(100, [&, ready, next, left]() { waitFor(ready, next, left - 1); });
    };
    auto ctxReady = [&]() { return root->property("claimCtxState").toString() == "ready"; };
    auto scrollSheetTo = [&](const char* prop) {
        QQuickItem* f = item("sheetFlickable"); QQuickItem* t = item(prop);
        if (!f || !t) return;
        auto* content = qvariant_cast<QQuickItem*>(f->property("contentItem"));
        const qreal y = t->mapToItem(content, QPointF(0, 0)).y();
        const qreal maxY = std::max<qreal>(0, f->property("contentHeight").toReal() - f->height());
        f->setProperty("contentY", std::clamp<qreal>(y - 12, 0, maxY));
    };

    // ── The pre-claim sheet, both paths ──────────────────────────────────────
    auto sheetTest = [&]() {
        if (offline) {
            // testnet4-lin as the testnet held it, read at its end plus an hour.
            const QString dir = QFileInfo(mainQml).absolutePath() + "/../../tests/fixtures";
            QFile f(dir + "/testnet4-lin.account.json");
            av::Schedule sch;
            QByteArray shardBytes;
            if (f.open(QIODevice::ReadOnly)) {
                const QJsonObject acc = QJsonDocument::fromJson(f.readAll()).object();
                for (const QJsonValue& v : acc.value("data").toObject().value("shards").toObject().value("FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X").toArray()) shardBytes.append(char(v.toInt()));
            }
            check(av::decodeSchedule(shardBytes, sch), "the fixture testnet4-lin decodes (" + dir + ")");
            QJsonObject d = av::scheduleJson(sch, sch.end + 3600000);
            const QByteArray acct = av::scheduleAccount(av::unbase58("FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X"), av::idBytes("testnet4-lin"));
            d["account"] = av::base58(acct);
            d["receivedAt"] = double(QDateTime::currentMSecsSinceEpoch());
            root->setProperty("opened", toVariant(QJsonObject{{"account", av::base58(acct)}, {"scheduleId", "testnet4-lin"}, {"batchId", ""}}));
            root->setProperty("detail", toVariant(d));
            root->setProperty("detailState", "ready");
        }
        const QVariantMap det = root->property("detail").toMap();
        check(!det.isEmpty(), "a schedule is open");
        const QString claimable = det.value("claimable").toString();
        check(!claimable.isEmpty() && claimable != "0", "the schedule has something to claim (" + claimable + ")");
        QMetaObject::invokeMethod(root, "setClaimPath", Q_ARG(QVariant, false));
        QMetaObject::invokeMethod(root, "prepareClaim");
        waitFor(ctxReady, [&, claimable]() {
            check(root->property("sheetOpen").toBool(), "Prepare claim opens the sheet");
            check(!shown("sheetDisclosure"), "public path: no privacy disclosure");
            check(shown("sheetCommand"), "public path: the command is shown for the claimable amount");
            check(!shown("sheetAmountError"), "public path: the claimable amount is accepted");
            const QString cmd = QString(root->property("claimAmount").toString());
            check(cmd == claimable, "the amount starts at the claimable amount");
            auto amount = [&](const char* v, bool ok, const QString& what) {
                QMetaObject::invokeMethod(root, "setClaimAmountForTest", Q_ARG(QVariant, QString::fromLatin1(v)));
                QCoreApplication::processEvents();
                check(shown("sheetAmountError") == !ok, what + (ok ? ": no error" : ": a named error"));
                check(shown("sheetCommand") == ok, what + (ok ? ": the command is shown" : ": the command is hidden"));
            };
            amount("0", false, "amount 0");
            amount("12a", false, "amount not a number");
            const QString over = QString::number(claimable.toULongLong() + 1);
            amount(over.toLatin1().constData(), false, "amount over the claimable");
            amount("1", true, "amount 1");
            QMetaObject::invokeMethod(root, "setClaimAmountForTest", Q_ARG(QVariant, claimable));
            QCoreApplication::processEvents();
            QVariant c; QMetaObject::invokeMethod(root, "claimCommand", Q_RETURN_ARG(QVariant, c), Q_ARG(QVariant, root->property("detail")));
            check(c.toString().contains("--amount " + claimable) && c.toString().contains("--to Public/"), "the public command carries --amount and a Public/ destination");
            QTimer::singleShot(300, [&]() {
                saveWindow(derived("-sheet-public"));
                QMetaObject::invokeMethod(root, "setClaimPath", Q_ARG(QVariant, true));
                QCoreApplication::processEvents();
                check(shown("sheetDisclosure"), "private path: the privacy disclosure is shown");
                check(!shown("sheetCommand"), "private path: no command before the disclosure is acknowledged");
                QMetaObject::invokeMethod(root, "acknowledgeForTest");
                QCoreApplication::processEvents();
                check(shown("sheetCommand"), "private path: the command is shown once acknowledged");
                QQuickItem* dsc = item("sheetDisclosure"); QQuickItem* cm = item("sheetCommand");
                check(dsc && cm && dsc->mapToScene(QPointF(0, 0)).y() < cm->mapToScene(QPointF(0, 0)).y(), "private path: the disclosure precedes the command");
                QVariant c2; QMetaObject::invokeMethod(root, "claimCommand", Q_RETURN_ARG(QVariant, c2), Q_ARG(QVariant, root->property("detail")));
                check(c2.toString().contains("--to Private/"), "the private command names a Private/ destination");
                scrollSheetTo("sheetDisclosure");
                QTimer::singleShot(300, [&]() {
                    saveWindow(derived("-sheet-private"));
                    std::printf("sheet: %d checks, %d failed, %d warning(s)\n", sheetChecks, failures, warnings);
                    leave();
                });
            });
        }, 150);
    };

    // ── A scripted tour, as numbered frames ──────────────────────────────────
    struct Seg { int frames; std::function<void()> start; std::function<void(double)> step; std::function<bool()> wait; };
    auto segs = std::make_shared<QList<Seg>>();
    auto frameNo = std::make_shared<int>(0);
    auto detailFlick = [&]() { return item("detailFlickable"); };
    auto yOf = [&](const char* name) -> qreal {
        QQuickItem* f = detailFlick(); auto* t = root->findChild<QQuickItem*>(name);
        if (!f || !t) return 0;
        auto* content = qvariant_cast<QQuickItem*>(f->property("contentItem"));
        const qreal maxY = std::max<qreal>(0, f->property("contentHeight").toReal() - f->height());
        return std::clamp<qreal>(t->mapToItem(content, QPointF(0, 0)).y() - 8, 0, maxY);
    };
    auto scrollSeg = [&](const char* target, int frames) {
        auto from = std::make_shared<qreal>(0), to = std::make_shared<qreal>(0);
        return Seg{frames, [&, from, to, target]() { *from = detailFlick()->property("contentY").toReal(); *to = yOf(target); },
                   [&, from, to](double t) { const double e = t < 0.5 ? 2 * t * t : 1 - 2 * (1 - t) * (1 - t);
                                             detailFlick()->setProperty("contentY", *from + (*to - *from) * e); }, nullptr};
    };
    auto sheetScrollSeg = [&](const char* target, int frames) {
        auto from = std::make_shared<qreal>(0), to = std::make_shared<qreal>(0);
        return Seg{frames, [&, from, to, target]() {
                       QQuickItem* f = item("sheetFlickable"); QQuickItem* t = item(target);
                       auto* content = qvariant_cast<QQuickItem*>(f->property("contentItem"));
                       const qreal maxY = std::max<qreal>(0, f->property("contentHeight").toReal() - f->height());
                       *from = f->property("contentY").toReal();
                       *to = std::clamp<qreal>(t->mapToItem(content, QPointF(0, 0)).y() - 12, 0, maxY); },
                   [&, from, to](double t) { const double e = t < 0.5 ? 2 * t * t : 1 - 2 * (1 - t) * (1 - t);
                                             item("sheetFlickable")->setProperty("contentY", *from + (*to - *from) * e); }, nullptr};
    };
    auto hold = [](int frames, std::function<void()> start = nullptr) { return Seg{frames, start, nullptr, nullptr}; };
    std::function<void(int, int)> runSeg;
    runSeg = [&](int si, int fi) {
        if (si >= segs->size()) { std::printf("walkthrough: %d frames in %s, %d warning(s)\n", *frameNo, qPrintable(walkDir), warnings); leave(); return; }
        Seg& sg = (*segs)[si];
        if (fi == 0) {
            if (sg.start) sg.start();
            if (sg.wait && !sg.wait()) { QTimer::singleShot(100, [&, si]() { (*segs)[si].start = nullptr; runSeg(si, 0); }); return; }
        }
        if (fi >= sg.frames) { runSeg(si + 1, 0); return; }
        if (sg.step) sg.step(sg.frames > 1 ? double(fi) / (sg.frames - 1) : 1.0);
        QTimer::singleShot(sg.step || fi == 0 ? 60 : 0, [&, si, fi]() {
            ++*frameNo;
            saveWindow(QStringLiteral("%1/f%2.png").arg(walkDir).arg(*frameNo, 5, 10, QChar('0')));
            runSeg(si, fi + 1);
        });
    };
    auto walkthrough = [&]() {
        QDir().mkpath(walkDir);
        *segs = {
            hold(45),
            scrollSeg("postCurve", 30), hold(60),
            scrollSeg("postEscrow", 30), hold(45),
            scrollSeg("postActivity", 30), hold(75),
            scrollSeg("postTop", 20),
            Seg{1, [&]() { QMetaObject::invokeMethod(root, "setClaimPath", Q_ARG(QVariant, false)); QMetaObject::invokeMethod(root, "prepareClaim"); }, nullptr, nullptr},
            Seg{0, nullptr, nullptr, ctxReady},
            hold(75),
            hold(45, [&]() { QMetaObject::invokeMethod(root, "setClaimPath", Q_ARG(QVariant, true)); }),
            sheetScrollSeg("sheetDisclosure", 30), hold(90),
            hold(15, [&]() { QMetaObject::invokeMethod(root, "acknowledgeForTest"); }),
            sheetScrollSeg("sheetCommand", 25), hold(90),
        };
        runSeg(0, 0);
    };

    auto grab = [&]() {
        auto after = [&]() {
            if (!walkDir.isEmpty()) { walkthrough(); return; }
            if (offline || sheetLive) { sheetTest(); return; }
            leave();
        };
        if (out == "-" || !walkDir.isEmpty()) { after(); return; }
        root->setSize(QSizeF(w, h));
        auto res = root->grabToImage(QSize(w, h));
        if (!res) { std::fprintf(stderr, "the view could not be grabbed\n"); std::_Exit(1); }
        QObject::connect(res.data(), &QQuickItemGrabResult::ready, [res, out, after, &warnings]() {
            res->saveToFile(out);
            std::printf("saved %s, %d warning(s)\n", qPrintable(out), warnings);
            after();
        });
    };
    // Wait for what the screen shows to have arrived, then grab.
    auto poll = std::make_shared<QTimer>();
    int ticks = 0;
    bool asked = false;
    QObject::connect(poll.get(), &QTimer::timeout, [&]() {
        ++ticks;
        const QString ex = root->property("examplesState").toString();
        if (!query.isEmpty() && !asked && ex != "loading") { QMetaObject::invokeMethod(root, "lookup", Q_ARG(QVariant, query)); asked = true; return; }
        const QString ds = root->property("detailState").toString(), as = root->property("activityState").toString();
        const bool looking = root->property("looking").toBool();
        bool settled = ex != "loading" && !looking;
        if (!query.isEmpty()) settled = settled && asked && (ds == "ready" ? as == "ready" : ds != "loading");
        if ((settled && ticks > 4) || ticks > 200) { poll->stop(); QTimer::singleShot(400, grab); }
    });
    poll->start(150);
    return app.exec();
}

#include "qml_host.moc"
