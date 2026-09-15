# Altair 3D rendering

The runtime GLB is rendered by `src/app/ui/front_panel_3d.rs`, independently of
Blender. Matching the mesh alone does not reproduce Blender's studio lighting.

The rendering path keeps glTF base colours in linear space, imports the authored
metallic and roughness factors, and uses GGX direct illumination plus a neutral
procedural reflection environment. The camera position drives the specular view
vector. Highlights are compressed before writing the offscreen sRGB texture.
There is no external HDRI, path-traced shadowing or Blender AgX transform, so this
is a real-time material preview, not a pixel-identical copy of a Blender render.
The current asset uses material factors and geometric graphics, not image textures.

Presentation must account for egui's destination format. Sampling the offscreen
sRGB texture **decodes it to linear**. On an UNORM egui surface we explicitly
encode linear to sRGB; on an sRGB surface the attachment performs that encoding.
Omitting this step made charcoal almost black and blue excessively saturated.
Never fix that mismatch by editing the asset colours or arbitrarily raising lights.

Four-sample MSAA is resolved into the offscreen texture before presentation. The
near plane adapts to distance from the model's bounding sphere, preserving depth
precision for the closely layered front-panel graphics. World normals use the
inverse transpose so nonuniform node scales do not change the lighting incorrectly.
No mesh asset, front-panel binding or emulated hardware state is changed by this path.

## Regression checks

`cargo test --locked --all-targets` includes material import, vertex/uniform layout,
normal transform, GLB mesh and LED binding checks. The real GPU check is opt-in
because CI and headless development machines may not have a compatible adapter:

```powershell
cargo test --locked --lib gpu_color_and_material_render -- --ignored
```

It renders the actual model with both UNORM and sRGB destinations and compares
readbacks. It also checks that linear 18% gray reaches the display as approximately
118/255, detecting missing or duplicated sRGB conversion. Both shader pipelines,
MSAA resolve and camera/material bindings are exercised on the adapter.

To save that actual emulator render for visual inspection:

```powershell
$env:RUSTAIR_3D_CAPTURE_DIR="$PWD\target\altair-3d-capture"; cargo test --locked --lib gpu_color_and_material_render -- --ignored
```
