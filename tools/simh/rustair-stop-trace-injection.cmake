# One-off Open-SIMH diagnostic injection for the AltairZ80 timer-stop and
# M2SIO receive investigations. Keep the normal FrontPanel compatibility/build
# injection, retain the scheduler stop trace, apply the timer guard, and finally
# replace only AltairZ80's s100_2sio.c with a focused TMXR RX state trace.
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-stop-trace.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-timer-stop-guard.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-m2sio-rx-trace.cmake")
