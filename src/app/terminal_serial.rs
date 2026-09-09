use super::*;

impl RusTairApp {
    /// Drive whichever physical serial port the Text Terminal cable is attached
    /// to. Window visibility does not affect the cable: a hidden terminal keeps
    /// receiving guest output until the user explicitly disconnects it.
    pub(in crate::app) fn process_terminal_serial(&mut self, ctx: &egui::Context) {
        let Some(port) = Self::backend_serial_port(self.terminal_connection()) else {
            self.terminal.tx_started = None;
            return;
        };
        let machine = &mut self.machine;
        if let Some(delay) = self.terminal.receive_output(Instant::now(), || machine.serial_tx_complete(port)) {
            ctx.request_repaint_after(delay);
        }
    }
}
