# Focused diagnostic instrumentation for the Open-SIMH AltairZ80 88-2SIO
# receive path.  This patches only a build-tree copy of s100_2sio.c and emits
# state transitions rather than every scheduler event, so the direct probe can
# distinguish a card-level connection from the underlying TMXR line/socket.

if(CMAKE_VERSION VERSION_LESS 3.19)
    message(FATAL_ERROR "RusTair M2SIO RX trace requires CMake 3.19 or newer")
endif()

function(rustair_m2sio_replace_unique contents_var old_var new_var context output_var)
    set(rustair_contents "${${contents_var}}")
    set(rustair_old "${${old_var}}")
    set(rustair_new "${${new_var}}")

    string(FIND "${rustair_contents}" "${rustair_old}" rustair_first)
    if(rustair_first EQUAL -1)
        message(FATAL_ERROR
            "RusTair M2SIO RX trace could not find ${context}. Open-SIMH may have changed; review the diagnostic patch.")
    endif()

    string(LENGTH "${rustair_old}" rustair_old_len)
    math(EXPR rustair_after_first "${rustair_first} + ${rustair_old_len}")
    string(SUBSTRING "${rustair_contents}" ${rustair_after_first} -1 rustair_tail)
    string(FIND "${rustair_tail}" "${rustair_old}" rustair_second)
    if(NOT rustair_second EQUAL -1)
        message(FATAL_ERROR
            "RusTair M2SIO RX trace found multiple ${context} sites; refusing an ambiguous patch.")
    endif()

    string(REPLACE "${rustair_old}" "${rustair_new}" rustair_contents "${rustair_contents}")
    set(${output_var} "${rustair_contents}" PARENT_SCOPE)
endfunction()

