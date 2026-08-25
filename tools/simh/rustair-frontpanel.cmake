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

    # Do not use MATCHALL + list(LENGTH) here: the C statement being matched
    # ends in ';', and CMake list semantics treat that semicolon as a list
    # separator, which makes one match appear as two list elements.
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
    # ':' in the response.  The classic Altair monitor can emit symbolic-output
    # diagnostics before the actual memory value, e.g.:
    #
    #   %SIM-ERROR: No such opcode:
    #   %SIM-ERROR: No such opcode:
    #   1000:      A5
    #
    # sim_panel_gen_examine() and sim_panel_mem_examine() therefore parse the
    # first diagnostic line as a numeric value and return zero even though the
    # memory operation itself succeeded.  RusTair builds an otherwise identical
    # private copy of sim_frontpanel.c with only those two parsers selecting the
    # last ':' in the response instead.  The Open-SIMH checkout itself is never
    # modified.
    set(rustair_frontpanel_source "${CMAKE_SOURCE_DIR}/sim_frontpanel.c")
    set(rustair_frontpanel_dir "${CMAKE_BINARY_DIR}/rustair-frontpanel-src")
    set(rustair_frontpanel_patched "${rustair_frontpanel_dir}/sim_frontpanel.c")

    file(READ "${rustair_frontpanel_source}" rustair_frontpanel_contents)

    # Patch sim_panel_gen_examine() only. There are other strchr(response, ':')
    # calls elsewhere in sim_frontpanel.c with unrelated parsing semantics and
    # they must remain untouched.
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

    # Patch sim_panel_mem_examine() only. Recalculate offsets after the first
    # replacement so this remains correct if replacement lengths change later.
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

    file(MAKE_DIRECTORY "${rustair_frontpanel_dir}")
    file(WRITE "${rustair_frontpanel_patched}" "${rustair_frontpanel_contents}")

    set(${output_var} "${rustair_frontpanel_patched}" PARENT_SCOPE)
endfunction()

function(rustair_patch_altairz80_event_source input_path output_path context)
    # This is diagnostic instrumentation for the RusTair-owned AltairZ80 build.
    # It deliberately lives in the build tree.  When sim_process_event() stops
    # the CPU, record which UNIT was at the head of SIMH's event queue before
    # dispatch.  The trace is unconditional (sim_printf) so it is visible over
    # FrontPanel/REM-CON even when normal SIMH debug categories are disabled.
    file(READ "${input_path}" rustair_cpu_contents)
    string(REPLACE "\r\n" "\n" rustair_cpu_contents "${rustair_cpu_contents}")

    set(rustair_declaration_anchor "#include \"altairz80_defs.h\"")
    set(rustair_declarations [=[#include "altairz80_defs.h"

/* RusTair private-build event-stop diagnostics. */
extern UNIT *sim_clock_queue;
extern DEVICE *find_dev_from_unit(UNIT *uptr);
extern const char *sim_uname(UNIT *uptr);
extern const char *sim_dname(DEVICE *dptr);
extern int32 sim_qcount(void);
extern double sim_gtime(void);]=])

    string(FIND "${rustair_cpu_contents}" "${rustair_declaration_anchor}"
        rustair_decl_first)
    if(rustair_decl_first EQUAL -1)
        message(FATAL_ERROR
            "RusTair could not find the AltairZ80 include anchor in ${context}. "
            "Open-SIMH may have changed; review the event-stop diagnostic patch.")
    endif()
    string(LENGTH "${rustair_declaration_anchor}" rustair_decl_len)
    math(EXPR rustair_decl_after "${rustair_decl_first} + ${rustair_decl_len}")
    string(SUBSTRING "${rustair_cpu_contents}" ${rustair_decl_after} -1 rustair_decl_tail)
    string(FIND "${rustair_decl_tail}" "${rustair_declaration_anchor}" rustair_decl_second)
    if(NOT rustair_decl_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair found more than one AltairZ80 include anchor in ${context}. "
            "Open-SIMH may have changed; review the event-stop diagnostic patch.")
    endif()
    string(REPLACE "${rustair_declaration_anchor}" "${rustair_declarations}"
        rustair_cpu_contents "${rustair_cpu_contents}")

    set(rustair_event_old [=[            if ((reason = sim_process_event()))
                break;]=])
    set(rustair_event_new [=[            {
                UNIT *rustair_event_unit = sim_clock_queue;
                DEVICE *rustair_event_device = rustair_event_unit
                    ? find_dev_from_unit(rustair_event_unit)
                    : NULL;

                reason = sim_process_event();
                if (reason != SCPE_OK) {
                    sim_printf(
                        "RUSTAIR_EVENT_TRACE: status=%d pc=%05X simtime=%.0f qcount=%d unit=%s device=%s\n",
                        (int)reason,
                        (unsigned int)PC,
                        sim_gtime(),
                        (int)sim_qcount(),
                        rustair_event_unit ? sim_uname(rustair_event_unit) : "<none>",
                        rustair_event_device ? sim_dname(rustair_event_device) : "<none>");
                    break;
                }
            }]=])

    string(FIND "${rustair_cpu_contents}" "${rustair_event_old}" rustair_event_first)
    if(rustair_event_first EQUAL -1)
        message(FATAL_ERROR
            "RusTair could not find sim_process_event() dispatch in ${context}. "
            "Open-SIMH may have changed; review the event-stop diagnostic patch.")
    endif()
    string(LENGTH "${rustair_event_old}" rustair_event_len)
    math(EXPR rustair_event_after "${rustair_event_first} + ${rustair_event_len}")
    string(SUBSTRING "${rustair_cpu_contents}" ${rustair_event_after} -1 rustair_event_tail)
    string(FIND "${rustair_event_tail}" "${rustair_event_old}" rustair_event_second)
    if(NOT rustair_event_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair found more than one sim_process_event() dispatch in ${context}. "
            "Open-SIMH may have changed; review the event-stop diagnostic patch.")
    endif()

    string(REPLACE "${rustair_event_old}" "${rustair_event_new}"
        rustair_cpu_contents "${rustair_cpu_contents}")
    file(WRITE "${output_path}" "${rustair_cpu_contents}")
