cmake_minimum_required(VERSION 3.16)
project(unit2nix-plugin)

find_package(PkgConfig REQUIRED)
pkg_check_modules(NIX REQUIRED nix-expr>=2.33)

find_package(nlohmann_json REQUIRED)

add_library(unit2nix_plugin MODULE plugin.cc)
target_include_directories(unit2nix_plugin PUBLIC ${NIX_INCLUDE_DIRS})
target_compile_options(unit2nix_plugin PUBLIC ${NIX_CFLAGS_OTHER})
target_link_libraries(unit2nix_plugin nlohmann_json::nlohmann_json)

target_link_directories(unit2nix_plugin PRIVATE ${RUST_LIB_DIR})
target_link_libraries(unit2nix_plugin unit2nix)

if(APPLE)
  set_target_properties(unit2nix_plugin PROPERTIES
    LINK_FLAGS "-flat_namespace -undefined suppress")
endif()

install(TARGETS unit2nix_plugin DESTINATION lib/nix/plugins)
