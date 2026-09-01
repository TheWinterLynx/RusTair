use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{
    SerialBoard, SioHardwareConfig, SioInterruptTarget, SioInterruptWiring,
};

#[test]
fn both_backends_preserve_irq_wiring_inside_the_same_physical_sio_card() {
    let mut hardware = SioHardwareConfig::default();
    hardware.interrupt_wiring = SioInterruptWiring {
        input: SioInterruptTarget::Vi3,
        output: SioInterruptTarget::Disconnected,
    };

    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).unwrap();
        host.configure_sio_hardware(hardware);
        host.configure_serial_board(SerialBoard::Sio88);
        assert_eq!(host.sio_hardware(), hardware);
        assert_eq!(host.sio_hardware().interrupt_wiring.input, SioInterruptTarget::Vi3);
        assert_eq!(
            host.sio_hardware().interrupt_wiring.output,
            SioInterruptTarget::Disconnected
        );
    }
}
