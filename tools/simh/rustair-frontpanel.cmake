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

function(rustair_replace_unique_parser block_var context output_var)
    set(rustair_parser_old "c = strchr (response, ':');")
    set(rustair_parser_new "c = strrchr (response, ':');")
    set(rustair_block "${${block_var}}")

    string(FIND "${rustair_block}" "${rustair_parser_old}" rustair_first_parser)
    if(rustair_first_parser EQUAL -1)
        message(FATAL_ERROR
            "RusTair could not find the EXAMINE parser inside ${context}. "
            "Open-SIMH may have changed; review the compatibility patch before building.")
    endif()

    string(LENGTH "${rustair_parser_old}" rustair_parser_len)
    math(EXPR rustair_after_first "${rustair_first_parser} + ${rustair_parser_len}")
    string(SUBSTRING "${rustair_block}" ${rustair_after_first} -1 rustair_parser_tail)
    string(FIND "${rustair_parser_tail}" "${rustair_parser_old}" rustair_second_parser)
    if(NOT rustair_second_parser EQUAL -1)
        message(FATAL_ERROR
            "RusTair found more than one EXAMINE parser site inside ${context}. "
            "Open-SIMH may have changed; review the compatibility patch before building.")
    endif()

    string(REPLACE "${rustair_parser_old}" "${rustair_parser_new}"
        rustair_block "${rustair_block}")
    set(${output_var} "${rustair_block}" PARENT_SCOPE)
endfunction()

