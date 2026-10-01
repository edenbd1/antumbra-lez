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
        var shown = frac.length ? whole + "." + frac.slice(0, 6) : whole
        // Six decimals of an eighteen-decimal amount round a small balance to
        // "0.000000", which a reader cannot tell from nothing at all. A total of
        // 2 base units rendered as zero is a false statement about the chain, so
        // an amount that is not zero never renders as zero: below the display
        // precision it is shown in base units instead.
        if (Number(shown) === 0 && !/^0*$/.test(s)) {
            return String(raw) + (String(raw) === "1" ? " base unit" : " base units")
        }
        return shown
    }

    // Weights are stored at 1e18 as a fraction of unity.
    function pct(raw) {
        if (!raw) return "—"
        var v = Number(String(raw)) / 1e18
        return (v * 100).toFixed(2) + "%"
    }

    // Which program the panel is showing. Vesting by default: it is the one
    // deployed on the current testnet. The launchpad views read on request.
    property string only: "vesting"
    // "" while reading or showing state; otherwise why there is nothing to read.
    property string notDeployed: ""
    property bool showSettings: false
    property string rpcShown: bridge.endpoint()
    property string programShown: bridge.program()
    // The two vesting reads answer in either order; the escrow line is kept
    // so the schedule's answer does not overwrite it.
    property string schedEscrow: ""
    onOnlyChanged: root.only === "vesting" ? bridge.refresh() : bridge.refreshLaunchpad()

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
                    text: root.only === "all"
                          ? "live state of three deployed programs, read from the sequencer"
                          : "live state, read from the sequencer"
                    color: root.muted
                    font.pixelSize: 12
                }
            }
            Item { Layout.fillWidth: true }
            Row {
                spacing: 6
                Repeater {
                    model: ["all", "curve", "pool", "vesting"]
                    Button {
                        text: modelData
                        checkable: true
                        checked: root.only === modelData
                        onClicked: root.only = modelData
                    }
                }
            }
            Button {
                text: "Refresh"
                onClicked: root.only === "vesting" ? bridge.refresh() : bridge.refreshLaunchpad()
            }
            Button {
                text: "Settings"
                checkable: true
                checked: root.showSettings
                onClicked: root.showSettings = !root.showSettings
            }
        }

        Text {
            Layout.fillWidth: true
            text: "sequencer  " + root.rpcShown + (root.programShown ? "    program  " + root.programShown : "    program  (none)")
            color: root.muted
            font.pixelSize: 11
            font.family: "Menlo, monospace"
            elide: Text.ElideMiddle
        }

        // ---- settings: which sequencer, which program, which schedule ----
        Rectangle {
            id: settingsCard
            visible: root.showSettings
            Layout.fillWidth: true
            implicitHeight: settingsCol.implicitHeight + 24
            color: root.panel
            border.color: root.line
            radius: 8

            ColumnLayout {
                id: settingsCol
                anchors.fill: parent
                anchors.margins: 12
                spacing: 8
                Text {
                    text: "Settings"
                    color: root.accent
                    font.pixelSize: 13
                    font.weight: Font.DemiBold
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: root.muted
                    font.pixelSize: 11
                    text: "Read-only: these say where to read, nothing here can sign. "
                          + "`antumbra-vesting ids --schedule-id <id>` prints a schedule's two accounts. "
                          + "Copy a value, then Paste: Basecamp 0.3.0 does not pass typing to this panel."
                          + (bridge.envOverrides().length
                             ? "  Set in the environment, and winning over a saved value: " + bridge.envOverrides().join(", ")
                             : "")
                }
                Repeater {
                    id: fields
                    model: [
                        { k: "Sequencer RPC",    v: bridge.endpoint(), hint: bridge.defaultEndpoint() },
                        { k: "Program id",       v: bridge.program(),  hint: "empty until the v0.3 program is deployed" },
                        { k: "Schedule account", v: bridge.schedule(), hint: "base58 account id" },
                        { k: "Holding account",  v: bridge.holding(),  hint: "base58 account id" },
                    ]
                    RowLayout {
                        property alias value: field.text
                        Layout.fillWidth: true
                        Text {
                            text: modelData.k
                            color: root.muted
                            font.pixelSize: 12
                            Layout.preferredWidth: 130
                        }
                        TextField {
                            id: field
                            Layout.fillWidth: true
                            text: modelData.v
                            placeholderText: modelData.hint
                            font.pixelSize: 12
                            font.family: "Menlo, monospace"
                            selectByMouse: true
                        }
                        Button {
                            text: "Paste"
                            onClicked: field.text = bridge.clipboardText()
                        }
                        Button {
                            text: "Clear"
                            onClicked: field.text = ""
                        }
                    }
                }
                RowLayout {
                    spacing: 8
                    Button {
                        text: "Save and read"
                        onClicked: {
                            bridge.saveSettings(fields.itemAt(0).value, fields.itemAt(1).value,
                                                fields.itemAt(2).value, fields.itemAt(3).value)
                            root.rpcShown = bridge.endpoint()
                            root.programShown = bridge.program()
                            root.only = "vesting"
                            bridge.refresh()
                        }
                    }
                    Button {
                        text: "Public testnet"
                        onClicked: fields.itemAt(0).value = bridge.defaultEndpoint()
                    }
                    Item { Layout.fillWidth: true }
                    Text {
                        text: bridge.settingsFile()
                        color: root.muted
                        font.pixelSize: 10
                        elide: Text.ElideLeft
                        Layout.maximumWidth: 320
                    }
                }
            }
        }

        // ---- not deployed yet: a state, not an error ----
        Rectangle {
            visible: root.notDeployed !== "" && root.only === "vesting"
            Layout.fillWidth: true
            implicitHeight: ndCol.implicitHeight + 28
            color: "#1d1a10"
            border.color: "#6b5a1e"
            radius: 8
            ColumnLayout {
                id: ndCol
                anchors.fill: parent
                anchors.margins: 14
                spacing: 6
                Text {
                    text: "Not deployed yet"
                    color: "#e8c766"
                    font.pixelSize: 15
                    font.weight: Font.DemiBold
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: root.notDeployed
                    color: root.fg
                    font.pixelSize: 12
                }
                Button {
                    text: "Open settings"
                    visible: !root.showSettings
                    onClicked: root.showSettings = true
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: root.line }

        // ---- bonding curve, RFP-015 ----
        Card {
            id: saleCard
            visible: root.only === "all" || root.only === "curve"
            title: "Bonding curve — RFP-015"
            subtitle: "k = Vt · Vc is 1e45 here, thirteen orders past u128::MAX, and is never materialised"
        }

        // ---- LBP, RFP-016 ----
        Card {
            id: poolCard
            visible: root.only === "all" || root.only === "pool"
            title: "Weighted pool — RFP-016"
            subtitle: "the account stores the schedule, never a current weight"
        }

        // ---- vesting, RFP-017 ----
        Card {
            id: schedCard
            visible: (root.only === "all" || root.only === "vesting") && root.notDeployed === ""
            title: "Vesting position — RFP-017"
            subtitle: "claimable now is computed against the chain's own clock, as a claim would be"
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
            if (which === "schedule") root.schedEscrow = balance
            var card = which === "sale" ? saleCard : schedCard
            if (card.rows.length === 0) return
            var rows = card.rows.filter(function (r) { return r.k !== "escrowed on chain" })
            rows.push({ k: "escrowed on chain", v: balance })
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
        // Base units, not 18-decimal amounts: a vesting total is whatever the
        // token's own convention is, and the program never scales it.
        function onNotDeployed(why) {
            root.notDeployed = why
            schedCard.rows = []
            status.text = "not deployed"
        }
        function onScheduleUpdated(s) {
            root.notDeployed = ""
            schedCard.rows = [
                { k: "schedule type",          v: s.kind },
                { k: "asset",                  v: s.asset },
                { k: "total locked",           v: s.total },
                { k: "vested so far",          v: s.vested },
                { k: "claimed so far",         v: s.claimed },
                { k: "claimable now",          v: s.claimable },
                { k: "next unlock",            v: s.next },
                { k: "cancelable",             v: s.cancelable },
                { k: "computed at",            v: s.clock + "  (the LEZ clock account)" },
            ].concat(root.schedEscrow ? [{ k: "escrowed on chain", v: root.schedEscrow }] : [])
        }
        function onStatusChanged(t) { status.text = t }
        function onFailed(which, why) { status.text = which + ": " + why }
    }

    Component.onCompleted: bridge.refresh()
}
