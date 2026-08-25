#ifndef DMC2_STATUS_COPY_HH
#define DMC2_STATUS_COPY_HH

#include "emc_nml.hh"
#include "status_snapshot.h"

void dmc2_copy_status(
    const EMC_STAT &source,
    dmc2_task_status_snapshot &destination) noexcept;

#endif
