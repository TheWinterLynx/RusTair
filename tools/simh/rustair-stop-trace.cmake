# Temporary RusTair diagnostic instrumentation for the Open-SIMH scheduler.
#
# This file is included by rustair-frontpanel.cmake and patches only the
# build-tree copy of scp.c used by the simulator core linked into altairz80.
# The Open-SIMH checkout itself is never modified.
#
# The trace answers one narrow question: where does the simulation stop come
# from? It records:
#   - signals delivered through int_handler();
#   - the UNIT/device identity for every dispatched event while SCP EVENT
#     debugging is enabled;
#   - the status returned by each event action;
#   - whether stop_cpu was already set on entry to sim_process_event();
#   - the final stop_cpu -> SCPE_STOP conversion.

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
            "RusTair stop trace could not find ${context} in scp.c. "
            "Open-SIMH may have changed; review the diagnostic patch.")
    endif()

    string(LENGTH "${rustair_old}" rustair_old_len)
    math(EXPR rustair_after_first "${rustair_first} + ${rustair_old_len}")
    string(SUBSTRING "${rustair_contents}" ${rustair_after_first} -1 rustair_tail)
    string(FIND "${rustair_tail}" "${rustair_old}" rustair_second)
    if(NOT rustair_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair stop trace found multiple ${context} sites in scp.c; refusing an ambiguous patch.")
    endif()

    string(REPLACE "${rustair_old}" "${rustair_new}" rustair_contents "${rustair_contents}")
    set(${output_var} "${rustair_contents}" PARENT_SCOPE)
endfunction()

function(rustair_prepare_stop_trace_source output_var)
    set(rustair_source "${CMAKE_SOURCE_DIR}/scp.c")
    set(rustair_dir "${CMAKE_BINARY_DIR}/rustair-stop-trace-src")
    set(rustair_patched "${rustair_dir}/scp.c")

    file(READ "${rustair_source}" rustair_contents)
    # Work with deterministic LF text regardless of core.autocrlf in the local
    # Open-SIMH checkout.  The patched file lives only in the build tree.
    string(REPLACE "\r\n" "\n" rustair_contents "${rustair_contents}")

    set(rustair_old_globals [=[volatile t_bool stop_cpu = FALSE;
volatile t_bool sigterm_received = FALSE;]=])
    set(rustair_new_globals [=[volatile t_bool stop_cpu = FALSE;
static volatile int rustair_stop_signal = 0;
volatile t_bool sigterm_received = FALSE;]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_globals rustair_new_globals
        "stop_cpu globals" rustair_contents)

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
        "sim_process_event entry stop_cpu check" rustair_contents)

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
            "RUSTAIR EVENT TRACE: unit='%s' device='%s' index=%d flags=0x%08X dynflags=0x%08X wait=%d\n",
            sim_uname (uptr),
            rustair_dptr ? sim_dname (rustair_dptr) : "<none>",
            rustair_unit_index,
            (unsigned int)uptr->flags,
            (unsigned int)uptr->dynflags,
            uptr->wait);
        sim_debug (SIM_DBG_EVENT, &sim_scp_dev, "Processing Event for %s\n", sim_uname (uptr));
        if (uptr->action != NULL)
            reason = uptr->action (uptr);
        else
            reason = SCPE_OK;
        if ((reason != SCPE_OK) || stop_cpu)
            sim_debug (SIM_DBG_EVENT, &sim_scp_dev,
                "RUSTAIR ACTION TRACE: unit='%s' device='%s' index=%d reason=%d stop_cpu=%d signal=%d\n",
                sim_uname (uptr),
                rustair_dptr ? sim_dname (rustair_dptr) : "<none>",
                rustair_unit_index,
                reason,
                stop_cpu,
                rustair_stop_signal);
        }]=])
    rustair_trace_replace_unique(
        rustair_contents rustair_old_dispatch rustair_new_dispatch
        "sim_process_event dispatch block" rustair_contents)

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
        "sim_process_event final stop_cpu conversion" rustair_contents)

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
        "int_handler" rustair_contents)

    file(MAKE_DIRECTORY "${rustair_dir}")
    file(WRITE "${rustair_patched}" "${rustair_contents}")
    set(${output_var} "${rustair_patched}" PARENT_SCOPE)
endfunction()

function(rustair_apply_stop_trace)
    if(NOT TARGET altairz80)
        message(FATAL_ERROR "RusTair stop trace ran before the altairz80 target existed")
    endif()

    rustair_prepare_stop_trace_source(rustair_patched_scp)

    # altairz80 links one of Open-SIMH's simhcore* static libraries.  Find the
    # actual linked target rather than hard-coding the feature-dependent name,
    # and replace only its scp.c source with the private build-tree copy.
    get_target_property(rustair_altairz80_links altairz80 LINK_LIBRARIES)
    set(rustair_replaced 0)
    foreach(rustair_link IN LISTS rustair_altairz80_links)
        if(TARGET "${rustair_link}")
            get_target_property(rustair_sources "${rustair_link}" SOURCES)
            if(rustair_sources)
                list(FIND rustair_sources "${CMAKE_SOURCE_DIR}/scp.c" rustair_scp_index)
                if(NOT rustair_scp_index EQUAL -1)
                    list(REMOVE_AT rustair_sources ${rustair_scp_index})
                    list(INSERT rustair_sources ${rustair_scp_index} "${rustair_patched_scp}")
                    set_property(TARGET "${rustair_link}" PROPERTY SOURCES "${rustair_sources}")
                    math(EXPR rustair_replaced "${rustair_replaced} + 1")
                    message(STATUS
                        "RusTair: stop diagnostic trace injected into ${rustair_link} for altairz80")
                endif()
            endif()
        endif()
    endforeach()

    if(NOT rustair_replaced EQUAL 1)
        message(FATAL_ERROR
            "RusTair expected exactly one altairz80 simhcore target containing scp.c, "
            "but replaced ${rustair_replaced}. Review Open-SIMH target wiring.")
    endif()
endfunction()

# Avoid duplicate scheduling if CMAKE_PROJECT_INCLUDE causes the parent
# injection file to be evaluated by more than one project() invocation.
get_property(rustair_stop_trace_scheduled GLOBAL PROPERTY RUSTAIR_STOP_TRACE_SCHEDULED)
if(NOT rustair_stop_trace_scheduled)
    set_property(GLOBAL PROPERTY RUSTAIR_STOP_TRACE_SCHEDULED TRUE)
    cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_apply_stop_trace)
endif()
