// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Bridge exposed to QML as `bridge`. It reads the live state of the three
// Antumbra programs straight from a LEZ sequencer over JSON-RPC and decodes the
// borsh account data, so the panel shows what the chain holds rather than a
// cached copy of what we last wrote.
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

    // Point the panel at a different sequencer. Defaults to public testnet.
    Q_INVOKABLE void setEndpoint(const QString& url);
    Q_INVOKABLE QString endpoint() const { return m_rpc; }

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
    void statusChanged(const QString& text);

private:
    void fetch(const QString& label, const QString& accountId);

    QNetworkAccessManager m_net;
    QString m_rpc;
    quint64 m_nowMs = 0;
};
