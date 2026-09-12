//! Pertec FD-400 mechanics and MITS disk-unit timing foundation.
//!
//! Phase 3 deliberately models mechanics without media bytes. Time is supplied
//! explicitly by the caller, so an idle drive is query-derived rather than
//! advanced by a 2 MHz polling loop. The fixed-point unit is one sixth of a
//! microsecond: one Altair 2 MHz T-state is exactly three units, a 360 RPM
//! revolution is exactly 1,000,000 units, and one of 32 hard sectors is exactly
//! 31,250 units. This keeps long-running rotational phase deterministic without
//! accumulating floating-point drift.
//!
//! Source-backed base timings come from the July 1977 MITS 88-DCDD Operator's
//! Guide / contemporary MITS disk specifications: 360 RPM, 32 hard sectors plus
//! index, 77 tracks, 10 ms track-to-track access, 40 ms Head Status after head
//! load or a step with the head already loaded, and a 30 us Sector True window.
//! Media/read/write electronics remain later phases.

#![allow(dead_code)] // Phase 3 engine is intentionally consumed by later disk phases.

const TIME_UNITS_PER_MICROSECOND: u64 = 6;
const TIME_UNITS_PER_8080_T_STATE: u64 = 3;
const REVOLUTION_TIME_UNITS: u64 = 1_000_000;
const SECTORS_PER_REVOLUTION: u64 = 32;
const SECTOR_TIME_UNITS: u64 = REVOLUTION_TIME_UNITS / SECTORS_PER_REVOLUTION;
const SECTOR_TRUE_TIME_UNITS: u64 = 30 * TIME_UNITS_PER_MICROSECOND;
const TRACK_TO_TRACK_TIME_UNITS: u64 = 10_000 * TIME_UNITS_PER_MICROSECOND;
const HEAD_STATUS_DELAY_UNITS: u64 = 40_000 * TIME_UNITS_PER_MICROSECOND;
const TRACK_COUNT: u8 = 77;
const LAST_TRACK: u8 = TRACK_COUNT - 1;
const MAX_DRIVES: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Fd400Time(u64);

impl Fd400Time {
    pub(super) const ZERO: Self = Self(0);

    pub(super) const fn from_units(units: u64) -> Self {
        Self(units)
    }

    pub(super) const fn from_microseconds(microseconds: u64) -> Self {
        Self(microseconds.saturating_mul(TIME_UNITS_PER_MICROSECOND))
    }

    pub(super) const fn from_8080_t_states(t_states: u64) -> Self {
        Self(t_states.saturating_mul(TIME_UNITS_PER_8080_T_STATE))
    }

    pub(super) const fn units(self) -> u64 {
        self.0
    }

    pub(super) const fn saturating_add(self, delta: u64) -> Self {
        Self(self.0.saturating_add(delta))
    }

