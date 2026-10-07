// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The Basecamp side of Antumbra Vesting. It reads the chain through av::Chain
// (vesting_chain.h, tested on its own) and hands the view JSON; it holds no
// keys and signs nothing. Settings live under the Basecamp user directory, so
// a throwaway --user-dir never touches another instance's.
#pragma once

#include <QJsonObject>
#include <QString>

#include "logos_ui_plugin_context.h"
#include "rep_antumbra_lez_source.h"
#include "vesting_chain.h"

class AntumbraLezBackend : public AntumbraLezSimpleSource, public LogosUiPluginContext {
public:
    AntumbraLezBackend();

    QString lookup(QString token, QString query) override;
    QString openSchedule(QString token, QString scheduleAccount, QString batchId) override;
    QString examples(QString token) override;
    QString activity(QString token, QString scheduleAccount) override;
    QString refreshStatus() override;
    QString saveSettings(QString rpc, QString program, QString explorer) override;
    QString openLink(QString url) override;

protected:
    void onContextReady() override;

private:
    QString dataDir() const;
    void loadSettings();
    void apply(const QString& rpc, const QString& program, const QString& explorer);
    void reply(const QString& token, const QJsonObject& json);
    QString ready() const;  // "" or "error: …" when there is no program to read

    av::Chain chain_;
};
