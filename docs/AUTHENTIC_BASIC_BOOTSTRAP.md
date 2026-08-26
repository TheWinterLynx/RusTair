# Authentic Microsoft 4K BASIC 3.2 bootstrap

RusTair keeps two deliberately different BASIC load paths:

- **Quick Load** copies the bundled `4kbas32.bin` image directly into RAM and starts it. This is an emulator convenience.
- **Authentic Load** uses the front-panel bootstrap below. The Intel 8080 executes that loader and every paper-tape byte reaches the guest through the installed MITS serial board and the ASR-33 reader.

The authentic path must never call `load_bytes()` for BASIC itself.

## Provenance

The byte listings are the period MITS bootstrap listings cross-checked against later archival documentation:

1. **MITS Altair BASIC Reference Manual (1977), Appendix B — Loading and Initializing BASIC**  
   https://altairclone.com/downloads/manuals/BASIC%20Manual%2077.pdf
   - gives the front-panel entry procedure;
   - gives the 88-SIO rev. 1 and 88-2SIO bootstrap layouts;
   - identifies `017` octal as the 4K checksum-loader selector;
   - documents the 88-2SIO `021` octal two-stop-bit / `025` octal one-stop-bit ACIA initialization alternatives.

2. **Martin Eberhard, “Loading Basic with the 88-2SIO Board” (2013)**  
   https://deramp.com/downloads/mfe_archive/010-S100%20Computers%20and%20Boards/00-MITS/10-MITS%20S100%20Boards/88-2SIO%20Dual%20Serial%20Board/Loading%20Basic%20with%20the%2088-2SIO%20Board.pdf
   - explicitly identifies `256` octal at bootstrap address `011` as the BASIC 3.X value (`302` is BASIC 4.X);
   - documents BASIC 3.X sense-switch settings: 88-SIO rev. 1 = all A15..A8 down; 88-2SIO Port 0 with two stop bits = A11 up, all other A15..A8 down.

3. **Martin Eberhard, “Altair Paper Tape Format”**  
   https://deramp.com/downloads/altair/software/papertape_cassette/Checksum%20Loader/Altair%20Paper%20Tape%20Format.pdf
   - explains why the bootstrap byte is both the leader character and checksum-loader length marker;
   - confirms `256` octal for BASIC 3.2 and `017` octal for 4K BASIC;
   - documents the leader → reverse checksum loader → checksummed program records → go-record flow.

No downloaded BASIC tape image is added by this work. The user mounts a legally obtained `.tap`/`.bin` image in the existing ASR-33 reader.

## 88-SIO rev. 1 — 4K BASIC 3.2

Ports: status `00h`, data `01h`.  
BASIC 3.X sense byte: `00h` (A15..A8 all down).

| Octal address | Octal data | Hex | 8080 meaning |
|---:|---:|---:|---|
| 000 | 041 | 21 | LXI H,0FAEh |
| 001 | 256 | AE | BASIC 3.2 leader / checksum-loader marker |
| 002 | 017 | 0F | high byte of 0FAEh |
| 003 | 061 | 31 | LXI SP,0012h |
| 004 | 022 | 12 | low byte of stack address |
| 005 | 000 | 00 | high byte of stack address |
| 006 | 333 | DB | IN 00h |
| 007 | 000 | 00 | 88-SIO status port |
| 010 | 017 | 0F | RRC |
| 011 | 330 | D8 | RC — active-low receiver-ready loop |
| 012 | 333 | DB | IN 01h |
| 013 | 001 | 01 | 88-SIO data port |
| 014 | 275 | BD | CMP L — leader? |
| 015 | 310 | C8 | RZ |
| 016 | 055 | 2D | DCR L |
| 017 | 167 | 77 | MOV M,A |
| 020 | 300 | C0 | RNZ |
| 021 | 351 | E9 | PCHL — execute checksum loader |
| 022 | 003 | 03 | stack return address low byte |
| 023 | 000 | 00 | stack return address high byte |

Hex byte sequence:

```text
21 AE 0F 31 12 00 DB 00 0F D8 DB 01 BD C8 2D 77 C0 E9 03 00
```

