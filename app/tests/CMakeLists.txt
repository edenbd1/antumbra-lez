# The chain reader on its own, with plain Qt (Core, Network): no Basecamp, no
# Logos SDK. `cmake -S app/tests -B build && cmake --build build && build/chain_test app/tests/fixtures`
cmake_minimum_required(VERSION 3.20)
project(antumbra_chain_test LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
find_package(Qt6 REQUIRED COMPONENTS Core Network)
add_executable(chain_test chain_test.cpp ../src/vesting_chain.cpp ../src/vesting_chain.h)
target_include_directories(chain_test PRIVATE ../src)
target_link_libraries(chain_test PRIVATE Qt6::Core Qt6::Network)

# The real view outside Basecamp, against the real reader (needs Qt Quick).
find_package(Qt6 COMPONENTS Gui Quick Qml QuickControls2)
if(Qt6Quick_FOUND)
    add_executable(qml_host qml_host.cpp ../src/vesting_chain.cpp ../src/vesting_chain.h)
    target_include_directories(qml_host PRIVATE ../src)
    target_link_libraries(qml_host PRIVATE Qt6::Core Qt6::Network Qt6::Gui Qt6::Quick Qt6::Qml)
endif()
