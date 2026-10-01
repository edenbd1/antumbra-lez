// Drives the Basecamp panel's ChainBridge against a v0.3 sequencer and prints
// what it decodes: the schedule map and the escrow line.
#include <QCoreApplication>
#include <QTimer>
#include <cstdio>
#include "chain_bridge.h"

int main(int argc, char** argv) {
    QCoreApplication app(argc, argv);
    ChainBridge b;
    int pending = 2;
    auto done = [&] { if (--pending == 0) app.quit(); };
    QObject::connect(&b, &ChainBridge::scheduleUpdated, [&](const QVariantMap& m) {
        for (auto it = m.begin(); it != m.end(); ++it)
            std::printf("schedule.%s = %s\n", qPrintable(it.key()), qPrintable(it.value().toString()));
        done();
    });
    QObject::connect(&b, &ChainBridge::escrowUpdated, [&](const QString& w, const QString& v) {
        std::printf("escrow.%s = %s\n", qPrintable(w), qPrintable(v));
        done();
    });
    QObject::connect(&b, &ChainBridge::failed, [&](const QString& w, const QString& why) {
        std::printf("FAILED %s: %s\n", qPrintable(w), qPrintable(why));
        app.exit(1);
    });
    std::printf("endpoint %s\n", qPrintable(b.endpoint()));
    b.refresh();
    QTimer::singleShot(20000, &app, [&] { std::printf("timeout\n"); app.exit(2); });
    return app.exec();
}
