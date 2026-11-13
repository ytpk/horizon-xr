# RD24: supplied Quest measurements and revised profiles

These are the user's device results, supplied after the local Adreno audit. Codex did not
run the headset benchmark or inspect its raw logs. The user reports 12 clean runs, with
player position saved before the final comparisons. Earlier warm-up results around
65 FPS are **not comparable**: without the saved position the visible chunk count varied
from 968 to 1344. Keep that position/save, scene and eye resolution fixed for follow-ups.

| Variant | Runs | FPS | World GPU ms | Indices | Relevant settings |
| --- | ---: | ---: | ---: | ---: | --- |
| A, default, 90 Hz | 3 | 41.7 | 22.9 | 7.71 M | vanilla look |
| A72, 72 Hz control | 1 | 43.1 | 22.4 | 7.71 M | otherwise A |
| Bnf, balanced without foveation, 72 Hz | 3 | 46.9 | 20.5 | 6.15 M | aggressive merge, shrunken quads, 50% grass |
| Cnf, throughput without foveation, 120 Hz | 3 | 43.6 | 21.7 | 6.21 M | no grass, nearest lightmap, split streams |

Two further single runs with foveation reportedly changed FPS by +0.4 for B and 0 for C.
They do not establish a repeatable foveation benefit. It also violates the user's
no-blur preference, so revised profiles explicitly disable it. CPU application time was
5.5–5.9 ms across the variants: this stationary scene remains GPU-bound.

Bnf improves FPS by about 12.5% versus A and reduces indices by about 20.2%. Relative to
the same-refresh A72 control it gains 3.8 FPS and 1.9 ms of world GPU time. A72 itself is
only one run; separate it from the repeated profile comparisons rather than assigning
precise causal shares to refresh and geometry changes.

Cnf is not a recommended upgrade over Bnf. Its reported fragment rates are 1002 M/s
versus 680 M/s for Bnf. Dividing by their respective rendered FPS gives approximately
23.0 M versus 14.5 M fragments per rendered frame (+58.5%). A higher requested display
rate alone does not establish the reason for that difference. Nearest lightmap and split
streams were not isolated, and neither should intrinsically increase raster coverage.
Before attributing it, also compare actual eye sizes, draw paths, visibility/overdraw,
counter definitions and runtime behavior in single-switch tests at the same refresh.
Keep both switches off in the recommended baseline.

## Revised launch files

- `packaging/quest/profiles/rd24-balanced.launch.txt`: Bnf settings, 72 Hz, no foveation,
  nearest sampling or split streams. Affinity experiments are disabled.
- `packaging/quest/profiles/rd24-spacewarp36.launch.txt`: the same geometry/settings,
  requesting 36→72 Application SpaceWarp. First headset test candidate.
- `packaging/quest/profiles/rd24-spacewarp45.launch.txt`: 45→90 candidate, requiring much
  tighter sustained GPU timing. Not a promise of achieving 45 real frames per second.
- `rd24-throughput.launch.txt`: retained as an explicitly experimental Cnf comparison,
  with foveation/affinity disabled; not the recommended performance profile.

Append a profile only after backing up the existing launch configuration. Preserve the
user's world/account parameters, saved player position and eye size. A launch-file
`HORIZON_XR_HZ` sets the initial cadence and is reflected in the UI. Moving the rate
slider or pressing the room's rate +/- takes over for this process; the saved choice is
then used. On the next launch the profile again supplies its initial cadence. Restart
for mesh/environment switches.

At Bnf's 20.5 ms **world** GPU time, 45→90's 22.22 ms total frame budget has at most
1.72 ms remaining **before existing HUD/submit/compositor work**. Its observed 46.9 FPS
corresponds to roughly 21.32 ms per delivered frame, leaving less margin still. The
motion/depth pass and compositor extrapolation have real cost; only device measurements
can establish whether 45→90 stays stable. 36→72 permits 27.78 ms, and 40→80 permits
25 ms when the headset advertises 80 Hz. Test sustained timings and dropped frames,
including movement and streaming, not only a stationary average.

## Affinity failure

The supplied logs show `sched_setaffinity: Invalid argument (os error 22)` for pool
workers and E-only occlusion/sprite work. The original plan intersected topology with
an allowance captured before foreground pinning, but a particular Android thread can
have a different/later cpuset, and cores can become unavailable. The logs alone do not
identify which restriction applied on this headset.

The revised code tries the preferred mask, then the broader worker mask, then the
original allowance. Linux/Android intersects each request with its **current** cpuset and
online CPUs. A successful request is read back with `sched_getaffinity`; logs report the
actual mask and any fallback. This preserves the ability to leave an inherited
foreground-only mask, while recovering when a preferred single/E-only mask is rejected.
If only foreground CPUs remain usable, the final fallback shares them explicitly rather
than claiming an E-core assignment. Failed requests retain diagnostics. This is a
startup policy, not a way to override the OS or guarantee later scheduling.

A Linux kernel test rejects an unavailable preferred CPU and verifies recovery onto an
available worker outside the inherited foreground affinity. The existing affinity
restoration and worker tests also pass locally. Quest-specific recovery still requires
checking the new device logs. Both pinning controls are off in the recommended profiles:
the supplied benchmark gives no reason to add CPU scheduling experiments to a GPU test.

See [Application SpaceWarp implementation and device checks](quest-spacewarp.md).
