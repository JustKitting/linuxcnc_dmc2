#include "status_fixture.hh"

namespace dmc2::status_fixture {

namespace {

void fill_flag(
    unsigned char &source,
    std::uint32_t &expected,
    Generator &generator) noexcept {
    const std::uint32_t id = generator.claim();
    source = generator.bit(id) ? 1U : 0U;
    expected = generator.bit(id) ? 1U : 0U;
}

void fill_integer(
    int &source,
    std::int32_t &expected,
    Generator &generator) noexcept {
    const std::uint32_t id = generator.claim();
    source = expected = generator.i32(id);
}

void fill_double(
    double &source,
    double &expected,
    Generator &generator) noexcept {
    const std::uint32_t id = generator.claim();
    source = expected = generator.f64(id);
}

} // namespace

void fill_joint(
    EMC_JOINT_STAT &source,
    dmc2_joint_snapshot &expected,
    Generator &generator) noexcept {
    fill_rcs(source, expected.rcs, generator);
    fill_integer(source.joint, expected.joint_number, generator);

    const std::uint32_t joint_type = generator.claim();
    source.jointType = generator.byte(joint_type);
    expected.joint_type = static_cast<std::int32_t>(source.jointType);

    fill_double(source.units, expected.units, generator);
    fill_double(source.backlash, expected.backlash, generator);
    fill_double(
        source.minPositionLimit,
        expected.min_position_limit,
        generator);
    fill_double(
        source.maxPositionLimit,
        expected.max_position_limit,
        generator);
    fill_double(source.maxFerror, expected.max_ferror, generator);
    fill_double(source.minFerror, expected.min_ferror, generator);
    fill_double(source.ferrorCurrent, expected.ferror_current, generator);
    fill_double(
        source.ferrorHighMark,
        expected.ferror_high_mark,
        generator);
    fill_double(source.output, expected.output, generator);
    fill_double(source.input, expected.input, generator);
    fill_double(source.velocity, expected.velocity, generator);

    fill_flag(source.inpos, expected.in_position, generator);
    fill_flag(source.homing, expected.homing, generator);
    fill_flag(source.homed, expected.homed, generator);
    fill_flag(source.fault, expected.fault, generator);
    fill_flag(source.enabled, expected.enabled, generator);
    fill_flag(source.minSoftLimit, expected.min_soft_limit, generator);
    fill_flag(source.maxSoftLimit, expected.max_soft_limit, generator);
    fill_flag(source.minHardLimit, expected.min_hard_limit, generator);
    fill_flag(source.maxHardLimit, expected.max_hard_limit, generator);
    fill_flag(source.overrideLimits, expected.override_limits, generator);
}

std::uint32_t fill_axis(
    EMC_AXIS_STAT &source,
    dmc2_axis_snapshot &expected,
    Generator &generator) noexcept {
    fill_rcs(source, expected.rcs, generator);
    fill_integer(source.axis, expected.axis_number, generator);
    fill_double(
        source.minPositionLimit,
        expected.min_position_limit,
        generator);
    fill_double(
        source.maxPositionLimit,
        expected.max_position_limit,
        generator);
    fill_double(source.velocity, expected.velocity, generator);

    const std::uint32_t stopped = generator.claim();
    expected.stopped = 0;
    return stopped;
}

void fill_spindle(
    EMC_SPINDLE_STAT &source,
    dmc2_spindle_snapshot &expected,
    Generator &generator) noexcept {
    fill_rcs(source, expected.rcs, generator);
    fill_double(source.speed, expected.speed, generator);
    fill_double(source.spindle_scale, expected.spindle_scale, generator);
    fill_double(source.css_maximum, expected.css_maximum, generator);
    fill_double(source.css_factor, expected.css_factor, generator);
    fill_integer(source.state, expected.state, generator);
    fill_integer(source.direction, expected.direction, generator);
    fill_integer(source.brake, expected.brake, generator);
    fill_integer(source.increasing, expected.increasing, generator);
    fill_integer(source.enabled, expected.enabled, generator);
    fill_integer(source.orient_state, expected.orient_state, generator);
    fill_integer(source.orient_fault, expected.orient_fault, generator);

    const std::uint32_t override_enabled = generator.claim();
    source.spindle_override_enabled = generator.bit(override_enabled);
    expected.override_enabled = generator.bit(override_enabled) ? 1U : 0U;
    const std::uint32_t homed = generator.claim();
    source.homed = generator.bit(homed);
    expected.homed = generator.bit(homed) ? 1U : 0U;
}

} // namespace dmc2::status_fixture
