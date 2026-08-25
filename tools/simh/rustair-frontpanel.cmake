# Inject a reusable Open-SIMH FrontPanel shared library into an ordinary
# Open-SIMH CMake configure without modifying the SIMH checkout.
#
# Usage (CMake >= 3.19):
#   cmake -S <open-simh> -B <open-simh-build> \
#     -DCMAKE_PROJECT_INCLUDE=<RusTair>/tools/simh/rustair-frontpanel.cmake
#   cmake --build <open-simh-build> --config Release --target simh_frontpanel
#
# The deferred callback runs at the end of SIMH's top-level directory, after
# os_features/thread_lib and simulator targets have been defined.
#
# Important: do NOT link against simh_network here. simh_network is the full
# simulator networking feature interface and may pull optional dependencies
# such as SLiRP/pcap/VDE. FrontPanel only needs sim_sock.c plus the normal OS
# socket support already exported by os_features (ws2_32/wsock32/winmm on
# Windows).

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR
        "RusTair FrontPanel injection requires CMake 3.19 or newer "
        "for cmake_language(DEFER).")
endif()

function(rustair_add_simh_frontpanel)
    if(NOT TARGET thread_lib OR NOT TARGET os_features)
        message(FATAL_ERROR
            "RusTair FrontPanel target was injected before Open-SIMH dependencies were ready")
    endif()

    if(NOT TARGET simh_frontpanel)
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

        # thread_lib provides the pthread implementation used by sim_frontpanel.c.
        # os_features provides platform feature definitions and the native socket
        # libraries required by sim_sock.c. Linking simh_network here would also
        # inherit optional simulator networking stacks (notably SLiRP) that the
        # FrontPanel client does not use.
        target_link_libraries(simh_frontpanel PRIVATE os_features thread_lib)

        if(MSVC)
            target_compile_definitions(simh_frontpanel PRIVATE
                _CRT_NONSTDC_NO_WARNINGS
                _CRT_SECURE_NO_WARNINGS
                _WINSOCK_DEPRECATED_NO_WARNINGS)
        endif()

        message(STATUS
            "RusTair: added minimal simh_frontpanel shared library from Open-SIMH source ${CMAKE_SOURCE_DIR}")
    endif()

    # Open-SIMH historically sends every Windows simulator to BIN/Win32,
    # including x64 builds. Keep the RusTair-owned x64 simulator artifacts in
    # this architecture-specific build tree instead so they cannot overwrite or
    # be confused with an existing Win32 build from the same source checkout.
    foreach(rustair_simh_target IN ITEMS altair altairz80)
        if(TARGET ${rustair_simh_target})
            set_target_properties(${rustair_simh_target} PROPERTIES
                RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rustair-simh/$<CONFIG>")
            message(STATUS
                "RusTair: redirected ${rustair_simh_target} runtime output to ${CMAKE_BINARY_DIR}/rustair-simh/$<CONFIG>")
        endif()
    endforeach()
endfunction()

cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_add_simh_frontpanel)
