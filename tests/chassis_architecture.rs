mod machine_probe {
    use std::time::Duration;

    #[derive(Default)]
    struct PanelProbe {
        address: u16,
    }

    impl PanelProbe {
        fn set_address_latch(&mut self, address: u16) {
            self.address = address;
        }

        fn reset_address(&mut self) -> u16 {
            self.address = 0;
            0
        }
    }

    #[derive(Default)]
    pub(super) struct AltairBus {
        panel: PanelProbe,
        run: bool,
        inte: bool,
        hlda: bool,
        reset: bool,
        ready: bool,
        hold: bool,
        protected: bool,
        frozen: bool,
        memory_initialized: bool,
        powered_off: bool,
        last_dynamic: Option<bool>,
    }

    impl AltairBus {
        fn cancel_cpu_diagnostic_meter(&mut self) {}
        fn clear_transient_memory_guards(&mut self) {}
        fn clear_serial(&mut self) {}

        fn clear_protection(&mut self) {
            self.protected = false;
        }

        fn set_run(&mut self, run: bool) {
            self.run = run;
        }

        fn sync_cpu_inte(&mut self, enabled: bool) {
            self.inte = enabled;
        }

        fn set_hlda(&mut self, hlda: bool) {
            self.hlda = hlda;
        }

        fn hlda(&self) -> bool {
            self.hlda
        }

        fn drive_power_on_state(&mut self, address: u16, run: bool) {
            self.panel.address = address;
            self.run = run;
        }

        fn initialize_memory(&mut self) {
            self.memory_initialized = true;
        }

        fn power_off_s100(&mut self) {
            self.powered_off = true;
        }

        fn assert_front_panel_reset_bus(&mut self, run: bool) {
            self.reset = true;
            self.run = run;
        }

        fn release_front_panel_reset_bus(&mut self, address: u16, run: bool) {
            self.reset = false;
            self.panel.address = address;
            self.run = run;
        }

        fn reset_asserted(&self) -> bool {
            self.reset
        }

        fn commit_panel_activity(&mut self, _dt: Duration, dynamic: bool) {
            self.last_dynamic = Some(dynamic);
        }

        fn hold_requested(&self) -> bool {
            self.hold
        }

        fn panel_address(&self) -> u16 {
            self.panel.address
        }

        fn set_protected(&mut self, _address: u16, protected: bool) {
            self.protected = protected;
        }

        fn freeze_panel_bus(&mut self) {
            self.frozen = true;
        }

        fn cycle_set_ready_input(&mut self, ready: bool) {
            self.ready = ready;
        }
    }

    mod chassis {
        include!("../src/machine/chassis.rs");
    }

    pub(super) fn exercise_cycle_chassis_contracts() {
        let mut chassis = chassis::AltairChassis::default();
        assert!(!chassis.powered);
        assert!(!chassis.running);

        chassis.cycle_power_chassis(true, true, 0x1234, true);
        assert!(chassis.powered);
        assert!(chassis.running);
        assert!(chassis.bus.run);
        assert!(chassis.bus.inte);
        assert_eq!(chassis.bus.panel.address, 0x1234);

        chassis.cycle_commit_panel_activity(Duration::from_millis(1), false);
        assert_eq!(chassis.bus.last_dynamic, Some(true));

        // While HALT suppresses PSYNC, STOP is physically held but must not yet
        // clear the RUN latch. The next exact PSYNC captures it.
        chassis.cycle_assert_run_stop(false, true, false);
        assert!(chassis.running);
        assert!(chassis.cycle_capture_pending_stop_at_psync());
        assert!(!chassis.running);
        assert!(!chassis.bus.ready);

        chassis.cycle_front_panel_set_memory_protection(true, false, false);
        assert!(chassis.bus.protected);
        assert!(chassis.bus.frozen);

        chassis.cycle_assert_front_panel_reset_from_cpu();
        assert!(chassis.bus.reset);
        assert!(!chassis.bus.inte);
        assert!(!chassis.bus.hlda);
        assert_eq!(chassis.bus.panel.address, 0);

        // RUN is the asynchronous SET input of the physical R-S latch and can
        // therefore be asserted while RESET remains held.
        chassis.cycle_assert_run_stop(true, false, false);
        assert!(chassis.running);
        assert!(chassis.bus.run);
        assert!(chassis.bus.ready);

        chassis.cycle_release_front_panel_reset_from_cpu();
        assert!(!chassis.bus.reset);

        chassis.cycle_power_chassis(false, false, 0, false);
        assert!(!chassis.powered);
        assert!(!chassis.running);
        assert!(chassis.bus.memory_initialized);
        assert!(chassis.bus.powered_off);
    }
}

#[test]
fn staged_cycle_chassis_control_contracts_compile_in_isolation() {
    machine_probe::exercise_cycle_chassis_contracts();
}

#[test]
fn staged_chassis_contains_no_cpu_or_deref_escape_hatch() {
    let source = include_str!("../src/machine/chassis.rs");

    assert!(source.contains("struct AltairChassis"));
    assert!(!source.contains("Cpu8080"), "the physical chassis must not own a Fast CPU");
    assert!(!source.contains("Deref"), "the chassis migration must not hide ownership behind Deref");
    assert!(!source.contains("DerefMut"), "the chassis migration must preserve explicit field ownership");
}
