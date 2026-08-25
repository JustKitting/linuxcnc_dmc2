#include "status_fixture.hh"

#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>

namespace dmc2::status_fixture {

namespace {

void fill_integer(
    int &source,
    std::int32_t &expected,
    Generator &generator) noexcept {
    const std::uint32_t id = generator.claim();
    source = expected = generator.i32(id);
}

} // namespace

void fill_fixture(
    EMC_STAT &source,
    dmc2_task_status_snapshot &expected,
    Generator &generator) noexcept {
    dmc2_task_status_snapshot_initialize(&expected);
    generator.claim(); // abi_version is supplied by the initializer.
    generator.claim(); // struct_size is supplied by the initializer.

    fill_rcs(source, expected.top_rcs, generator);
    fill_task(source.task, expected.task, generator);
    fill_rcs(source.motion, expected.motion_rcs, generator);

    const std::uint32_t heartbeat = generator.claim();
    source.motion.heartbeat = expected.motion_heartbeat =
        generator.u32(heartbeat);
    fill_trajectory(source.motion.traj, expected.trajectory, generator);

    for (std::size_t index = 0; index < DMC2_MAX_JOINTS; ++index) {
        fill_joint(source.motion.joint[index], expected.joints[index], generator);
    }

    std::array<std::uint32_t, DMC2_MAX_AXES> stopped_fields{};
    for (std::size_t index = 0; index < DMC2_MAX_AXES; ++index) {
        stopped_fields[index] =
            fill_axis(source.motion.axis[index], expected.axes[index], generator);
    }
    for (std::size_t index = 0; index < DMC2_MAX_SPINDLES; ++index) {
        fill_spindle(
            source.motion.spindle[index],
            expected.spindles[index],
            generator);
    }

    fill_i32_array(
        source.motion.synch_di,
        expected.synchronized_digital_inputs,
        generator);
    fill_i32_array(
        source.motion.synch_do,
        expected.synchronized_digital_outputs,
        generator);
    fill_double_array(
        source.motion.analog_input,
        expected.analog_inputs,
        generator);
    fill_double_array(
        source.motion.analog_output,
        expected.analog_outputs,
        generator);
    fill_i32_array(source.motion.misc_error, expected.misc_error, generator);
    fill_integer(source.motion.debug, expected.motion_debug, generator);
    fill_integer(source.motion.on_soft_limit, expected.on_soft_limit, generator);
    fill_integer(
        source.motion.external_offsets_applied,
        expected.external_offsets_applied,
        generator);
    fill_pose(
        source.motion.eoffset_pose,
        expected.external_offset_pose,
        generator);
    fill_integer(
        source.motion.numExtraJoints,
        expected.num_extra_joints,
        generator);
    const std::uint32_t jogging = generator.claim();
    source.motion.jogging_active = generator.bit(jogging);
    expected.jogging_active = generator.bit(jogging) ? 1U : 0U;

    fill_io(source.io, expected.io, generator);
    fill_integer(source.debug, expected.top_debug, generator);

    const int axis_mask = generator.round == INACTIVE_AXIS_ROUND
        ? 0x155
        : (1 << DMC2_MAX_AXES) - 1;
    source.motion.traj.axis_mask = axis_mask;
    expected.trajectory.axis_mask = axis_mask;

    for (std::size_t index = 0; index < DMC2_MAX_AXES; ++index) {
        const bool active = (axis_mask & (1 << index)) != 0;
        const bool stopped = generator.bit(stopped_fields[index]);
        const double axis_velocity = stopped ? 0.0 : 10.0 + index;
        const double joint_velocity = stopped ? 0.0 : 20.0 + index;

        source.motion.axis[index].velocity = axis_velocity;
        expected.axes[index].velocity = axis_velocity;
        source.motion.joint[index].inpos = stopped ? 1U : 0U;
        expected.joints[index].in_position = stopped ? 1U : 0U;
        source.motion.joint[index].velocity = joint_velocity;
        expected.joints[index].velocity = joint_velocity;
        expected.axes[index].stopped = (!active || stopped) ? 1U : 0U;
    }
}

} // namespace dmc2::status_fixture

extern "C" int dmc2_task_status_copy_self_test(
    std::uint32_t *logical_fields,
    std::size_t *failure_offset) noexcept {
    if (logical_fields == nullptr || failure_offset == nullptr) {
        return -1;
    }
    *logical_fields = 0;
    *failure_offset = std::numeric_limits<std::size_t>::max();

    std::uint32_t reference_field_count = 0;
    for (int round = 0; round < dmc2::status_fixture::TOTAL_ROUNDS; ++round) {
        EMC_STAT source;
        dmc2_task_status_snapshot expected;
        dmc2_task_status_snapshot actual;
        dmc2::status_fixture::Generator generator{0, round};
        dmc2::status_fixture::fill_fixture(source, expected, generator);
        dmc2_copy_status(source, actual);

        if (round == 0) {
            reference_field_count = generator.fields;
        } else if (generator.fields != reference_field_count) {
            return -2;
        }

        const auto *expected_bytes =
            reinterpret_cast<const std::uint8_t *>(&expected);
        const auto *actual_bytes =
            reinterpret_cast<const std::uint8_t *>(&actual);
        for (std::size_t offset = 0; offset < sizeof(actual); ++offset) {
            if (actual_bytes[offset] != expected_bytes[offset]) {
                *logical_fields = generator.fields;
                *failure_offset = offset;
                return round + 1;
            }
        }
    }

    *logical_fields = reference_field_count;
    return 0;
}
