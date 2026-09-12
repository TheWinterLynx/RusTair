# MITS 88-DCDD / 88-DISK + Pertec FD-400 hardware contract

Status: **Phase 0 source contract PASS; Phase 1 topology PASS; Phase 2 decode/electrical evidence locked pending test gate**

This document is the source-backed hardware contract for the first RusTair implementation of the original Altair 8-inch floppy subsystem. It deliberately separates facts that are sufficiently established for production code from behavior that remains deferred until a later phase has the corresponding schematic/mechanical evidence.

The implementation plan is `docs/88_DCDD_FD400_IMPLEMENTATION_PLAN.md`.

## Primary source set

The first implementation is based on original MITS documentation, not on the behavior of another emulator.

1. **MITS 88-DCDD Operator's Guide, July 1977** — the 20-page operator/programming section archived by DeRamp as `Operator's Guide- 88-DCDD Manual.pdf`. This is the primary source for system topology, drive-address wiring, I/O channel definitions, active polarity, programming-visible status/control bits and read/write timing.
2. **MITS Altair 88-DCDD Disk Drive System / full hardware manual scans** — archived by DeRamp, including the controller-board schematics. This is the primary source for Board #1 / Board #2 wiring and the interrupt-option hardware.
3. **MITS contemporary catalog/literature** — source for the advertised mechanical/data-rate figures: Pertec FD-400, 360 RPM, 10 ms track-to-track access, 250,000 bit/s, 77 tracks, 32 hard sectors, controller capable of up to 16 drives.
4. **MITS Computer Notes controller timing/service material** — useful as secondary primary-source material for component-level timing and later field modifications. It must not silently replace the July 1977 contract; revision differences are to be represented explicitly.
5. **MITS New Write Delay (NWD) material / 1978 Computer Notes** — deferred revision source. NWD will not be folded into the base controller until its exact component/timing changes are represented as a selectable historical revision.

Convenience archive entry points:

- https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-DCDD%20Floppy%20Disk%20Controller/
- https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/30-Disk%20Storage%20Devices/20-88-DCDD%208%20inch%20Floppy%20System/
- https://altairclone.com/altair_manuals.html

## 1. Physical topology and ownership

The July 1977 guide explicitly divides the system as follows.

### Controller Board #1

Board #1 performs the **input-side** functions between the disk system and the Altair bus:

- control/address selection for disk/computer I/O;
- sector circuitry / sector-position presentation;
- read-data circuitry;
- status output.

The block diagram identifies Board #1 separately on the Altair bus and shows a direct controller-board interconnect to Board #2.

Schematic review for Phase 2 further fixes the S-100 ownership used by the implementation: Board #1 performs the fixed I/O decode from the mirrored upper address byte `A8..A15`, observes `sINP`, `sOUT`, `pDBIN`, `pWR` and `pINTE`, and owns the tri-state S-100 data-input drivers. Its decoded output strobes are carried to Board #2 over the controller harness; Board #1 does not acquire a synthetic copy of Board #2's output circuitry.

### Controller Board #2

Board #2 performs the **output-side** functions from the Altair bus:

- disk enable / drive selection;
- write-data circuitry;
- disk-drive control circuitry.

Board #2 is a second physical S-100 card, not a logical half of a synthetic single-slot device. The schematic connects the Altair `DO0..DO7` lines and Power-On Clear to Board #2; the DCL/CD/WDS selection strobes arrive from Board #1 through the inter-board harness rather than by independently decoding the S-100 I/O address on Board #2.

### Board-to-board and controller cable

The installation instructions are explicit:

- Board #2 has a short wired cable which connects to the 20-pin connector on Board #1.
- The controller harness also connects to Board #2 via one 20-pin and one 10-pin connector, and to Board #1 via one 10-pin connector.
- The harness terminates at a 37-pin rear-panel connector.
- The instructions place Board #2 immediately to the left of Board #1 and slide the pair into S-100 connectors together.

RusTair therefore models the board-to-board/controller harness as a **real physical signal bundle**. The two cards must never hold software references to one another. Sharing a harness signal authority is allowed because that represents documented copper. Phase 2 propagates DCL/CD/WDS changes as harness edges, so Board #2 receives the strobe independent of software slot-observation order and samples its own resolved S-100 `DO0..DO7` at that edge; no per-T-state harness polling is introduced.

RusTair requires one Board #1 and one Board #2 as a controller pair. The pair must occupy adjacent fitted S-100 connectors. RusTair does not yet assign historical meaning to increasing-vs-decreasing slot number as physical left/right; only adjacency is claimed until chassis orientation is explicitly documented in the slot model.

### External disk cable and daisy chain

The guide describes the external interconnect as an **18-pair flat cable with 37-pin connectors**. A drive connects to the controller and additional disk units can be daisy-chained from one drive cabinet to the next.

### Disk Buffer board

Each disk cabinet contains a Disk Buffer board. The guide assigns it:

- long-cable line drivers/receivers;
- disk-drive address circuitry;
- line drivers for daisy-chaining multiple disk systems.

Four address straps select one of **16 drive addresses, 0 through 15**. Drive addressing therefore belongs to each disk unit/buffer, not to a host-side vector index hidden in the controller.

### Pertec FD-400

The guide identifies the drive mechanism as a **Pertec FD-400** and assigns the actual mechanism/read-write electronics to it. RusTair keeps mechanism state below the MITS Disk Buffer/cable boundary and below the DCDD cards.

## 2. Media geometry established for the base model

For the first authentic medium model:

- 8-inch removable floppy;
- hard sectored;
- **32 sector holes plus one index hole**;
- **77 tracks**;
- **360 RPM**;
- controller/drive data rate **250,000 bit/s**;
- track-to-track mechanical access figure **10 ms**.

The programming guide exposes up to **137 physical bytes per sector including the sync-bearing first byte**. This is the physical-sector size relevant to the controller stream; a filesystem or operating system may use only part of those bytes as payload.

No production controller code may equate a guest sector request with a 128-byte filesystem sector operation.

## 3. Fixed I/O channels

The July 1977 guide specifies octal channels `010`, `011`, `012`, which are hexadecimal/decimal ports `08h`, `09h`, `0Ah`.

| Port | IN | OUT |
| --- | --- | --- |
| `08h` (`010` octal) | Disk/controller status | Select/latch/enable controller and one drive |
| `09h` (`011` octal) | Sector position | Disk-function control |
| `0Ah` (`012` octal) | Read data | Write data |

These addresses are part of the historical base controller contract. The implementation must not add arbitrary user-remappable ports merely for emulator convenience.

Intel's 8080 I/O cycle mirrors the 8-bit port number onto both address-bus bytes. The DCDD schematic exploits that property by decoding the upper address lines on Board #1. RusTair therefore decodes the actual `A8..A15` connector signals for this controller rather than silently reusing the generic low-byte serial-card decoder.

## 4. Port 08h OUT — drive/controller selection

Documented output bits:

- `D0..D3`: four-bit drive address, selecting one of 16 drive addresses and enabling the selected drive/controller path.
- `D4..D6`: marked not used / user selectable by the MITS guide. The base implementation must not assign invented controller semantics to them.
- `D7`: when written as `1`, clears/disables Disk Control; `D0..D6` are then user selectable according to the guide.

The guide also states that Disk Control is cleared when the disk-drive door is opened or drive power is turned off.

The controller/drive cannot be enabled when:

- the drive door is open;
- disk power is off;
- the disk interconnect cable is absent.

Those are physical conditions, not host UI special cases. Consequently the Phase-2 no-drive assembly cannot become enabled after an `OUT 08h`; the DCL edge is real, Board #2 latches the selected address, but the absent external cable/drive path leaves Disk Enable false.

## 5. Port 08h IN — status

The guide explicitly states that the status truth convention is **True = 0, False = 1** for the active status functions. When disk/controller are not enabled, the status functions are reported false.

| Bit | Name | Source-backed behavior |
| --- | --- | --- |
| D0 | ENWD — Enter New Write Data | true when the write circuit is ready for a new byte; occurs every 32 µs after the write sequence reaches the data window; reset by OUT to 0Ah |
| D1 | Move Head | indicates when head movement is allowed; constrained by step/write/trim-erase timing |
| D2 | HS — Head Status | true 40 ms after head load, or 40 ms after a step while already loaded; also gates valid sector-position presentation |
| D3 | unused | schematic/alternate-source review fixes this output at logic `0`; no functional semantics are assigned |
| D4 | unused | schematic/alternate-source review fixes this output at logic `0`; no functional semantics are assigned |
| D5 | INTE | the manual identifies this as INTE status; the schematic shows a `PINTE` input into the Board #1 status circuitry, so the base model treats this as observation of the S-100 `pINTE`/Interrupt Enable bus state when the controller is enabled, not as a fabricated private vector state |
| D6 | TRACK 0 | true when the head is at the outermost track |
| D7 | NRDA — New Read Data Available | true when one read byte is ready; after sync detection it occurs every 32 µs and is reset by IN from 0Ah |

With Disk Control disabled, D0/D1/D2/D5/D6/D7 are all false and therefore read as `1`, while D3/D4 are physically `0`. The exact disabled/no-drive status byte is therefore **`E7h` (`1110_0111b`)**. RusTair must not substitute `FFh` for this state.

## 6. Port 09h OUT — disk-function control

The guide defines a logic `1` on these output bits as the command assertion:

- `D0`: STEP IN — move one position toward a higher-numbered track.
- `D1`: STEP OUT — move one position toward a lower-numbered track.
- `D2`: HEAD LOAD.
- `D3`: HEAD UNLOAD.
- `D4`: IE — Interrupt Enable. Enables controller interrupts when SRO/Sector True occurs.
- `D5`: ID — Interrupt Disable. Clearing Disk Control also disables the interrupt circuit.
- `D6`: HCS — Head Current Switch. Must be asserted for writes on tracks 43–76; automatically reset at the end of writing a sector.
- `D7`: WRITE ENABLE — starts the documented write sequence.

No step/head/write command may mutate a host image directly. In the Phase-2 no-drive state the CD strobe is still generated over the physical board harness, but no mechanics or write state is fabricated in response; those effects begin only when the source-backed drive engine exists.

## 7. Port 09h IN — sector position

Sector-position input is valid only with the drive/controller enabled and after the head-status timing requirement has been satisfied.

- `D0`: SRO / Sector True. The guide defines True as logic `0` and approximately **30 µs** long.
- `D1..D5`: binary sector number `0..31` as shown in the MITS table.
- `D6..D7`: unused in the sector-position value; their driven value during a valid Head-Status window remains deferred until the mechanics phase requires it.

Schematic review resolves the Phase-2 non-enabled behavior: the sector-position line drivers are gated by Head Status. With no attached drive, Head Status is false and **IN 09h does not drive the S-100 DI bus**. The normal bus/open-bus resolution therefore applies; RusTair does not synthesize a sector byte while the output drivers are disabled.

The guide states:

- read data becomes available **140 µs after Sector True**;
- write data is requested **280 µs after Sector True**.

At 360 RPM with 32 hard sectors, one revolution is approximately 166.667 ms and the sector period is approximately 5.208 ms. The implementation will derive rotation from virtual time rather than increment sector number because software polled the port.

## 8. Port 0Ah — serial byte path

- OUT 0Ah supplies Write Data in response to ENWD.
- IN 0Ah takes Read Data in response to NRDA.
- Once synchronized, the documented cadence is **one byte every 32 µs**, corresponding to 250 kbit/s serial data.

The Board #1 schematic shows the read strobe enabling the read-data line drivers; it does **not** establish a defined power-up value for the G3/H1 read-data latches. Therefore an `IN 0Ah` before valid disk data exists is not source-backed as either `00h`, `FFh`, or open bus. Phase 2 represents those uninitialized TTL latch bits as an indeterminate power-up byte and deliberately has no test asserting a particular value. The line drivers remain electrically enabled, which avoids falsely claiming high impedance. A valid read-data latch and NRDA side effect are introduced in the authentic read phase.

The disk never waits for the 8080 merely because guest software failed to service NRDA/ENWD on time. Authentic mode must preserve the hardware consequence of late service.

## 9. Authentic write timing contract

The July 1977 guide/figure establishes the following observable timing sequence for the base controller:

- Sector True pulse: approximately 30 µs.
- Trim erase turns on approximately **200 µs** after the write sequence is enabled in the documented sector sequence.
- ENWD becomes true approximately **280 µs from the start of the sector**.
- Subsequent ENWD opportunities occur every **32 µs**.
- The first byte written has its most-significant bit (`D7`, sync bit) asserted.
- Maximum normal sector content is **137 bytes including sync**.
- At end of sector the write circuit disables automatically.
- Trim erase remains active for approximately **475 µs after the end of the sector/write interval**.

The scan's wording for the special final/fill byte must be cross-checked against the alternate scan before RusTair encodes that exact byte-value rule. The current phase does not implement write data, so no assumption is needed yet.

## 10. Head movement / head status contract

Source-backed facts used by later mechanics work:

- track-to-track access: 10 ms advertised for the FD-400 system;
- head status becomes valid approximately 40 ms after head load;
- after stepping with the head already loaded, head status is not valid until approximately 40 ms after the step command;
- the Move Head status is suppressed by documented step/write/trim-erase intervals;
- the guide states stepping may occur every 10 ms;
- write/trim-erase sequencing can hold the head in the required state even if software commands unload during the write cycle.

The exact one-shot waveform around Move Head will be implemented from the timing figure/schematic in Phase 3 rather than approximated from prose.

## 11. Interrupt hardware contract

The programming guide says controller interrupt enable causes an interrupt on **SRO / Sector True**. Interrupt Disable or clearing Disk Control disables the circuit.

The Board #1 schematic contains an **INTERRUPT OPTION** selecting the interrupt output between:

- S-100 `pINT` / pin 73; and
- S-100 `VI7` / pin 11.

Therefore the DCDD must never contain a hard-coded `RST 7` injection. It asserts the configured physical request line. If pINT is used, the normal Altair interrupt acknowledge/open-bus behavior determines what the CPU receives; if VI7 is used, any installed 88-VI-class interrupt hardware owns prioritization/vectoring.

The first controller configuration type should consequently model the physical interrupt strap/wiring as `PINT` vs `VI7` (and, if the schematic/installation documentation proves a disconnected position, that state may be represented explicitly). Phase 2 does not assert either line; Phase 7 implements the dynamic interrupt behavior.

## 12. Phase implementation boundaries

### Phase 1 topology boundary — complete

Phase 1 encoded only:

- two distinct historical S-100 cards: MITS 88-DCDD Controller Board #1 and Board #2;
- exactly one matched pair for the first supported controller subsystem;
- adjacent fitted S-100 connectors;
- one explicit shared physical controller harness object;
- a distinct external disk-cable/bus boundary owned by that harness;
- electrically quiescent cards before the source-backed register surface existed.

Its local full-test/release gate passed before Phase 2 began.

### Phase 2 decode/register boundary

Phase 2 may make only the no-media register/electrical surface guest-visible:

- fixed 08h/09h/0Ah decode through Board #1's actual upper address inputs;
- DCL/CD/WDS board-to-board harness strobes;
- Board #2 disk-enable/selection state sufficient to prove an absent drive cannot enable;
- exact disabled status `E7h`;
- Head-Status-gated high impedance on sector input with no drive;
- enabled-but-indeterminate read-data latch output before valid media data;
- source-backed Power-On-Clear/D7 clear paths;
- no interrupt assertion, media access, rotation, stepping, head timing, read cadence or write cadence.

This phase must remain event-driven: neither fitted DCDD board subscribes to the 2 MHz clock merely to poll an idle controller. Clock/timing circuitry is activated only when the later timing model can consume it without O(T-states × drives) work.

## 13. Deferred items which are not production assumptions

The following are intentionally deferred rather than guessed:

- exact D6/D7 electrical values on a **valid, Head-Status-enabled** sector-position read; the Phase-2 disabled-driver behavior is already fixed as high impedance;
- the source-backed 2 MHz controller clock functions that become observable with serial read/write timing; they will be represented by virtual-time/event logic rather than an idle per-T-state subscription;
- exact special final/fill-byte value/rule in the write sequence where the July 1977 scan is visually ambiguous;
- full DB-37 signal-by-signal electrical table and active polarity, to be transcribed before the external drive electronics are activated;
- exact Pertec FD-400 connector-level electrical interface beyond the MITS-visible mechanics/timing already established;
- NWD controller timing differences and applicability to disk/media vintages.

None of these deferred points is needed to make the Phase-2 no-drive register surface exact. They must be resolved before the phase that would make them guest-observable.
