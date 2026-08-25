# One-off Open-SIMH diagnostic injection for the AltairZ80 stop_cpu investigation.
# Keep the normal FrontPanel compatibility/build injection and add the private
# scheduler trace without changing the ordinary RusTair SIMH build path.
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-stop-trace.cmake")
