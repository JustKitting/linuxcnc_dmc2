#ifndef DMC2_STATUS_FIXTURE_HH
#define DMC2_STATUS_FIXTURE_HH

#include <cstddef>
#include <cstdint>

#include "status_copy.hh"

namespace dmc2::status_fixture {

constexpr int SIGNATURE_ROUNDS = 20;
constexpr int INACTIVE_AXIS_ROUND = SIGNATURE_ROUNDS;
constexpr int TOTAL_ROUNDS = SIGNATURE_ROUNDS + 1;

struct Generator {
    std::uint32_t fields = 0;
    int round;

    std::uint32_t claim() noexcept { return fields++; }

    std::int32_t i32(std::uint32_t id, std::size_t element = 0) const noexcept {
        return static_cast<std::int32_t>(
            1000000U + id * 1024U + static_cast<std::uint32_t>(element) +
            static_cast<std::uint32_t>(round));
    }

    std::int64_t i64(std::uint32_t id, std::size_t element = 0) const noexcept {
        return 1000000000LL + static_cast<std::int64_t>(id) * 4096LL +
            static_cast<std::int64_t>(element) + round;
    }

    std::uint32_t u32(std::uint32_t id, std::size_t element = 0) const noexcept {
        return 2000000000U + id * 1024U +
            static_cast<std::uint32_t>(element) +
            static_cast<std::uint32_t>(round);
    }

    std::uint64_t u64(std::uint32_t id, std::size_t element = 0) const noexcept {
        return 0x100000000ULL + static_cast<std::uint64_t>(id) * 4096ULL +
            static_cast<std::uint64_t>(element) +
            static_cast<std::uint64_t>(round);
    }

    double f64(std::uint32_t id, std::size_t element = 0) const noexcept {
        return 1000.0 + static_cast<double>(id) * 2.0 +
            static_cast<double>(element) / 512.0 +
            static_cast<double>(round) / 64.0;
    }

    float f32(std::uint32_t id, std::size_t element = 0) const noexcept {
        return 100.0F + static_cast<float>(id) / 8.0F +
            static_cast<float>(element) / 128.0F +
            static_cast<float>(round) / 32.0F;
    }

    std::uint8_t byte(std::uint32_t id, std::size_t element = 0) const noexcept {
        return static_cast<std::uint8_t>(
            1U + (id * 31U + static_cast<std::uint32_t>(element) +
                  static_cast<std::uint32_t>(round)) % 251U);
    }

    bool bit(std::uint32_t id, std::size_t element = 0) const noexcept {
        const std::uint64_t key =
            ((static_cast<std::uint64_t>(id) + 1ULL) << 9U) |
            static_cast<std::uint64_t>(element);
        return ((key >> static_cast<unsigned int>(round)) & 1ULL) != 0ULL;
    }
};

void fill_bytes(
    char *source,
    std::uint8_t *expected,
    std::size_t size,
    Generator &generator) noexcept;

template <typename Source, typename Destination, std::size_t Size>
void fill_i32_array(
    Source (&source)[Size],
    Destination (&expected)[Size],
    Generator &generator) noexcept {
    static_assert(sizeof(Source) == sizeof(std::int32_t));
    static_assert(sizeof(Destination) == sizeof(std::int32_t));
    const std::uint32_t id = generator.claim();
    for (std::size_t index = 0; index < Size; ++index) {
        const std::int32_t value = generator.i32(id, index);
        source[index] = static_cast<Source>(value);
        expected[index] = static_cast<Destination>(value);
    }
}

template <std::size_t Size>
void fill_double_array(
    double (&source)[Size],
    double (&expected)[Size],
    Generator &generator) noexcept {
    const std::uint32_t id = generator.claim();
    for (std::size_t index = 0; index < Size; ++index) {
        const double value = generator.f64(id, index);
        source[index] = value;
        expected[index] = value;
    }
}

void fill_pose(
    EmcPose &source,
    dmc2_pose_snapshot &expected,
    Generator &generator) noexcept;
void fill_state_tag(
    StateTag &source,
    dmc2_state_tag_snapshot &expected,
    Generator &generator) noexcept;
void fill_rcs(
    RCS_STAT_MSG &source,
    dmc2_rcs_status_snapshot &expected,
    Generator &generator) noexcept;
void fill_task(
    EMC_TASK_STAT &source,
    dmc2_task_snapshot &expected,
    Generator &generator) noexcept;
void fill_trajectory(
    EMC_TRAJ_STAT &source,
    dmc2_trajectory_snapshot &expected,
    Generator &generator) noexcept;
void fill_joint(
    EMC_JOINT_STAT &source,
    dmc2_joint_snapshot &expected,
    Generator &generator) noexcept;
std::uint32_t fill_axis(
    EMC_AXIS_STAT &source,
    dmc2_axis_snapshot &expected,
    Generator &generator) noexcept;
void fill_spindle(
    EMC_SPINDLE_STAT &source,
    dmc2_spindle_snapshot &expected,
    Generator &generator) noexcept;
void fill_io(
    EMC_IO_STAT &source,
    dmc2_io_snapshot &expected,
    Generator &generator) noexcept;
void fill_fixture(
    EMC_STAT &source,
    dmc2_task_status_snapshot &expected,
    Generator &generator) noexcept;

} // namespace dmc2::status_fixture

#endif
