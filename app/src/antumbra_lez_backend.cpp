// SPDX-License-Identifier: MIT OR Apache-2.0
#include "antumbra_lez_backend.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QProcess>
#include <QStandardPaths>
#include <QUrl>

namespace {

const char* kDefaultRpc = "https://testnet.lez.logos.co";
// The vesting program's header on the public testnet v0.3 (DEPLOYMENTS.md).
const char* kDefaultProgram = "FCrja8g2ZKvxZwNZchdppKWQCDxPNUHxnEMidrmqrt6X";
const char* kDefaultExplorer = "https://explorer.testnet.lez.logos.co";

QString envOr(const char* name, const QString& fallback) {
    const QString v = qEnvironmentVariable(name).trimmed();
    return v.isEmpty() ? fallback : v;
}

// A URL to read from: http(s) with a host, without a trailing slash.
bool usableUrl(const QString& s) {
    const QUrl u(s, QUrl::StrictMode);
    return u.isValid() && (u.scheme() == "https" || u.scheme() == "http") && !u.host().isEmpty();
}

}  // namespace

AntumbraLezBackend::AntumbraLezBackend() {
    setAppVersion(QStringLiteral(ANTUMBRA_VERSION));
    setDefaultsJson(QString::fromUtf8(QJsonDocument(QJsonObject{
        {"rpc", kDefaultRpc}, {"program", kDefaultProgram}, {"explorer", kDefaultExplorer}}).toJson(QJsonDocument::Compact)));
    QStringList env;
    for (const char* e : {"ANTUMBRA_RPC", "ANTUMBRA_PROGRAM", "ANTUMBRA_EXPLORER"})
        if (!qEnvironmentVariable(e).trimmed().isEmpty()) env << QString::fromLatin1(e);
    setEnvOverrides(env.join(", "));
    setStatusJson(QStringLiteral("{\"state\":\"connecting\",\"text\":\"Connecting…\"}"));
    loadSettings();
}

void AntumbraLezBackend::onContextReady() {
    // The view asks for the status when it attaches and every 15 s after.
    refreshStatus();
}

QString AntumbraLezBackend::dataDir() const {
    // Basecamp exports --user-dir to its modules as LOGOS_USER_DIR.
    const QString userDir = qEnvironmentVariable("LOGOS_USER_DIR");
    const QString dir = userDir.isEmpty()
        ? QStandardPaths::writableLocation(QStandardPaths::AppDataLocation) + QStringLiteral("/antumbra_lez")
        : userDir + QStringLiteral("/module_data/antumbra_lez");
    QDir().mkpath(dir);
    return dir;
}

void AntumbraLezBackend::loadSettings() {
    QJsonObject saved;
    QFile f(dataDir() + QStringLiteral("/settings.json"));
    if (f.open(QIODevice::ReadOnly)) saved = QJsonDocument::fromJson(f.readAll()).object();
    auto pick = [&](const char* env, const char* key, const char* fallback) {
        const QString s = saved.value(QLatin1String(key)).toString().trimmed();
        return envOr(env, s.isEmpty() ? QString::fromLatin1(fallback) : s);
    };
    apply(pick("ANTUMBRA_RPC", "rpc", kDefaultRpc), pick("ANTUMBRA_PROGRAM", "program", kDefaultProgram),
          pick("ANTUMBRA_EXPLORER", "explorer", kDefaultExplorer));
}

void AntumbraLezBackend::apply(const QString& rpc, const QString& program, const QString& explorer) {
    QString r = rpc, e = explorer;
    while (r.endsWith('/')) r.chop(1);
    while (e.endsWith('/')) e.chop(1);
    chain_.rpc = r;
    chain_.explorer = e;
    chain_.program = av::unbase58(program);
    setRpc(r);
    setProgram(program);
    setExplorer(e);
}

QString AntumbraLezBackend::saveSettings(QString rpc, QString program, QString explorer) {
    rpc = rpc.trimmed(); program = program.trimmed(); explorer = explorer.trimmed();
    while (rpc.endsWith('/')) rpc.chop(1);
    while (explorer.endsWith('/')) explorer.chop(1);
    if (rpc.isEmpty()) rpc = QString::fromLatin1(kDefaultRpc);
    if (program.isEmpty()) program = QString::fromLatin1(kDefaultProgram);
    if (explorer.isEmpty()) explorer = QString::fromLatin1(kDefaultExplorer);
    if (!usableUrl(rpc)) return QStringLiteral("error: The node URL must be an http or https URL.");
    if (!usableUrl(explorer)) return QStringLiteral("error: The explorer URL must be an http or https URL.");
    if (av::unbase58(program).isEmpty()) return QStringLiteral("error: The program id is a base58 account id of 32 bytes.");
    QFile f(dataDir() + QStringLiteral("/settings.json"));
    if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate)) return QStringLiteral("error: The settings could not be written.");
    f.write(QJsonDocument(QJsonObject{{"rpc", rpc}, {"program", program}, {"explorer", explorer}}).toJson());
    f.close();
    // What the environment pins stays pinned.
    apply(envOr("ANTUMBRA_RPC", rpc), envOr("ANTUMBRA_PROGRAM", program), envOr("ANTUMBRA_EXPLORER", explorer));
    refreshStatus();
    return QString();
}