## 88-2SIO Port 0 — 4K BASIC 3.2, ASR-33 two stop bits

Ports: status/control `10h`, data `11h`.  
BASIC 3.X sense byte: `08h` (A11 up, A15/A14/A13/A12/A10/A9/A8 down).

RusTair deliberately uses the `021` octal ACIA initialization byte here. The ASR-33 configuration is the historical two-stop-bit case. `025` octal is the documented one-stop-bit alternative, but is not the profile selected for RusTair's ASR-33 authentic loader.

| Octal address | Octal data | Hex | 8080 meaning |
|---:|---:|---:|---|
| 000 | 076 | 3E | MVI A,03h |
| 001 | 003 | 03 | ACIA reset value |
| 002 | 323 | D3 | OUT 10h |
| 003 | 020 | 10 | 88-2SIO control/status port |
| 004 | 076 | 3E | MVI A,11h |
| 005 | 021 | 11 | ACIA init, two stop bits |
| 006 | 323 | D3 | OUT 10h |
| 007 | 020 | 10 | 88-2SIO control/status port |
| 010 | 041 | 21 | LXI H,0FAEh |
| 011 | 256 | AE | BASIC 3.2 leader / checksum-loader marker |
| 012 | 017 | 0F | high byte of 0FAEh |
| 013 | 061 | 31 | LXI SP,001Ah |
| 014 | 032 | 1A | low byte of stack address |
| 015 | 000 | 00 | high byte of stack address |
| 016 | 333 | DB | IN 10h |
| 017 | 020 | 10 | 88-2SIO status port |
| 020 | 017 | 0F | RRC |
| 021 | 320 | D0 | RNC — active-high receiver-ready loop |
| 022 | 333 | DB | IN 11h |
| 023 | 021 | 11 | 88-2SIO data port |
| 024 | 275 | BD | CMP L — leader? |
| 025 | 310 | C8 | RZ |
| 026 | 055 | 2D | DCR L |
| 027 | 167 | 77 | MOV M,A |
| 030 | 300 | C0 | RNZ |
| 031 | 351 | E9 | PCHL — execute checksum loader |
| 032 | 013 | 0B | stack return address low byte |
| 033 | 000 | 00 | stack return address high byte |

Hex byte sequence:

```text
3E 03 D3 10 3E 11 D3 10 21 AE 0F 31 1A 00 DB 10 0F D0 DB 11 BD C8 2D 77 C0 E9 0B 00
```

## What the tiny loader actually does

The bootstrap does not understand the full BASIC tape format. Its job is only to get the larger checksum loader into page `0Fxxh` and execute it:

1. Poll the serial status port until a byte is ready.
2. Read the byte with a real 8080 `IN` instruction.
3. Ignore `AEh` leader bytes used by BASIC 3.2.
4. Store the checksum-loader bytes backwards from `0FADh` down through `0F00h`.
5. `PCHL` into the newly loaded checksum loader.
6. The checksum loader then parses the BASIC program records, checks their checksums, writes the requested RAM addresses and finally handles the go record.

This is why RusTair's existing ASR-33 `WAIT GUEST RX` behaviour is important: the physical tape cannot advance merely because a host timer fired. It advances only after the emulated UART/guest has consumed the previous byte.

## Front-panel procedure represented by RusTair

The assisted action is intentionally equivalent to the manual sequence, not a shortcut around it:

1. Power ON and STOP.
2. RESET.
3. Put A15..A0 down and EXAMINE address `0000h`.
4. Put the first byte on A7..A0 and DEPOSIT.
5. Put each following byte on A7..A0 and operate DEPOSIT NEXT.
6. Read back/verify the bootstrap.
7. Put all switches down and EXAMINE `0000h`.
8. Set the BASIC 3.2 sense byte (`00h` for 88-SIO rev. 1, `08h` for 88-2SIO Port 0 / two stop bits).
9. Mount the tape at its leader and use the appropriate reader/RUN order.

`Install bootstrap via front panel` performs steps 2–6 through `BackendHost::set_switch_register`, `examine` and `deposit`. It does **not** use direct memory loading, and it deliberately does not set the BASIC sense switches for the operator.
