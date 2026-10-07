import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Antumbra Vesting. The view holds no chain state of its own: it asks the
// backend, which reads the LEZ chain over HTTPS, and shows the answer. It
// holds no keys and signs nothing; to claim, it gives the exact command.
//
// The look is Logos Forum's, control for control; only the accent differs.
Item {
    id: root

    readonly property var backend: logos.module("antumbra_lez")
    property bool ready: false

    readonly property string appVersion:   backend ? backend.appVersion : ""
    readonly property string rpcUrl:       backend ? backend.rpc : ""
    readonly property string programId:    backend ? backend.program : ""
    readonly property string explorerUrl:  backend ? backend.explorer : ""
    readonly property string envOverrides: backend ? backend.envOverrides : ""
    readonly property var defaults: parse(backend ? backend.defaultsJson : "{}", {})
    readonly property var status: parse(backend ? backend.statusJson : "{}", {})

    // ── Palette: the Forum's, accent included ───────────────────────────────
    readonly property color bg: "#101114"
    readonly property color panel: "#17191e"
    readonly property color raised: "#20242b"
    readonly property color line: "#2a2f37"
    readonly property color text: "#e7e9ee"
    readonly property color dim: "#8b93a1"
    readonly property color accent: "#f5925e"        // Logos orange, light: text, markers, the curve
    readonly property color accentStrong: "#e2552b"  // Logos orange: buttons, badges, bars
    readonly property color focusRing: "#f7b58a"     // light orange: the field being typed in
    readonly property color ok: "#4cc38a"
    readonly property color warn: "#f2c14e"
    readonly property color bad: "#e5484d"
    property bool claimPrivate: false
    property string lastQuery: ""
    readonly property color link: "#6cb4ff"
    readonly property var rim: ["#ee5a2c", "#f07a42", "#f29a58", "#f3c46c"]

    palette.window: panel
    palette.windowText: text
    palette.base: bg
    palette.alternateBase: panel
    palette.text: text
    palette.button: raised
    palette.buttonText: text
    palette.brightText: "#ffffff"
    palette.highlight: accentStrong
    palette.highlightedText: "#ffffff"
    palette.placeholderText: dim
    palette.mid: line
    palette.dark: accent
    palette.light: raised
    palette.midlight: line
    palette.shadow: "#000000"
    palette.toolTipBase: raised
    palette.toolTipText: text
    palette.disabled.buttonText: "#5d646f"
    palette.disabled.button: "#191c21"
    palette.disabled.text: "#5d646f"

    // Breakpoints, as in the Forum. Below `compact` one pane at a time: the
    // list, or the open schedule with a way back. Below `narrow` the header
    // stacks.
    readonly property bool compact: width < 720
    readonly property bool narrow: width < 960
    readonly property bool phone: width < 480
    readonly property int gap: compact ? 10 : 16

    // ── State ────────────────────────────────────────────────────────────────
    property var examples: []
    property string examplesState: "loading"    // loading | ready | error
    property string examplesError: ""
    // What the list pane shows: the examples, or the answer to a lookup.
    property var result: null                    // {kind: "list", title, subtitle, note, items} or null
    property string listMessage: ""              // a lookup's "none" or "error" answer
    property string listMessageTone: "dim"
    property bool looking: false
    property string selectedAccount: ""
    property var detail: null                    // the open schedule
    property string detailState: "idle"          // idle | loading | ready | error
    property string detailError: ""
    property var opened: ({})                    // what the open schedule was opened with: {account, scheduleId, batchId}
    property var activity: null
    property string activityState: "idle"        // idle | loading | ready
    property string lastError: ""

    function parse(s, fallback) { try { return JSON.parse(s) } catch (e) { return fallback } }
    function log(m) { console.log("[antumbra qml] " + m) }

    // Slots answer through `answered(token, json)`; each request names its own
    // token, so an answer to a question the view has moved on from is dropped.
    property int nextToken: 1
    property var waiting: ({})
    function ask(slotCall, handler) {
        var token = "t" + (nextToken++)
        var w = waiting; w[token] = handler; waiting = w
        logos.watch(slotCall(token), function (r) {
            if (r && String(r).indexOf("error: ") === 0) { forget(token); handler({ kind: "error", text: String(r).substring(7) }) }
        }, function (err) { forget(token); handler({ kind: "error", text: "The Antumbra backend did not answer: " + err }) })
        return token
    }
    function forget(token) { var w = waiting; delete w[token]; waiting = w }
    function call(pending, then) {
        logos.watch(pending, function (r) { if (then) then(r) },
                    function (err) { root.lastError = "The Antumbra backend did not answer: " + err })
    }

    function loadExamples() {
        if (!ready) return
        examplesState = "loading"
        ask(function (t) { return backend.examples(t) }, function (o) {
            if (o.kind === "list") { examples = o.items || []; examplesState = examples.length ? "ready" : "error"
                                     examplesError = examples.length ? "" : "None of the example schedules was found on this chain." }
            else { examplesState = "error"; examplesError = o.text || "The examples could not be read." }
        })
    }
    function lookup(q) {
        q = String(q).trim()
        if (q === "" || !ready) return
        looking = true; listMessage = ""; lastQuery = q
        ask(function (t) { return backend.lookup(t, q) }, function (o) {
            looking = false
            if (o.kind === "schedule") { result = null; showDetail(o.schedule, { account: o.schedule.account }) }
            else if (o.kind === "list") { result = o; listMessage = "" }
            else { listMessage = o.text || "Nothing found."; listMessageTone = o.kind === "error" ? "bad" : "dim" }
        })
    }
    function open(item) {
        if (!ready || !item) return
        if (item.isBatch) { lookupField.text = item.scheduleId; lookup(item.scheduleId); return }
        var with_ = { account: item.account, scheduleId: item.scheduleId || "", batchId: item.batchId || "" }
        opened = with_; selectedAccount = item.account
        detail = null; detailState = "loading"; activity = null; activityState = "idle"
        ask(function (t) { return backend.openSchedule(t, item.account, item.batchId || "") }, function (o) {
            if (o.kind === "schedule") showDetail(o.schedule, with_)
            else { detailState = "error"; detailError = o.text || "The schedule could not be read." }
        })
    }
    function showDetail(s, with_) {
        opened = with_ || { account: s.account }
        if (!opened.scheduleId && s.scheduleId) { var w = opened; w.scheduleId = s.scheduleId; opened = w }
        selectedAccount = s.account
        s.receivedAt = Date.now()
        var fresh = !detail || detail.account !== s.account
        detail = s; detailState = "ready"
        if (fresh) detailScroll.contentItem.contentY = 0   // a new schedule opens at its top
        activity = null; activityState = "loading"
        ask(function (t) { return backend.activity(t, s.account) }, function (o) {
            if (!root.detail || root.detail.account !== s.account) return
            activity = o; activityState = "ready"
            var w2 = opened
            if (!w2.scheduleId && o.scheduleId) w2.scheduleId = o.scheduleId
            if (!w2.batchId && o.batchId) w2.batchId = o.batchId
            opened = w2
        })
    }
    // Read the open schedule again in place: on ↻ (with its activity), and
    // quietly every minute.
    function refreshDetail(withActivity) {
        if (!ready || !detail || detailState !== "ready") return
        var acc = detail.account, with_ = opened
        ask(function (t) { return backend.openSchedule(t, acc, with_.batchId || "") }, function (o) {
            if (o.kind !== "schedule" || !root.detail || root.detail.account !== acc) return
            var s = o.schedule; s.receivedAt = Date.now(); root.detail = s
            if (withActivity) {
                root.activityState = "loading"
                root.ask(function (t) { return root.backend.activity(t, acc) }, function (a) {
                    if (root.detail && root.detail.account === acc) { root.activity = a; root.activityState = "ready" } })
            }
        })
    }
    property double nowLocal: Date.now()
    Timer { interval: 5000; repeat: true; running: true; onTriggered: root.nowLocal = Date.now() }
    Timer { interval: 60000; repeat: true; running: root.detailState === "ready"; onTriggered: root.refreshDetail(false) }
    function reloadDetail() { if (opened.account) open({ account: opened.account, scheduleId: opened.scheduleId, batchId: opened.batchId }) }
    function titleOf(s) {
        if (opened.scheduleId) return opened.scheduleId.length === 64 ? shortKey(opened.scheduleId) : opened.scheduleId
        return "Schedule " + shortKey(s ? s.account : "")
    }
    function dayMonth(ms) { if (!ms) return ""; var d = new Date(ms); return d.getUTCDate() + " " + months[d.getUTCMonth()] }
    function curveMeta(s) {
        if (!s) return ""
        if (s.kind === 2) return s.signalledCount + " of " + s.tranches.length + " signalled, " + fmt(s.perTranche) + " " + s.unit + " each"
        var mins = Math.round((s.end - s.start) / 60000)
        var span = mins >= 2880 ? Math.round(mins / 1440) + " days" : mins >= 120 ? Math.round(mins / 60) + " hours" : mins + " min"
        return s.kindLabel.toLowerCase() + " over " + span + ", " + utcShort(s.start) + " to " + utcShort(s.end) + " UTC"
    }
    function claimsOf(a) {
        var out = []
        if (!a || !a.events) return out
        for (var i = 0; i < a.events.length; i++) {
            var e = a.events[i]
            if (e.ok && e.at > 0 && /^Claimed/.test(e.what)) out.push({ at: e.at, label: e.detail.split(" ")[0] })
        }
        return out
    }
    readonly property string networkLabel: (rpcUrl === defaults.rpc ? "testnet" : rpcUrl.replace(/^https?:\/\//, "")) + " · " + shortKey(programId)
    function closeDetail() { detail = null; detailState = "idle"; selectedAccount = ""; activity = null }

    // ── Formatting ───────────────────────────────────────────────────────────
    // Amounts arrive as decimal strings (they can pass 2^53): grouped by hand.
    function fmt(d) {
        var s = String(d === undefined || d === null ? "0" : d)
        if (!/^[0-9]+$/.test(s)) return s
        return s.replace(/\B(?=(\d{3})+(?!\d))/g, ",")
    }
    function isZero(d) { return String(d) === "0" || String(d) === "" }
    function shortKey(k) { k = String(k || ""); return k.length > 10 ? k.substring(0, 4) + "…" + k.substring(k.length - 4) : k }
    function pad(n) { return (n < 10 ? "0" : "") + n }
    readonly property var months: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    // Times are the chain's, in UTC: the same everywhere, and what the CLI prints.
    function utc(ms) { var d = new Date(ms); return d.getUTCDate() + " " + months[d.getUTCMonth()] + " " + d.getUTCFullYear() + ", " + pad(d.getUTCHours()) + ":" + pad(d.getUTCMinutes()) + " UTC" }
    function utcShort(ms) { var d = new Date(ms); return d.getUTCDate() + " " + months[d.getUTCMonth()] + ", " + pad(d.getUTCHours()) + ":" + pad(d.getUTCMinutes()) }
    function hm(ms) { var d = new Date(ms); return pad(d.getUTCHours()) + ":" + pad(d.getUTCMinutes()) }
    // "in 3 days", "4 days ago", "in 5 min", against the chain's clock.
    function rel(ms, now) {
        var s = (ms - now) / 1000, a = Math.abs(s), past = s < 0, v
        if (a < 60) return past ? "just now" : "in under a minute"
        if (a < 3600) v = Math.round(a / 60) + " min"
        else if (a < 172800) v = Math.round(a / 3600) + (Math.round(a / 3600) === 1 ? " hour" : " hours")
        else v = Math.round(a / 86400) + " days"
        return past ? v + " ago" : "in " + v
    }
    function tone(n) { return n === "ok" ? ok : n === "bad" ? bad : n === "warn" ? warn : n === "accent" ? accent : dim }
    // The chain's clock now: the latest reading, the status line's or the schedule's.
    readonly property double chainNow: Math.max(status.clock || 0, detail ? detail.now : 0)

    function explorerAccount(id) { return explorerUrl + "/account/" + id }
    function explorerTx(hash) { return explorerUrl + "/transaction/" + hash }
    function openLink(url) {
        if (!/^https:\/\/[^\/?#&.\s]/.test(url)) { lastError = "Only https links can be opened, and " + url + " is not one."; return }
        call(backend.openLink(url), function (r) { if (r !== "") lastError = "The link was not opened: " + String(r).replace(/^error: /, "") })
    }
    // Basecamp's clipboard, through a hidden text edit: the view can reach it,
    // the backend runs in another process.
    TextEdit { id: clip; visible: false; textFormat: TextEdit.PlainText }
    function copy(t) { clip.text = t; clip.selectAll(); clip.copy(); clip.deselect() }

    // The command that claims this schedule, from the beneficiary's wallet.
    function claimCommand(s) {
        if (!s || !opened.scheduleId) return ""
        var c = "antumbra-vesting claim \\\n  --program " + programId + " \\\n  --schedule-id " + opened.scheduleId
        if (opened.batchId) c += " \\\n  --batch-id " + opened.batchId
        c += " \\\n  --beneficiary " + s.beneficiary + " \\\n  --to " + (claimPrivate ? "Private/<your shielded account>" : "Public/" + s.beneficiary)
        return c
    }

    Connections {
        target: logos
        function onViewModuleReadyChanged(name, isReady) {
            if (name === "antumbra_lez") root.ready = isReady && root.backend !== null
        }
    }
    Connections {
        target: root.backend
        ignoreUnknownSignals: true
        function onAnswered(token, json) {
            var h = root.waiting[token]
            if (!h) return
            root.forget(token)
            h(root.parse(json, { kind: "error", text: "The backend's answer could not be read." }))
        }
    }
    onReadyChanged: if (ready) { loadExamples(); call(backend.refreshStatus()) }
    // The chain's head and clock, every 15 s: the status line and the "now" marker.
    Timer { interval: 15000; repeat: true; running: root.ready; onTriggered: root.call(root.backend.refreshStatus()) }
    Component.onCompleted: ready = backend !== null && logos.isViewModuleReady("antumbra_lez")

    Rectangle { anchors.fill: parent; color: root.bg }

    // ── The Forum's pieces ───────────────────────────────────────────────────
    // Plain text by default: ids and amounts come from the chain.
    component Label2: Text { color: root.text; font.pixelSize: 14; wrapMode: Text.WrapAtWordBoundaryOrAnywhere; textFormat: Text.PlainText }
    component Dim: Text { color: root.dim; font.pixelSize: 12; wrapMode: Text.WrapAtWordBoundaryOrAnywhere; textFormat: Text.PlainText }
    readonly property string monoFamily: Qt.platform.os === "osx" ? "Menlo" : Qt.platform.os === "windows" ? "Consolas" : "DejaVu Sans Mono"
    component Mono: Text { color: root.text; font.family: root.monoFamily; font.pixelSize: 12; textFormat: Text.PlainText }
    component Section: Text { color: root.text; font.pixelSize: 15; font.bold: true; textFormat: Text.PlainText }
    component Rule: Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: root.line }

    // The main action: a dark button outlined in the accent's gradient.
    component AccentButton: Button {
        id: ab
        HoverHandler { cursorShape: ab.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor }
        contentItem: Text {
            text: ab.text; font: ab.font
            color: ab.enabled ? "#ffffff" : root.dim
            horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            implicitWidth: 100; implicitHeight: 40
            radius: 7
            color: root.line
            opacity: ab.down ? 0.8 : 1.0
            gradient: ab.enabled ? rimGradient : null
            Gradient {
                id: rimGradient
                orientation: Gradient.Horizontal
                GradientStop { position: 0.0; color: root.rim[0] }
                GradientStop { position: 0.1; color: root.rim[1] }
                GradientStop { position: 0.55; color: root.rim[2] }
                GradientStop { position: 1.0; color: root.rim[3] }
            }
            Rectangle { anchors.fill: parent; anchors.margins: 2; radius: 5; color: root.panel }
        }
    }
    component AppButton: Button {
        id: nb
        HoverHandler { cursorShape: nb.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor }
        contentItem: Text {
            text: nb.text; font: nb.font; color: nb.enabled ? root.text : root.dim
            horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            implicitWidth: 80; implicitHeight: 40; radius: 6
            color: nb.down ? root.line : root.raised
            border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.06)
        }
    }
    component SmallButton: AppButton {
        font.pixelSize: 12
        background: Rectangle {
            implicitWidth: 54; implicitHeight: 28; radius: 6
            color: parent.down ? root.line : root.raised
            border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.06)
        }
        leftPadding: 10; rightPadding: 10
    }
    component AppField: TextField {
        id: tf
        color: root.text; placeholderTextColor: root.dim
        selectByMouse: true
        background: Rectangle { implicitHeight: 40; radius: 6; color: root.bg; border.color: tf.activeFocus ? root.focusRing : root.line }
    }
    component AppCombo: ComboBox {
        id: cbx
        HoverHandler { cursorShape: cbx.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor }
        background: Rectangle { implicitWidth: 120; implicitHeight: 40; radius: 6; color: cbx.down ? root.line : root.raised; border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.06) }
        popup.palette: root.palette
    }
    // Status chips: the shape of the Forum's "1 new" badge, tinted.
    component Chip: Rectangle {
        property string label
        property color tint: root.dim
        property bool solid: false
        implicitWidth: ct.implicitWidth + 14; implicitHeight: 20; radius: 8
        color: solid ? root.accentStrong : Qt.rgba(tint.r, tint.g, tint.b, 0.14)
        Text { id: ct; anchors.centerIn: parent; text: parent.label; textFormat: Text.PlainText
               color: parent.solid ? "white" : parent.tint; font.pixelSize: 11; font.bold: true }
    }
    component Pane: Rectangle { color: root.panel; radius: 6; border.color: root.line }
    // A time, relative to the chain's clock, with the exact one on hover.
    component When: Dim {
        property double ms: 0
        property string prefix: ""
        text: prefix + root.rel(ms, root.chainNow)
        HoverHandler { id: wh }
        ToolTip.visible: wh.hovered; ToolTip.delay: 300; ToolTip.text: root.utc(ms)
    }
    // Claimed | claimable | still locked, as one bar.
    component Split: Item {
        property real total: 1
        property real vested: 0
        property real claimed: 0
        implicitHeight: 8
        Rectangle { anchors.fill: parent; radius: 4; color: root.line }
        Rectangle { width: parent.width * Math.min(1, parent.vested / Math.max(1, parent.total)); height: parent.height; radius: 4; color: root.accentStrong }
        Rectangle { width: parent.width * Math.min(1, parent.claimed / Math.max(1, parent.total)); height: parent.height; radius: 4; color: root.text; opacity: 0.85 }
    }
    component Busy: RowLayout {
        property string label
        spacing: 8
        BusyIndicator { running: true; implicitWidth: 18; implicitHeight: 18; palette.dark: root.accent }
        Dim { text: parent.label; font.pixelSize: 13; Layout.fillWidth: true }
    }

    // The vesting curve, drawn from the schedule and the chain's clock.
    component Curve: Canvas {
        id: cv
        property var s
        property var claims: []
        property double now: 0
        property bool small: false
        onSChanged: requestPaint()
        onClaimsChanged: requestPaint()
        onNowChanged: requestPaint()
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onPaint: {
            var c = getContext("2d"); c.reset()
            if (!s) return
            var L = small ? 44 : 56, R = small ? 10 : 18, T0 = 14, B = 44
            var w = width - L - R, h = height - T0 - B
            if (w < 40 || h < 40) return
            var F = "px sans-serif"
            var total = Math.max(1, s.totalF)
            c.font = "11" + F
            function amount(v) { return root.fmt(String(Math.round(v))) }
            if (s.kind === 2) {
                var n = s.tranches.length, gp = small ? 8 : 14, bw = (w - gp * (n - 1)) / n
                var per = total / n, claimedLeft = s.claimedF
                var Yt = function (v) { return T0 + h - v / per * h }
                c.strokeStyle = root.line; c.fillStyle = root.dim; c.textAlign = "right"
                for (var gi = 0; gi <= 1; gi++) { var gy = Math.round(Yt(per * gi)) + 0.5
                    c.beginPath(); c.moveTo(L, gy); c.lineTo(L + w, gy); c.stroke(); c.fillText(amount(per * gi), L - 8, gy + 4) }
                for (var k = 0; k < n; k++) {
                    var x = L + k * (bw + gp), lit = s.tranches[k], top = Yt(per), base = Yt(0)
                    if (lit) {
                        c.fillStyle = root.accentStrong; c.beginPath(); c.roundedRect(x, top, bw, base - top, 4, 4); c.fill()
                        if (claimedLeft >= per - 0.5) { c.fillStyle = Qt.rgba(0.906, 0.914, 0.933, 0.85); c.fillRect(x, top, bw, 4); claimedLeft -= per }
                    } else {
                        c.save(); c.beginPath(); c.rect(x, top, bw, base - top); c.clip(); c.strokeStyle = root.line; c.lineWidth = 1.2
                        for (var hx = -h; hx < bw + h; hx += 7) { c.beginPath(); c.moveTo(x + hx, base); c.lineTo(x + hx + (base - top), top); c.stroke() }
                        c.restore(); c.strokeStyle = root.line; c.lineWidth = 1; c.strokeRect(x + 0.5, top + 0.5, bw - 1, base - top - 1)
                    }
                    if (n <= 12) {
                        c.textAlign = "center"; c.font = "bold 12" + F; c.fillStyle = lit ? root.text : root.dim
                        c.fillText((small || n > 6 ? "No. " : "Milestone ") + (k + 1), x + bw / 2, base + 18)
                        c.font = "11" + F; c.fillStyle = lit ? root.accent : root.dim
                        c.fillText(lit ? "signalled" : "waiting", x + bw / 2, base + 33)
                    }
                }
                return
            }
            var t0 = s.start, t1 = Math.max(s.end, s.start + 1), span = t1 - t0
            var cut = s.cancelledAt > 0 ? Math.min(s.cancelledAt, t1) : 0
            // The scale runs from the start (or now, if earlier) to the end. A
            // now far past the end is pinned to the right edge, the axis broken.
            var pinned = now > t1 + span * 0.25
            var lo = Math.min(t0, now > 0 ? now : t0), hi = pinned ? t1 : Math.max(t1, now)
            var x0 = lo - (hi - lo) * 0.04, x1 = hi + (hi - lo) * (pinned ? 0.16 : 0.04)
            function X(ms) { return L + (ms - x0) / (x1 - x0) * w }
            function Y(v) { return T0 + h - v / total * h }
            function vested(ms) {
                var u = cut ? Math.min(ms, cut) : ms
                if (s.kind === 0 && u < s.cliff) return 0
                if (u <= t0) return 0
                if (u >= t1) return total
                return total * (u - t0) / span
            }
            // Corner points of the curve, so it is exact without sampling.
            var pts = [[x0, 0], [t0, 0]]
            if (s.kind === 0 && s.cliff > t0) { pts.push([s.cliff, 0]); pts.push([s.cliff, vested(s.cliff)]) }
            if (cut && cut < t1) { pts.push([cut, vested(cut)]); pts.push([x1, vested(cut)]) }
            else { pts.push([t1, total]); pts.push([x1, total]) }
            function curve() { c.moveTo(X(pts[0][0]), Y(pts[0][1])); for (var i = 1; i < pts.length; i++) c.lineTo(X(pts[i][0]), Y(pts[i][1])) }
            c.strokeStyle = root.line; c.lineWidth = 1; c.fillStyle = root.dim; c.textAlign = "right"
            for (var g2 = 0; g2 <= 2; g2++) { var v = total * g2 / 2, y = Math.round(Y(v)) + 0.5
                c.beginPath(); c.moveTo(L, y); c.lineTo(L + w, y); c.stroke(); c.fillText(amount(v), L - 8, y + 4) }
            // Not vested at that time: hatched, between the curve and the total.
            c.save(); c.beginPath(); curve(); c.lineTo(X(x1), Y(total)); c.lineTo(X(x0), Y(total)); c.closePath(); c.clip()
            c.strokeStyle = root.line; for (var hx2 = L - h; hx2 < L + w; hx2 += 7) { c.beginPath(); c.moveTo(hx2, Y(0)); c.lineTo(hx2 + h, Y(total)); c.stroke() }
            c.restore()
            c.beginPath(); curve(); c.lineTo(X(x1), Y(0)); c.closePath(); c.fillStyle = Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.16); c.fill()
            // What has been claimed, under the curve.
            c.save(); c.beginPath(); c.rect(L, Y(s.claimedF), w, Y(0) - Y(s.claimedF)); c.clip()
            c.beginPath(); curve(); c.lineTo(X(x1), Y(0)); c.closePath(); c.fillStyle = Qt.rgba(0.906, 0.914, 0.933, 0.30); c.fill(); c.restore()
            c.beginPath(); c.strokeStyle = root.accent; c.lineWidth = 2; c.lineJoin = "round"; curve(); c.stroke()
            function vline(ms, col) { var x = Math.round(X(ms)) + 0.5; c.strokeStyle = col; c.setLineDash([3, 4]); c.lineWidth = 1
                c.beginPath(); c.moveTo(x, T0); c.lineTo(x, Y(0) + 5); c.stroke(); c.setLineDash([]) }
            var labels = []
            function xl(ms, a, b, col) {
                var x = X(ms), al = "center"
                for (var i = 0; i < labels.length; i++) if (Math.abs(labels[i] - x) < 90) return   // no overlap
                labels.push(x)
                if (x < L + 40) al = "left"; else if (x > L + w - 40) al = "right"
                c.textAlign = al; c.font = "bold 12" + F; c.fillStyle = col || root.text; c.fillText(a, x, Y(0) + 19)
                c.font = "11" + F; c.fillStyle = root.dim; c.fillText(b, x, Y(0) + 34)
            }
            vline(t0, root.dim); xl(t0, "Start", root.utcShort(t0))
            if (s.kind === 0 && s.cliff > t0) { vline(s.cliff, root.warn); xl(s.cliff, "Cliff", root.utcShort(s.cliff), root.warn) }
            if (cut) { vline(cut, root.bad); xl(cut, "Cancelled", root.utcShort(cut), root.bad) }
            else { vline(t1, root.dim); xl(t1, "Fully vested", root.utcShort(t1)) }
            for (var q = 0; q < claims.length; q++) {
                var cl = claims[q]; if (cl.at < x0 || cl.at > x1) continue
                var cx = X(cl.at), cy = Y(vested(cl.at))
                c.beginPath(); c.fillStyle = root.panel; c.arc(cx, cy, 5, 0, Math.PI * 2); c.fill()
                c.beginPath(); c.strokeStyle = root.text; c.lineWidth = 2; c.arc(cx, cy, 5, 0, Math.PI * 2); c.stroke()
                if (claims.length <= 4) {
                    c.font = "bold 12" + F; var lab = "Claimed " + cl.label, lw = c.measureText(lab).width
                    var bx0 = Math.max(L, Math.min(L + w - lw - 16, cx - lw / 2 - 8))
                    c.fillStyle = root.raised; c.strokeStyle = root.line; c.lineWidth = 1
                    c.beginPath(); c.roundedRect(bx0, cy - 34, lw + 16, 22, 5, 5); c.fill(); c.stroke()
                    c.textAlign = "left"; c.fillStyle = root.text; c.fillText(lab, bx0 + 8, cy - 19)
                }
            }
            if (now <= 0) return
            var nx = pinned ? L + w - 1 : X(now)
            c.strokeStyle = root.text; c.lineWidth = 1.5; c.beginPath(); c.moveTo(nx, T0); c.lineTo(nx, Y(0) + 5); c.stroke()
            c.beginPath(); c.fillStyle = root.text; c.arc(nx, Y(vested(now)), 4, 0, Math.PI * 2); c.fill()
            if (pinned) {
                var bx = (X(t1) + nx) / 2; c.strokeStyle = root.dim; c.lineWidth = 1.3
                for (var b = -1; b <= 1; b += 2) { c.beginPath(); c.moveTo(bx + b * 3 - 3, Y(0) + 5); c.lineTo(bx + b * 3 + 3, Y(0) - 5); c.stroke() }
            }
            var right = nx > L + w / 2
            c.textAlign = right ? "right" : "left"; var off = right ? -8 : 8
            c.font = "bold 12" + F; c.fillStyle = root.text; c.fillText("Now", nx + off, T0 + 30)
            c.font = "11" + F; c.fillStyle = root.dim; c.fillText(root.utcShort(now) + " UTC", nx + off, T0 + 45)
        }
    }
    component Legend: Flow {
        spacing: 14
        Repeater { model: [ { c: root.text, l: "Claimed" }, { c: root.accent, l: "Claimable" }, { c: root.line, l: "Not vested yet" } ]
            Row { spacing: 6
                Rectangle { width: 9; height: 9; radius: 2; color: modelData.c; anchors.verticalCenter: parent.verticalCenter }
                Dim { text: modelData.l } } }
    }
    component AccountRow: RowLayout {
        property string role
        property string addr
        spacing: 8
        visible: addr !== ""
        ColumnLayout { Layout.fillWidth: true; Layout.minimumWidth: 0; spacing: 1
            Dim { text: role; Layout.fillWidth: true }
            Mono { text: root.phone ? root.shortKey(addr) : addr; Layout.fillWidth: true; elide: Text.ElideMiddle } }
        SmallButton { id: cb; text: copied ? "Copied" : "Copy"; property bool copied: false
            onClicked: { root.copy(addr); copied = true; copiedTimer.restart() }
            Timer { id: copiedTimer; interval: 1500; onTriggered: cb.copied = false } }
        SmallButton { text: "Explorer ↗"; onClicked: root.openLink(root.explorerAccount(addr))
            ToolTip.visible: hovered; ToolTip.delay: 400; ToolTip.text: root.explorerAccount(addr) }
    }
    // A schedule row, the shape of the Forum's topic rows: the id in bold, a
    // one-line preview, a muted meta line.
    component ScheduleRow: Rectangle {
        property var item
        implicitHeight: col.implicitHeight + 16
        radius: 4
        color: item && !item.isBatch && item.account === root.selectedAccount ? root.line : "transparent"
        Accessible.role: Accessible.Button
        Accessible.name: item ? (item.scheduleId || item.account || "") : ""
        Accessible.onPressAction: root.open(item)
        HoverHandler { cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: root.open(item) }
        ColumnLayout {
            id: col
            anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; anchors.margins: 8
            spacing: 3
            RowLayout { Layout.fillWidth: true; spacing: 6
                Rectangle { visible: item && !root.isZero(item.claimable); width: 7; height: 7; radius: 4; color: root.accent }
                Label2 { Layout.fillWidth: true; font.bold: true; maximumLineCount: 1; elide: Text.ElideRight
                         text: item ? (item.label ? item.label : (item.scheduleId ? (item.scheduleId.length === 64 ? root.shortKey(item.scheduleId) : item.scheduleId) : "Schedule " + root.shortKey(item.account))) : "" } }
            Text { Layout.fillWidth: true; elide: Text.ElideRight; maximumLineCount: 1; color: root.dim; font.pixelSize: 12; textFormat: Text.PlainText
                   text: item ? (item.kindLabel + " · " + root.fmt(item.total) + " " + item.unit + " · "
                                 + (root.isZero(item.claimable) ? "nothing to claim" : root.fmt(item.claimable) + " claimable")) : "" }
            Dim { Layout.fillWidth: true; elide: Text.ElideRight; maximumLineCount: 1
                  color: item && (item.tone === "bad" || item.tone === "warn") ? root.tone(item.tone) : root.dim
                  text: item ? [item.state, item.roles ? "as " + item.roles : (item.note || ""), item.end ? root.dayMonth(item.cancelledAt > 0 ? item.cancelledAt : item.end) : ""]
                                 .filter(function (x) { return x }).join(" · ") : "" }
        }
    }

    // A section of the open schedule, laid out as the Forum lays out a post:
    // an author line (bold accent label, muted meta), then the body.
    component Post: ColumnLayout {
        property string label
        property string meta: ""
        default property alias body: postBody.data
        Layout.fillWidth: true
        spacing: 6
        Flow { Layout.fillWidth: true; spacing: 6
            Text { text: label; color: root.accent; font.pixelSize: 13; font.bold: true; textFormat: Text.PlainText }
            Dim { visible: meta !== ""; text: "· " + meta; wrapMode: Text.NoWrap } }
        ColumnLayout { id: postBody; Layout.fillWidth: true; spacing: 8 }
    }

    // The pieces of a schedule, shared by every arrangement.
    component TitleRow: Flow {
        property var s
        spacing: 8
        Text { id: tt; text: root.titleOf(s); color: root.text; font.pixelSize: 18; font.bold: true; textFormat: Text.PlainText
               HoverHandler { id: th } ToolTip.visible: th.hovered && (root.opened.scheduleId || "").length === 64; ToolTip.text: root.opened.scheduleId || "" }
        Repeater { model: s ? s.chips : []
            Item { width: chipItem.width; height: tt.height; Chip { id: chipItem; anchors.verticalCenter: parent.verticalCenter; label: modelData.text; tint: root.tone(modelData.tone) } } }
        Item { visible: root.opened.batchId ? true : false; width: bchip.width; height: tt.height
               Chip { id: bchip; anchors.verticalCenter: parent.verticalCenter; label: "Batch " + (root.opened.batchId || ""); tint: root.dim } }
    }
    component Byline: Flow {
        property var s
        spacing: 6
        Text { text: s ? root.shortKey(s.account) : ""; color: root.accent; font.pixelSize: 13; font.bold: true; textFormat: Text.PlainText
               HoverHandler { id: bh } ToolTip.visible: bh.hovered; ToolTip.text: s ? "The schedule's account, " + s.account : "" }
        Dim { text: s ? "· created by " + root.shortKey(s.creator) : ""; wrapMode: Text.NoWrap }
        Dim { visible: s && s.start > 0; text: s ? "· from " + root.utcShort(s.start) + " UTC" : ""; wrapMode: Text.NoWrap }
    }
    component Claimable: ColumnLayout {
        property var s
        property bool hero: false
        spacing: 8
        RowLayout { spacing: 8
            Text { text: s ? root.fmt(s.claimable) : ""; color: s && !root.isZero(s.claimable) ? root.accent : root.dim
                   font.pixelSize: root.phone ? 40 : (hero ? 52 : 44); font.bold: true; textFormat: Text.PlainText }
            Text { text: s ? s.unit + " claimable now" : ""; color: root.dim; font.pixelSize: 14; Layout.alignment: Qt.AlignBaseline } }
        Flow { Layout.fillWidth: true; spacing: 22
            Repeater { model: s ? [ { l: "Total", v: s.total }, { l: "Vested", v: s.vested }, { l: "Claimed", v: s.claimed } ] : []
                ColumnLayout { spacing: 0
                    Dim { text: modelData.l }
                    Label2 { text: root.fmt(modelData.v) + " " + s.unit; font.bold: true; wrapMode: Text.NoWrap } } } }
        Split { Layout.fillWidth: true; total: s ? s.totalF : 1; vested: s ? s.vestedF : 0; claimed: s ? s.claimedF : 0 }
        Label2 { Layout.fillWidth: true; text: root.summary(s); color: root.text }
        Text { visible: text !== ""; Layout.fillWidth: true; text: root.nextUnlock(s); color: root.accent; font.pixelSize: 13; wrapMode: Text.WordWrap; textFormat: Text.PlainText }
    }
    component CurveBlock: ColumnLayout {
        property var s
        property var claims: []
        spacing: 6
        Curve { Layout.fillWidth: true; Layout.preferredHeight: root.phone ? 170 : 210; s: parent.s; claims: parent.claims; now: root.chainNow; small: root.phone }
        Legend { visible: s && s.kind !== 2; Layout.fillWidth: true }
    }
    component EscrowBlock: ColumnLayout {
        property var s
        spacing: 6
        RowLayout { spacing: 8; visible: s && s.escrowHeld !== undefined
            Text { text: s ? root.fmt(s.escrowHeld) : ""; color: root.text; font.pixelSize: 15; font.bold: true; textFormat: Text.PlainText }
            Dim { text: s ? s.unit + " held" : ""; font.pixelSize: 13 }
            Chip { label: s ? (s.escrowShared ? "Shared by the batch" : s.escrowCovers ? "Covers what is owed" : "Holds less than owed") : ""
                   tint: s ? (s.escrowShared ? root.dim : s.escrowCovers ? root.ok : root.bad) : root.dim } }
        Text { visible: s && s.escrowError ? true : false; Layout.fillWidth: true; color: root.warn; font.pixelSize: 13; wrapMode: Text.WordWrap; textFormat: Text.PlainText
               text: s && s.escrowError ? "The escrow could not be read: " + s.escrowError : "" }
        Label2 { Layout.fillWidth: true
                 text: s && s.escrowShared
                       ? "One program address holds the escrow of every schedule in the batch; nobody holds a key for it. This schedule still needs " + root.fmt(s.escrowOwed) + " " + s.unit + " of it."
                       : "A program address nobody holds a key for. Only a claim or a cancellation of this schedule can move it." + (s && s.escrowOwed !== undefined ? " It owes " + root.fmt(s.escrowOwed) + " " + s.unit + "." : "") }
    }
    component AccountsBlock: ColumnLayout {
        property var s
        spacing: 10
        AccountRow { Layout.fillWidth: true; role: "Beneficiary"; addr: s ? s.beneficiary : "" }
        AccountRow { Layout.fillWidth: true; addr: s ? s.creator : ""
                     role: s ? "Creator" + (s.cancelAuthority === s.creator && s.cancelable ? ", also the cancel authority" : "")
                                    + (s.kind === 2 && s.milestoneAuthority === s.creator ? ", also the milestone authority" : "") : "" }
        AccountRow { Layout.fillWidth: true; role: "Cancel authority"; addr: s && s.cancelAuthority !== s.creator ? s.cancelAuthority : "" }
        AccountRow { Layout.fillWidth: true; role: "Milestone authority"; addr: s && s.kind === 2 && s.milestoneAuthority !== s.creator ? s.milestoneAuthority : "" }
        AccountRow { Layout.fillWidth: true; role: "Refunds go to"; addr: s ? s.refundTo : "" }
        AccountRow { Layout.fillWidth: true; role: "Escrow"; addr: s ? s.escrow : "" }
        AccountRow { Layout.fillWidth: true; role: "Token"; addr: s && s.tokenDefinition ? s.tokenDefinition : "" }
    }
    component ActivityBlock: ColumnLayout {
        spacing: 6
        Busy { visible: root.activityState === "loading"; label: "Reading the explorer's index…"; Layout.fillWidth: true }
        Repeater { model: root.activity && root.activity.events ? root.activity.events : []
            delegate: ColumnLayout {
                Layout.fillWidth: true; spacing: 1
                Flow { Layout.fillWidth: true; spacing: 6
                    Text { text: modelData.what; color: modelData.ok ? root.text : root.dim; font.pixelSize: 13; font.bold: modelData.ok; textFormat: Text.PlainText }
                    When { visible: modelData.at > 0; ms: modelData.at; prefix: "· "; wrapMode: Text.NoWrap }
                    Text { text: "· " + root.shortKey(modelData.tx) + " ↗"; color: root.link; font.pixelSize: 12; textFormat: Text.PlainText
                           HoverHandler { id: txh; cursorShape: Qt.PointingHandCursor }
                           TapHandler { onTapped: root.openLink(root.explorerTx(modelData.tx)) }
                           ToolTip.visible: txh.hovered; ToolTip.delay: 400; ToolTip.text: root.explorerTx(modelData.tx) } }
                Label2 { Layout.fillWidth: true; text: modelData.detail; color: modelData.ok ? root.text : root.dim; font.pixelSize: 13 }
            } }
        Dim { visible: root.activityState === "ready"; Layout.fillWidth: true
              color: root.activity && root.activity.consistent === false && root.activity.events && root.activity.events.length ? root.warn : root.dim
              text: root.activity ? root.activity.note || "" : "" }
    }
    // How to claim, placed and styled as the Forum's reply box: a bordered
    // area holding the command, then the choice of where to and the button.
    component ClaimComposer: ColumnLayout {
        property var s
        spacing: 8
        Rectangle {
            Layout.fillWidth: true; implicitHeight: Math.max(64, cmd.implicitHeight + 20); radius: 6; color: root.bg; border.color: root.line
            TextEdit { id: cmd; x: 10; y: 10; width: parent.width - 20; readOnly: true; selectByMouse: true; wrapMode: TextEdit.WrapAnywhere
                       textFormat: TextEdit.PlainText; font.family: root.monoFamily; font.pixelSize: 12; selectionColor: root.accentStrong
                       color: root.claimCommand(s) ? root.text : root.dim
                       text: root.claimCommand(s) || (root.activityState === "loading" ? "Finding the schedule's id in its transactions…"
                             : "The schedule's id is not in its transactions on the explorer, so the command cannot be written. Look it up by its id.") }
        }
        RowLayout { Layout.fillWidth: true; spacing: 8
            Dim { visible: !root.phone; text: "Claim into"; wrapMode: Text.NoWrap }
            AppCombo { id: dest; model: root.phone ? ["Public", "Shielded"] : ["a public account", "a shielded account"]; implicitWidth: root.phone ? 116 : 170
                       onActivated: function (i) { root.claimPrivate = i === 1 } }
            Item { Layout.fillWidth: true }
            AccentButton { id: copyCmd; property bool copied: false; text: copied ? "Copied" : "Copy command"; enabled: root.claimCommand(s) !== ""
                           onClicked: { root.copy(root.claimCommand(s).replace(/ \\\n {2}/g, " ")); copied = true; copyTimer.restart() }
                           Timer { id: copyTimer; interval: 1500; onTriggered: copyCmd.copied = false } } }
        Dim { Layout.fillWidth: true
              text: (s && root.isZero(s.claimable) ? root.nothingToClaim(s) + " " : "")
                    + (root.claimPrivate ? "Into a shielded account of your wallet: the schedule records the claim, the account and what it received stay private. Replace <your shielded account> with its id. " : "")
                    + "This app only reads the chain and signs nothing. Run the command with the v0.3 wallet that holds the beneficiary's key." }
    }

    // ── Layout ───────────────────────────────────────────────────────────────

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: root.gap
        spacing: root.compact ? 8 : 12

        // Header, as in the Forum: the app on the left, the network and
        // Settings on the right; stacked when narrow.
        GridLayout {
            Layout.fillWidth: true
            columns: root.narrow ? 1 : 2
            columnSpacing: 12; rowSpacing: 8
            RowLayout {
                Layout.fillWidth: true; Layout.minimumWidth: 0; spacing: 10
                Image { source: "icons/logo.png"; sourceSize: Qt.size(128, 128)
                        Layout.preferredWidth: root.compact ? 36 : 40; Layout.preferredHeight: Layout.preferredWidth }
                ColumnLayout {
                    Layout.fillWidth: true; Layout.minimumWidth: 0; spacing: 0
                    RowLayout { spacing: 8
                        Text { text: "Antumbra Vesting"; color: root.text; font.pixelSize: root.compact ? 18 : 20; font.bold: true }
                        Dim { text: root.appVersion ? "v" + root.appVersion : ""; wrapMode: Text.NoWrap } }
                    Dim { Layout.fillWidth: true; wrapMode: root.phone ? Text.Wrap : Text.NoWrap; elide: Text.ElideRight
                          text: "Read straight from the chain · signs nothing · LEZ testnet v0.3" }
                }
            }
            RowLayout {
                Layout.fillWidth: root.narrow
                Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
                spacing: 10
                AppCombo {
                    id: networkBox
                    Layout.fillWidth: root.phone
                    Layout.preferredWidth: 220; Layout.minimumWidth: 140
                    model: [root.networkLabel, "Another node or program…"]
                    currentIndex: 0
                    onActivated: function (i) { if (i === 1) { currentIndex = 0; settings.open() } }
                }
                AppButton { text: "Settings…"; onClicked: settings.open() }
                Item { visible: root.narrow && !root.phone; Layout.fillWidth: true }
            }
        }
        // Where it reads from, and whether that answers.
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Rectangle {
                Layout.alignment: Qt.AlignTop; Layout.topMargin: 4
                width: 8; height: 8; radius: 4
                color: root.status.state === "ok" ? root.ok : root.status.state === "error" ? "#e5484d" : root.warn
            }
            Dim {
                Layout.fillWidth: true
                text: root.status.state === "ok"
                      ? "Connected · block " + root.fmt(String(root.status.block)) + " · chain clock " + root.dayMonth(root.status.clock) + " " + root.hm(root.status.clock) + " UTC"
                        + root.lagNote(root.status.clock)
                      : root.status.state === "warn"
                      ? "No vesting program at " + root.shortKey(root.programId) + " on " + root.rpcUrl + ". Check the program id in Settings."
                      : (root.status.text || "Connecting to " + root.rpcUrl + "…")
            }
        }
        Text {
            Layout.fillWidth: true; visible: root.lastError !== ""
            text: root.lastError; textFormat: Text.PlainText; color: root.warn; font.pixelSize: 13; wrapMode: Text.WrapAtWordBoundaryOrAnywhere
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 12

            // ── Schedules, as the Forum's Topics ────────────────────────────
            Pane {
                visible: !root.compact || root.detailState === "idle"
                Layout.fillWidth: root.compact
                Layout.preferredWidth: root.compact ? -1 : Math.min(420, Math.max(300, root.width * 0.34))
                Layout.fillHeight: true

                ColumnLayout {
                    anchors.fill: parent; anchors.margins: 10; spacing: 8
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        Text { text: "Schedules"; color: root.text; font.pixelSize: 15; font.bold: true }
                        Rectangle {
                            visible: !root.result && root.examples.length > 0
                            radius: 8; color: root.accentStrong
                            implicitWidth: exText.implicitWidth + 12; implicitHeight: 18
                            Text { id: exText; anchors.centerIn: parent; color: "white"; font.pixelSize: 11; font.bold: true
                                   text: root.examples.length + " examples" }
                        }
                        Item { Layout.fillWidth: true }
                        AppButton { text: "↻"; implicitWidth: 40
                                    onClicked: root.result ? root.lookup(root.lastQuery) : root.loadExamples()
                                    ToolTip.visible: hovered; ToolTip.text: "Read again from the chain" }
                        AccentButton { text: "Look up"; onClicked: { if (lookupField.text.trim() === "") lookupField.forceActiveFocus(); else root.lookup(lookupField.text) } }
                    }
                    AppField {
                        id: lookupField
                        objectName: "lookupField"
                        Layout.fillWidth: true
                        placeholderText: "Schedule id, account or batch id"
                        onAccepted: root.lookup(text)
                        Keys.onReturnPressed: root.lookup(text)
                        Keys.onEnterPressed: root.lookup(text)
                    }
                    Busy { visible: root.looking; label: "Looking up “" + root.lastQuery + "”…"; Layout.fillWidth: true }
                    Text { visible: root.listMessage !== "" && !root.looking; Layout.fillWidth: true; text: root.listMessage; textFormat: Text.PlainText
                           color: root.listMessageTone === "bad" ? root.warn : root.dim; font.pixelSize: 13; wrapMode: Text.WrapAtWordBoundaryOrAnywhere }
                    RowLayout { visible: root.result !== null; Layout.fillWidth: true; spacing: 6
                        Label2 { text: root.result ? root.result.title : ""; font.bold: true; Layout.fillWidth: true; elide: Text.ElideRight; maximumLineCount: 1 }
                        Text { text: "‹ Examples"; color: root.accent; font.pixelSize: 12; textFormat: Text.PlainText
                               HoverHandler { cursorShape: Qt.PointingHandCursor }
                               TapHandler { onTapped: { root.result = null; root.listMessage = "" } } } }

                    ListView {
                        id: scheduleList
                        Layout.fillWidth: true; Layout.fillHeight: true
                        clip: true; spacing: 4
                        model: root.result ? root.result.items : root.examples
                        header: Dim { width: ListView.view.width; bottomPadding: 6
                                      text: root.result ? root.result.subtitle : "Examples on the public testnet, from the end-to-end run of 2 Oct 2026. Every number is read from the chain now." }
                        delegate: ScheduleRow { width: ListView.view.width; item: modelData }
                        footer: Dim { width: ListView.view.width; topPadding: 8; visible: root.result && root.result.note ? true : false
                                      text: root.result && root.result.note ? root.result.note : "" }
                        Busy { anchors.centerIn: parent; visible: !root.result && root.examplesState === "loading"; label: "Reading the examples from the testnet…" }
                        ColumnLayout { anchors.centerIn: parent; width: parent.width - 32; visible: !root.result && root.examplesState === "error"; spacing: 8
                            Dim { Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter; text: root.examplesError; color: root.warn; font.pixelSize: 13 }
                            AppButton { Layout.alignment: Qt.AlignHCenter; text: "Try again"; onClicked: root.loadExamples() } }
                    }
                }
            }

            // ── The open schedule, as the Forum's thread ────────────────────
            Pane {
                visible: !root.compact || root.detailState !== "idle"
                Layout.fillWidth: true; Layout.fillHeight: true

                // Nothing open: what this is and what to type.
                Flickable {
                    id: welcomeScroll
                    anchors.fill: parent
                    visible: root.detailState === "idle"
                    contentHeight: Math.max(height, welcome.implicitHeight + 60)
                    clip: true
                    ColumnLayout {
                        id: welcome
                        width: Math.min(welcomeScroll.width - 60, 560)
                        x: (welcomeScroll.width - width) / 2
                        y: Math.max(30, (welcomeScroll.height - implicitHeight) / 2)
                        spacing: 14
                        Text { Layout.fillWidth: true; text: "Look up a vesting schedule"; color: root.text; font.pixelSize: 18; font.bold: true; wrapMode: Text.WordWrap }
                        Label2 { Layout.fillWidth: true; color: root.dim
                                 text: "Antumbra reads vesting schedules straight from the LEZ testnet: how much has vested, what can be claimed now, who controls it, and what the escrow holds. It holds no keys and signs nothing." }
                        Repeater {
                            model: [
                                ["A schedule id", "The text or 64 hex digits it was created with, like testnet4-lin."],
                                ["An account", "A schedule's own account, or a beneficiary's. For a beneficiary it finds the schedules that account has claimed from or been transferred, plus the examples: the chain cannot list schedules by beneficiary."],
                                ["A batch id", "Every schedule of a batch, like testnet4-batch8, with what each one can claim."]
                            ]
                            delegate: RowLayout {
                                Layout.fillWidth: true; spacing: 10
                                Rectangle { Layout.alignment: Qt.AlignTop; Layout.topMargin: 6; width: 6; height: 6; radius: 3; color: root.accent }
                                ColumnLayout { Layout.fillWidth: true; spacing: 2
                                    Label2 { text: modelData[0]; font.bold: true }
                                    Dim { Layout.fillWidth: true; text: modelData[1]; font.pixelSize: 13 } }
                            }
                        }
                        Dim { Layout.fillWidth: true; text: "Pick an example on the left, type an id and press Look up, or try one of these:" }
                        Flow {
                            Layout.fillWidth: true; spacing: 8
                            Repeater {
                                model: [ { q: "testnet4-lin", l: "testnet4-lin" }, { q: "testnet4-mil", l: "testnet4-mil" }, { q: "testnet4-batch8", l: "testnet4-batch8" },
                                         { q: "2xub3k7XNpfsPk44Gt52dZtadG4zXrqqdPKKgjFZuKfn", l: "a beneficiary, 2xub…uKfn" } ]
                                delegate: AppButton { text: modelData.l; font.pixelSize: 13
                                                      onClicked: { lookupField.text = modelData.q; root.lookup(modelData.q) } }
                            }
                        }
                    }
                }

                // Reading, or could not read.
                ColumnLayout {
                    visible: root.detailState === "loading" || root.detailState === "error"
                    anchors.fill: parent; anchors.margins: root.compact ? 10 : 14; spacing: 12
                    AppButton { visible: root.compact; text: "‹ Schedules"; onClicked: root.closeDetail() }
                    Item { Layout.fillHeight: true }
                    Busy { visible: root.detailState === "loading"; Layout.alignment: Qt.AlignHCenter
                           label: "Reading " + (root.opened.scheduleId || root.shortKey(root.opened.account || "")) + " from the chain…" }
                    Dim { visible: root.detailState === "error"; Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter
                          text: root.detailError; color: root.warn; font.pixelSize: 14 }
                    AppButton { visible: root.detailState === "error"; Layout.alignment: Qt.AlignHCenter; text: "Try again"; onClicked: root.reloadDetail() }
                    Item { Layout.fillHeight: true }
                }

                // The schedule as a thread, the claim as the reply box.
                ColumnLayout {
                    visible: root.detailState === "ready" && root.detail !== null
                    anchors.fill: parent; anchors.margins: root.compact ? 10 : 14; spacing: 10
                    AppButton { visible: root.compact; text: "‹ Schedules"; onClicked: root.closeDetail() }
                    ScrollView {
                        id: detailScroll
                        objectName: "detailScroll"
                        Layout.fillWidth: true; Layout.fillHeight: true
                        clip: true
                        contentWidth: availableWidth
                        ColumnLayout {
                            width: detailScroll.availableWidth - 10   // room for the scroll bar
                            spacing: 14
                            TitleRow { Layout.fillWidth: true; s: root.detail }
                            RowLayout { Layout.fillWidth: true; spacing: 8
                                Byline { Layout.fillWidth: true; s: root.detail }
                                Dim { text: root.detail ? "read " + root.ago(root.detail.receivedAt) : ""; wrapMode: Text.NoWrap }
                                AppButton { text: "↻"; implicitWidth: 32; implicitHeight: 28; onClicked: root.refreshDetail(true)
                                            ToolTip.visible: hovered; ToolTip.text: "Read this schedule again" } }
                            Claimable { Layout.fillWidth: true; s: root.detail }
                            Rule {}
                            Post { label: root.detail && root.detail.kind === 2 ? "Milestones" : "Vesting curve"; meta: root.curveMeta(root.detail)
                                   CurveBlock { Layout.fillWidth: true; s: root.detail; claims: root.claimsOf(root.activity) } }
                            Post { label: "Escrow"; meta: root.detail ? root.shortKey(root.detail.escrow) : ""
                                   EscrowBlock { Layout.fillWidth: true; s: root.detail } }
                            Post { label: "Accounts"
                                   AccountsBlock { Layout.fillWidth: true; s: root.detail } }
                            Post { label: "Activity"; meta: "from the explorer's index, refused attempts marked"
                                   ActivityBlock { Layout.fillWidth: true } }
                            // On a narrow screen the claim is the last section rather than a box pinned below.
                            Post { visible: root.compact; label: "How to claim"
                                   ClaimComposer { Layout.fillWidth: true; s: root.detail } }
                            Item { implicitHeight: 2 }
                        }
                    }
                    ClaimComposer { visible: !root.compact; Layout.fillWidth: true; s: root.detail }
                }

            }
        }
    }

    // The sentence under the claimable number.
    function summary(s) {
        if (!s) return ""
        if (s.cancelledAt > 0) return "Cancelled on " + utc(s.cancelledAt) + ", " + rel(s.cancelledAt, chainNow) + ". What had vested by then stays claimable; the rest went back to the refund account."
        if (s.kind === 2) return s.signalledCount + " of " + s.tranches.length + " milestones signalled. Each is released when the milestone authority signals it."
        if (chainNow < s.start) return "Starts " + rel(s.start, chainNow) + ", on " + utc(s.start) + "."
        if (s.kind === 0 && chainNow < s.cliff) return "Nothing vests before the cliff, " + rel(s.cliff, chainNow) + " (" + utc(s.cliff) + "); then everything accrued since the start at once."
        if (chainNow < s.end) return "Vesting until " + utc(s.end) + ", " + rel(s.end, chainNow) + ", by the chain's clock."
        return "Fully vested on " + utc(s.end) + ", " + rel(s.end, chainNow) + " by the chain's clock."
    }
    // What unlocks next, from the schedule's rule and the chain's clock.
    function nextUnlock(s) {
        if (!s || s.cancelledAt > 0 || s.claimed === s.total) return ""
        if (s.kind === 2) {
            var waiting = []
            for (var i = 0; i < s.tranches.length; i++) if (!s.tranches[i]) waiting.push(i + 1)
            if (!waiting.length) return ""
            return "Next: " + fmt(s.perTranche) + " " + s.unit + " when " + shortKey(s.milestoneAuthority) + " signals milestone " + waiting.join(" or ") + "."
        }
        var now = chainNow
        if (now >= s.end) return ""
        if (now < s.start) return "Starts " + rel(s.start, now) + "."
        var span = s.end - s.start
        if (s.kind === 0 && now < s.cliff)
            return fmt(String(Math.floor(s.totalF * (s.cliff - s.start) / span))) + " " + s.unit + " unlock at the cliff, " + rel(s.cliff, now) + ", then the rest accrues until " + utcShort(s.end) + " UTC."
        var units = [["hour", 3600000], ["day", 86400000], ["week", 604800000]]
        for (var k = 0; k < units.length; k++) {
            var per = Math.floor(s.totalF * units[k][1] / span)
            if (per >= 1) return "Accruing about " + fmt(String(per)) + " " + s.unit + " a " + units[k][0] + "; fully vested " + rel(s.end, now) + "."
        }
        return "Fully vested " + rel(s.end, now) + "."
    }
    // The chain's clock is its last block's time; on a quiet chain it trails.
    function lagNote(clock) {
        var lag = (nowLocal - clock) / 60000
        if (!clock || lag < 5) return ""
        return lag < 120 ? " (" + Math.round(lag) + " min behind this computer)" : " (" + Math.round(lag / 60) + " h behind this computer)"
    }
    function ago(ms) {
        var s = Math.max(0, Math.round((nowLocal - ms) / 1000))
        return s < 5 ? "just now" : s < 60 ? s + " s ago" : Math.round(s / 60) + " min ago"
    }
    function nothingToClaim(s) {
        if (s.claimed === s.total) return "Everything has been claimed."
        if (s.cancelledAt > 0) return "Everything that vested before the cancellation has been claimed."
        if (s.kind === 2) return "Nothing to claim until another milestone is signalled."
        if (chainNow < s.start || (s.kind === 0 && chainNow < s.cliff)) return "Nothing has vested yet."
        return "Nothing more to claim right now."
    }

    // ── Settings ─────────────────────────────────────────────────────────────
    Dialog {
        id: settings
        palette: root.palette
        background: Rectangle { color: root.panel; radius: 8; border.color: root.line }
        header: Text { text: settings.title; color: root.text; font.pixelSize: 16; font.bold: true; padding: 16 }
        title: "Settings"
        modal: true; anchors.centerIn: parent
        width: Math.min(640, root.width - 24)
        footer: Item { implicitHeight: 12 }
        property string error: ""
        onOpened: { rpcField.text = root.rpcUrl; programField.text = root.programId; explorerField.text = root.explorerUrl; error = "" }
        ColumnLayout {
            anchors.fill: parent; spacing: 8
            Dim { text: "Node (JSON-RPC)" }
            AppField { id: rpcField; objectName: "rpcField"; Layout.fillWidth: true; placeholderText: root.defaults.rpc || "" }
            Dim { text: "Vesting program (its header account)" }
            AppField { id: programField; objectName: "programField"; Layout.fillWidth: true; placeholderText: root.defaults.program || ""; font.family: root.monoFamily }
            Dim { text: "Block explorer" }
            AppField { id: explorerField; Layout.fillWidth: true; placeholderText: root.defaults.explorer || "" }
            Dim { Layout.fillWidth: true; text: "Saved on this device, in this Basecamp profile. An empty field goes back to the public testnet." }
            Text { visible: root.envOverrides !== ""; Layout.fillWidth: true; color: root.warn; font.pixelSize: 12; wrapMode: Text.WordWrap; textFormat: Text.PlainText
                   text: "Set in the environment, and used instead of what is saved here: " + root.envOverrides + "." }
            Text { visible: settings.error !== ""; Layout.fillWidth: true; color: root.warn; font.pixelSize: 13; wrapMode: Text.WordWrap; textFormat: Text.PlainText; text: settings.error }
            RowLayout {
                Layout.fillWidth: true; spacing: 10
                AppButton { text: "Restore defaults"; onClicked: { rpcField.text = ""; programField.text = ""; explorerField.text = "" } }
                Item { Layout.fillWidth: true }
                AppButton { text: "Cancel"; onClicked: settings.close() }
                AccentButton { text: "Save"; objectName: "saveSettings"
                    onClicked: root.call(root.backend.saveSettings(rpcField.text, programField.text, explorerField.text), function (r) {
                        if (r && String(r).indexOf("error: ") === 0) { settings.error = String(r).substring(7); return }
                        settings.close(); root.result = null; root.closeDetail(); root.loadExamples()
                    }) }
            }
        }
    }
}
