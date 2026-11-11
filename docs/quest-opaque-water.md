# Optional opaque water on Quest

`horizonOpaqueWater` is a saved VR boolean, off by default.

- Startup room: Settings → Graphics → additional graphics options → Opaque Water.
- In game: VR Settings → Rendering → Opaque Water (Visor and Vivecraft pages).

The mode keeps water geometry, texture coordinates, biome tint and lighting, but makes
water surfaces opaque. The bottom and objects behind the surface are hidden. Underwater
fog and fluid gameplay still use the original world state.

Near water moves into the depth-writing solid mesh and carries `OPAQUE_LAYER`, avoiding
both transparent sorting/blending and alpha cutout. Distant water uses the corresponding
solid material in the LOD mesher. Other block models, including the model of a waterlogged
block, are not moved along with their water.

Mesh workers capture the mode in immutable `WorkerContext` snapshots. A mode change resets
loaded section meshes, pending uploads and LOD, and requeues meshing. Existing sequence
checks discard older near-mesh results; resetting LOD stops its former workers. Terrain
can temporarily disappear while the replacement meshes arrive.

Selecting a shader pack preserves the preference but uses normal transparent water.
Returning to the built-in renderer reapplies the preference and rebuilds terrain.
This feature does not enable combinations of multiview, foveation and GPU culling that
are currently unsupported by the renderer.

## Validation

The resource-free waterlogged-block fixture checks transfer from transparent to solid
geometry, forced-opaque material, unchanged positions/UV/color/light and block model,
shader fallback and restoration when disabled. Existing settings and workspace checks
are also run. Actual Quest appearance, mode changes during streaming, LOD transitions,
underwater views and CPU/GPU/FPS improvement require device testing. No APK is built here.

Completed locally: 3 source/mesher tests, 141 VR-settings tests and the room-step
regression test passed. Workspace/all-target Clippy with warnings denied, formatting,
layer and public-tree checks passed. Prepared-resource mesher tests were not run.
