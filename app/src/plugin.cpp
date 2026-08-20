// SPDX-License-Identifier: MIT OR Apache-2.0
#include "plugin.h"
#include "chain_bridge.h"

#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickWidget>
#include <QUrl>

AntumbraPlugin::AntumbraPlugin(QObject* parent) : QObject(parent) {}

AntumbraPlugin::~AntumbraPlugin() = default;

QWidget* AntumbraPlugin::createWidget(LogosAPI* /*api*/) {
    // The bridge reads the three deployed programs straight from a sequencer.
    // It holds no keys and signs nothing: this panel is read-only by design.
    m_bridge = new ChainBridge(this);

    auto* view = new QQuickWidget();
    view->engine()->rootContext()->setContextProperty(
        QStringLiteral("bridge"), m_bridge);
    view->setResizeMode(QQuickWidget::SizeRootObjectToView);
    view->setSource(QUrl(QStringLiteral("qrc:/qml/Main.qml")));
    return view;
}

void AntumbraPlugin::destroyWidget(QWidget* widget) {
    if (widget) {
        widget->deleteLater();
    }
}
