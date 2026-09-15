use super::*;

pub(super) const SWITCH_COUNT: usize = 25;
pub(super) const SWITCH_UNIFORM_BYTES: u64 = SWITCH_COUNT as u64 * 32;
pub(super) const MOMENTARY_FIRST_INDEX: usize = 17;
const MOMENTARY_SWITCH_COUNT: usize = SWITCH_COUNT - MOMENTARY_FIRST_INDEX;
const POWER_SWITCH_INDEX: usize = 16;
const MOMENTARY_LATCH_HOLD: Duration = Duration::from_secs(3);

const SWITCH_IDS: [&str; SWITCH_COUNT] = [
    "A15",
    "A14",
    "A13",
    "A12",
    "A11",
    "A10",
    "A09",
    "A08",
    "A07",
    "A06",
    "A05",
    "A04",
    "A03",
    "A02",
    "A01",
    "A00",
    "POWER",
    "STOP_RUN",
    "SINGLE_STEP",
    "EXAMINE",
    "DEPOSIT",
    "RESET_CLR",
    "PROTECT",
    "AUX1",
    "AUX2",
];

const MOMENTARY_LABELS: [&str; MOMENTARY_SWITCH_COUNT] = [
    "STOP / RUN",
    "SINGLE STEP",
    "EXAMINE / EXAMINE NEXT",
    "DEPOSIT / DEPOSIT NEXT",
    "RESET / CLR",
    "PROTECT / UNPROTECT",
    "AUX 1 (unassigned)",
    "AUX 2 (unassigned)",
];

#[derive(Clone, Copy, Debug)]
pub(super) struct SwitchRuntime {
    pub(super) pivot: [f32; 3],
    pub(super) axis: [f32; 3],
    pub(super) rest: f32,
    pub(super) radians_per_state: f32,
    pub(super) xy_radius: f32,
}

#[derive(Debug)]
struct SwitchBinding {
    lever: String,
    axis: [f32; 3],
    rest: f32,
    radians_per_state: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlDirection {
    Up,
    Down,
}

impl ControlDirection {
    fn is_down(self) -> bool {
        matches!(self, Self::Down)
    }

