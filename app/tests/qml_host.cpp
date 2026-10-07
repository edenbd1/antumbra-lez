// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Runs the app's real view (src/qml/Main.qml) outside Basecamp, against the
// real chain reader, with a stand-in for what Basecamp gives a view: a
// `logos` object whose module() is the backend and whose watch() resolves at
// once. For layout checks at every width and for screenshots; the app itself
// is verified in Basecamp.
//
//   qml_host <Main.qml> <width> <height> <out.png|-> [query] [--offline]
//
// With a query, it is looked up (and the first list item opened) before the
// screenshot. --offline points the reader at an address that refuses
// connections, to see the error states; the run fails on any QML warning.
#include <QGuiApplication>
#include <QJsonDocument>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QTimer>
#include <cstdio>
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
    QString appVersion() const { return QStringLiteral("0.4.0"); }

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

int main(int argc, char** argv) {
    QGuiApplication app(argc, argv);
    if (argc < 5) { std::printf("usage: qml_host <Main.qml> <width> <height> <out.png|-> [query] [--offline]\n"); return 2; }
    const QString mainQml = QString::fromLocal8Bit(argv[1]);
    const int w = atoi(argv[2]), h = atoi(argv[3]);
    if (w < 1 || h < 1) { std::printf("width and height are positive integers\n"); return 2; }
    const QString out = QString::fromLocal8Bit(argv[4]);
    QString query; bool offline = false;
    for (int i = 5; i < argc; ++i) { if (QByteArray(argv[i]) == "--offline") offline = true; else query = QString::fromLocal8Bit(argv[i]); }

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

    auto grab = [&]() {
        // Leave at once: the run's answer is in, and tearing the scene down
        // under the offscreen platform is not what is being tested.
        const int* count = &warnings;
        auto leave = [count]() { std::fflush(stdout); std::fflush(stderr); std::_Exit(*count ? 1 : 0); };
        if (out == "-") { leave(); return; }
        root->setSize(QSizeF(w, h));
        auto res = root->grabToImage(QSize(w, h));
        if (!res) { std::fprintf(stderr, "the view could not be grabbed\n"); std::_Exit(1); }
        QObject::connect(res.data(), &QQuickItemGrabResult::ready, [res, out, leave, count]() {
            res->saveToFile(out);
            std::printf("saved %s, %d warning(s)\n", qPrintable(out), *count);
            leave();
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
