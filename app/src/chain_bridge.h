// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Bridge exposed to QML as `bridge`. It reads the live state of the three
// Antumbra programs straight from a LEZ sequencer over JSON-RPC and decodes the
// borsh account data, so the panel shows what the chain holds rather than a
// cached copy of what we last wrote. On LEZ v0.3 that data is one shard of an
// account's `shards` map, picked by program id; there is no `program_owner`.
//
// It holds no keys and signs nothing: this is the analytics surface, and giving
// it signing power would make a read-only panel a custody risk for no gain.
//
// Asynchronous by construction. Basecamp 0.2.2 does not bundle QtConcurrent, and
// a nested event loop inside createWidget would freeze the host, so requests are
// issued through QNetworkAccessManager and answered by signal.

#pragma once

#include <QNetworkAccessManager>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantMap>

class ChainBridge : public QObject {
    Q_OBJECT
public:
    explicit ChainBridge(QObject* parent = nullptr);

    // Re-read the vesting schedule against the chain's clock. Results arrive as
    // the signals below; every failure path emits `failed` rather than leaving
    // the panel showing stale numbers as if they were fresh.
    Q_INVOKABLE void refresh();
    // The launchpad programs' accounts (RFP-015/016), read on request only.
    Q_INVOKABLE void refreshLaunchpad();

    // Where the panel reads from. The defaults are the public LEZ testnet and
    // no program: the v0.3 vesting program is not deployed there yet, and the
    // panel says so instead of inventing an address. Each value is taken, in
    // order, from the environment (ANTUMBRA_RPC, ANTUMBRA_PROGRAM,
    // ANTUMBRA_SCHEDULE, ANTUMBRA_HOLDING), the module's saved settings, then
    // the default.
    Q_INVOKABLE QString endpoint() const { return m_rpc; }
    Q_INVOKABLE QString program() const { return m_program; }
    Q_INVOKABLE QString schedule() const { return m_schedule; }
    Q_INVOKABLE QString holding() const { return m_holding; }
    Q_INVOKABLE QString settingsFile() const { return m_settingsFile; }
    Q_INVOKABLE QString defaultEndpoint() const;
    // Which values the environment pinned, so the panel can say a saved
    // setting will not take effect.
    Q_INVOKABLE QStringList envOverrides() const;

    // The clipboard's text, trimmed. Basecamp 0.3.0 does not route key events
    // to a legacy widget module, so a field is filled with a click on Paste.
    Q_INVOKABLE QString clipboardText() const;

    Q_INVOKABLE void setEndpoint(const QString& url);
    Q_INVOKABLE void setVestingTarget(const QString& program, const QString& schedule,
                                      const QString& holding);
    // Set all four and write them to the settings file.
    Q_INVOKABLE void saveSettings(const QString& rpc, const QString& program,
                                  const QString& schedule, const QString& holding);

signals:
    void saleUpdated(const QString& vt, const QString& vc,
                     const QString& saleReserve, const QString& realCollateral,
                     const QString& seedReserve, const QString& feesAccrued);
    /// Native balance actually escrowed, per holding. `which` is "sale" or
    /// "schedule".
    void escrowUpdated(const QString& which, const QString& balance);
    void poolUpdated(const QString& reserveToken, const QString& reserveCollateral,
                     const QString& weightStart, const QString& weightEnd,
                     const QString& lastSeen);
    /// The recipient view: kind, asset, total, claimed, vested, claimable,
    /// next unlock, cancelable, and the clock reading it was computed at.
    void scheduleUpdated(const QVariantMap& s);
    void failed(const QString& which, const QString& reason);
    /// No v0.3 program to read: none configured, or the configured id holds
    /// no account on this sequencer. Not an error, a state.
    void notDeployed(const QString& reason);
    void statusChanged(const QString& text);

private:
    void fetch(const QString& label, const QString& accountId);

    QNetworkAccessManager m_net;
    QString m_rpc;
    QString m_program;
    QString m_schedule;
    QString m_holding;
    QString m_settingsFile;
    quint64 m_nowMs = 0;
};
