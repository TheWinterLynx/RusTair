# Production compatibility/performance injection for RusTair's validated
# Open-SIMH Windows x64 bundle.
#
# - rustair-frontpanel.cmake: FrontPanel EXAMINE parser compatibility plus
#   RusTair's optional halted sim> extension / FrontPanel startup port fix.
# - rustair-frontpanel-command-buffer.cmake: allow RusTair's <4 KiB batched
#   EXECUTE/DEPOSIT commands on MSVC without _vsnprintf truncation failures.
# - rustair-timer-stop-guard.cmake: guard the default zero timer stop time on
#   the AltairZ80 FrontPanel/M2SIO path.
# - rustair-timer-startup-latency.cmake: retain Open-SIMH's host Sleep()
#   calibration algorithm but reduce its 100+100 startup samples to 16+16.
#
# No SCP scheduler tracing or M2SIO RX diagnostic tracing is included here.
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel-command-buffer.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-timer-stop-guard.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-timer-startup-latency.cmake")
