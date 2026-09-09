//! Manual breakdown of the active S-100 edge cost.
//!
//! This profiler deliberately changes no production behavior.  It separates
//! Intel-package mutation, the production CPU-slot cache update, Display/Control
//! drive construction, incremental resolution, and changed-card observation so
//! optimization work can target measured host cost rather than the physical
//! model itself.

use std::hint::black_box;
use std::time::Instant;

use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::cpu8080_cycle::Cpu8080Pins;
use rustair::s100_backplane::{s100_slot_mask, S100Backplane};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_cpu::{Mits8080CpuBoard, Mits8080CpuBoardHandle};
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};
use rustair::s100_runtime::{DisplayControlLines, S100RuntimeFabric};
use rustair::s100_runtime_ram::RuntimeRamCard;

const ITER: u64 = 2_000_000;
const CPU_SLOT: usize = 1;
const RAM_SLOT: usize = 2;

fn report(label: &str, elapsed: std::time::Duration) {
    let ns = elapsed.as_secs_f64() * 1_000_000_000.0 / ITER as f64;
    let mops = ITER as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!("{label:<52} {ns:>10.2} ns/op   {mops:>8.3} Mops/s");
}

fn display() -> DisplayControlLines {
    DisplayControlLines {
        ready: true,
        run: true,
        ..DisplayControlLines::default()
    }
}

#[inline]
fn address_pins(address: u16) -> Cpu8080Pins {
    Cpu8080Pins {
        phi1: false,
        phi2: true,
        address: Some(address),
        data_out: None,
        sync: false,
        dbin: true,
        wr_n: true,
        inte: false,
        wait: false,
        hlda: false,
    }
}

fn hardware() -> S100HardwareConfig {
    let mut config = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    config
        .set_slot(CPU_SLOT, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    config
        .set_slot(
            RAM_SLOT,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(
                    S100RamBoardModel::Mits4KStatic88_4Mcs,
                    0,
                ),
            )),
        )
        .unwrap();
    config.validate().unwrap()
}

fn two_card_backplane() -> (S100Backplane, Mits8080CpuBoardHandle, u32) {
    let (cpu_card, cpu) = Mits8080CpuBoard::new();
    let (ram_card, ram) = RuntimeRamCard::historical(
        S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
        RamInit::Zeroed,
    )
    .unwrap();
    // Alternate distinct bytes so RAM observation can produce a real DI drive
    // transition rather than merely execute an identical-value code path.
    assert!(ram.write_byte(0x0122, 0xa5, false));
    assert!(ram.write_byte(0x0123, 0x5a, false));

    let mut backplane = S100Backplane::new(2);
    backplane.insert(CPU_SLOT, Box::new(cpu_card)).unwrap();
    backplane.insert(RAM_SLOT, Box::new(ram_card)).unwrap();
    let selected = s100_slot_mask(CPU_SLOT) | s100_slot_mask(RAM_SLOT);

    cpu.set_package_pins(address_pins(0x0122));
    backplane
        .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
        .unwrap();
    let d = display();
    // Establish a valid incremental accumulator and let the two physical cards
    // observe enough causal deltas to reach the starting read state.
    for _ in 0..3 {
        let drive = d.drive(backplane.sample());
        let change = backplane.resolve_cached_selected_drives(selected, &[drive]);
        backplane
            .observe_changed_cards(change, s100_slot_mask(CPU_SLOT), selected)
            .unwrap();
    }

    (backplane, cpu, selected)
}

