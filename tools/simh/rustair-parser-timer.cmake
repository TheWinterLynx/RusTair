# Minimal compatibility injection used to test the two functional RusTair
# Open-SIMH compatibility patches together, without any diagnostic tracing.
#
# - rustair-frontpanel.cmake: FrontPanel EXAMINE parser compatibility
# - rustair-timer-stop-guard.cmake: guard the default zero timer stop time
#
# No SCP scheduler tracing or M2SIO RX tracing is included here.
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-timer-stop-guard.cmake")
