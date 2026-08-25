# Temporary RusTair diagnostic instrumentation for the Open-SIMH scheduler.
#
# This file is included by rustair-frontpanel.cmake and patches only build-tree
# copies of Open-SIMH sources used by the simulator core linked into altairz80.
# The Open-SIMH checkout itself is never modified.
#
# Besides tracing scheduler stops, this diagnostic tests one concrete upstream
# lifecycle hypothesis: sim_timer_precalibrate_execution_rate() owns a local
# stop UNIT but does not cancel it explicitly before returning.  If sim_instr()
# returns for a different reason during the final calibration iteration, that
# stack-local UNIT can remain queued and later stop normal simulator execution.

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR "RusTair stop trace requires CMake 3.19 or newer")
endif()

function(rustair_trace_replace_unique contents_var old_var new_var context output_var)
    set(rustair_contents "${${contents_var}}")
    set(rustair_old "${${old_var}}")
    set(rustair_new "${${new_var}}")

    string(FIND "${rustair_contents}" "${rustair_old}" rustair_first)
    if(rustair_first EQUAL -1)
        message(FATAL_ERROR
            "RusTair stop trace could not find ${context}. "
            "Open-SIMH may have changed; review the diagnostic patch.")
    endif()

    string(LENGTH "${rustair_old}" rustair_old_len)
    math(EXPR rustair_after_first "${rustair_first} + ${rustair_old_len}")
    string(SUBSTRING "${rustair_contents}" ${rustair_after_first} -1 rustair_tail)
    string(FIND "${rustair_tail}" "${rustair_old}" rustair_second)
    if(NOT rustair_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair stop trace found multiple ${context} sites; refusing an ambiguous patch.")
    endif()

    string(REPLACE "${rustair_old}" "${rustair_new}" rustair_contents "${rustair_contents}")
    set(${output_var} "${rustair_contents}" PARENT_SCOPE)
endfunction()

function(rustair_prepare_stop_trace_source output_var)
    set(rustair_source "${CMAKE_SOURCE_DIR}/scp.c")
    set(rustair_dir "${CMAKE_BINARY_DIR}/rustair-stop-trace-src")
    set(rustair_patched "${rustair_dir}/scp.c")

    file(READ "${rustair_source}" rustair_contents)
    string(REPLACE "\r\n" "\n" rustair_contents "${rustair_contents}")

    set(rustair_old_globals [=[volatile t_bool stop_cpu = FALSE;
volatile t_bool sigterm_received = FALSE;]=])
    set(rustair_new_globals [=[volatile t_bool stop_cpu = FALSE;
static volatile int rustair_stop_signal = 0;
extern UNIT sim_stop_unit;
extern t_stat sim_timer_stop_svc (UNIT *uptr);
volatile t_bool sigterm_received = FALSE;]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_globals rustair_new_globals
        "stop_cpu globals in scp.c" rustair_contents)

    set(rustair_old_entry [=[if (stop_cpu) {                                         /* stop CPU? */
    stop_cpu = 0;
    return SCPE_STOP;
    }]=])
    set(rustair_new_entry [=[if (stop_cpu) {                                         /* stop CPU? */
    sim_debug (SIM_DBG_EVENT, &sim_scp_dev,
        "RUSTAIR STOP TRACE: stop_cpu already set on sim_process_event entry; signal=%d next='%s'\n",
        rustair_stop_signal,
        (sim_clock_queue != QUEUE_LIST_END) ? sim_uname (sim_clock_queue) : "<empty>");
    rustair_stop_signal = 0;
    stop_cpu = 0;
    return SCPE_STOP;
    }]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_entry rustair_new_entry
        "sim_process_event entry stop_cpu check in scp.c" rustair_contents)

    set(rustair_old_dispatch [=[    else {
        sim_debug (SIM_DBG_EVENT, &sim_scp_dev, "Processing Event for %s\n", sim_uname (uptr));
        if (uptr->action != NULL)
            reason = uptr->action (uptr);
        else
            reason = SCPE_OK;
        }]=])
    set(rustair_new_dispatch [=[    else {
        DEVICE *rustair_dptr = find_dev_from_unit (uptr);
        int rustair_unit_index = rustair_dptr ? (int)(uptr - rustair_dptr->units) : -1;
        sim_debug (SIM_DBG_EVENT, &sim_scp_dev,
            "RUSTAIR EVENT TRACE: unit='%s' device='%s' index=%d flags=0x%08X dynflags=0x%08X wait=%d timer_stop_action=%d global_stop_unit=%d\n",
            sim_uname (uptr),
            rustair_dptr ? sim_dname (rustair_dptr) : "<none>",
            rustair_unit_index,
            (unsigned int)uptr->flags,
            (unsigned int)uptr->dynflags,
            uptr->wait,
            uptr->action == &sim_timer_stop_svc,
            uptr == &sim_stop_unit);
        sim_debug (SIM_DBG_EVENT, &sim_scp_dev, "Processing Event for %s\n", sim_uname (uptr));
        if (uptr->action != NULL)
            reason = uptr->action (uptr);
        else
            reason = SCPE_OK;
        if ((reason != SCPE_OK) || stop_cpu)
            sim_debug (SIM_DBG_EVENT, &sim_scp_dev,
                "RUSTAIR ACTION TRACE: unit='%s' device='%s' index=%d reason=%d stop_cpu=%d signal=%d timer_stop_action=%d global_stop_unit=%d\n",
                sim_uname (uptr),
                rustair_dptr ? sim_dname (rustair_dptr) : "<none>",
                rustair_unit_index,
                reason,
                stop_cpu,
                rustair_stop_signal,
                uptr->action == &sim_timer_stop_svc,
                uptr == &sim_stop_unit);
        }]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_dispatch rustair_new_dispatch
        "sim_process_event dispatch block in scp.c" rustair_contents)

    set(rustair_old_final [=[if ((reason == SCPE_OK) && stop_cpu) {
    stop_cpu = FALSE;
    reason = SCPE_STOP;
    }]=])
    set(rustair_new_final [=[if ((reason == SCPE_OK) && stop_cpu) {
    sim_debug (SIM_DBG_EVENT, &sim_scp_dev,
        "RUSTAIR STOP TRACE: converting stop_cpu to SCPE_STOP; signal=%d next='%s'\n",
        rustair_stop_signal,
        (sim_clock_queue != QUEUE_LIST_END) ? sim_uname (sim_clock_queue) : "<empty>");
    rustair_stop_signal = 0;
    stop_cpu = FALSE;
    reason = SCPE_STOP;
    }]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_final rustair_new_final
        "sim_process_event final stop_cpu conversion in scp.c" rustair_contents)

    set(rustair_old_signal [=[void int_handler (int sig)
{
stop_cpu = TRUE;
if (sig == SIGTERM)
    sigterm_received = TRUE;
sim_interval = 0;               /* Minimize when stop_cpu gets noticed */
}]=])
    set(rustair_new_signal [=[void int_handler (int sig)
{
rustair_stop_signal = sig;
stop_cpu = TRUE;
if (sig == SIGTERM)
    sigterm_received = TRUE;
sim_interval = 0;               /* Minimize when stop_cpu gets noticed */
}]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_signal rustair_new_signal
        "int_handler in scp.c" rustair_contents)

    file(MAKE_DIRECTORY "${rustair_dir}")
    file(WRITE "${rustair_patched}" "${rustair_contents}")
    set(${output_var} "${rustair_patched}" PARENT_SCOPE)
endfunction()

function(rustair_prepare_timer_precalibration_source output_var)
    set(rustair_source "${CMAKE_SOURCE_DIR}/sim_timer.c")
    set(rustair_dir "${CMAKE_BINARY_DIR}/rustair-stop-trace-src")
    set(rustair_patched "${rustair_dir}/sim_timer.c")

    file(READ "${rustair_source}" rustair_contents)
    string(REPLACE "\r\n" "\n" rustair_contents "${rustair_contents}")

    set(rustair_old_cleanup [=[    } while ((end - start) < SIM_PRE_CALIBRATE_MIN_MS);
sim_precalibrate_ips = (int32)(1000.0 * (sim_precalibrate_ips / (double)(end - start)));]=])
    set(rustair_new_cleanup [=[    } while ((end - start) < SIM_PRE_CALIBRATE_MIN_MS);
/* RusTair diagnostic: guarantee the stack-local stop UNIT cannot survive
   precalibration if sim_instr() returned for some other stop condition. */
sim_cancel (&precalib_unit);
sim_precalibrate_ips = (int32)(1000.0 * (sim_precalibrate_ips / (double)(end - start)));]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_cleanup rustair_new_cleanup
        "precalibration cleanup in sim_timer.c" rustair_contents)

    file(MAKE_DIRECTORY "${rustair_dir}")
    file(WRITE "${rustair_patched}" "${rustair_contents}")
    set(${output_var} "${rustair_patched}" PARENT_SCOPE)
endfunction()

function(rustair_apply_stop_trace)
    if(NOT TARGET altairz80)
        message(FATAL_ERROR "RusTair stop trace ran before the altairz80 target existed")
    endif()

    rustair_prepare_stop_trace_source(rustair_patched_scp)
    rustair_prepare_timer_precalibration_source(rustair_patched_timer)

    # altairz80 links one of Open-SIMH's simhcore* static libraries. Find the
    # actual linked target and replace only its private build-tree copies.
    get_target_property(rustair_altairz80_links altairz80 LINK_LIBRARIES)
    set(rustair_scp_replaced 0)
    set(rustair_timer_replaced 0)
    foreach(rustair_link IN LISTS rustair_altairz80_links)
        if(TARGET "${rustair_link}")
            get_target_property(rustair_sources "${rustair_link}" SOURCES)
            if(rustair_sources)
                list(FIND rustair_sources "${CMAKE_SOURCE_DIR}/scp.c" rustair_scp_index)
                if(NOT rustair_scp_index EQUAL -1)
                    list(REMOVE_AT rustair_sources ${rustair_scp_index})
                    list(INSERT rustair_sources ${rustair_scp_index} "${rustair_patched_scp}")
                    math(EXPR rustair_scp_replaced "${rustair_scp_replaced} + 1")
                endif()

                list(FIND rustair_sources "${CMAKE_SOURCE_DIR}/sim_timer.c" rustair_timer_index)
                if(NOT rustair_timer_index EQUAL -1)
                    list(REMOVE_AT rustair_sources ${rustair_timer_index})
                    list(INSERT rustair_sources ${rustair_timer_index} "${rustair_patched_timer}")
                    math(EXPR rustair_timer_replaced "${rustair_timer_replaced} + 1")
                endif()

                set_property(TARGET "${rustair_link}" PROPERTY SOURCES "${rustair_sources}")
                if((NOT rustair_scp_index EQUAL -1) OR (NOT rustair_timer_index EQUAL -1))
                    message(STATUS
                        "RusTair: stop/precalibration diagnostic injected into ${rustair_link} for altairz80")
                endif()
            endif()
        endif()
    endforeach()

    if(NOT rustair_scp_replaced EQUAL 1 OR NOT rustair_timer_replaced EQUAL 1)
        message(FATAL_ERROR
            "RusTair expected exactly one altairz80 simhcore target containing both scp.c and sim_timer.c, "
            "but replaced scp=${rustair_scp_replaced}, sim_timer=${rustair_timer_replaced}. "
            "Review Open-SIMH target wiring.")
    endif()
endfunction()

get_property(rustair_stop_trace_scheduled GLOBAL PROPERTY RUSTAIR_STOP_TRACE_SCHEDULED)
if(NOT rustair_stop_trace_scheduled)
    set_property(GLOBAL PROPERTY RUSTAIR_STOP_TRACE_SCHEDULED TRUE)
    cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_apply_stop_trace)
endif()
