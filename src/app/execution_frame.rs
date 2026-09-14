use std::time::{Duration, Instant};

use crate::backend::BackendHost;
use crate::config::EmulationSpeed;

use super::execution_clock::UNLIMITED_CHUNK_T_STATES;

const SERVICE_SLICE_T_STATES: u32 = 4_096;
const UNLIMITED_SERVICE_SLICE_T_STATES: u32 = 65_536;
/// Throttled CPU modes replay wall time as a CPU/serial timeline instead of
/// letting egui repaint cadence become the UART clock. Four microseconds is
/// below the fastest possible 88-2SIO bit time (9600 tap with MC6850 /1 is
/// about 6.51 us), so even that extreme configuration cannot cross a complete
/// serial bit boundary without returning to the physical-time scheduler.
const SERIAL_PHYSICAL_SLICE: Duration = Duration::from_micros(4);
pub(super) const CPU_FRAME_TIME: Duration = Duration::from_millis(8);
/// Unlimited remains host-throughput driven, but it must yield often enough for
/// the egui event/render loop to stay interactive. The CPU executes on this same
/// thread, so a 100 ms burst hard-limited the GUI to roughly 10 FPS. A 12 ms
/// deadline preserves the large service slices while returning control within a
/// normal interactive frame budget.
const UNLIMITED_CPU_FRAME_TIME: Duration = Duration::from_millis(12);

#[inline]
fn host_profile(budget: u32, requested_limit: Duration) -> (u32, Duration) {
    if budget == UNLIMITED_CHUNK_T_STATES {
        (UNLIMITED_SERVICE_SLICE_T_STATES, UNLIMITED_CPU_FRAME_TIME)
    } else {
        (SERVICE_SLICE_T_STATES, requested_limit)
    }
}

#[inline]
const fn throttled_speed_multiplier(speed: EmulationSpeed) -> Option<u32> {
    match speed {
        EmulationSpeed::Authentic => Some(1),
        EmulationSpeed::X2 => Some(2),
        EmulationSpeed::X5 => Some(5),
        EmulationSpeed::X10 => Some(10),
        EmulationSpeed::Unlimited => None,
    }
}

#[inline]
fn throttled_serial_slice_t_states(cpu_clock_hz: u32, speed: EmulationSpeed) -> u32 {
    let multiplier = throttled_speed_multiplier(speed)
        .expect("Unlimited never uses the managed physical serial scheduler");
    let numerator = u128::from(cpu_clock_hz)
        .saturating_mul(u128::from(multiplier))
        .saturating_mul(SERIAL_PHYSICAL_SLICE.as_nanos());
    ((numerator / 1_000_000_000).max(1).min(u128::from(u32::MAX))) as u32
}

#[inline]
fn physical_time_for_executed_t_states(
    t_states: u64,
    cpu_clock_hz: u32,
    speed: EmulationSpeed,
) -> Duration {
    let multiplier = throttled_speed_multiplier(speed)
        .expect("Unlimited derives serial time directly from the host wall clock");
    let effective_cpu_hz = u128::from(cpu_clock_hz).saturating_mul(u128::from(multiplier));
    let numerator = u128::from(t_states).saturating_mul(1_000_000_000);
    let nanos = numerator / effective_cpu_hz.max(1);
    Duration::from_nanos(nanos.min(u128::from(u64::MAX)) as u64)
}

/// Host-throughput frame helper retained for diagnostics/performance tests that
/// intentionally exercise the old GUI slice contract without a throttled
/// CPU-to-physical-time mapping. Production throttled execution must use
/// `run_cpu_frame_timed` so serial time remains independent from CPU speed.
pub(super) fn run_cpu_frame(
    machine: &mut BackendHost,
    budget: u32,
    limit: Duration,
) -> u64 {
    run_cpu_frame_timed(
        machine,
        budget,
        limit,
        crate::machine::CLOCK_HZ,
        EmulationSpeed::Unlimited,
    )
}

