# Diagnostic-only guard for an Open-SIMH timer edge case exposed by
# FrontPanel/REMOTE MASTER startup.
#
# sim_timer_stop_time defaults to 0.  During FrontPanel startup sim_gtime()
# can temporarily be negative.  The upstream sim_start_timer_services() check
# then interprets the default 0 as a future requested stop and schedules the
# global sim_stop_unit, which later returns SCPE_STOP.
#
# This file patches only a build-tree copy of sim_timer.c.  It is intentionally
# included only by rustair-stop-trace-injection.cmake until the direct M2SIO
# diagnostic confirms the diagnosis.

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR "RusTair timer stop guard requires CMake 3.19 or newer")
endif()

function(rustair_prepare_timer_stop_guard_source output_var)
    set(rustair_source "${CMAKE_SOURCE_DIR}/sim_timer.c")
    set(rustair_dir "${CMAKE_BINARY_DIR}/rustair-timer-stop-guard-src")
    set(rustair_patched "${rustair_dir}/sim_timer.c")

    file(READ "${rustair_source}" rustair_contents)
    string(REPLACE "\r\n" "\n" rustair_contents "${rustair_contents}")

    set(rustair_old [=[if (sim_timer_stop_time > sim_gtime())
    sim_activate_abs (&sim_stop_unit, (int32)(sim_timer_stop_time - sim_gtime()));]=])
    set(rustair_new [=[if ((sim_timer_stop_time != 0.0) &&
    (sim_timer_stop_time > sim_gtime()))
    sim_activate_abs (&sim_stop_unit, (int32)(sim_timer_stop_time - sim_gtime()));]=])

    string(FIND "${rustair_contents}" "${rustair_old}" rustair_first)
    if(rustair_first EQUAL -1)
        message(FATAL_ERROR
            "RusTair timer stop guard could not find the expected sim_start_timer_services() stop scheduling block. "
            "Open-SIMH may have changed; review the compatibility guard.")
    endif()

    string(LENGTH "${rustair_old}" rustair_old_len)
    math(EXPR rustair_after_first "${rustair_first} + ${rustair_old_len}")
    string(SUBSTRING "${rustair_contents}" ${rustair_after_first} -1 rustair_tail)
    string(FIND "${rustair_tail}" "${rustair_old}" rustair_second)
    if(NOT rustair_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair timer stop guard found multiple matching scheduling blocks; refusing an ambiguous patch.")
    endif()

    string(REPLACE "${rustair_old}" "${rustair_new}" rustair_contents "${rustair_contents}")
    file(MAKE_DIRECTORY "${rustair_dir}")
    file(WRITE "${rustair_patched}" "${rustair_contents}")
    set(${output_var} "${rustair_patched}" PARENT_SCOPE)
endfunction()

function(rustair_apply_timer_stop_guard)
    if(NOT TARGET altairz80)
        message(FATAL_ERROR "RusTair timer stop guard ran before the altairz80 target existed")
    endif()

    rustair_prepare_timer_stop_guard_source(rustair_guarded_timer)

    get_target_property(rustair_altairz80_links altairz80 LINK_LIBRARIES)
    set(rustair_replaced 0)
    foreach(rustair_link IN LISTS rustair_altairz80_links)
        if(TARGET "${rustair_link}")
            get_target_property(rustair_sources "${rustair_link}" SOURCES)
            if(rustair_sources)
                # The stop-trace diagnostic may already have replaced sim_timer.c
                # with its own build-tree copy.  Replace either form with this
                # clean source+guard copy so the disproved precalibration patch
                # is not present in the executable under test.
                set(rustair_timer_index -1)
                list(FIND rustair_sources "${CMAKE_SOURCE_DIR}/sim_timer.c" rustair_timer_index)
                if(rustair_timer_index EQUAL -1)
                    list(FIND rustair_sources "${CMAKE_BINARY_DIR}/rustair-stop-trace-src/sim_timer.c" rustair_timer_index)
                endif()

                if(NOT rustair_timer_index EQUAL -1)
                    list(REMOVE_AT rustair_sources ${rustair_timer_index})
                    list(INSERT rustair_sources ${rustair_timer_index} "${rustair_guarded_timer}")
                    set_property(TARGET "${rustair_link}" PROPERTY SOURCES "${rustair_sources}")
                    math(EXPR rustair_replaced "${rustair_replaced} + 1")
                    message(STATUS
                        "RusTair: diagnostic timer stop guard injected into ${rustair_link} for altairz80")
                endif()
            endif()
        endif()
    endforeach()

    if(NOT rustair_replaced EQUAL 1)
        message(FATAL_ERROR
            "RusTair expected exactly one altairz80 simhcore target containing sim_timer.c, "
            "but replaced ${rustair_replaced}. Review Open-SIMH target wiring.")
    endif()
endfunction()

get_property(rustair_timer_stop_guard_scheduled GLOBAL PROPERTY RUSTAIR_TIMER_STOP_GUARD_SCHEDULED)
if(NOT rustair_timer_stop_guard_scheduled)
    set_property(GLOBAL PROPERTY RUSTAIR_TIMER_STOP_GUARD_SCHEDULED TRUE)
    # This include comes after rustair-stop-trace.cmake, so this deferred call
    # runs after the trace source substitution and intentionally wins for
    # sim_timer.c while preserving the scp.c scheduler instrumentation.
    cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_apply_timer_stop_guard)
endif()
