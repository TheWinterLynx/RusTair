# Accuracy and validation record

## Scope and honest acceptance status

The model is an early original 8800 exterior/front-panel reconstruction, based
on the documented unexpanded #220081K. The user requested museum-grade, exact
1:1 fidelity. **That acceptance threshold is not met**: no dimensioned factory
enclosure drawing, measured specimen, calibrated color sample or original
switch mechanical drawing was available to this build.

The geometry uses a provisional nominal **431.8 mm width × 457.2 mm cabinet
depth × 177.8 mm cabinet height**, excluding the feet and rear cord projection.
These numbers are modelling parameters, not measurements of the reference unit.
The CHM record for a different Altair specimen lists 7 × 16¾ × 17¾ inches;
different published records cannot be used to certify this specimen's size.

## Required 14-point review

| Item | Evidence and result | Acceptance |
|---|---|---|
| 1. Chassis dimensions | Nominal parameters above; metre mesh units and millimetre display. Specimen dimensions unresolved. | Provisional |
| 2. Front-panel dimensions | Dress plate inferred from photographed frame margins; 416.8 × 162.8 mm, 1.6 mm assumed thickness. | Provisional |
| 3. LED count | 36: 16 address, 8 data, 8 status plus INTE, PROT, WAIT, HLDA. MITS manual and parts list agree. | Verified count |
| 4. Switch count | 25: 16 address/data, power, 8 momentary controls. 17 maintained + 8 momentary in parts list. | Verified count |
| 5. LED positions | Centre positions digitized from the early-unit photograph. Slight perspective/lens error remains; no millimetre drawing. | Photo-grounded |
| 6. Switch positions | Same digitized source; irregular octal group spacing preserved. | Photo-grounded |
| 7. Printed labels | Actual early labels, paired controls and WO legend reproduced. AUX labels remain AUX on both controls. | Wording checked |
| 8. Separator lines | Early straight SENSE guide, dashed address/data guides, STATUS underline and octal bars. | Photo-grounded |
| 9. Typography | Main wordmark traced as geometric contours. Small type is Nimbus Sans Bold, a substitute, not verified original artwork. MITS arrow reconstructed. | Incomplete exactness |
| 10. Switch orientations | UP +1, DOWN -1, center for 8 control switches; OFF above ON. Single-step down function left unassigned. | Layout checked; travel provisional |
| 11. Colors | Blue frame/tray, warm-grey cover, charcoal dress panel, ivory ink. Chosen visually from uncalibrated photos. | Provisional |
| 12. Fasteners | Rear and side locations photograph-derived; front mounting screws sit behind dress panel. Thread and head dimensions estimated. | Photo-grounded, unmeasured |
| 13. Front/side/perspective | Orthographic front/right/rear and perspective views rendered for visual QA. Side vents need measured count/pitch confirmation. | Visual QA only |
| 14. Pivots, animation, LED control | Each lever has a fixed local pivot and driver. Supported states checked for no pivot translation. 36 unique LED materials and one-at-a-time states checked. | Functional checks; physical pivot unmeasured |

## Mechanical assumptions that prevent museum-grade certification

- Cover/tray thickness is 1.7 mm; dress panel 1.6 mm and mounting plate 2 mm.
- Lever pivot is 3.2 mm outward from dress-panel plane; travel is ±20 degrees.
- LED lens body is 4.96 mm diameter; dome and projection are visually estimated.
- Switch bat shape, washer, nut, bushing thread and hidden body dimensions are
  visually estimated, not verified against a drawing for the ST-1 series.
- The side cover has 40 real slots per side. The available oblique photograph
  does not prove that exact count; this is explicitly provisional.
- The cover is represented by separate editable sheet objects. Exact bends,
  concealed attachment details and extrusion cross-sections are not replicated.
- The rear open grille and rectangular connector apertures follow the early
  specimen. Grille pitch and screw dimensions are estimated.
- The rear sticker identifies the reference specimen as 220081K. Its small
  typography is reconstructed, not an exact scan of the original sticker.
- Internal electronics are not reconstructed. Only structural mounting parts
  are included. A full original power supply/backplane/board model needs an
  additional source-grounded task.

## What would close the accuracy gap

Measure the selected specimen or provide its dimensioned drawings: envelope,
front extrusion profile, dress plate and mounting plate dimensions, every hole
centre, slot count/pitch, sheet thicknesses, and switch mechanical part drawings.
A straight-on high-resolution scan of the bare dress panel would allow the
remaining typography and artwork to be traced exactly. Calibrated neutral-light
photos with a color target or original paint specifications are needed for
reliable PBR colors. The parameterized source and clean component hierarchy
allow those corrections without rebuilding the interaction system.

## Iterations completed

The first build was inspected and corrected: mitred frame members replaced
square joins; overlapping front/side surfaces were removed; switch mounting
hardware was recessed behind the dress panel; small-label cap heights were
adjusted; exposure and material colors were revised; the rear layout was
corrected for the opposite viewing direction; and grille coverage was extended
to the full openings. LED/lever pole vertices were welded.

The final export contains 620 nodes, 584 meshes, 45 materials and 389,705
triangles. All 25 switch pivots and fixed nuts passed the supported-state tests.
All 36 LEDs passed one-at-a-time shader emission tests. glTF names, properties,
parent hierarchy and font/graphic geometry are present. Studio elements are
excluded from the export. The runtime adapter was exercised using the exported
node hierarchy and material stubs; it has not been integrated into a live emulator.

These checks establish asset structure and control behavior. They do not certify
the historical accuracy of provisional physical measurements.