    fn state_value(self) -> f32 {
        if self.is_down() { -1.0 } else { 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MomentaryControlState {
    latched: Option<ControlDirection>,
    press_started: Option<Instant>,
    press_direction: Option<ControlDirection>,
    press_began_on_latched: bool,
    long_latched_this_press: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ControlState {
    switches: [MomentaryControlState; MOMENTARY_SWITCH_COUNT],
    active_index: Option<usize>,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            switches: [MomentaryControlState::default(); MOMENTARY_SWITCH_COUNT],
            active_index: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ControlTransition {
    action: Option<bool>,
    pressed: Option<bool>,
    released: Option<bool>,
    just_latched: bool,
    released_latch: bool,
}

fn load_switch_bindings() -> Result<Vec<SwitchBinding>, String> {
    let bytes = embedded_assets::get(BINDINGS_PATH)
        .ok_or_else(|| format!("missing embedded runtime asset: {BINDINGS_PATH}"))?;
    let mut parser = JsonParser::new(bytes);
    let root = parser.parse()?;
    let switches = json_array(root.require("switches")?)?;
    if switches.len() != SWITCH_COUNT {
        return Err(format!(
            "Altair 3D bindings define {} switches; expected {SWITCH_COUNT}",
            switches.len()
        ));
    }

    let mut bindings = Vec::with_capacity(SWITCH_COUNT);
    for (index, switch) in switches.iter().enumerate() {
        let id = json_string(switch.require("id")?)?;
        if id != SWITCH_IDS[index] {
            return Err(format!(
                "Altair 3D switch binding {index} is {id:?}; expected {:?}",
                SWITCH_IDS[index]
            ));
        }
        let axis_values = json_array(switch.require("axis")?)?;
        if axis_values.len() != 3 {
            return Err(format!("Altair 3D switch {id} axis is not a vec3"));
        }
        bindings.push(SwitchBinding {
            lever: json_string(switch.require("lever")?)?.to_owned(),
            axis: [
                json_f32(&axis_values[0])?,
                json_f32(&axis_values[1])?,
                json_f32(&axis_values[2])?,
            ],
            rest: json_f32(switch.require("rest")?)?,
            radians_per_state: json_f32(switch.require("radians_per_state")?)?,
        });
    }
    Ok(bindings)
}

pub(super) fn load_switch_lever_indices() -> Result<HashMap<String, u32>, String> {
    let bindings = load_switch_bindings()?;
    Ok(bindings
        .into_iter()
        .enumerate()
        .map(|(index, binding)| (binding.lever, index as u32))
        .collect())
}

pub(super) fn load_switch_runtime() -> Result<[SwitchRuntime; SWITCH_COUNT], String> {
    let bindings = load_switch_bindings()?;
    let lever_indices: HashMap<&str, usize> = bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| (binding.lever.as_str(), index))
        .collect();

    let glb = embedded_assets::get(GLB_PATH)
        .ok_or_else(|| format!("missing embedded runtime asset: {GLB_PATH}"))?;
    let (root, bin) = parse_glb(glb)?;
    let scene_index = root.get("scene").map(json_usize).transpose()?.unwrap_or(0);
    let scenes = json_array(root.require("scenes")?)?;
    let scene = scenes
        .get(scene_index)
        .ok_or_else(|| format!("glTF scene index {scene_index} is out of range"))?;
    let roots = json_array(scene.require("nodes")?)?;

    let mut runtime = vec![None; SWITCH_COUNT];
    for node in roots {
        collect_switch_runtime(
            &root,
            bin,
            json_usize(node)?,
            Mat4::identity(),
            &bindings,
            &lever_indices,
            &mut runtime,
        )?;
    }

    let resolved = runtime
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            entry.ok_or_else(|| {
                format!(
                    "Altair runtime GLB is missing bound switch lever {:?}",
                    bindings[index].lever
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    resolved
        .try_into()
        .map_err(|_| "Altair 3D switch runtime count changed unexpectedly".to_owned())
}

fn collect_switch_runtime(
    root: &JsonValue,
    bin: &[u8],
    node_index: usize,
    parent_transform: Mat4,
    bindings: &[SwitchBinding],
    lever_indices: &HashMap<&str, usize>,
    runtime: &mut [Option<SwitchRuntime>],
) -> Result<(), String> {
    let nodes = json_array(root.require("nodes")?)?;
    let node = nodes
        .get(node_index)
        .ok_or_else(|| format!("glTF node index {node_index} is out of range"))?;
    let world = parent_transform.mul(node_transform(node)?);

    if let Some(name) = node.get("name").map(json_string).transpose()? {
        if let Some(&switch_index) = lever_indices.get(name) {
            if runtime[switch_index].is_some() {
                return Err(format!("Altair runtime GLB repeats bound lever {name:?}"));
            }
            let binding = &bindings[switch_index];
            // The package reference adapter animates the lever by setting the
            // lever node quaternion directly. glTF therefore defines the
            // mechanical hinge at that node's local origin. Do not apply the
            // manifest's informational pivot offset a second time.
            let pivot = world.transform_point([0.0, 0.0, 0.0]);
            let axis = normalize3(world.transform_vector(binding.axis));
            let xy_radius = lever_xy_radius(root, bin, node, world, pivot)?;
            runtime[switch_index] = Some(SwitchRuntime {
                pivot,
                axis,
                rest: binding.rest,
                radians_per_state: binding.radians_per_state,
                xy_radius,
            });
        }
    }

    if let Some(children) = node.get("children") {
        for child in json_array(children)? {
            collect_switch_runtime(
                root,
                bin,
                json_usize(child)?,
                world,
                bindings,
                lever_indices,
                runtime,
            )?;
        }
    }
    Ok(())
}

fn lever_xy_radius(
    root: &JsonValue,
    bin: &[u8],
    node: &JsonValue,
    world: Mat4,
    pivot: [f32; 3],
) -> Result<f32, String> {
    let mesh_index = json_usize(node.require("mesh")?)?;
    let meshes = json_array(root.require("meshes")?)?;
    let mesh = meshes
        .get(mesh_index)
        .ok_or_else(|| format!("glTF mesh index {mesh_index} is out of range"))?;
    let primitives = json_array(mesh.require("primitives")?)?;
    let mut radius_sq = 0.0f32;
    for primitive in primitives {
        let attributes = primitive.require("attributes")?;
        let position_accessor = json_usize(attributes.require("POSITION")?)?;
        for position in read_vec3_f32_accessor(root, bin, position_accessor)? {
            let world_position = world.transform_point(position);
            let dx = world_position[0] - pivot[0];
            let dy = world_position[1] - pivot[1];
            radius_sq = radius_sq.max(dx * dx + dy * dy);
        }
    }
    if radius_sq <= f32::EPSILON {
        return Err("bound Altair switch lever has no usable XY extent".into());
    }
    Ok(radius_sq.sqrt() + 0.00035)
}

fn authored_switch_state(index: usize, live_state: f32) -> f32 {
    // The live snapshot uses +1 for an active/true boolean. Address switches
    // naturally share that polarity (+1 = physical UP = 1). POWER is the one
    // inverse control in the manifest: +1 is UP/OFF and -1 is DOWN/ON.
    if index == POWER_SWITCH_INDEX {
        -live_state
    } else {
        live_state
    }
}

pub(super) fn encode_switch_uniform(
    runtime: &[SwitchRuntime; SWITCH_COUNT],
    states: [f32; SWITCH_COUNT],
) -> Vec<u8> {
    let mut bytes = vec![0u8; SWITCH_UNIFORM_BYTES as usize];
    for (index, switch) in runtime.iter().enumerate() {
        let offset = index * 32;
        let authored_state = authored_switch_state(index, states[index]);
        let angle = (authored_state - switch.rest) * switch.radians_per_state;
        for (slot, value) in [switch.pivot[0], switch.pivot[1], switch.pivot[2], angle]
            .into_iter()
            .enumerate()
        {
            let start = offset + slot * 4;
            bytes[start..start + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (slot, value) in [
            switch.axis[0],
            switch.axis[1],
            switch.axis[2],
            switch.xy_radius,
        ]
        .into_iter()
        .enumerate()
        {
            let start = offset + 16 + slot * 4;
            bytes[start..start + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn begin_press(
    state: &mut MomentaryControlState,
    direction: ControlDirection,
    now: Instant,
) -> ControlTransition {
    let already_latched = state.latched.is_some();
    state.press_started = Some(now);
    state.press_direction = Some(direction);
    state.press_began_on_latched = already_latched;
    state.long_latched_this_press = false;
    ControlTransition {
        pressed: (!already_latched).then_some(direction.is_down()),
        ..ControlTransition::default()
    }
}

fn advance_press(
    state: &mut MomentaryControlState,
    now: Instant,
    primary_down: bool,
    primary_released: bool,
) -> ControlTransition {
    let mut transition = ControlTransition::default();

    if state.press_started.is_some()
        && primary_down
        && !state.press_began_on_latched
        && state.latched.is_none()
        && !state.long_latched_this_press
        && state
            .press_started
            .is_some_and(|started| now.duration_since(started) >= MOMENTARY_LATCH_HOLD)
    {
        let direction = state.press_direction.unwrap_or(ControlDirection::Up);
        state.latched = Some(direction);
        state.long_latched_this_press = true;
        transition.action = Some(direction.is_down());
        transition.just_latched = true;
    }

    if state.press_started.is_some() && primary_released {
        if state.press_began_on_latched {
            let direction = state
                .latched
                .or(state.press_direction)
                .unwrap_or(ControlDirection::Up);
            state.latched = None;
            transition.released = Some(direction.is_down());
            transition.released_latch = true;
        } else if !state.long_latched_this_press {
            let direction = state.press_direction.unwrap_or(ControlDirection::Up);
            let down = direction.is_down();
            transition.action = Some(down);
            transition.released = Some(down);
        }
        clear_press(state);
    } else if state.press_started.is_some() && !primary_down && !primary_released {
        if !state.press_began_on_latched && !state.long_latched_this_press {
            if let Some(direction) = state.press_direction {
                transition.released = Some(direction.is_down());
            }
        }
        clear_press(state);
    }

    transition
}

fn clear_press(state: &mut MomentaryControlState) {
    state.press_started = None;
    state.press_direction = None;
    state.press_began_on_latched = false;
    state.long_latched_this_press = false;
}

fn visual_direction(state: &MomentaryControlState) -> Option<ControlDirection> {
    if state.press_started.is_some() {
        if state.press_began_on_latched {
            state.latched
        } else {
            state.latched.or(state.press_direction)
        }
    } else {
        state.latched
    }
}

pub(super) fn write_control_states(
    state: &ControlState,
    states: &mut [f32; SWITCH_COUNT],
) {
    for (local_index, control) in state.switches.iter().enumerate() {
        states[MOMENTARY_FIRST_INDEX + local_index] = visual_direction(control)
            .map(ControlDirection::state_value)
            .unwrap_or(0.0);
    }
}

fn picked_direction(
    scene: &SwitchPickScene,
    rect: egui::Rect,
    pointer: egui::Pos2,
    camera: CameraState,
    index: usize,
) -> Option<ControlDirection> {
    let switch = scene.switches.get(index)?;
    let (origin, direction) = pointer_ray(scene, rect, pointer, camera)?;
    let radius = (switch.xy_radius * SWITCH_PICK_RADIUS_SCALE)
        .clamp(SWITCH_PICK_RADIUS_MIN_M, SWITCH_PICK_RADIUS_MAX_M);
    let distance = ray_sphere_distance(origin, direction, switch.pivot, radius)?;
    let hit = add3(origin, scale3(direction, distance));
    Some(if hit[1] < switch.pivot[1] {
        ControlDirection::Down
    } else {
        ControlDirection::Up
    })
}

pub(super) fn handle_control_input(
    app: &mut RusTairApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    camera: CameraState,
    state: &mut ControlState,
) -> bool {
    let now = Instant::now();
    let (primary_down, primary_pressed, primary_released, pointer_pos) = ui.input(|input| {
        (
            input.pointer.primary_down(),
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
            input.pointer.interact_pos(),
        )
    });
    let mut suppress_primary_orbit = state.active_index.is_some();

    if primary_pressed && response.is_pointer_button_down_on() && state.active_index.is_none() {
        if let (Some(pointer), Ok(scene)) = (pointer_pos, switch_pick_scene()) {
            if let Some(index) = pick_switch(scene, rect, pointer, camera) {
                if index >= MOMENTARY_FIRST_INDEX {
                    if let Some(direction) = picked_direction(scene, rect, pointer, camera, index) {
                        let local_index = index - MOMENTARY_FIRST_INDEX;
                        let transition = begin_press(&mut state.switches[local_index], direction, now);
                        state.active_index = Some(index);
                        suppress_primary_orbit = true;
                        apply_transition(app, ui.ctx(), index, transition);
                    }
                }
            }
        }
    }

    if let Some(index) = state.active_index {
        let local_index = index - MOMENTARY_FIRST_INDEX;
        let transition = advance_press(
            &mut state.switches[local_index],
            now,
            primary_down,
            primary_released,
        );
        apply_transition(app, ui.ctx(), index, transition);
        if state.switches[local_index].press_started.is_none() {
            state.active_index = None;
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(8));
        }
    }

    suppress_primary_orbit || state.active_index.is_some()
}

fn apply_transition(
    app: &mut RusTairApp,
    ctx: &egui::Context,
    index: usize,
    transition: ControlTransition,
) {
    if let Some(down) = transition.pressed {
        apply_pressed(app, ctx, index, down);
    }

    if let Some(down) = transition.action {
        app.audio.play_once("assets/click.mp3");
        if transition.just_latched {
            let label = MOMENTARY_LABELS[index - MOMENTARY_FIRST_INDEX];
            app.status = format!(
                "{label} held {} — click the switch to release it",
                if down { "DOWN" } else { "UP" }
            );
        }
        apply_action(app, index, down);
    }

    if let Some(down) = transition.released {
        if transition.released_latch {
            app.audio.play_once("assets/click.mp3");
            let label = MOMENTARY_LABELS[index - MOMENTARY_FIRST_INDEX];
            app.status = format!("{label} released to center");
        }
        apply_released(app, ctx, index, down);
    }
}

fn apply_pressed(app: &mut RusTairApp, ctx: &egui::Context, index: usize, down: bool) {
    match index {
        17 => {
            let run = down;
            app.machine.assert_run_stop(run);
            app.execution_clock.reset_at(Instant::now());
            let cpu = app.machine.intel8080_state();
            let panel = app.machine.front_panel_state();
            app.status = if !run && cpu.halted.unwrap_or(false) && panel.running {
                "STOP held while CPU is halted — no PSYNC to capture STOP; hold STOP and assert RESET"
                    .into()
            } else if run {
                "RUN asserted".into()
            } else {
                "STOP asserted".into()
            };
            ctx.request_repaint();
        }
        21 => {
            let clear = down;
            if clear {
                app.machine.assert_front_panel_clear();
                app.status =
                    "CLR held: S-100 EXT CLR asserted; installed I/O boards cleared".into();
            } else {
                app.machine.assert_front_panel_reset();
                app.execution_clock.reset_at(Instant::now());
                app.status =
                    "RESET held: ADDRESS/DATA on, status lamps off; RUN/STOP latch preserved"
                        .into();
            }
            ctx.request_repaint();
        }
        _ => {}
    }
}

fn apply_action(app: &mut RusTairApp, index: usize, down: bool) {
    match index {
        18 if !down => app.machine.step(),
        19 => app.machine.examine(down),
        20 => app.machine.deposit(down),
        22 => app.machine.protect_current_board(!down),
        _ => {}
    }
}

fn apply_released(app: &mut RusTairApp, ctx: &egui::Context, index: usize, down: bool) {
    match index {
        17 => app.machine.release_run_stop(down),
        21 => {
            if down {
                app.machine.release_front_panel_clear();
                app.status = "CLR released: S-100 EXT CLR inactive".into();
            } else {
                app.machine.release_front_panel_reset();
                app.execution_clock.reset_at(Instant::now());
                app.status = if app.machine.running() {
                    "RESET released: RUN latch preserved; execution resumes from 0000h".into()
                } else {
                    "RESET released: 0000h fetch held in WAIT".into()
                };
            }
            ctx.request_repaint();
        }
        _ => {}
    }
}

pub(super) fn release_all_controls(
    app: &mut RusTairApp,
    ctx: &egui::Context,
    state: &mut ControlState,
) {
    for (local_index, control) in state.switches.iter_mut().enumerate() {
        let index = MOMENTARY_FIRST_INDEX + local_index;
        if let Some(direction) = control.latched.or(control.press_direction) {
            apply_released(app, ctx, index, direction.is_down());
        }
        *control = MomentaryControlState::default();
    }
    state.active_index = None;
}

#[cfg(test)]
pub(super) fn rest_switch_states(runtime: &[SwitchRuntime; SWITCH_COUNT]) -> [f32; SWITCH_COUNT] {
    std::array::from_fn(|index| {
        if index == POWER_SWITCH_INDEX {
            -runtime[index].rest
        } else {
            runtime[index].rest
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_altair_bindings_define_expected_switch_order() {
        let bindings = load_switch_bindings().expect("embedded switch bindings must parse");
        assert_eq!(bindings.len(), SWITCH_COUNT);
        assert_eq!(bindings[0].lever, "SW_A15_LEVER");
        assert_eq!(bindings[15].lever, "SW_A00_LEVER");
        assert_eq!(bindings[16].lever, "SW_POWER_LEVER");
        assert_eq!(bindings[17].lever, "SW_STOP_RUN_LEVER");
        assert_eq!(bindings[24].lever, "SW_AUX2_LEVER");
    }

    #[test]
    fn embedded_altair_glb_resolves_every_bound_switch_lever() {
        let runtime = load_switch_runtime().expect("all bound switch levers must resolve");
        for (index, switch) in runtime.iter().enumerate() {
            assert!(switch.xy_radius > 0.0015 && switch.xy_radius < 0.0100);
            assert!((length3(switch.axis) - 1.0).abs() < 1.0e-5);
            assert!(
                switch.pivot.iter().all(|value| value.is_finite()),
                "{index}"
            );
        }
    }

    #[test]
    fn switch_uniform_rotates_from_authored_rest_not_from_zero() {
        let runtime = load_switch_runtime().unwrap();
        let rest = rest_switch_states(&runtime);
        let bytes = encode_switch_uniform(&runtime, rest);
        for index in 0..SWITCH_COUNT {
            assert_eq!(read_f32_le(&bytes, index * 32 + 12).unwrap(), 0.0);
        }

        let mut moved = rest;
        moved[0] = 1.0;
        let bytes = encode_switch_uniform(&runtime, moved);
        let expected = 2.0 * runtime[0].radians_per_state;
        assert!((read_f32_le(&bytes, 12).unwrap() - expected).abs() < 1.0e-6);
    }

    #[test]
    fn power_live_true_maps_to_physical_down_on() {
        let runtime = load_switch_runtime().unwrap();
        assert_eq!(runtime[POWER_SWITCH_INDEX].rest, 1.0);

        let mut states = rest_switch_states(&runtime);
        assert_eq!(states[POWER_SWITCH_INDEX], -1.0);
        let off = encode_switch_uniform(&runtime, states);
        assert_eq!(
            read_f32_le(&off, POWER_SWITCH_INDEX * 32 + 12).unwrap(),
            0.0
        );

        states[POWER_SWITCH_INDEX] = 1.0;
        let on = encode_switch_uniform(&runtime, states);
        let expected = -2.0 * runtime[POWER_SWITCH_INDEX].radians_per_state;
        assert!(
            (read_f32_le(&on, POWER_SWITCH_INDEX * 32 + 12).unwrap() - expected).abs() < 1.0e-6
        );
    }

    #[test]
    fn short_momentary_press_actions_on_release_and_returns_to_center() {
        let now = Instant::now();
        let mut state = MomentaryControlState::default();
        let pressed = begin_press(&mut state, ControlDirection::Up, now);
        assert_eq!(pressed.pressed, Some(false));
        assert_eq!(visual_direction(&state), Some(ControlDirection::Up));

        let released = advance_press(
            &mut state,
            now + Duration::from_millis(50),
            false,
            true,
        );
        assert_eq!(released.action, Some(false));
        assert_eq!(released.released, Some(false));
        assert_eq!(visual_direction(&state), None);
    }

    #[test]
    fn three_second_hold_latches_until_second_click_releases() {
        let now = Instant::now();
        let mut state = MomentaryControlState::default();
        let _ = begin_press(&mut state, ControlDirection::Down, now);

        let latched = advance_press(
            &mut state,
            now + MOMENTARY_LATCH_HOLD,
            true,
            false,
        );
        assert_eq!(latched.action, Some(true));
        assert!(latched.just_latched);
        assert_eq!(state.latched, Some(ControlDirection::Down));

        let released_after_latch = advance_press(
            &mut state,
            now + MOMENTARY_LATCH_HOLD + Duration::from_millis(10),
            false,
            true,
        );
        assert_eq!(released_after_latch, ControlTransition::default());
        assert_eq!(state.latched, Some(ControlDirection::Down));

        let second_press = begin_press(
            &mut state,
            ControlDirection::Down,
            now + Duration::from_secs(4),
        );
        assert_eq!(second_press.pressed, None);
        let second_release = advance_press(
            &mut state,
            now + Duration::from_secs(4) + Duration::from_millis(40),
            false,
            true,
        );
        assert_eq!(second_release.released, Some(true));
        assert!(second_release.released_latch);
        assert_eq!(state.latched, None);
    }
}