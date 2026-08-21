// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The read-only side of the Antumbra launchpad: what the three deployed
// programs currently hold, fetched from a sequencer rather than from anything
// this panel remembers. Nothing here signs, so nothing here can lose funds.
//
// Amounts are shown at 18 decimals because that is the pair the programs are
// built for and the pair whose product overflows a u128 — displaying raw base
// units would hide the thing worth noticing.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    implicitWidth: 720
    implicitHeight: 560

    readonly property color bg:     "#0e1116"
    readonly property color panel:  "#161b22"
    readonly property color line:   "#2a313b"
    readonly property color fg:     "#e4e8ef"
    readonly property color muted:  "#8d97a6"
    readonly property color accent: "#7fb0e0"

    // 18 decimals, trimmed. A schedule total of 1e21 reads as "1000", which is
    // the number a person actually holds.
    function human(raw) {
        if (raw === undefined || raw === null || raw === "") return "—"
        var s = String(raw)
        if (s.length <= 18) s = "0".repeat(19 - s.length) + s
        var whole = s.slice(0, s.length - 18)
        var frac  = s.slice(s.length - 18).replace(/0+$/, "")
        return frac.length ? whole + "." + frac.slice(0, 6) : whole
    }

    // Weights are stored at 1e18 as a fraction of unity.
    function pct(raw) {
        if (!raw) return "—"
        var v = Number(String(raw)) / 1e18
        return (v * 100).toFixed(2) + "%"
    }

    Rectangle { anchors.fill: parent; color: root.bg }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 14

        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            ColumnLayout {
                spacing: 2
                Text {
                    text: "Antumbra on LEZ"
                    color: root.fg
                    font.pixelSize: 21
                    font.weight: Font.DemiBold
                }
                Text {
                    text: "live state of three deployed programs, read from the sequencer"
                    color: root.muted
                    font.pixelSize: 12
                }
            }
            Item { Layout.fillWidth: true }
            Button {
                text: "Refresh"
                onClicked: bridge.refresh()
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: root.line }

        // ---- bonding curve, RFP-015 ----
        Card {
            id: saleCard
            title: "Bonding curve — RFP-015"
            subtitle: "k = Vt · Vc is 1e45 here, thirteen orders past u128::MAX, and is never materialised"
        }

        // ---- LBP, RFP-016 ----
        Card {
            id: poolCard
            title: "Weighted pool — RFP-016"
            subtitle: "the account stores the schedule, never a current weight"
        }

        // ---- vesting, RFP-017 ----
        Card {
            id: schedCard
            title: "Vesting schedule — RFP-017"
            subtitle: "accrual recomputed from the terms on every claim"
        }

        Item { Layout.fillHeight: true }

        Text {
            id: status
            Layout.fillWidth: true
            text: "press Refresh"
            color: root.muted
            font.pixelSize: 11
            elide: Text.ElideRight
        }
    }

    component Card: Rectangle {
        property string title
        property string subtitle
        property var rows: []

        Layout.fillWidth: true
        implicitHeight: col.implicitHeight + 24
        color: root.panel
        border.color: root.line
        border.width: 1
        radius: 8

        ColumnLayout {
            id: col
            anchors.fill: parent
            anchors.margins: 12
            spacing: 6

            Text {
                text: parent.parent.title
                color: root.accent
                font.pixelSize: 13
                font.weight: Font.DemiBold
            }
            Text {
                Layout.fillWidth: true
                text: parent.parent.subtitle
                color: root.muted
                font.pixelSize: 11
                wrapMode: Text.WordWrap
            }
            Repeater {
                model: parent.parent.rows
                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: modelData.k
                        color: root.muted
                        font.pixelSize: 12
                        Layout.preferredWidth: 190
                    }
                    Text {
                        text: modelData.v
                        color: root.fg
                        font.pixelSize: 12
                        font.family: "Menlo, monospace"
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                    }
                }
            }
        }
    }

    Connections {
        target: bridge
        function onSaleUpdated(vt, vc, sr, rc, seed, accrued) {
            saleCard.rows = [
                { k: "virtual token reserve",  v: root.human(vt) },
                { k: "virtual collateral",     v: root.human(vc) },
                { k: "sale reserve left",      v: root.human(sr) },
                { k: "collateral raised",      v: root.human(rc) },
                { k: "DEX seed reserve",       v: root.human(seed) + "  (untouched until close)" },
                { k: "fee accrued, unswept",   v: root.human(accrued) },
            ]
        }
        function onEscrowUpdated(which, balance) {
            var card = which === "sale" ? saleCard : schedCard
            var rows = card.rows.slice()
            rows.push({ k: "escrowed on chain", v: balance + "  (native balance actually held)" })
            card.rows = rows
        }
        function onPoolUpdated(rt, rc, ws, we, last) {
            poolCard.rows = [
                { k: "token reserve",          v: root.human(rt) },
                { k: "collateral reserve",     v: root.human(rc) },
                { k: "weight schedule",        v: root.pct(ws) + "  →  " + root.pct(we) },
                { k: "newest timestamp seen",  v: last },
            ]
        }
        function onScheduleUpdated(total, claimed, last, kind) {
            schedCard.rows = [
                { k: "schedule type",          v: kind },
                { k: "total",                  v: root.human(total) },
                { k: "claimed",                v: root.human(claimed) },
                { k: "newest timestamp seen",  v: last },
            ]
        }
        function onStatusChanged(t) { status.text = t }
        function onFailed(which, why) { status.text = which + ": " + why }
    }

    Component.onCompleted: bridge.refresh()
}