/// Yield between exact engine budgets. In throttled modes this function also
/// owns CPU/serial physical-time interleaving: CPU speed changes only how many
/// guest T-states fit inside one 4 us physical slice, while the installed UART
/// receives the same elapsed duration at every speed. The UART/card remains the
/// sole bit/frame authority; the scheduler only advances elapsed time.
///
/// Unlimited has no guest-CPU-to-wall-time ratio, so it retains the independent
/// `Instant` serial source and large host-throughput slices.
pub(super) fn run_cpu_frame_timed(
    machine: &mut BackendHost,
    budget: u32,
    limit: Duration,
    cpu_clock_hz: u32,
    speed: EmulationSpeed,
) -> u64 {
    let managed_serial = speed != EmulationSpeed::Unlimited;
    machine.set_serial_clock_managed(managed_serial);

    let (default_service_slice, effective_limit) = host_profile(budget, limit);
    let service_slice_t_states = if managed_serial {
        throttled_serial_slice_t_states(cpu_clock_hz, speed).min(default_service_slice)
    } else {
        default_service_slice
    };

    let started = Instant::now();
    let before = machine
        .intel8080_state()
        .total_t_states
        .expect("8080 T-state clock");
    let mut executed = 0;
    while executed < u64::from(budget) && machine.running() {
        let slice = (u64::from(budget) - executed).min(u64::from(service_slice_t_states)) as u32;
        let before_slice = executed;
        machine.run_cycles(slice);
        let total = machine
            .intel8080_state()
            .total_t_states
            .expect("8080 T-state clock")
            - before;
        if total == before_slice {
            break;
        }

        if managed_serial {
            machine.advance_serial_physical_time(physical_time_for_executed_t_states(
                total - before_slice,
                cpu_clock_hz,
                speed,
            ));
        }
        executed = total;

        if started.elapsed() >= effective_limit {
            break;
        }
    }
    executed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendSerialPort;
    use crate::config::{
        RamInit, S100HardwareConfig, S100InstalledCardConfig, SioBaudRate, SioHardwareConfig,
    };

    const TWO_MHZ: u32 = 2_000_000;

    fn machine() -> BackendHost {
        let mut machine = BackendHost::default();
        machine.configure_s100_hardware(
            S100HardwareConfig::historical_8800b_18_slot_starter(),
            RamInit::Zeroed,
        );
        machine.power(true);
        machine.set_running(false);
        machine.reset();
        // Power-on registers are undefined. Initialize through guest code so
        // equality below compares identical, physically constructed states.
        machine.load_bytes(
            0,
            &[
                0x31, 0, 0x0f, 0x01, 0, 0, 0x11, 0, 0, 0x21, 0, 0, 0xaf, 0, 0xc3, 13, 0,
            ],
        );
        machine.set_running(true);
        machine
    }

    fn sio_machine(baud: u32) -> BackendHost {
        let mut hardware = S100HardwareConfig::historical_8800b_18_slot_starter();
        let serial_slot = hardware
            .serial_slots()
            .map(|(slot, _)| slot)
            .next()
            .expect("historical fixture has one serial slot");
        hardware
            .set_slot(
                serial_slot,
                Some(S100InstalledCardConfig::Mits88Sio(SioHardwareConfig {
                    baud: SioBaudRate::try_new(baud).unwrap(),
                    ..SioHardwareConfig::default()
                })),
            )
            .unwrap();

        let mut machine = BackendHost::default();
        machine.configure_s100_hardware(hardware.validate().unwrap(), RamInit::Zeroed);
        machine.power(true);
        machine.set_running(false);
        machine.reset();
        machine.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]);
        machine.set_running(true);
        machine
    }

    #[test]
    fn expired_frame_yields_then_resumes_the_same_exact_timeline() {
        let mut sliced = machine();
        let mut uninterrupted = machine();
        let expected_first = throttled_serial_slice_t_states(TWO_MHZ, EmulationSpeed::Authentic);
        let first = run_cpu_frame_timed(
            &mut sliced,
            40_000,
            Duration::ZERO,
            TWO_MHZ,
            EmulationSpeed::Authentic,
        );
        assert_eq!(first, u64::from(expected_first));
        let rest = run_cpu_frame_timed(
            &mut sliced,
            40_000 - first as u32,
            Duration::from_secs(1),
            TWO_MHZ,
            EmulationSpeed::Authentic,
        );
        assert_eq!(first + rest, 40_000);
        uninterrupted.run_cycles(40_000);
        assert_eq!(sliced.intel8080_state(), uninterrupted.intel8080_state());
    }

    #[test]
    fn throttled_modes_map_the_same_four_microseconds_to_cpu_speed_only() {
        assert_eq!(
            throttled_serial_slice_t_states(TWO_MHZ, EmulationSpeed::Authentic),
            8
        );
        assert_eq!(
            throttled_serial_slice_t_states(TWO_MHZ, EmulationSpeed::X2),
            16
        );
        assert_eq!(
            throttled_serial_slice_t_states(TWO_MHZ, EmulationSpeed::X5),
            40
        );
        assert_eq!(
            throttled_serial_slice_t_states(TWO_MHZ, EmulationSpeed::X10),
            80
        );
    }

    #[test]
    fn physical_9600_baud_output_is_identical_across_throttled_cpu_speeds() {
        for (speed, budget) in [
            (EmulationSpeed::Authentic, 5_000),
            (EmulationSpeed::X2, 10_000),
            (EmulationSpeed::X5, 25_000),
            (EmulationSpeed::X10, 50_000),
        ] {
            let mut machine = sio_machine(9_600);
            machine.set_serial_clock_managed(true);
            machine.debugger_output_port(0x01, b'A');
            machine.debugger_output_port(0x01, b'B');

            let executed = run_cpu_frame_timed(
                &mut machine,
                budget,
                Duration::from_secs(1),
                TWO_MHZ,
                speed,
            );
            assert_eq!(executed, u64::from(budget), "speed={speed:?}");
            assert_eq!(
                machine.serial_tx_complete(BackendSerialPort::Port0),
                Some(b'A'),
                "first 9600-baud frame differs at {speed:?}"
            );
            assert_eq!(
                machine.serial_tx_complete(BackendSerialPort::Port0),
                Some(b'B'),
                "second 9600-baud frame differs at {speed:?}"
            );
            assert_eq!(machine.serial_tx_complete(BackendSerialPort::Port0), None);
        }
    }

    #[test]
    fn physical_110_remote_echo_requires_rx_then_tx_frame_time() {
        let mut machine = sio_machine(110);
        machine.set_serial_clock_managed(true);

        machine.serial_receive(BackendSerialPort::Port0, b'P');
        assert!(!machine.serial_rx_line_idle(BackendSerialPort::Port0));
        machine.advance_serial_physical_time(Duration::from_millis(99));
        assert!(
            !machine.serial_rx_line_idle(BackendSerialPort::Port0),
            "110-baud 8N2 RX must still be shifting before 100 ms"
        );
        machine.advance_serial_physical_time(Duration::from_millis(1));
        assert!(machine.serial_rx_line_idle(BackendSerialPort::Port0));
        assert_eq!(machine.debugger_input_port(0x01), b'P');

        machine.debugger_output_port(0x01, b'P');
        machine.advance_serial_physical_time(Duration::from_millis(99));
        assert_eq!(
            machine.serial_tx_complete(BackendSerialPort::Port0),
            None,
            "remote echo TX must not become visible before its own 100 ms frame"
        );
        machine.advance_serial_physical_time(Duration::from_millis(1));
        assert_eq!(
            machine.serial_tx_complete(BackendSerialPort::Port0),
            Some(b'P')
        );
    }

    #[test]
    fn unlimited_budget_selects_high_throughput_host_profile() {
        assert_eq!(
            host_profile(UNLIMITED_CHUNK_T_STATES, CPU_FRAME_TIME),
            (UNLIMITED_SERVICE_SLICE_T_STATES, UNLIMITED_CPU_FRAME_TIME)
        );
        assert!(
            UNLIMITED_CPU_FRAME_TIME <= Duration::from_millis(16),
            "Unlimited must yield within an interactive GUI frame budget"
        );
        assert_eq!(
            host_profile(400_000, CPU_FRAME_TIME),
            (SERVICE_SLICE_T_STATES, CPU_FRAME_TIME)
        );
    }

    #[test]
    fn large_unlimited_slice_is_only_host_scheduling_and_preserves_exact_timeline() {
        let mut sliced = machine();
        let mut uninterrupted = machine();
        // Exercise exactly one large host service slice without depending on
        // wall-clock performance of the test runner.
        sliced.set_serial_clock_managed(false);
        let before = sliced.intel8080_state().total_t_states.unwrap();
        sliced.run_cycles(UNLIMITED_SERVICE_SLICE_T_STATES);
        let after = sliced.intel8080_state().total_t_states.unwrap();
        uninterrupted.run_cycles(UNLIMITED_SERVICE_SLICE_T_STATES);
        assert_eq!(after - before, u64::from(UNLIMITED_SERVICE_SLICE_T_STATES));
        assert_eq!(sliced.intel8080_state(), uninterrupted.intel8080_state());
    }

    #[test]
    fn stopped_machine_does_not_consume_frame_budget() {
        let mut machine = machine();
        machine.set_running(false);
        assert_eq!(
            run_cpu_frame_timed(
                &mut machine,
                UNLIMITED_CHUNK_T_STATES,
                CPU_FRAME_TIME,
                TWO_MHZ,
                EmulationSpeed::Unlimited,
            ),
            0
        );
    }
}
