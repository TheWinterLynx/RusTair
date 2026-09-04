use std::hint::black_box;
use std::time::Instant;

use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, TwoSioInterruptWiring, TwoSioStraps,
};
use rustair::cpu8080_cycle::Cpu8080Pins;
use rustair::s100_backplane::{s100_slot_mask, S100Backplane};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_cpu::Mits8080CpuBoard;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};
use rustair::s100_runtime::{DisplayControlLines, S100RuntimeFabric};
use rustair::s100_runtime_ram::RuntimeRamCard;

const ITER: u64 = 2_000_000;

fn simple_hardware() -> S100HardwareConfig {
    let mut config = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    config
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    config
        .set_slot(
            2,
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

fn serial_hardware() -> S100HardwareConfig {
    let mut config = simple_hardware();
    config
        .set_slot(
            3,
            Some(S100InstalledCardConfig::Mits88TwoSio {
                straps: TwoSioStraps::default(),
                interrupt_wiring: TwoSioInterruptWiring::default(),
            }),
        )
        .unwrap();
    config.validate().unwrap()
}

fn report(label: &str, iterations: u64, elapsed: std::time::Duration) {
    let ns = elapsed.as_secs_f64() * 1_000_000_000.0 / iterations as f64;
    let mops = iterations as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!("{label:<42} {ns:>10.2} ns/op   {mops:>8.3} Mops/s");
}

fn profile_runtime_address_edges(label: &str, hardware: S100HardwareConfig) {
    let mut fabric = S100RuntimeFabric::new(hardware, RamInit::Zeroed).unwrap();
    let display = DisplayControlLines {
        ready: true,
        run: true,
        ..DisplayControlLines::default()
    };
    let start = Instant::now();
    let mut checksum = 0u8;
    for i in 0..ITER {
        let address = if i & 1 == 0 { 0x0122 } else { 0x0123 };
        fabric.set_cpu_package_pins(Cpu8080Pins {
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
        });
        checksum ^= fabric.settle(display, &[]).unwrap().data_in_or(0xff);
    }
    black_box(checksum);
    report(label, ITER, start.elapsed());
}

#[test]
#[ignore = "manual S-100 hot-path profiler"]
fn profile_s100_hot_path_components() {
    println!();
    println!("RusTair S-100 hot-path microprofile");
    println!("Iterations per section: {ITER}");
    println!();

    // Host-side storage access establishes the lower bound. This is NOT a guest
    // execution path; it only tells us how much of Fast's cost is above RAM.
    let mut fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
    fabric.write_unique_memory(0x0123, 0x5a, false);
    let start = Instant::now();
    let mut checksum = 0u8;
    for _ in 0..ITER {
        checksum ^= black_box(fabric.peek_unique_memory(black_box(0x0123))).unwrap();
    }
    black_box(checksum);
    report("host RAM handle read (diagnostic baseline)", ITER, start.elapsed());

    // Measure the full compiled Fast physical transaction independently of the
    // 8080 instruction core and backend/UI bookkeeping.
    let start = Instant::now();
    let mut checksum = 0u8;
    for _ in 0..ITER {
        checksum ^= black_box(fabric.fast_memory_read(black_box(0x0123), 0x82).unwrap());
    }
    black_box(checksum);
    report("S100RuntimeFabric::fast_memory_read", ITER, start.elapsed());

    // Build exactly CPU + one no-wait RAM card so resolver and card costs can be
    // measured without BackendHost or instruction execution.
    let (cpu_card, cpu) = Mits8080CpuBoard::new();
    let (ram_card, ram) = RuntimeRamCard::historical(
        S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
        RamInit::Zeroed,
    )
    .unwrap();
    ram.write_byte(0x0123, 0x5a, false);

    let mut backplane = S100Backplane::new(2);
    backplane.insert(1, Box::new(cpu_card)).unwrap();
    backplane.insert(2, Box::new(ram_card)).unwrap();
    let selected = s100_slot_mask(1) | s100_slot_mask(2);

    cpu.set_package_pins(Cpu8080Pins {
        phi1: true,
        phi2: false,
        address: Some(0x0123),
        data_out: Some(0x82),
        sync: true,
        dbin: false,
        wr_n: true,
        inte: false,
        wait: false,
        hlda: false,
    });
    backplane.refresh_cached_drives(selected).unwrap();
    let display = DisplayControlLines {
        ready: true,
        run: true,
        ..DisplayControlLines::default()
    };
    let display_drive = display.drive(backplane.sample());
    let _ = backplane.resolve_cached_selected_drives(selected, &[display_drive]);

    // Pure re-resolution with unchanged cached card drives. If this is costly,
    // rebuilding driver counters/sample state is the dominant avoidable work.
    let start = Instant::now();
    for _ in 0..ITER {
        black_box(backplane.resolve_cached_selected_drives(selected, &[display_drive]));
    }
    report("cached two-card full re-resolve", ITER, start.elapsed());

    // CPU-board non-S100 side refresh: isolates Rc/RefCell + drive construction +
    // connector contract validation without resolving the bus.
    let start = Instant::now();
    for i in 0..ITER {
        let high = i & 1 == 0;
        cpu.set_package_pins(Cpu8080Pins {
            phi1: high,
            phi2: !high,
            address: Some(0x0123),
            data_out: None,
            sync: false,
            dbin: true,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        black_box(backplane.refresh_cached_drives(s100_slot_mask(1)).unwrap());
    }
    report("CPU board package->cached drive refresh", ITER, start.elapsed());

    // One production-style event delta: CPU refresh, cached resolve and only
    // electrically affected card observations. This approximates one live edge
    // without the exact 8080 core itself.
    let start = Instant::now();
    for i in 0..ITER {
        let high = i & 1 == 0;
        cpu.set_package_pins(Cpu8080Pins {
            phi1: high,
            phi2: !high,
            address: Some(0x0123),
            data_out: None,
            sync: false,
            dbin: true,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        let _ = backplane.refresh_cached_drives(s100_slot_mask(1)).unwrap();
        let display_drive = display.drive(backplane.sample());
        let change = backplane.resolve_cached_selected_drives(selected, &[display_drive]);
        black_box(
            backplane
                .observe_changed_cards(change, s100_slot_mask(1), selected)
                .unwrap(),
        );
    }
    report("one CPU+RAM event-driven edge", ITER, start.elapsed());

    // Cycle's remaining serial penalty is not in idle connector refresh: profile
    // an irrelevant A0 transition while no I/O status strobe is asserted. A real
    // 88-2SIO sees those address wires, but its register decoder has no work to
    // perform during a memory transaction. Comparing these two rows tells us the
    // software cost of merely waking the installed serial card on that edge.
    profile_runtime_address_edges("runtime address edge CPU+RAM", simple_hardware());
    profile_runtime_address_edges("runtime address edge +88-2SIO", serial_hardware());
}
