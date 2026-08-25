#include "status_fixture.hh"

namespace dmc2::status_fixture {

void fill_task(
    EMC_TASK_STAT &source,
    dmc2_task_snapshot &expected,
    Generator &generator) noexcept {
    fill_rcs(source, expected.rcs, generator);
    const std::uint32_t heartbeat = generator.claim();
    source.heartbeat = expected.heartbeat = generator.u32(heartbeat);
    const std::uint32_t mode = generator.claim();
    source.mode = static_cast<EMC_TASK_MODE_ENUM>(generator.i32(mode));
    expected.mode = generator.i32(mode);
    const std::uint32_t state = generator.claim();
    source.state = static_cast<EMC_TASK_STATE_ENUM>(generator.i32(state));
    expected.state = generator.i32(state);
    const std::uint32_t exec = generator.claim();
    source.execState = static_cast<EMC_TASK_EXEC_ENUM>(generator.i32(exec));
    expected.exec_state = generator.i32(exec);
    const std::uint32_t interp = generator.claim();
    source.interpState = static_cast<EMC_TASK_INTERP_ENUM>(generator.i32(interp));
    expected.interp_state = generator.i32(interp);
    const std::uint32_t call = generator.claim();
    source.callLevel = expected.call_level = generator.i32(call);
    const std::uint32_t motion_line = generator.claim();
    source.motionLine = expected.motion_line = generator.i32(motion_line);
    const std::uint32_t current_line = generator.claim();
    source.currentLine = expected.current_line = generator.i32(current_line);
    const std::uint32_t read_line = generator.claim();
    source.readLine = expected.read_line = generator.i32(read_line);
    const std::uint32_t optional_stop = generator.claim();
    source.optional_stop_state = generator.bit(optional_stop);
    expected.optional_stop_state = generator.bit(optional_stop) ? 1U : 0U;
    const std::uint32_t block_delete = generator.claim();
    source.block_delete_state = generator.bit(block_delete);
    expected.block_delete_state = generator.bit(block_delete) ? 1U : 0U;
    const std::uint32_t input_timeout = generator.claim();
    source.input_timeout = generator.bit(input_timeout);
    expected.input_timeout = generator.bit(input_timeout) ? 1U : 0U;
    fill_bytes(source.file, expected.file, sizeof(expected.file), generator);
    fill_bytes(source.command, expected.command, sizeof(expected.command), generator);
    fill_bytes(
        source.ini_filename,
        expected.ini_filename,
        sizeof(expected.ini_filename),
        generator);
    fill_pose(source.g5x_offset, expected.g5x_offset, generator);
    const std::uint32_t g5x_index = generator.claim();
    source.g5x_index = expected.g5x_index = generator.i32(g5x_index);
    fill_pose(source.g92_offset, expected.g92_offset, generator);
    const std::uint32_t rotation = generator.claim();
    source.rotation_xy = expected.rotation_xy = generator.f64(rotation);
    fill_pose(source.toolOffset, expected.tool_offset, generator);
    fill_i32_array(source.activeGCodes, expected.active_g_codes, generator);
    fill_i32_array(source.activeMCodes, expected.active_m_codes, generator);
    fill_double_array(source.activeSettings, expected.active_settings, generator);
    const std::uint32_t units = generator.claim();
    source.programUnits = static_cast<CANON_UNITS>(generator.i32(units));
    expected.program_units = generator.i32(units);
    const std::uint32_t interpreter = generator.claim();
    source.interpreter_errcode = expected.interpreter_errcode =
        generator.i32(interpreter);
    const std::uint32_t paused = generator.claim();
    source.task_paused = expected.task_paused = generator.i32(paused);
    const std::uint32_t delay = generator.claim();
    source.delayLeft = expected.delay_left = generator.f64(delay);
    const std::uint32_t queued = generator.claim();
    source.queuedMDIcommands = expected.queued_mdi_commands = generator.i32(queued);
}

} // namespace dmc2::status_fixture