function(rustair_prepare_m2sio_rx_trace_source output_var)
    set(rustair_source "${CMAKE_SOURCE_DIR}/AltairZ80/s100_2sio.c")
    set(rustair_dir "${CMAKE_BINARY_DIR}/rustair-m2sio-rx-trace-src")
    set(rustair_patched "${rustair_dir}/s100_2sio.c")

    file(READ "${rustair_source}" rustair_contents)
    string(REPLACE "\r\n" "\n" rustair_contents "${rustair_contents}")

    set(rustair_old_entry [=[    xptr = (M2SIO_CTX *) uptr->dptr->ctxt;

    /* Check for new incoming connection */]=])
    set(rustair_new_entry [=[    static int rustair_last_card_conn[2] = {-1, -1};
    static int rustair_last_line_conn[2] = {-1, -1};
    static int rustair_last_rcve[2] = {-1, -1};
    static int rustair_last_notelnet[2] = {-1, -1};
    static int32 rustair_last_rxbpi[2] = {-1, -1};
    static int32 rustair_last_rxbpr[2] = {-1, -1};
    static int32 rustair_last_rxcnt[2] = {-1, -1};
    static uint32 rustair_last_rxbps[2] = {0xFFFFFFFFu, 0xFFFFFFFFu};
    int rustair_port;

    xptr = (M2SIO_CTX *) uptr->dptr->ctxt;
    rustair_port = xptr->port;

    /* Check for new incoming connection */]=])
    rustair_m2sio_replace_unique(
        rustair_contents rustair_old_entry rustair_new_entry
        "m2sio_svc() entry" rustair_contents)

    set(rustair_old_poll [=[        if (uptr->flags & UNIT_ATT) {
            tmxr_poll_rx(xptr->tmxr);

            c = tmxr_getc_ln(xptr->tmln);]=])
    set(rustair_new_poll [=[        if (uptr->flags & UNIT_ATT) {
            tmxr_poll_rx(xptr->tmxr);

            if ((rustair_port >= 0) && (rustair_port < 2) &&
                ((rustair_last_card_conn[rustair_port] != (int)xptr->conn) ||
                 (rustair_last_line_conn[rustair_port] != xptr->tmln->conn) ||
                 (rustair_last_rcve[rustair_port] != xptr->tmln->rcve) ||
                 (rustair_last_notelnet[rustair_port] != (int)xptr->tmln->notelnet) ||
                 (rustair_last_rxbpi[rustair_port] != xptr->tmln->rxbpi) ||
                 (rustair_last_rxbpr[rustair_port] != xptr->tmln->rxbpr) ||
                 (rustair_last_rxcnt[rustair_port] != xptr->tmln->rxcnt) ||
                 (rustair_last_rxbps[rustair_port] != xptr->tmln->rxbps))) {
                sim_debug(STATUS_MSG, uptr->dptr,
                    "RUSTAIR RX TRACE: card_conn=%d line_conn=%d sock=%d connecting=%d rcve=%d notelnet=%d rxbpi=%d rxbpr=%d rxcnt=%d rxbps=%u\n",
                    (int)xptr->conn,
                    xptr->tmln->conn,
                    xptr->tmln->sock ? 1 : 0,
                    xptr->tmln->connecting ? 1 : 0,
                    xptr->tmln->rcve,
                    (int)xptr->tmln->notelnet,
                    xptr->tmln->rxbpi,
                    xptr->tmln->rxbpr,
                    xptr->tmln->rxcnt,
                    (unsigned int)xptr->tmln->rxbps);
                rustair_last_card_conn[rustair_port] = (int)xptr->conn;
                rustair_last_line_conn[rustair_port] = xptr->tmln->conn;
                rustair_last_rcve[rustair_port] = xptr->tmln->rcve;
                rustair_last_notelnet[rustair_port] = (int)xptr->tmln->notelnet;
                rustair_last_rxbpi[rustair_port] = xptr->tmln->rxbpi;
                rustair_last_rxbpr[rustair_port] = xptr->tmln->rxbpr;
                rustair_last_rxcnt[rustair_port] = xptr->tmln->rxcnt;
                rustair_last_rxbps[rustair_port] = xptr->tmln->rxbps;
            }

            c = tmxr_getc_ln(xptr->tmln);]=])
    rustair_m2sio_replace_unique(
        rustair_contents rustair_old_poll rustair_new_poll
        "m2sio_svc() receive poll" rustair_contents)

    file(MAKE_DIRECTORY "${rustair_dir}")
    file(WRITE "${rustair_patched}" "${rustair_contents}")
    set(${output_var} "${rustair_patched}" PARENT_SCOPE)
endfunction()

function(rustair_apply_m2sio_rx_trace)
    if(NOT TARGET altairz80)
        message(FATAL_ERROR "RusTair M2SIO RX trace ran before the altairz80 target existed")
    endif()

    rustair_prepare_m2sio_rx_trace_source(rustair_m2sio_source)
    get_target_property(rustair_sources altairz80 SOURCES)

    set(rustair_matches 0)
    set(rustair_index 0)
    foreach(rustair_item IN LISTS rustair_sources)
        get_filename_component(rustair_name "${rustair_item}" NAME)
        if(rustair_name STREQUAL "s100_2sio.c")
            list(REMOVE_AT rustair_sources ${rustair_index})
            list(INSERT rustair_sources ${rustair_index} "${rustair_m2sio_source}")
            math(EXPR rustair_matches "${rustair_matches} + 1")
        endif()
        math(EXPR rustair_index "${rustair_index} + 1")
    endforeach()

    if(NOT rustair_matches EQUAL 1)
        message(FATAL_ERROR
            "RusTair expected exactly one s100_2sio.c source in altairz80, but found ${rustair_matches}.")
    endif()

    set_property(TARGET altairz80 PROPERTY SOURCES "${rustair_sources}")
    message(STATUS "RusTair: focused M2SIO/TMXR RX trace injected into altairz80")
endfunction()

get_property(rustair_m2sio_rx_trace_scheduled GLOBAL PROPERTY RUSTAIR_M2SIO_RX_TRACE_SCHEDULED)
if(NOT rustair_m2sio_rx_trace_scheduled)
    set_property(GLOBAL PROPERTY RUSTAIR_M2SIO_RX_TRACE_SCHEDULED TRUE)
    cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL rustair_apply_m2sio_rx_trace)
endif()
