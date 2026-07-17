cmake_minimum_required(VERSION 3.24)
project(spatial_window_ci_probe LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

enable_testing()

add_executable(portable_runtime_probe portable_runtime_probe.cpp)
if(MSVC)
  target_compile_options(portable_runtime_probe PRIVATE /W4 /WX /permissive-)
else()
  target_compile_options(portable_runtime_probe PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()
add_test(NAME portable_runtime_probe COMMAND portable_runtime_probe)

if(WIN32)
  add_executable(d3d11_runtime_probe d3d11_runtime_probe.cpp)
  target_compile_definitions(d3d11_runtime_probe PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)
  target_compile_options(d3d11_runtime_probe PRIVATE /W4 /WX /permissive-)
  target_link_libraries(d3d11_runtime_probe PRIVATE d3d11 dxgi)
  add_test(NAME d3d11_runtime_probe COMMAND d3d11_runtime_probe)
endif()
