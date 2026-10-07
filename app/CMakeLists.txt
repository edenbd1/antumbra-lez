cmake_minimum_required(VERSION 3.20)
project(AntumbraVestingPlugin LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT, or build with `nix build .#lgx-portable`. "
                        "The chain reader alone builds with plain Qt: see tests/CMakeLists.txt.")
endif()

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/metadata.json" METADATA_JSON)
string(JSON MODULE_NAME GET ${METADATA_JSON} name)
string(JSON MODULE_VERSION GET ${METADATA_JSON} version)

# A ui_qml module: the QML view runs in Basecamp, this backend in its own
# process, and the two talk over Qt Remote Objects through src/antumbra_lez.rep.
# The backend reads the chain over HTTPS with QtNetwork; vesting_chain.* is the
# reading and decoding, with no Basecamp dependency, tested on its own.
find_package(Qt6 REQUIRED COMPONENTS Network)

logos_module(
    NAME ${MODULE_NAME}
    REP_FILE src/antumbra_lez.rep
    SOURCES
        src/antumbra_lez_backend.h
        src/antumbra_lez_backend.cpp
        src/vesting_chain.h
        src/vesting_chain.cpp
    INCLUDE_DIRS
        src
)

target_link_libraries(${MODULE_NAME}_module_plugin PRIVATE Qt6::Network)
target_compile_definitions(${MODULE_NAME}_module_plugin PRIVATE
    ANTUMBRA_VERSION="${MODULE_VERSION}")
