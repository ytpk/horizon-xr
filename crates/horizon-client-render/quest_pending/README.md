Quest headset render files not wired in yet (see docs/quest-port-to-starling.md, "Render: what is left").
They are copied unchanged from the Quest fork and are not compiled: they are written against upstream
Horizon's renderer of 2026-11-10 (Renderer fields, `fluid.rs`, `terrain_model.rs`, `surface_setup.rs`,
`SolidPipeline`, `Frame::world_primitives`...) which Starling's gfx.rs (upstream of 2026-11-02) does not
have yet. The Quest changes to gfx.rs itself are in docs/quest-port-pending/gfx.rs.quest.diff.
