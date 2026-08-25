#include "status_fixture.hh"

namespace dmc2::status_fixture {

void fill_bytes(
    char *source,
    std::uint8_t *expected,
    std::size_t size,
    Generator &generator) noexcept {
    const std::uint32_t id = generator.claim();
    for (std::size_t index = 0; index < size; ++index) {
        const std::uint8_t value = generator.byte(id, index);
        source[index] = static_cast<char>(value);
        expected[index] = value;
    }
}

void fill_pose(
    EmcPose &source,
    dmc2_pose_snapshot &expected,
    Generator &generator) noexcept {
    const std::uint32_t x = generator.claim();
    source.tran.x = expected.x = generator.f64(x);
    const std::uint32_t y = generator.claim();
    source.tran.y = expected.y = generator.f64(y);
    const std::uint32_t z = generator.claim();
    source.tran.z = expected.z = generator.f64(z);
    const std::uint32_t a = generator.claim();
    source.a = expected.a = generator.f64(a);
    const std::uint32_t b = generator.claim();
    source.b = expected.b = generator.f64(b);
    const std::uint32_t c = generator.claim();
    source.c = expected.c = generator.f64(c);
    const std::uint32_t u = generator.claim();
    source.u = expected.u = generator.f64(u);
    const std::uint32_t v = generator.claim();
    source.v = expected.v = generator.f64(v);
    const std::uint32_t w = generator.claim();
    source.w = expected.w = generator.f64(w);
}

void fill_state_tag(
    StateTag &source,
    dmc2_state_tag_snapshot &expected,
    Generator &generator) noexcept {
    const std::uint32_t floats = generator.claim();
    for (std::size_t index = 0; index < DMC2_STATE_TAG_FLOAT_FIELDS; ++index) {
        source.fields_float[index] = expected.fields_float[index] =
            generator.f32(floats, index);
    }
    const std::uint32_t integers = generator.claim();
    for (std::size_t index = 0; index < DMC2_STATE_TAG_FIELDS; ++index) {
        source.fields[index] = expected.fields[index] =
            generator.i32(integers, index);
    }
    const std::uint32_t flags = generator.claim();
    source.packed_flags = static_cast<unsigned long>(generator.u64(flags));
    expected.packed_flags = generator.u64(flags);
}

void fill_rcs(
    RCS_STAT_MSG &source,
    dmc2_rcs_status_snapshot &expected,
    Generator &generator) noexcept {
    const std::uint32_t type = generator.claim();
    source.type = static_cast<NMLTYPE>(generator.i32(type));
    expected.message_type = generator.i32(type);
    const std::uint32_t size = generator.claim();
    source.size = static_cast<long>(generator.i64(size));
    expected.message_size = generator.i64(size);
    const std::uint32_t command = generator.claim();
    source.command_type = static_cast<NMLTYPE>(generator.i32(command));
    expected.command_type = generator.i32(command);
    const std::uint32_t echo = generator.claim();
    source.echo_serial_number = expected.echo_serial_number = generator.i32(echo);
    const std::uint32_t status = generator.claim();
    source.status = expected.status = generator.i32(status);
    const std::uint32_t state = generator.claim();
    source.state = expected.state = generator.i32(state);
    const std::uint32_t line = generator.claim();
    source.line = expected.line = generator.i32(line);
    const std::uint32_t source_line = generator.claim();
    source.source_line = expected.source_line = generator.i32(source_line);
    fill_bytes(
        source.source_file,
        expected.source_file,
        sizeof(expected.source_file),
        generator);
    generator.claim();
    expected.reserved = 0;
}

} // namespace dmc2::status_fixture
