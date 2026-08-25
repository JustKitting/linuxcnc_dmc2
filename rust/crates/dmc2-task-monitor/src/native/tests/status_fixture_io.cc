#include "status_fixture.hh"

namespace dmc2::status_fixture {

namespace {

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

void fill_tool_table(
    CANON_TOOL_TABLE &source,
    dmc2_tool_table_snapshot &expected,
    Generator &generator) noexcept {
    fill_integer(source.toolno, expected.tool_number, generator);
    fill_integer(source.pocketno, expected.pocket_number, generator);
    fill_pose(source.offset, expected.offset, generator);
    fill_double(source.diameter, expected.diameter, generator);
    fill_double(source.frontangle, expected.front_angle, generator);
    fill_double(source.backangle, expected.back_angle, generator);
    fill_integer(source.orientation, expected.orientation, generator);
}

} // namespace

void fill_io(
    EMC_IO_STAT &source,
    dmc2_io_snapshot &expected,
    Generator &generator) noexcept {
    fill_rcs(source, expected.rcs, generator);

    const std::uint32_t heartbeat = generator.claim();
    source.heartbeat = expected.heartbeat = generator.u32(heartbeat);
    fill_double(source.cycleTime, expected.cycle_time, generator);
    fill_integer(source.debug, expected.debug, generator);
    fill_integer(source.reason, expected.reason, generator);
    fill_integer(source.fault, expected.fault, generator);

    fill_rcs(source.tool, expected.tool.rcs, generator);
    fill_integer(
        source.tool.pocketPrepped,
        expected.tool.pocket_prepped,
        generator);
    fill_integer(
        source.tool.toolInSpindle,
        expected.tool.tool_in_spindle,
        generator);
    fill_integer(
        source.tool.toolFromPocket,
        expected.tool.tool_from_pocket,
        generator);
    fill_tool_table(
        source.tool.toolTableCurrent,
        expected.tool.current_tool,
        generator);

    fill_rcs(source.coolant, expected.coolant.rcs, generator);
    fill_integer(source.coolant.mist, expected.coolant.mist, generator);
    fill_integer(source.coolant.flood, expected.coolant.flood, generator);

    fill_rcs(source.aux, expected.aux.rcs, generator);
    fill_integer(source.aux.estop, expected.aux.estop, generator);

    fill_rcs(source.lube, expected.lube.rcs, generator);
    fill_integer(source.lube.on, expected.lube.on, generator);
    fill_integer(source.lube.level, expected.lube.level, generator);
}

} // namespace dmc2::status_fixture