function(rustair_prepare_simh_frontpanel_source output_var)
    # Open-SIMH FrontPanel API v12 parses EXAMINE responses by taking the first
    # ':' in the response. The classic Altair monitor can emit symbolic-output
    # diagnostics before the actual memory value. RusTair changes only the two
    # EXAMINE parsers to use the final ':' in the response.
    set(rustair_frontpanel_source "${CMAKE_SOURCE_DIR}/sim_frontpanel.c")
    set(rustair_frontpanel_dir "${CMAKE_BINARY_DIR}/rustair-frontpanel-src")
    set(rustair_frontpanel_patched "${rustair_frontpanel_dir}/sim_frontpanel.c")

    file(READ "${rustair_frontpanel_source}" rustair_frontpanel_contents)

    string(FIND "${rustair_frontpanel_contents}"
        "\nsim_panel_gen_examine (" rustair_gen_start)
    string(FIND "${rustair_frontpanel_contents}"
        "\nsim_panel_get_history (" rustair_gen_end)
    if(rustair_gen_start EQUAL -1 OR rustair_gen_end EQUAL -1 OR
       rustair_gen_end LESS_EQUAL rustair_gen_start)
        message(FATAL_ERROR
            "RusTair could not isolate sim_panel_gen_examine() in "
            "${rustair_frontpanel_source}. Open-SIMH may have changed; review "
            "the compatibility patch before building.")
    endif()
    math(EXPR rustair_gen_len "${rustair_gen_end} - ${rustair_gen_start}")
    string(SUBSTRING "${rustair_frontpanel_contents}"
        ${rustair_gen_start} ${rustair_gen_len} rustair_gen_block)
    rustair_replace_unique_parser(
        rustair_gen_block "sim_panel_gen_examine()" rustair_gen_block_patched)
    string(SUBSTRING "${rustair_frontpanel_contents}"
        0 ${rustair_gen_start} rustair_gen_prefix)
    string(SUBSTRING "${rustair_frontpanel_contents}"
        ${rustair_gen_end} -1 rustair_gen_suffix)
    set(rustair_frontpanel_contents
        "${rustair_gen_prefix}${rustair_gen_block_patched}${rustair_gen_suffix}")

    string(FIND "${rustair_frontpanel_contents}"
        "\nsim_panel_mem_examine (" rustair_mem_start)
    string(FIND "${rustair_frontpanel_contents}"
        "\nsim_panel_mem_deposit (" rustair_mem_end)
    if(rustair_mem_start EQUAL -1 OR rustair_mem_end EQUAL -1 OR
       rustair_mem_end LESS_EQUAL rustair_mem_start)
        message(FATAL_ERROR
            "RusTair could not isolate sim_panel_mem_examine() in "
            "${rustair_frontpanel_source}. Open-SIMH may have changed; review "
            "the compatibility patch before building.")
    endif()
    math(EXPR rustair_mem_len "${rustair_mem_end} - ${rustair_mem_start}")
    string(SUBSTRING "${rustair_frontpanel_contents}"
        ${rustair_mem_start} ${rustair_mem_len} rustair_mem_block)
    rustair_replace_unique_parser(
        rustair_mem_block "sim_panel_mem_examine()" rustair_mem_block_patched)
    string(SUBSTRING "${rustair_frontpanel_contents}"
        0 ${rustair_mem_start} rustair_mem_prefix)
    string(SUBSTRING "${rustair_frontpanel_contents}"
        ${rustair_mem_end} -1 rustair_mem_suffix)
    set(rustair_frontpanel_contents
        "${rustair_mem_prefix}${rustair_mem_block_patched}${rustair_mem_suffix}")

    # RusTair product integration also needs access to the same interactive SCP
    # console that a user would see at the simulator's `sim>` prompt. API v12
    # deliberately exposes structured operations but no arbitrary command
    # function. Add one small RusTair-only export inside this private DLL. It
    # reuses FrontPanel's existing synchronized command path and is available
    # only while the simulator is halted, matching the real SIMH console.
    string(FIND "${rustair_frontpanel_contents}"
        "rustair_panel_exec_command" rustair_existing_console_extension)
    if(NOT rustair_existing_console_extension EQUAL -1)
        message(FATAL_ERROR
            "Open-SIMH source already contains rustair_panel_exec_command; review the RusTair console extension before building.")
    endif()

    set(rustair_console_extension [=[

/* RusTair private FrontPanel extension: execute one halted SCP command. */
int
rustair_panel_exec_command (PANEL *panel,
                            const char *command,
                            char *buffer,
                            size_t buffer_size)
{
char *response = NULL;
int cmd_stat = 0;

if ((!panel) || (panel->State == Error) || (!command) || (!buffer) || (buffer_size == 0)) {
    sim_panel_set_error (NULL, "Invalid RusTair console request");
    return -1;
    }
if (panel->State == Run) {
    sim_panel_set_error (NULL, "Not Halted");
    return -1;
    }
buffer[0] = '\0';
if (_panel_sendf (panel, &cmd_stat, &response, "%s", command)) {
    free (response);
    return -1;
    }
if (response) {
    strncpy (buffer, response, buffer_size - 1);
    buffer[buffer_size - 1] = '\0';
    free (response);
    }
if (cmd_stat) {
    sim_panel_set_error (NULL, "SIMH command status %d: %s", cmd_stat, command);
    return -1;
    }
return 0;
}
]=])
    string(APPEND rustair_frontpanel_contents "${rustair_console_extension}")

    file(MAKE_DIRECTORY "${rustair_frontpanel_dir}")
    file(WRITE "${rustair_frontpanel_patched}" "${rustair_frontpanel_contents}")

    set(${output_var} "${rustair_frontpanel_patched}" PARENT_SCOPE)
endfunction()

function(rustair_add_simh_frontpanel)
    if(NOT TARGET thread_lib OR NOT TARGET os_features)
        message(FATAL_ERROR
            "RusTair FrontPanel target was injected before Open-SIMH dependencies were ready")
    endif()

    if(NOT TARGET simh_frontpanel)
        rustair_prepare_simh_frontpanel_source(rustair_frontpanel_source)

        add_library(simh_frontpanel SHARED
            "${rustair_frontpanel_source}"
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
            "RusTair: added minimal simh_frontpanel shared library from Open-SIMH source ${CMAKE_SOURCE_DIR}")
        message(STATUS
            "RusTair: applied classic Altair FrontPanel EXAMINE parser compatibility patch in build tree")
        message(STATUS
            "RusTair: added halted interactive SCP console export rustair_panel_exec_command")
    endif()

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