endfunction()

function(rustair_prepare_altairz80_event_sources cpu_output_var nommu_output_var)
    set(rustair_altairz80_source_dir "${CMAKE_SOURCE_DIR}/AltairZ80")
    set(rustair_altairz80_private_dir "${CMAKE_BINARY_DIR}/rustair-altairz80-src")
    set(rustair_altairz80_cpu "${rustair_altairz80_private_dir}/altairz80_cpu.c")
    set(rustair_altairz80_nommu "${rustair_altairz80_private_dir}/altairz80_cpu_nommu.c")

    file(MAKE_DIRECTORY "${rustair_altairz80_private_dir}")
    rustair_patch_altairz80_event_source(
        "${rustair_altairz80_source_dir}/altairz80_cpu.c"
        "${rustair_altairz80_cpu}"
        "AltairZ80/altairz80_cpu.c")
    rustair_patch_altairz80_event_source(
        "${rustair_altairz80_source_dir}/altairz80_cpu_nommu.c"
        "${rustair_altairz80_nommu}"
        "AltairZ80/altairz80_cpu_nommu.c")

    set(${cpu_output_var} "${rustair_altairz80_cpu}" PARENT_SCOPE)
    set(${nommu_output_var} "${rustair_altairz80_nommu}" PARENT_SCOPE)
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
        message(STATUS
            "RusTair: applied classic Altair FrontPanel EXAMINE parser compatibility patch in build tree")
    endif()

    if(TARGET altairz80)
        rustair_prepare_altairz80_event_sources(
            rustair_altairz80_cpu rustair_altairz80_nommu)

        # The simulator target was already created in AltairZ80/CMakeLists.txt.
        # Mark its two upstream CPU source entries header-only in the target's
        # directory scope, then compile the patched build-tree copies instead.
        # The Open-SIMH checkout remains byte-for-byte untouched.
        set_source_files_properties(
            "${CMAKE_SOURCE_DIR}/AltairZ80/altairz80_cpu.c"
            "${CMAKE_SOURCE_DIR}/AltairZ80/altairz80_cpu_nommu.c"
            TARGET_DIRECTORY altairz80
            PROPERTIES HEADER_FILE_ONLY TRUE)
        target_sources(altairz80 PRIVATE
            "${rustair_altairz80_cpu}"
            "${rustair_altairz80_nommu}")
        target_include_directories(altairz80 PRIVATE
            "${CMAKE_SOURCE_DIR}/AltairZ80")

        message(STATUS
            "RusTair: instrumented private AltairZ80 CPU sources for event-stop tracing")
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
