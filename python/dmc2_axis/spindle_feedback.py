"""Actual-RPM feedback beside AXIS's stock spindle controls."""

from __future__ import annotations

import math
from collections.abc import Mapping

from .constants import (
    SPINDLE_ACTUAL_RPM_PIN,
    SPINDLE_FEEDBACK_POLL_MILLISECONDS,
)
from .recovery_ui import RecoveryUiNotice
from .ui_fault import AxisUiFault, AxisUiFaultKind


class SpindleFeedbackBinding:
    """Render the AXIS-owned HAL feedback pin in the manual spindle row."""

    def __init__(self, *, component, namespace, root_window, text_variable) -> None:
        self.component = component
        self.namespace = namespace
        self.root_window = root_window
        self.text_variable = text_variable
        self.poll_after_id = None
        self.refresh_error_notice = RecoveryUiNotice(namespace)

    def _rpm(self) -> float:
        rpm = float(self.component[SPINDLE_ACTUAL_RPM_PIN])
        if not math.isfinite(rpm):
            raise ValueError(f"non-finite spindle feedback: {rpm!r}")
        return rpm

    def poll(self) -> None:
        try:
            rpm = self._rpm()
            self.text_variable.set(f"Actual: {abs(rpm):,.0f} RPM")
        except Exception as error:
            self.refresh_error_notice.present(
                fault=AxisUiFault(
                    AxisUiFaultKind.SPINDLE_FEEDBACK_REFRESH_FAILED,
                    error,
                )
            )
        else:
            self.refresh_error_notice.clear()
        try:
            self.poll_after_id = self.root_window.after(
                SPINDLE_FEEDBACK_POLL_MILLISECONDS,
                self.poll,
            )
        except Exception as error:
            self.refresh_error_notice.present(
                fault=AxisUiFault(
                    AxisUiFaultKind.SPINDLE_FEEDBACK_RESCHEDULE_FAILED,
                    error,
                )
            )


def install_axis_spindle_feedback(
    namespace: Mapping[str, object],
) -> SpindleFeedbackBinding:
    """Add one read-only actual-RPM display to AXIS's spindle controls."""
    live_plotter = namespace["live_plotter"]
    existing = getattr(live_plotter, "_dmc2_spindle_feedback_binding", None)
    if existing is not None:
        return existing

    component = namespace["comp"]
    hal_module = namespace["hal"]
    component.newpin(
        SPINDLE_ACTUAL_RPM_PIN,
        hal_module.HAL_FLOAT,
        hal_module.HAL_IN,
    )

    root_window = namespace["root_window"]
    tkinter_module = namespace["Tkinter"]
    row_path = f"{namespace['tabs_manual']}.spindlef.row2"
    widget_path = f"{row_path}.dmc2_actual_rpm"
    text_variable = tkinter_module.StringVar(
        master=root_window,
        value="Actual: 0 RPM",
    )
    root_window.tk.call(
        "label",
        widget_path,
        "-textvariable",
        str(text_variable),
        "-font",
        ("Helvetica", 10, "bold"),
        "-anchor",
        "w",
        "-width",
        19,
    )
    root_window.tk.call(
        "pack",
        widget_path,
        "-in",
        row_path,
        "-side",
        "left",
        "-padx",
        8,
        "-pady",
        2,
    )

    binding = SpindleFeedbackBinding(
        component=component,
        namespace=namespace,
        root_window=root_window,
        text_variable=text_variable,
    )
    live_plotter._dmc2_spindle_feedback_binding = binding
    binding.poll()
    return binding
