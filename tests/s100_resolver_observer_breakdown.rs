use std::hint::black_box;
use std::time::Instant;

use rustair::config::RamInit;
use rustair::cpu8080_cycle::Cpu8080Pins;
use rustair::s100_backplane::{s100_slot_mask, S100Backplane, S100BusChange};
use rustair::s100_cpu::{Mits8080CpuBoard, Mits8080CpuBoardHandle};
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};
use rustair::s100_runtime::DisplayControlLines;
use rustair::s100_runtime_ram::RuntimeRamCard;

const ITER: u64 = 2_000_000;
const CPU_SLOT: usize = 1;
const RAM_SLOT: usize = 2;

fn report(label: &str, elapsed: std::time::Duration) {
    let ns = elapsed.as_secs_f64() * 1_000_000_000.0 / ITER as f64;
    let mops = ITER as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    println!("{label:<58} {ns:>10.2} ns/op   {mops:>8.3} Mops/s");
}

fn display() -> DisplayControlLines {
    DisplayControlLines {
        ready: true,
        run: true,
        ..DisplayControlLines::default()
    }
}

fn pins(address: u16, dbin: bool, data_out: Option<u8>) -> Cpu8080Pins {
    Cpu8080Pins {
        phi1: false,
        phi2: true,
        address: Some(address),
        data_out,
        sync: false,
        dbin,
        wr_n: true,
        inte: false,
        wait: false,
        hlda: false,
    }
}

fn two_card_backplane() -> (S100Backplane, Mits8080CpuBoardHandle, u32) {
    let (cpu_card, cpu) = Mits8080CpuBoard::new();
    let (ram_card, ram) = RuntimeRamCard::historical(
        S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
        RamInit::Zeroed,
    )
    .unwrap();
    assert!(ram.write_byte(0x0122, 0xa5, false));
    assert!(ram.write_byte(0x0123, 0x5a, false));

    let mut backplane = S100Backplane::new(2);
    backplane.insert(CPU_SLOT, Box::new(cpu_card)).unwrap();
    backplane.insert(RAM_SLOT, Box::new(ram_card)).unwrap();
    let selected = s100_slot_mask(CPU_SLOT) | s100_slot_mask(RAM_SLOT);
    let d = display();

    cpu.set_package_pins(pins(0x0122, true, None));
    backplane
        .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
        .unwrap();
    for _ in 0..3 {
        let drive = d.drive(backplane.sample());
        let change = backplane.resolve_cached_selected_drives(selected, &[drive]);
        backplane
            .observe_changed_cards(change, s100_slot_mask(CPU_SLOT), selected)
            .unwrap();
    }

    (backplane, cpu, selected)
}

fn profile_source_update_only<F>(label: &str, mut make_pins: F)
where
    F: FnMut(u64) -> Cpu8080Pins,
{
    let (mut backplane, cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    for i in 0..ITER {
        cpu.set_package_pins(make_pins(i));
        black_box(
            backplane
                .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
                .unwrap(),
        );
    }
    report(label, start.elapsed());
}

fn profile_source_plus_resolve<F>(label: &str, mut make_pins: F)
where
    F: FnMut(u64) -> Cpu8080Pins,
{
    let (mut backplane, cpu, selected) = two_card_backplane();
    let d = display();
    let start = Instant::now();
    for i in 0..ITER {
        cpu.set_package_pins(make_pins(i));
        backplane
            .refresh_cached_drives(s100_slot_mask(CPU_SLOT))
            .unwrap();
        let drive = d.drive(backplane.sample());
        black_box(backplane.resolve_cached_selected_drives(selected, &[drive]));
    }
    report(label, start.elapsed());
}

#[test]
#[ignore = "manual S-100 resolver/observer breakdown profiler"]
fn profile_s100_resolver_and_observer_breakdown() {
    println!();
    println!("RusTair S-100 resolver/observer breakdown");
    println!("Iterations per section: {ITER}");
    println!();

    profile_source_update_only("R1 ADDRESS A0 toggle: source update + cache", |i| {
        pins(if i & 1 == 0 { 0x0122 } else { 0x0123 }, true, None)
    });
    profile_source_plus_resolve("R2 ADDRESS A0 toggle: R1 + active resolve", |i| {
        pins(if i & 1 == 0 { 0x0122 } else { 0x0123 }, true, None)
    });

    profile_source_update_only("R3 DBIN toggle: source update + cache", |i| {
        pins(0x0123, i & 1 == 0, None)
    });
    profile_source_plus_resolve("R4 DBIN toggle: R3 + active resolve", |i| {
        pins(0x0123, i & 1 == 0, None)
    });

    profile_source_update_only("R5 DO toggle: source update + cache", |i| {
        pins(0x0123, false, Some(if i & 1 == 0 { 0x00 } else { 0xff }))
    });
    profile_source_plus_resolve("R6 DO toggle: R5 + active resolve", |i| {
        pins(0x0123, false, Some(if i & 1 == 0 { 0x00 } else { 0xff }))
    });

    println!();

    let cpu_mask = s100_slot_mask(CPU_SLOT);
    let ram_mask = s100_slot_mask(RAM_SLOT);
    let selected = cpu_mask | ram_mask;

    let (mut backplane, _cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    for _ in 0..ITER {
        backplane.observe_selected_cards(cpu_mask);
    }
    report("O1 CPU observe_s100 only, stable sample", start.elapsed());

    let (mut backplane, _cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    for _ in 0..ITER {
        backplane.observe_selected_cards(ram_mask);
    }
    report("O2 RAM observe_s100 only, stable sample", start.elapsed());

    let (mut backplane, _cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    for _ in 0..ITER {
        backplane.observe_selected_cards(selected);
    }
    report("O3 CPU + RAM observe_s100 only, stable sample", start.elapsed());

    let (mut backplane, _cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    let mut changed = 0u32;
    for _ in 0..ITER {
        changed ^= backplane
            .observe_changed_cards(S100BusChange::default(), cpu_mask, selected)
            .unwrap();
    }
    black_box(changed);
    report("O4 CPU observe + drive_s100/compare", start.elapsed());

    let (mut backplane, _cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    let mut changed = 0u32;
    for _ in 0..ITER {
        changed ^= backplane
            .observe_changed_cards(S100BusChange::default(), ram_mask, selected)
            .unwrap();
    }
    black_box(changed);
    report("O5 RAM observe + drive_s100/compare", start.elapsed());

    let (mut backplane, _cpu, _selected) = two_card_backplane();
    let start = Instant::now();
    let mut changed = 0u32;
    for _ in 0..ITER {
        changed ^= backplane
            .observe_changed_cards(S100BusChange::default(), selected, selected)
            .unwrap();
    }
    black_box(changed);
    report("O6 CPU + RAM observe + drive_s100/compare", start.elapsed());

    println!();
    println!("Interpret R2-R1, R4-R3 and R6-R5 as approximate resolver costs for ADDRESS, DBIN and DO transitions.");
    println!("Interpret O4-O1 and O5-O2 as the extra drive_s100/cache-compare bookkeeping after each card observation.");
}
