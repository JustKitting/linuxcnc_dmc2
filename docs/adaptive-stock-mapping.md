# Adaptive measurement and placement

Use **Prepare adaptive mapping**, **Fit stock and choose measurements**, and
**Update fit from new measurements** in Object Mapper. The catalog comes from
the standard `native/bin/dmc2ctl`; use **Reload operations** after installing a
new binary. These commands calculate and retain files. They do not run motion.

The objective is the complete measurement-to-cutting process. The required
operation geometry and the uncertain stock estimate determine new measurement
locations. An old rim, rectangle fit, local support hull or scan depth does not
define the stock's allowed shape or cap the placement search.

## Iteration

1. Import original captures and companions into the intended object/setup.
   Estimate a V3 stock surface revision with fit/check roles, probe calibration,
   the shared frame and explicit miss-capture decisions. Failed local normal
   estimates do not remove their original contact observations from this model.
2. Prepare adaptive mapping with the unchanged required operation STL and a
   retained acquisition profile. Fill its request and save it. Model dimensions
   and measured physical stock dimensions have different roles.
3. Fit stock and choose measurements with a new analysis ID. Inspect the pose,
   residuals, posterior, required-volume evidence and proposed acquisition.
4. `adaptive-probe.ngc` is a proposed plan for the standard File Open/Run path.
   It declares its effects, prerequisites, exact starting work position and
   frame, feeds, target order, withdrawal and recovery. Execution requires
   review and authorization of those movements. X directions are physical
   RIGHT / LinuxCNC -X and physical LEFT / LinuxCNC +X.
5. Import the resulting exact ledger and companions. Update fit from new
   measurements takes the preceding cycle, new capture and next analysis ID.
   It checks the complete capture against the exact preceding plan, appends
   contact roles and miss evidence, creates a new surface revision, and runs
   the next fit/search/acquisition calculation. Source revisions remain intact.

## Request fields

- `surface_analysis`, `source_capture`, `design`, `stl_mm_per_unit` identify
  immutable observations, the acquisition profile and required geometry.
- `model_origin_*_bounds_mm` and `roll/pitch/yaw_bounds_deg` are absolute search
  bounds. Equal endpoints freeze a coordinate. Translation is in machine
  millimetres; rotations use `Rz(yaw) Ry(pitch) Rx(roll)` about the model origin.
- `acquisition_start_work_mm` and `acquisition_drop_mm` describe the proposed
  new acquisition. They can differ from the old scan. The retained profile's
  plate/travel, mounted-reach, frame and feed constraints still apply.
- `required_clearance_mm` preserves material clearance. Search never scales
  the required mesh or relaxes this field to obtain a fit.
- `field_length_mm`, `field_scale_mm`, `contact_sigma_mm`, `normal_sigma`,
  `empty_space_sigma_mm` and `uncertainty_multiplier` describe the statistical
  model. They are explicit assumptions to calibrate against independent data;
  a numerical fixture's values are not physical probe specifications.
- `basis_pairs` and `max_matrix_entries` bound the finite Bayesian basis.
  `posterior_iterations`, `posterior_weight_tolerance` and
  `max_conditioning_observations` bound numerical conditioning. The weight
  tolerance is dimensionless. All selected fit contacts and finite miss paths
  are accounted for; insufficient budgets produce readable errors.
- `cover_radius_mm` and `max_cover_samples` govern required-surface coverage.
  `interior_samples` and `interior_candidates` govern stochastic sampling of
  the required solid, including rejection of points in its exterior/cavities.
- `population`, `max_evaluations`, `mutation_scale`, `crossover_probability`
  and `random_seed` control replayable differential evolution. The population
  needs a parent and distinct donors. Out-of-domain mutations are resampled.
- `observation_candidates`, `max_observations` and `ray_resolution_mm` govern
  candidate ray sampling, batch size and numerical contact-bracket resolution.
  These do not change physical feeds or controller resolution.

## Model and output

The signed-distance estimate uses random Fourier features with Gaussian contact
and estimated-normal observations, and censored logistic empty-path evidence.
Contact ball centres use the source probe correction; their distance observation
is the ball radius. Independent check contacts and explicitly marked check-run
misses do not train the mean. Failed checks remain visible. Uncertainty is
conditional on this approximate model; an implicit prediction is not a measured
closed stock mesh.

Placement minimizes area-weighted surface deficit and sampled interior deficit,
plus retained empty-space overlap. Surface and interior have equal aggregate
weight. Every finite empty capsule is compared with the unchanged required
solid and clearance, including wholly enclosed paths. Further top/side rays
are chosen from competing placements' uncertain/violated regions and ranked
by predicted information gain conditional on contact. An all-miss batch still
supplies evidence to the next cycle.

Each bundle retains `request.txt`, original source records, `source.stl`,
`pose-candidate.txt`, `model-candidate.machine-mm.stl`, residuals/search history,
`stock-field.machine-mm.json`, `required-volume.machine-mm.json`, independent
check results, `adaptive-cycle.machine-mm.json` and any proposed probe program.
The stock-field file includes the exact mean weights, frequency basis and
posterior precision factor with its evaluation formula.

`cam-input.machine-mm.json` binds geometry and statistical stock to their frame.
Import positioned geometry at identity, or apply the recorded transform once
to original geometry converted to millimetres. Do not apply both or repeat an
existing CAD setup flip. A model-ready result permits CAM review; insufficient
material returns a typed pipeline-pause error after preserving its outputs.
Missing measurements, numerical convergence and acquisition-access issues have
named recovery states. None grants permission to cut or restricts Clear Fault
or Pendant Mode.

Native CAM creation, actual tool/fixture registration, physical observation of
this acquisition loop, and cutting remain separate unfinished integration work.
