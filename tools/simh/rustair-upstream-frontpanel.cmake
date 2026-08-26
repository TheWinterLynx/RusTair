# Build the RusTair FrontPanel DLL from the Open-SIMH sources without applying
# any RusTair compatibility patches. This is used only for A/B compatibility
# verification so we can prove which local patches are actually required.
#
# It still creates the shared FrontPanel DLL and redirects simulator outputs to
# architecture-specific build directories; those are build/integration details,
# not source-code compatibility patches.

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR
        "RusTair upstream FrontPanel verification requires CMake 3.19 or newer")
endif()

function(rustair_add_upstream_simh_frontpanel)
    if(NOT TARGET thread_lib OR NOT TARGET os_features)
        message(FATAL_ERROR
            "RusTair upstream FrontPanel target was injected before Open-SIMH dependencies were ready")
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
        target_link_libraries(simh_frontpanel PRIVATE os_features thread_lib)

        if(MSVC)
            target_compile_definitions(simh_frontpanel PRIVATE
                _CRT_NONSTDC_NO_WARNINGS
                _CRT_SECURE_NO_WARNINGS
                _WINSOCK_DEPRECATED_NO_WARNINGS)
        endif()

        message(STATUS
            "RusTair VERIFY: simh_frontpanel uses unmodified upstream sim_frontpanel.c")
    endif()

    foreach(rustair_simh_target IN ITEMS altair altairz80)
        if(TARGET ${rustair_simh_target})
            set_target_properties(${rustair_simh_target} PROPERTIES
                RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/rustair-simh/$<CONFIG>")
            message(STATUS
                "RusTair VERIFY: redirected ${rustair_simh_target} to ${CMAKE_BINARY_DIR}/rustair-simh/$<CONFIG>")
        endif()
    endforeach()
endfunction()

cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_add_upstream_simh_frontpanel)
