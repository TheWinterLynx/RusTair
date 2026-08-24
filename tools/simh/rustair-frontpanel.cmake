# Inject a reusable Open-SIMH FrontPanel shared library into an ordinary
# Open-SIMH CMake configure without modifying the SIMH checkout.
#
# Usage (CMake >= 3.19):
#   cmake -S <open-simh> -B <open-simh-build> \
#     -DCMAKE_PROJECT_INCLUDE=<RusTair>/tools/simh/rustair-frontpanel.cmake
#   cmake --build <open-simh-build> --config Release --target simh_frontpanel
#
# The deferred callback runs at the end of SIMH's top-level directory, after
# os_features, thread_lib and simh_network have been defined.

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR
        "RusTair FrontPanel injection requires CMake 3.19 or newer "
        "for cmake_language(DEFER).")
endif()

function(rustair_add_simh_frontpanel)
    if(TARGET simh_frontpanel)
        return()
    endif()

    if(NOT TARGET thread_lib OR NOT TARGET os_features)
        message(FATAL_ERROR
            "RusTair FrontPanel target was injected before Open-SIMH dependencies were ready")
    endif()

    add_library(simh_frontpanel SHARED
        "${CMAKE_SOURCE_DIR}/sim_frontpanel.c"
        "${CMAKE_SOURCE_DIR}/sim_sock.c")

    set_target_properties(simh_frontpanel PROPERTIES
        C_STANDARD 99
        WINDOWS_EXPORT_ALL_SYMBOLS ON
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rustair-frontpanel/$<CONFIG>"
        LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rustair-frontpanel/$<CONFIG>"
        ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rustair-frontpanel/$<CONFIG>")

    target_include_directories(simh_frontpanel PUBLIC "${CMAKE_SOURCE_DIR}")
    target_link_libraries(simh_frontpanel PRIVATE os_features thread_lib)

    if(WIN32)
        if(NOT TARGET simh_network)
            message(FATAL_ERROR
                "RusTair FrontPanel target requires Open-SIMH simh_network on Windows")
        endif()
        target_link_libraries(simh_frontpanel PRIVATE simh_network)
    endif()

    if(MSVC)
        target_compile_definitions(simh_frontpanel PRIVATE
            _CRT_NONSTDC_NO_WARNINGS
            _CRT_SECURE_NO_WARNINGS
            _WINSOCK_DEPRECATED_NO_WARNINGS)
    endif()

    message(STATUS
        "RusTair: added simh_frontpanel shared library from Open-SIMH source ${CMAKE_SOURCE_DIR}")
endfunction()

cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_add_simh_frontpanel)
