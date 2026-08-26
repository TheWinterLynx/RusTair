# Focused Open-SIMH diagnostic injection for the AltairZ80 M2SIO receive path.
#
# This intentionally excludes rustair-stop-trace.cmake: SCP-PROCESS/EVENT
# tracing is extremely noisy and slows the five-second receive probe by an
# order of magnitude.  At this stage we only need the normal FrontPanel
# compatibility patch, the confirmed timer-stop guard, and focused 88-2SIO RX
# state tracing.
include("${CMAKE_CURRENT_LIST_DIR}/rustair-frontpanel.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-timer-stop-guard.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/rustair-m2sio-rx-trace.cmake")
