#include "status_fixture.hh"

namespace dmc2::status_fixture {

void fill_trajectory(
    EMC_TRAJ_STAT &source,
    dmc2_trajectory_snapshot &expected,
    Generator &generator) noexcept {
    fill_rcs(source, expected.rcs, generator);
    const std::uint32_t linear = generator.claim();
    source.linearUnits = expected.linear_units = generator.f64(linear);
    const std::uint32_t angular = generator.claim();
    source.angularUnits = expected.angular_units = generator.f64(angular);
    const std::uint32_t cycle = generator.claim();
    source.cycleTime = expected.cycle_time = generator.f64(cycle);
    const std::uint32_t joints = generator.claim();
    source.joints = expected.joints = generator.i32(joints);
    const std::uint32_t spindles = generator.claim();
    source.spindles = expected.spindles = generator.i32(spindles);
    const std::uint32_t mask = generator.claim();
    source.axis_mask = expected.axis_mask = generator.i32(mask);
    const std::uint32_t mode = generator.claim();
    source.mode = static_cast<EMC_TRAJ_MODE_ENUM>(generator.i32(mode));
    expected.mode = generator.i32(mode);
    const std::uint32_t enabled = generator.claim();
    source.enabled = generator.bit(enabled);
    expected.enabled = generator.bit(enabled) ? 1U : 0U;
    const std::uint32_t in_position = generator.claim();
    source.inpos = generator.bit(in_position);
    expected.in_position = generator.bit(in_position) ? 1U : 0U;
    const std::uint32_t queue = generator.claim();
    source.queue = expected.queue = generator.i32(queue);
    const std::uint32_t active_queue = generator.claim();
    source.activeQueue = expected.active_queue = generator.i32(active_queue);
    const std::uint32_t queue_full = generator.claim();
    source.queueFull = generator.bit(queue_full);
    expected.queue_full = generator.bit(queue_full) ? 1U : 0U;
    const std::uint32_t id = generator.claim();
    source.id = expected.id = generator.i32(id);
    const std::uint32_t paused = generator.claim();
    source.paused = generator.bit(paused);
    expected.paused = generator.bit(paused) ? 1U : 0U;
    const std::uint32_t scale = generator.claim();
    source.scale = expected.scale = generator.f64(scale);
    const std::uint32_t rapid_scale = generator.claim();
    source.rapid_scale = expected.rapid_scale = generator.f64(rapid_scale);
    fill_pose(source.position, expected.position, generator);
    fill_pose(source.actualPosition, expected.actual_position, generator);
    const std::uint32_t velocity = generator.claim();
    source.velocity = expected.velocity = generator.f64(velocity);
    const std::uint32_t acceleration = generator.claim();
    source.acceleration = expected.acceleration = generator.f64(acceleration);
    const std::uint32_t max_velocity = generator.claim();
    source.maxVelocity = expected.max_velocity = generator.f64(max_velocity);
    const std::uint32_t max_acceleration = generator.claim();
    source.maxAcceleration = expected.max_acceleration = generator.f64(max_acceleration);
    fill_pose(source.probedPosition, expected.probed_position, generator);
    const std::uint32_t probe_tripped = generator.claim();
    source.probe_tripped = generator.bit(probe_tripped);
    expected.probe_tripped = generator.bit(probe_tripped) ? 1U : 0U;
    const std::uint32_t probing = generator.claim();
    source.probing = generator.bit(probing);
    expected.probing = generator.bit(probing) ? 1U : 0U;
    const std::uint32_t probe_value = generator.claim();
    source.probeval = expected.probe_value = generator.i32(probe_value);
    const std::uint32_t kinematics = generator.claim();
    source.kinematics_type = expected.kinematics_type = generator.i32(kinematics);
    const std::uint32_t motion_type = generator.claim();
    source.motion_type = expected.motion_type = generator.i32(motion_type);
    const std::uint32_t distance = generator.claim();
    source.distance_to_go = expected.distance_to_go = generator.f64(distance);
    fill_pose(source.dtg, expected.dtg, generator);
    const std::uint32_t current_velocity = generator.claim();
    source.current_vel = expected.current_velocity = generator.f64(current_velocity);
    const std::uint32_t feed_override = generator.claim();
    source.feed_override_enabled = generator.bit(feed_override);
    expected.feed_override_enabled = generator.bit(feed_override) ? 1U : 0U;
    const std::uint32_t adaptive_feed = generator.claim();
    source.adaptive_feed_enabled = generator.bit(adaptive_feed);
    expected.adaptive_feed_enabled = generator.bit(adaptive_feed) ? 1U : 0U;
    const std::uint32_t feed_hold = generator.claim();
    source.feed_hold_enabled = generator.bit(feed_hold);
    expected.feed_hold_enabled = generator.bit(feed_hold) ? 1U : 0U;
    fill_state_tag(source.tag, expected.state_tag, generator);
}

} // namespace dmc2::status_fixture