    pub(super) const fn saturating_duration_since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Fd400RotationSnapshot {
    /// Number of complete revolutions since the deterministic rotation epoch.
    pub(super) revolution: u64,
    /// Angular offset from the index reference in fixed-point time units.
    pub(super) revolution_offset_units: u64,
    /// Current hard-sector position, 0 through 31.
    pub(super) sector: u8,
    /// Angular/time offset from the beginning of the current sector.
    pub(super) sector_offset_units: u64,
    /// Controller Sector True window derived from the hard-sector boundary.
    pub(super) sector_true: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Fd400StepDirection {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingStep {
    complete_at: Fd400Time,
    target_track: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Fd400MechanicalSnapshot {
    pub(super) powered: bool,
    pub(super) door_open: bool,
    pub(super) motor_on: bool,
    pub(super) track: u8,
    pub(super) track_zero: bool,
    pub(super) head_loaded: bool,
    pub(super) head_status: bool,
    pub(super) step_command_allowed: bool,
    pub(super) rotation: Fd400RotationSnapshot,
}

#[derive(Debug)]
pub(super) struct PertecFd400 {
    powered: bool,
    door_open: bool,
    motor_commanded_on: bool,
    rotation_epoch: Fd400Time,
    rotation_revolution_base: u64,
    rotation_offset_base: u64,
    track: u8,
    pending_step: Option<PendingStep>,
    next_step_allowed_at: Fd400Time,
    head_commanded_loaded: bool,
    head_ready_at: Option<Fd400Time>,
}

impl Default for PertecFd400 {
    fn default() -> Self {
        Self {
            powered: false,
            door_open: true,
            motor_commanded_on: false,
            rotation_epoch: Fd400Time::ZERO,
            rotation_revolution_base: 0,
            rotation_offset_base: 0,
            track: 0,
            pending_step: None,
            next_step_allowed_at: Fd400Time::ZERO,
            head_commanded_loaded: false,
            head_ready_at: None,
        }
    }
}

impl PertecFd400 {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn power_on(&self) -> bool {
        self.powered
    }

    pub(super) fn door_open(&self) -> bool {
        self.door_open
    }

    fn spindle_rotating(&self) -> bool {
        self.powered && self.motor_commanded_on
    }

    fn rotation_snapshot_without_settle(&self, now: Fd400Time) -> Fd400RotationSnapshot {
        let elapsed = if self.spindle_rotating() {
            now.saturating_duration_since(self.rotation_epoch)
        } else {
            0
        };
        let accumulated = self.rotation_offset_base.saturating_add(elapsed);
        let revolution = self
            .rotation_revolution_base
            .saturating_add(accumulated / REVOLUTION_TIME_UNITS);
        let revolution_offset_units = accumulated % REVOLUTION_TIME_UNITS;
        let sector = (revolution_offset_units / SECTOR_TIME_UNITS) as u8;
        let sector_offset_units = revolution_offset_units % SECTOR_TIME_UNITS;
        Fd400RotationSnapshot {
            revolution,
            revolution_offset_units,
            sector,
            sector_offset_units,
            sector_true: sector_offset_units < SECTOR_TRUE_TIME_UNITS,
        }
    }

    fn capture_rotation_at(&mut self, now: Fd400Time) {
        let snapshot = self.rotation_snapshot_without_settle(now);
        self.rotation_revolution_base = snapshot.revolution;
        self.rotation_offset_base = snapshot.revolution_offset_units;
        self.rotation_epoch = now;
    }

    pub(super) fn set_power(&mut self, powered: bool, now: Fd400Time) {
        if self.powered == powered {
            return;
        }
        self.capture_rotation_at(now);
        self.powered = powered;
        if !powered {
            self.pending_step = None;
            self.head_commanded_loaded = false;
            self.head_ready_at = None;
        }
    }

    pub(super) fn set_door_open(&mut self, open: bool) {
        self.door_open = open;
    }

    pub(super) fn set_motor_on(&mut self, on: bool, now: Fd400Time) {
        if self.motor_commanded_on == on {
            return;
        }
        self.capture_rotation_at(now);
        self.motor_commanded_on = on;
    }

    pub(super) fn set_head_loaded(&mut self, loaded: bool, now: Fd400Time) {
        if !loaded {
            self.head_commanded_loaded = false;
            self.head_ready_at = None;
            return;
        }
        if !self.powered {
            return;
        }
        self.head_commanded_loaded = true;
        self.head_ready_at = Some(now.saturating_add(HEAD_STATUS_DELAY_UNITS));
        self.next_step_allowed_at = self
            .next_step_allowed_at
            .max(now.saturating_add(HEAD_STATUS_DELAY_UNITS));
    }

    fn settle_step(&mut self, now: Fd400Time) {
        if self
            .pending_step
            .is_some_and(|pending| pending.complete_at <= now)
        {
            let pending = self.pending_step.take().expect("pending step exists");
            self.track = pending.target_track;
        }
    }

    pub(super) fn step(&mut self, direction: Fd400StepDirection, now: Fd400Time) -> bool {
        self.settle_step(now);
        if !self.powered || self.pending_step.is_some() || now < self.next_step_allowed_at {
            return false;
        }

        let target_track = match direction {
            Fd400StepDirection::In => self.track.saturating_add(1).min(LAST_TRACK),
            Fd400StepDirection::Out => self.track.saturating_sub(1),
        };
        let complete_at = now.saturating_add(TRACK_TO_TRACK_TIME_UNITS);
        self.pending_step = Some(PendingStep {
            complete_at,
            target_track,
        });
        self.next_step_allowed_at = complete_at;
        if self.head_commanded_loaded {
            self.head_ready_at = Some(now.saturating_add(HEAD_STATUS_DELAY_UNITS));
        }
        true
    }

    pub(super) fn snapshot(&mut self, now: Fd400Time) -> Fd400MechanicalSnapshot {
        self.settle_step(now);
        let head_status = self.powered
            && self.head_commanded_loaded
            && self.head_ready_at.is_some_and(|ready_at| now >= ready_at);
        Fd400MechanicalSnapshot {
            powered: self.powered,
            door_open: self.door_open,
            motor_on: self.spindle_rotating(),
            track: self.track,
            track_zero: self.track == 0,
            head_loaded: self.head_commanded_loaded,
            head_status,
            step_command_allowed: self.powered
                && self.pending_step.is_none()
                && now >= self.next_step_allowed_at,
            rotation: self.rotation_snapshot_without_settle(now),
        }
    }
}

#[derive(Debug)]
pub(super) struct MitsDiskBuffer {
    address: u8,
}

impl MitsDiskBuffer {
    pub(super) fn new(address: u8) -> Option<Self> {
        (address < MAX_DRIVES as u8).then_some(Self { address })
    }

    pub(super) const fn address(&self) -> u8 {
        self.address
    }
}

#[derive(Debug)]
pub(super) struct MitsDiskUnit {
    buffer: MitsDiskBuffer,
    cable_connected: bool,
    drive: PertecFd400,
}

impl MitsDiskUnit {
    pub(super) fn new(address: u8) -> Option<Self> {
        Some(Self {
            buffer: MitsDiskBuffer::new(address)?,
            cable_connected: true,
            drive: PertecFd400::new(),
        })
    }

    pub(super) const fn address(&self) -> u8 {
        self.buffer.address()
    }

    pub(super) fn set_cable_connected(&mut self, connected: bool) {
        self.cable_connected = connected;
    }

    pub(super) fn drive(&self) -> &PertecFd400 {
        &self.drive
    }

    pub(super) fn drive_mut(&mut self) -> &mut PertecFd400 {
        &mut self.drive
    }

    /// The July 1977 guide explicitly states that open door, drive power off, or
    /// absent controller/drive cable prevents Disk Control from being enabled.
    pub(super) fn selectable(&self) -> bool {
        self.cable_connected && self.drive.power_on() && !self.drive.door_open()
    }
}

#[derive(Debug)]
pub(super) struct Mits88DiskCableBus {
    units: [Option<MitsDiskUnit>; MAX_DRIVES],
}

impl Default for Mits88DiskCableBus {
    fn default() -> Self {
        Self {
            units: std::array::from_fn(|_| None),
        }
    }
}

impl Mits88DiskCableBus {
    pub(super) fn install(&mut self, unit: MitsDiskUnit) -> Option<MitsDiskUnit> {
        let index = unit.address() as usize;
        self.units[index].replace(unit)
    }

    pub(super) fn remove(&mut self, address: u8) -> Option<MitsDiskUnit> {
        self.units.get_mut(address as usize)?.take()
    }

    pub(super) fn drive_available(&self, address: u8) -> bool {
        self.units
            .get(address as usize)
            .and_then(Option::as_ref)
            .is_some_and(MitsDiskUnit::selectable)
    }

    pub(super) fn unit_mut(&mut self, address: u8) -> Option<&mut MitsDiskUnit> {
        self.units.get_mut(address as usize)?.as_mut()
    }

    pub(super) fn unit(&self, address: u8) -> Option<&MitsDiskUnit> {
        self.units.get(address as usize)?.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_time_exactly_represents_altair_t_states_rotation_and_sectors() {
        assert_eq!(Fd400Time::from_8080_t_states(1).units(), 3);
        assert_eq!(Fd400Time::from_microseconds(1).units(), 6);
        assert_eq!(REVOLUTION_TIME_UNITS, 1_000_000);
        assert_eq!(SECTOR_TIME_UNITS, 31_250);
        assert_eq!(REVOLUTION_TIME_UNITS / SECTOR_TIME_UNITS, 32);
    }

    #[test]
    fn rotation_is_epoch_derived_and_advances_without_cpu_execution() {
        let mut drive = PertecFd400::new();
        let t0 = Fd400Time::ZERO;
        drive.set_power(true, t0);
        drive.set_motor_on(true, t0);

        let at_start = drive.snapshot(t0);
        assert_eq!(at_start.rotation.sector, 0);
        assert!(at_start.rotation.sector_true);

        let sector_1 = drive.snapshot(Fd400Time::from_units(SECTOR_TIME_UNITS));
        assert_eq!(sector_1.rotation.sector, 1);
        assert_eq!(sector_1.rotation.sector_offset_units, 0);

        let one_rev = drive.snapshot(Fd400Time::from_units(REVOLUTION_TIME_UNITS));
        assert_eq!(one_rev.rotation.revolution, 1);
        assert_eq!(one_rev.rotation.sector, 0);
        assert_eq!(one_rev.rotation.revolution_offset_units, 0);
    }

    #[test]
    fn large_rotation_jump_matches_incremental_observation() {
        let mut incremental = PertecFd400::new();
        let mut jumped = PertecFd400::new();
        incremental.set_power(true, Fd400Time::ZERO);
        incremental.set_motor_on(true, Fd400Time::ZERO);
        jumped.set_power(true, Fd400Time::ZERO);
        jumped.set_motor_on(true, Fd400Time::ZERO);

        let target_units = REVOLUTION_TIME_UNITS * 1_000 + SECTOR_TIME_UNITS * 17 + 123;
        let mut t = 0;
        while t < target_units {
            t = (t + SECTOR_TIME_UNITS).min(target_units);
            let _ = incremental.snapshot(Fd400Time::from_units(t));
        }
        assert_eq!(
            incremental.snapshot(Fd400Time::from_units(target_units)),
            jumped.snapshot(Fd400Time::from_units(target_units))
        );
    }

    #[test]
    fn head_status_obeys_forty_millisecond_load_deadline() {
        let mut drive = PertecFd400::new();
        drive.set_power(true, Fd400Time::ZERO);
        drive.set_head_loaded(true, Fd400Time::ZERO);

        assert!(!drive.snapshot(Fd400Time::from_microseconds(39_999)).head_status);
        assert!(drive.snapshot(Fd400Time::from_microseconds(40_000)).head_status);
    }

    #[test]
    fn step_completes_at_ten_ms_and_head_status_waits_forty_ms() {
        let mut drive = PertecFd400::new();
        let loaded_at = Fd400Time::ZERO;
        drive.set_power(true, loaded_at);
        drive.set_head_loaded(true, loaded_at);
        let step_at = Fd400Time::from_microseconds(40_000);
        assert!(drive.snapshot(step_at).head_status);
        assert!(drive.step(Fd400StepDirection::In, step_at));

        assert_eq!(
            drive
                .snapshot(Fd400Time::from_microseconds(49_999))
                .track,
            0
        );
        let completed = drive.snapshot(Fd400Time::from_microseconds(50_000));
        assert_eq!(completed.track, 1);
        assert!(completed.step_command_allowed);
        assert!(!completed.head_status);

        assert!(
            drive
                .snapshot(Fd400Time::from_microseconds(80_000))
                .head_status
        );
    }

    #[test]
    fn track_limits_never_underflow_or_exceed_seventy_six() {
        let mut drive = PertecFd400::new();
        drive.set_power(true, Fd400Time::ZERO);
        assert!(drive.step(Fd400StepDirection::Out, Fd400Time::ZERO));
        assert_eq!(
            drive.snapshot(Fd400Time::from_microseconds(10_000)).track,
            0
        );

        drive.track = LAST_TRACK;
        drive.pending_step = None;
        drive.next_step_allowed_at = Fd400Time::from_microseconds(10_000);
        assert!(drive.step(
            Fd400StepDirection::In,
            Fd400Time::from_microseconds(10_000)
        ));
        assert_eq!(
            drive.snapshot(Fd400Time::from_microseconds(20_000)).track,
            LAST_TRACK
        );
    }

    #[test]
    fn drive_selection_requires_cable_power_and_closed_door() {
        let mut bus = Mits88DiskCableBus::default();
        let mut unit = MitsDiskUnit::new(3).unwrap();
        assert!(!unit.selectable());
        unit.drive_mut().set_power(true, Fd400Time::ZERO);
        assert!(!unit.selectable());
        unit.drive_mut().set_door_open(false);
        assert!(unit.selectable());
        unit.set_cable_connected(false);
        assert!(!unit.selectable());
        unit.set_cable_connected(true);
        bus.install(unit);
        assert!(bus.drive_available(3));
        assert!(!bus.drive_available(2));
    }

    #[test]
    fn sixteen_idle_units_require_no_time_advance_loop() {
        let mut bus = Mits88DiskCableBus::default();
        for address in 0..16 {
            let mut unit = MitsDiskUnit::new(address).unwrap();
            unit.drive_mut().set_power(true, Fd400Time::ZERO);
            unit.drive_mut().set_door_open(false);
            unit.drive_mut().set_motor_on(true, Fd400Time::ZERO);
            bus.install(unit);
        }

        // Time is not pushed through every drive. A single selected drive derives
        // its state directly from the supplied absolute virtual timestamp.
        let now = Fd400Time::from_units(REVOLUTION_TIME_UNITS * 50_000 + 777);
        let snapshot = bus.unit_mut(15).unwrap().drive_mut().snapshot(now);
        assert_eq!(snapshot.rotation.revolution, 50_000);
        assert!(bus.drive_available(0));
        assert!(bus.drive_available(15));
    }
}
