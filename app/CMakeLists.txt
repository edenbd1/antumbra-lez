cmake_minimum_required(VERSION 3.20)
project(antumbra_lez LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Two build paths (same pattern as LP-0017's whistleblower plugin):
#
# 1. Framework build — when $LOGOS_MODULE_BUILDER_ROOT is set (i.e.
#    inside the logos-module-builder Nix dev shell), include
#    LogosModule.cmake and let it wire up Qt + the SDK + LGX packaging.
#    This is the production path that produces an installable .lgx.
#
# 2. Manual build — fallback for IDE workflows and the standalone
#    preview app. Plain Qt6 CMake. Useful for QML iteration without
#    spinning up the full Logos stack. Produces a loadable .so/.dylib
#    that pairs with the metadata.json + module.json placed next to it.

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)

    # QtConcurrent is deliberately absent: Basecamp 0.2.2 does not bundle it, so
    # linking it makes the plugin unloadable. The bridge runs QProcess
    # synchronously instead.
    find_package(Qt6 REQUIRED COMPONENTS
        Widgets Quick QuickWidgets Qml Gui Network OpenGL)
    find_package(nlohmann_json REQUIRED)
    find_package(OpenSSL REQUIRED)
    find_package(Boost CONFIG REQUIRED)

    # Keep module.json in sync with metadata.json on every configure —
    # the lambda-prize validation bot looks for the former, but the
    # Basecamp package_manager reads the latter.
    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/metadata.json
                   ${CMAKE_CURRENT_BINARY_DIR}/module.json COPYONLY)

    logos_module(
        NAME    antumbra_lez
        SOURCES src/plugin.h
                src/plugin.cpp
                src/chain_bridge.h
                src/chain_bridge.cpp
        LINK_TARGETS Qt6::Widgets Qt6::Quick Qt6::QuickWidgets
                     Qt6::Qml Qt6::Gui Qt6::Network Qt6::OpenGL
                     nlohmann_json::nlohmann_json
                     OpenSSL::SSL Boost::system
    )

    qt_add_resources(antumbra_lez_module_plugin "qml"
        PREFIX "/qml"
        BASE "qml"
        FILES qml/Main.qml qml/qmldir
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
        PREFIX "/qml"
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
        OUTPUT_NAME "antumbra_lez"
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
