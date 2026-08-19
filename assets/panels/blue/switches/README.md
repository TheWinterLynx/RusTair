# Separate front-panel switch sprites

These PNGs are loaded individually by RusTair; there is no runtime switch atlas.

- SENSE A15-A8: red `up` / `down`
- SENSE A7-A0: white `up` / `down`
- POWER OFF/ON: white `up` / `down`
- STOP/RUN, SINGLE STEP, EXAMINE, DEPOSIT, RESET, PROTECT: blue `up` / `center` / `down`
- AUX: grey `up` / `center` / `down`

All files keep the original 1254x1254 transparent canvas supplied in `switches.zip`, so RusTair changes only the selected texture between states and never applies state-dependent scaling.
