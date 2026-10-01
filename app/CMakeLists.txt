cmake_minimum_required(VERSION 3.20)
project(antumbra_lez LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Two build paths:
#
# 1. Framework build: when $LOGOS_MODULE_BUILDER_ROOT is set, which
#    `nix build .#lgx-portable` (mkLogosModule, see flake.nix) does, include
#    LogosModule.cmake and let it wire up Qt, the SDK and the .lgx packaging.
#    This is the production path that produces the installable package.
#
# 2. Manual build — fallback for IDE workflows and the standalone
#    preview app. Plain Qt6 CMake. Useful for QML iteration without
#    spinning up the full Logos stack. Produces a loadable .so/.dylib
#    that pairs with the metadata.json + module.json placed next to it.

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)

    # The plugin is a legacy `type: ui` IComponent widget: the QML scene runs
    # in a QQuickWidget inside Basecamp's own process, and the bridge talks to
    # the sequencer through QtNetwork. QtConcurrent is deliberately absent, as
    # Basecamp does not bundle it.
    find_package(Qt6 REQUIRED COMPONENTS
        Widgets Quick QuickWidgets Qml Gui Network)

    # Q_PLUGIN_METADATA(FILE "metadata.json") resolves next to src/plugin.h.
    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/metadata.json
                   ${CMAKE_CURRENT_SOURCE_DIR}/src/metadata.json COPYONLY)

    logos_module(
        NAME    antumbra_lez
        SOURCES src/plugin.h
                src/plugin.cpp
                src/chain_bridge.h
                src/chain_bridge.cpp
        LINK_TARGETS Qt6::Widgets Qt6::Quick Qt6::QuickWidgets
                     Qt6::Qml Qt6::Gui Qt6::Network
    )

    # zlib, not zstd: a zstd-compressed resource needs a QtCore built with
    # zstd (the symbol qt_resourceFeatureZstd), and not every Qt 6.9.2 is.
    qt_add_resources(antumbra_lez_module_plugin "qml"
        PREFIX "/antumbra_lez"
        BASE "qml"
        FILES qml/Main.qml qml/qmldir
        OPTIONS --no-zstd
    )

else()
    set(CMAKE_AUTOMOC ON)
    set(CMAKE_AUTORCC ON)
    find_package(Qt6 REQUIRED COMPONENTS
        Core Gui Widgets Quick QuickWidgets Qml Network
    )

    # Qt's moc resolves Q_PLUGIN_METADATA(FILE "...") relative to the
    # location of the header that declares the plugin (here src/plugin.h).
    # Stage a copy of metadata.json into src/ so moc finds it.
    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/metadata.json
                   ${CMAKE_CURRENT_SOURCE_DIR}/src/metadata.json COPYONLY)

    qt_add_plugin(antumbra_lez_plugin CLASS_NAME AntumbraPlugin)

    target_sources(antumbra_lez_plugin PRIVATE
        src/plugin.h
        src/plugin.cpp
        src/chain_bridge.h
        src/chain_bridge.cpp
    )

    qt_add_resources(antumbra_lez_plugin "qml"
        PREFIX "/antumbra_lez"
        BASE "qml"
        FILES qml/Main.qml qml/qmldir
    )

    target_link_libraries(antumbra_lez_plugin PRIVATE
        Qt6::Core Qt6::Gui Qt6::Widgets Qt6::Quick
        Qt6::QuickWidgets Qt6::Qml Qt6::Network
    )

    # Basecamp's plugin discovery expects <plugin>/<plugin>.<ext>,
    # not the default lib-prefixed name.
    set_target_properties(antumbra_lez_plugin PROPERTIES
        OUTPUT_NAME "antumbra_lez_plugin"
        PREFIX ""
    )

    # Copy the manifest next to the binary so the user can drop the
    # whole directory into Basecamp's user-plugins dir.
    add_custom_command(TARGET antumbra_lez_plugin POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy
            ${CMAKE_CURRENT_SOURCE_DIR}/metadata.json
            $<TARGET_FILE_DIR:antumbra_lez_plugin>/metadata.json)
    add_custom_command(TARGET antumbra_lez_plugin POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy
            ${CMAKE_CURRENT_SOURCE_DIR}/metadata.json
            $<TARGET_FILE_DIR:antumbra_lez_plugin>/module.json)
endif()
