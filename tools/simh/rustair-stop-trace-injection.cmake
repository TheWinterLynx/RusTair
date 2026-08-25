# One-off Open-SIMH diagnostic injection for the AltairZ80 timer-stop investigation.
# Keep the normal FrontPanel compatibility/build injection, retain the private
# scheduler trace, and apply the diagnostic-only timer guard last so its clean
# sim_timer.c copy replaces the earlier disproved precalibration experiment.
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-stop-trace.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-timer-stop-guard.cmake")
