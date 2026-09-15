/** Runtime binding for a Three.js scene loaded from Altair8800_1975.glb.
 * No polling or CPU emulation is implemented here. The caller supplies emulator I/O.
 * +1 is physical UP, -1 physical DOWN; 0 is neutral on momentary controls.
 */
export function bindAltair(root, manifest, onInput = () => {}) {
  const switches = new Map(), leds = new Map();
  for (const spec of manifest.switches) {
    const assembly = root.getObjectByName(spec.node);
    const lever = root.getObjectByName(spec.lever);
    if (!assembly || !lever) throw new Error(`Missing switch ${spec.id}`);
    switches.set(spec.id, { spec, assembly, lever, state: spec.rest });
  }
  for (const spec of manifest.leds) {
    const node = root.getObjectByName(spec.node);
    if (!node) throw new Error(`Missing LED ${spec.id}`);
    // A loader can create a child mesh for a multi-primitive node.
    const meshes = [];
    node.traverse(o => { if (o.isMesh) meshes.push(o); });
    const flange = root.getObjectByName(`${spec.node}_Flange`);
    if (flange) flange.traverse(o => { if (o.isMesh) meshes.push(o); });
    if (!meshes.length) throw new Error(`LED ${spec.id} has no mesh`);
    // Copy once to guarantee isolation even when a consuming engine deduplicates materials.
    const materials = meshes.flatMap(mesh => {
      const list = (Array.isArray(mesh.material) ? mesh.material : [mesh.material]).map(m => m.clone());
      mesh.material = Array.isArray(mesh.material) ? list : list[0];
      return list;
    });
    leds.set(spec.id, { spec, materials });
  }
  function setSwitch(id, state, emit = false) {
    const s = switches.get(id);
    if (!s || !s.spec.states.includes(state)) throw new RangeError(`Invalid switch/state: ${id}/${state}`);
    s.state = state;
    // Blender -Y maps to glTF +Z; the pivot axis remains local X.
    const a = s.spec.radians_per_state * state;
    s.lever.quaternion.set(Math.sin(a / 2), 0, 0, Math.cos(a / 2));
    s.assembly.userData.switch_state = state;
    if (emit) onInput({ id, state, momentary: s.spec.momentary,
      function: state === 1 ? s.spec.up : state === -1 ? s.spec.down : 'RELEASE' });
    return state;
  }
  function release(id) {
    const s = switches.get(id);
    if (!s) throw new RangeError(`Unknown switch: ${id}`);
    if (s.spec.momentary) setSwitch(id, 0, true);
  }
  function toggle(id) {
    const s = switches.get(id);
    if (!s || s.spec.momentary) throw new RangeError(`Use press(id, direction) for momentary controls: ${id}`);
    return setSwitch(id, -s.state, true);
  }
  function setLED(id, intensity) {
    const led = leds.get(id);
    if (!led || !Number.isFinite(intensity) || intensity < 0 || intensity > 1)
      throw new RangeError(`Invalid LED/intensity: ${id}/${intensity}`);
    for (const m of led.materials) {
      m.emissive.setRGB(...led.spec.on_emissive);
      m.emissiveIntensity = intensity * led.spec.on_strength;
    }
  }
  function hitSwitch(object) {
    for (let o = object; o; o = o.parent) {
      const id = o.userData?.logical_id;
      if (id && switches.has(id) && ['switch', 'switch_lever'].includes(o.userData.component_type)) return id;
      // Works if a loader strips extras from primitives but preserves node names.
      for (const [key, s] of switches) if (o === s.assembly || o === s.lever) return key;
    }
    return null;
  }
  for (const [id, s] of switches) setSwitch(id, s.spec.rest);
  for (const id of leds.keys()) setLED(id, 0);
  return { switches, leds, setSwitch, release, toggle, setLED, hitSwitch,
    press: (id, direction) => setSwitch(id, direction, true),
    allOff: () => { for (const id of leds.keys()) setLED(id, 0); } };
}
