cmake_minimum_required(VERSION 3.28)
project(SpatialWindowNamedPipeCheckpoint17 LANGUAGES CXX)

if(NOT WIN32)
  message(FATAL_ERROR "This proof requires a native Windows runner")
endif()

include(CTest)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_library(spatial_common_uuid STATIC
  source/modules/common/src/uuid.cpp
)
target_include_directories(spatial_common_uuid PUBLIC
  source/modules/common/include
)

add_library(spatial_transport_records STATIC
  source/modules/transport-records/src/record_codec.cpp
)
target_include_directories(spatial_transport_records PUBLIC
  source/modules/common/include
  source/modules/transport-records/include
)

add_library(spatial_transport_named_pipe STATIC
  source/modules/transport-named-pipe/src/pipe_types.cpp
  source/modules/transport-named-pipe/src/named_pipe_internal.cpp
  source/modules/transport-named-pipe/src/pipe_security.cpp
  source/modules/transport-named-pipe/src/named_pipe_server.cpp
  source/modules/transport-named-pipe/src/named_pipe_client.cpp
)
target_include_directories(spatial_transport_named_pipe
  PUBLIC
    source/modules/common/include
    source/modules/transport-named-pipe/include
  PRIVATE
    source/modules/transport-named-pipe/src
)
target_link_libraries(spatial_transport_named_pipe
  PUBLIC spatial_common_uuid
  PRIVATE advapi32
)

add_executable(spatial_named_pipe_windows_roundtrip_tests
  source/tests/transport/named_pipe_windows_roundtrip_tests.cpp
)
target_include_directories(spatial_named_pipe_windows_roundtrip_tests PRIVATE
  source/modules/common/include
  source/modules/transport-records/include
  source/modules/transport-named-pipe/include
)
target_link_libraries(spatial_named_pipe_windows_roundtrip_tests PRIVATE
  spatial_transport_named_pipe
  spatial_transport_records
)

foreach(target
    spatial_common_uuid
    spatial_transport_records
    spatial_transport_named_pipe
    spatial_named_pipe_windows_roundtrip_tests)
  target_compile_options(${target} PRIVATE /W4 /WX /permissive-)
endforeach()

add_test(
  NAME spatial_named_pipe_windows_roundtrip_tests
  COMMAND spatial_named_pipe_windows_roundtrip_tests
)
