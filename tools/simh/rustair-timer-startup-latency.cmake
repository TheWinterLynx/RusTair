# Reduce only Open-SIMH's host Sleep() calibration sample count for the
# RusTair Windows bundle.
#
# Upstream sim_timer.c measures minimum host sleep granularity with 100 calls
# at one delay and another 100 calls at the next delay. On Windows hosts where
# Sleep(1) resolves near the legacy ~15.6 ms tick, those 200 samples alone can
# consume roughly 4-5 seconds before the Remote Master console can accept the
# FrontPanel connection.
#
# RusTair keeps the upstream calibration algorithm and all timer/scheduler
# semantics, but 16 samples are sufficient to classify host sleep granularity
# for this purpose. This patch touches build-tree copies only and is deliberately
# source-shape-sensitive so an upstream change fails loudly.

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR "RusTair timer startup calibration patch requires CMake 3.19 or newer")
endif()

function(rustair_prepare_timer_startup_source input_source target_name output_var)
    set(rustair_dir "${CMAKE_BINARY_DIR}/rustair-timer-startup-src/${target_name}")
    set(rustair_patched "${rustair_dir}/sim_timer.c")

    file(READ "${input_source}" rustair_contents)
    string(REPLACE "\r\n" "\n" rustair_contents "${rustair_contents}")

    set(rustair_old "#define sleep1Samples       100")
    set(rustair_new "#define sleep1Samples       16")
    string(FIND "${rustair_contents}" "${rustair_old}" rustair_first)
    if(rustair_first EQUAL -1)
        message(FATAL_ERROR
            "RusTair could not find Open-SIMH's expected sleep1Samples=100 definition in ${input_source}. "
            "Review the startup calibration patch for this upstream revision.")
    endif()

    string(LENGTH "${rustair_old}" rustair_old_len)
    math(EXPR rustair_after_first "${rustair_first} + ${rustair_old_len}")
    string(SUBSTRING "${rustair_contents}" ${rustair_after_first} -1 rustair_tail)
    string(FIND "${rustair_tail}" "${rustair_old}" rustair_second)
    if(NOT rustair_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair found multiple sleep1Samples definitions; refusing an ambiguous timer patch.")
    endif()

    string(REPLACE "${rustair_old}" "${rustair_new}" rustair_contents "${rustair_contents}")
    file(MAKE_DIRECTORY "${rustair_dir}")
    file(WRITE "${rustair_patched}" "${rustair_contents}")
    set(${output_var} "${rustair_patched}" PARENT_SCOPE)
endfunction()

function(rustair_tune_timer_core_for_target simulator_target processed_property)
    if(NOT TARGET "${simulator_target}")
        message(FATAL_ERROR "RusTair timer startup patch ran before ${simulator_target} existed")
    endif()

    get_target_property(rustair_links "${simulator_target}" LINK_LIBRARIES)
    set(rustair_found 0)
    foreach(rustair_link IN LISTS rustair_links)
        if(NOT TARGET "${rustair_link}")
            continue()
        endif()

        get_target_property(rustair_sources "${rustair_link}" SOURCES)
        if(NOT rustair_sources)
            continue()
        endif()

        set(rustair_timer_index -1)
        set(rustair_timer_source "")
        set(rustair_index 0)
        foreach(rustair_source IN LISTS rustair_sources)
            get_filename_component(rustair_name "${rustair_source}" NAME)
            if(rustair_name STREQUAL "sim_timer.c")
                if(NOT rustair_timer_index EQUAL -1)
                    message(FATAL_ERROR
                        "RusTair found more than one sim_timer.c source in ${rustair_link}")
                endif()
                set(rustair_timer_index ${rustair_index})
                set(rustair_timer_source "${rustair_source}")
            endif()
            math(EXPR rustair_index "${rustair_index} + 1")
        endforeach()

        if(rustair_timer_index EQUAL -1)
            continue()
        endif()

        math(EXPR rustair_found "${rustair_found} + 1")
        get_property(rustair_processed GLOBAL PROPERTY "${processed_property}_${rustair_link}")
        if(rustair_processed)
            continue()
        endif()

        rustair_prepare_timer_startup_source(
            "${rustair_timer_source}" "${rustair_link}" rustair_tuned_timer)
        list(REMOVE_AT rustair_sources ${rustair_timer_index})
        list(INSERT rustair_sources ${rustair_timer_index} "${rustair_tuned_timer}")
        set_property(TARGET "${rustair_link}" PROPERTY SOURCES "${rustair_sources}")
        set_property(GLOBAL PROPERTY "${processed_property}_${rustair_link}" TRUE)
        message(STATUS
            "RusTair: reduced Open-SIMH host sleep calibration samples 100 -> 16 in ${rustair_link} for ${simulator_target}")
    endforeach()

    if(NOT rustair_found EQUAL 1)
        message(FATAL_ERROR
            "RusTair expected exactly one linked simulator core containing sim_timer.c for ${simulator_target}, found ${rustair_found}.")
    endif()
endfunction()

function(rustair_apply_timer_startup_latency_patch)
    # rustair-timer-stop-guard.cmake is included first. For AltairZ80 this
    # therefore reads and preserves the already guarded build-tree sim_timer.c,
    # then applies only the sample-count change on top of it.
    rustair_tune_timer_core_for_target(altair RUSTAIR_TIMER_STARTUP_PATCHED)
    rustair_tune_timer_core_for_target(altairz80 RUSTAIR_TIMER_STARTUP_PATCHED)
endfunction()

get_property(rustair_timer_startup_scheduled GLOBAL PROPERTY RUSTAIR_TIMER_STARTUP_SCHEDULED)
if(NOT rustair_timer_startup_scheduled)
    set_property(GLOBAL PROPERTY RUSTAIR_TIMER_STARTUP_SCHEDULED TRUE)
    cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_apply_timer_startup_latency_patch)
endif()
