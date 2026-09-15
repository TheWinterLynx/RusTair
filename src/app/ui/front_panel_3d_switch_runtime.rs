use super::*;

pub(super) const SWITCH_COUNT: usize = 25;
pub(super) const SWITCH_UNIFORM_BYTES: u64 = SWITCH_COUNT as u64 * 32;

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
    pivot_local: [f32; 3],
    rest: f32,
    radians_per_state: f32,
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
        let pivot_values = json_array(switch.require("pivot_local_gltf_m")?)?;
        if pivot_values.len() != 3 {
            return Err(format!(
                "Altair 3D switch {id} pivot_local_gltf_m is not a vec3"
            ));
        }
        bindings.push(SwitchBinding {
            lever: json_string(switch.require("lever")?)?.to_owned(),
            axis: [
                json_f32(&axis_values[0])?,
                json_f32(&axis_values[1])?,
                json_f32(&axis_values[2])?,
            ],
            pivot_local: [
                json_f32(&pivot_values[0])?,
                json_f32(&pivot_values[1])?,
                json_f32(&pivot_values[2])?,
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
            let pivot = world.transform_point(binding.pivot_local);
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

pub(super) fn encode_switch_uniform(
    runtime: &[SwitchRuntime; SWITCH_COUNT],
    states: [f32; SWITCH_COUNT],
) -> Vec<u8> {
    let mut bytes = vec![0u8; SWITCH_UNIFORM_BYTES as usize];
    for (index, switch) in runtime.iter().enumerate() {
        let offset = index * 32;
        let angle = (states[index] - switch.rest) * switch.radians_per_state;
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

#[cfg(test)]
pub(super) fn rest_switch_states(
    runtime: &[SwitchRuntime; SWITCH_COUNT],
) -> [f32; SWITCH_COUNT] {
    std::array::from_fn(|index| runtime[index].rest)
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
        assert_eq!(bindings[0].pivot_local, [0.0, 0.0, 0.0032]);
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
}