QString AntumbraLezBackend::ready() const {
    if (chain_.program.size() != 32) return QStringLiteral("error: the program id in Settings is not a base58 account id");
    return QString();
}

void AntumbraLezBackend::reply(const QString& token, const QJsonObject& json) {
    emit answered(token, QString::fromUtf8(QJsonDocument(json).toJson(QJsonDocument::Compact)));
}

QString AntumbraLezBackend::lookup(QString token, QString query) {
    if (const QString e = ready(); !e.isEmpty()) return e;
    chain_.lookup(query, [this, token](const QJsonObject& o) { reply(token, o); });
    return QString();
}

QString AntumbraLezBackend::openSchedule(QString token, QString scheduleAccount, QString batchId) {
    if (const QString e = ready(); !e.isEmpty()) return e;
    const QByteArray acc = av::unbase58(scheduleAccount.trimmed());
    if (acc.isEmpty()) return QStringLiteral("error: not a schedule account");
    chain_.openSchedule(acc, batchId.trimmed().isEmpty() ? QByteArray() : av::idBytes(batchId),
                        [this, token](const QJsonObject& o) { reply(token, o); });
    return QString();
}

QString AntumbraLezBackend::examples(QString token) {
    if (const QString e = ready(); !e.isEmpty()) return e;
    QList<QPair<QString, QString>> ids;
    QJsonObject notes;
    for (const av::Example& x : av::examples()) {
        ids.append({QString::fromLatin1(x.id), QString::fromLatin1(x.batch)});
        notes[QString::fromLatin1(x.id)] = QString::fromLatin1(x.note);
    }
    chain_.summaries(ids, [this, token, notes](QJsonObject o) {
        QJsonArray items;
        for (QJsonValue v : o.value("items").toArray()) {
            QJsonObject i = v.toObject();
            i["note"] = notes.value(i.value("scheduleId").toString());
            items.append(i);
        }
        o["items"] = items;
        reply(token, o);
    });
    return QString();
}

QString AntumbraLezBackend::activity(QString token, QString scheduleAccount) {
    if (const QString e = ready(); !e.isEmpty()) return e;
    const QByteArray acc = av::unbase58(scheduleAccount.trimmed());
    if (acc.isEmpty()) return QStringLiteral("error: not a schedule account");
    chain_.activity(acc, [this, token](const QJsonObject& o) { reply(token, o); });
    return QString();
}

QString AntumbraLezBackend::claimContext(QString token, QString beneficiary, QString asset) {
    if (const QString e = ready(); !e.isEmpty()) return e;
    const QByteArray acc = av::unbase58(beneficiary.trimmed());
    if (acc.isEmpty()) return QStringLiteral("error: not an account id");
    chain_.claimContext(acc, asset == QLatin1String("token"), [this, token](const QJsonObject& o) { reply(token, o); });
    return QString();
}

QString AntumbraLezBackend::refreshStatus() {
    if (chain_.program.size() != 32) {
        setStatusJson(QStringLiteral("{\"state\":\"error\",\"text\":\"The program id in Settings is not a base58 account id.\"}"));
        return QString();
    }
    chain_.status([this](const QJsonObject& s) {
        QJsonObject out = s;
        if (s.contains("error")) {
            out["state"] = "error";
            out["text"] = chain_.rpc + " did not answer: " + s.value("error").toString();
        } else {
            out["state"] = s.value("program").toString() == "missing" ? "warn" : "ok";
        }
        setStatusJson(QString::fromUtf8(QJsonDocument(out).toJson(QJsonDocument::Compact)));
    });
    return QString();
}

QString AntumbraLezBackend::openLink(QString url) {
    // Checked again here, on the raw string and as parsed, since this slot
    // opens whatever it is given.
    const QUrl u(url, QUrl::StrictMode);
    // No QRegularExpression here or anywhere in the backend: PCRE2 JIT traps
    // under a hardened runtime without the allow-jit entitlement.
    bool safe = url.startsWith(QLatin1String("https://")) && url.size() > 8 && url.size() < 2048;
    for (QChar c : url) {
        if (!safe) break;
        const ushort k = c.unicode();
        safe = k < 128 && (QChar(c).isLetterOrNumber() || QByteArray("-._~:/?#[]@!$&'()*+,;=%").contains(char(k)));
    }
    if (!safe || !u.isValid() || u.scheme() != QLatin1String("https") || u.host().isEmpty()
        || !u.userInfo().isEmpty())
        return QStringLiteral("error: only https links can be opened");
    // The system's own opener, given the URL as one argument (no shell).
#if defined(Q_OS_MACOS)
    const bool ok = QProcess::startDetached(QStringLiteral("/usr/bin/open"), {url});
#elif defined(Q_OS_WIN)
    const bool ok = QProcess::startDetached(QStringLiteral("rundll32.exe"), {QStringLiteral("url.dll,FileProtocolHandler"), url});
#else
    const bool ok = QProcess::startDetached(QStringLiteral("xdg-open"), {url});
#endif
    return ok ? QString() : QStringLiteral("error: no browser could be started");
}