#[test]
#[ignore = "manual S-100 active-edge breakdown profiler"]
fn profile_s100_active_edge_breakdown() {
    println!();
    println!("RusTair S-100 active-edge breakdown");
    println!("Iterations per section: {ITER}");
    println!();

    // A. Intel-package-side state mutation only. No S-100 slot or resolver is
    // involved; this isolates Rc/RefCell + CPU-board incremental drive updates.
    let (_card, cpu) = Mits8080CpuBoard::new();
    let start = Instant::now();
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        cpu.set_package_pins(black_box(address_pins(address)));
    }
    black_box(cpu.package_pins());
    report("A  CPU package mutation only", start.elapsed());

    // B. Exact production boundary: mutate the same physical CPU board and push
    // its already-built connector drive into the normal slot cache. No resolve.
    let mut fabric = S100RuntimeFabric::new(hardware(), RamInit::Zeroed).unwrap();
    let start = Instant::now();
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        fabric.set_cpu_package_pins(black_box(address_pins(address)));
    }
    black_box(fabric.sample().address());
    report("B  production CPU package -> slot cache", start.elapsed());

    // C. Display/Control drive construction by itself. This reads the currently
    // resolved pWR/sOUT state and builds its small non-slot connector drive.
    let d = display();
    let start = Instant::now();
    let mut last = None;
    for _ in 0..ITER {
        last = Some(black_box(d.drive(black_box(fabric.sample()))));
    }
    black_box(last);
    report("C  Display/Control drive construction", start.elapsed());

    // D. A cached re-resolve when absolutely nothing changed: lower bound for
    // incremental resolver bookkeeping with CPU + RAM selected.
    let (mut backplane, _cpu, selected) = two_card_backplane();
    let drive = d.drive(backplane.sample());
    let _ = backplane.resolve_cached_selected_drives(selected, &[drive]);
    let start = Instant::now();
    for _ in 0..ITER {
        black_box(backplane.resolve_cached_selected_drives(selected, &[drive]));
    }
    report("D  cached resolver, no electrical change", start.elapsed());

    // E. Package mutation plus the generic card refresh API. This is useful as a
    // diagnostic comparison, but B above is the production runtime boundary.
    let (mut backplane, cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        cpu.set_package_pins(address_pins(address));
        black_box(
            backplane
                .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
                .unwrap(),
        );
    }
    report("E  package mutation + generic CPU cache refresh", start.elapsed());

    // F. Add the active incremental electrical resolve, but deliberately do not
    // wake RAM/CPU inputs yet. E -> F approximates the cost of changing resolved
    // nets and cached address/data buses for one active CPU drive transition.
    let (mut backplane, cpu, selected) = two_card_backplane();
    let start = Instant::now();
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        cpu.set_package_pins(address_pins(address));
        backplane
            .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
            .unwrap();
        let drive = d.drive(backplane.sample());
        black_box(backplane.resolve_cached_selected_drives(selected, &[drive]));
    }
    report("F  E + active incremental resolve", start.elapsed());

    // G. Full production-style causal delta: after resolution, wake exactly the
    // physical cards wired to changed input contacts plus the forced CPU sampler.
    // F -> G exposes observer lookup + card observe + resulting drive comparison.
    let (mut backplane, cpu, selected) = two_card_backplane();
    let start = Instant::now();
    let mut changed_drives = 0u32;
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        cpu.set_package_pins(address_pins(address));
        backplane
            .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
            .unwrap();
        let drive = d.drive(backplane.sample());
        let change = backplane.resolve_cached_selected_drives(selected, &[drive]);
        changed_drives ^= backplane
            .observe_changed_cards(change, s100_slot_mask(CPU_SLOT), selected)
            .unwrap();
    }
    black_box(changed_drives);
    report("G  F + changed-card observation", start.elapsed());

    // H. Exact RuntimeFabric public production path for the same alternating
    // address edge. Comparing B -> H measures everything settle adds after the
    // CPU connector cache is already updated.
    let mut fabric = S100RuntimeFabric::new(hardware(), RamInit::Zeroed).unwrap();
    let start = Instant::now();
    let mut checksum = 0u8;
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        fabric.set_cpu_package_pins(address_pins(address));
        checksum ^= fabric
            .settle(d, &[])
            .unwrap()
            .data_in_or(0xff);
    }
    black_box(checksum);
    report("H  production fabric CPU update + settle", start.elapsed());

    println!();
    println!("Read the deltas between adjacent rows; do not subtract timings from different runs as if they were exact accounting.");
    println!("B and H are the production RuntimeFabric boundaries. E/F/G expose internal causal stages using the public backplane API.");
}
