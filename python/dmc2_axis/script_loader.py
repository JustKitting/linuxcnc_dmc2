"""Route stock AXIS file selection through the typed script inspector."""

from __future__ import annotations

from collections.abc import Mapping
import os
from pathlib import Path

from .recovery_ui import RecoveryUiNotice
from .script_contract import (
    ScriptContract,
    ScriptContractSource,
    ScriptInspector,
    ScriptLoaderFailure,
    ScriptLoaderFailureKind,
    same_machine_file,
)
from .ui_fault import AxisUiFault, AxisUiFaultKind


class AxisScriptLoader:
    """Inspect every AXIS-selected file before forwarding it to stock AXIS."""

    def __init__(self, *, namespace, stock_open_file_name, inspector) -> None:
        self.namespace = namespace
        self.stock_open_file_name = stock_open_file_name
        self.inspector = inspector
        self.selected_contract: ScriptContract | None = None
        self.inspection_notice = RecoveryUiNotice(namespace)
        self.load_notice = RecoveryUiNotice(namespace)

    def _present(self, kind: AxisUiFaultKind, cause: object) -> None:
        route = None
        presentation_cause = cause
        try:
            reader = self.namespace["live_plotter"]._dmc2_diagnostic_reader
            route = reader.recovery_route(kind.contract.recovery_code)
        except Exception as presentation_error:
            presentation_cause = (
                f"{cause}; dynamic recovery catalog unavailable: {presentation_error}"
            )
        notice = (
            self.inspection_notice
            if kind is AxisUiFaultKind.SCRIPT_CONTRACT_INSPECTION_FAILED
            else self.load_notice
        )
        notice.present(
            fault=AxisUiFault(kind, presentation_cause),
            route=route,
        )

    def contract_for_loaded_path(self, path: object) -> ScriptContract | None:
        contract = self.selected_contract
        if contract is not None and same_machine_file(contract.path, path):
            return contract
        return None

    def _inspect_with_notice(self, path: object) -> ScriptContract | None:
        try:
            contract = self.inspector.inspect(path)
        except ScriptLoaderFailure as error:
            self._present(AxisUiFaultKind.SCRIPT_CONTRACT_INSPECTION_FAILED, error)
            return None
        except Exception as error:
            self._present(
                AxisUiFaultKind.SCRIPT_CONTRACT_INSPECTION_FAILED,
                ScriptLoaderFailure(
                    ScriptLoaderFailureKind.INSPECTION_INTEGRATION_FAILED,
                    error,
                ),
            )
            return None
        self.inspection_notice.clear()
        return contract

    def inspect_for_run(self, path: object) -> ScriptContract | None:
        """Refresh the path's contract immediately before an explicit Run."""
        current = self._inspect_with_notice(path)
        if current is None:
            return None
        selected = self.contract_for_loaded_path(path)
        if selected is None:
            if current.source is ScriptContractSource.HEADER:
                self._present(
                    AxisUiFaultKind.SCRIPT_CONTRACT_INSPECTION_FAILED,
                    ScriptLoaderFailure(
                        ScriptLoaderFailureKind.HEADER_NOT_ESTABLISHED_AT_LOAD,
                        f"path={current.path!r}; reopen it through AXIS File Open",
                    ),
                )
                return None
            self.selected_contract = current
            return current
        if current.revision != selected.revision:
            self._present(
                AxisUiFaultKind.SCRIPT_CONTRACT_INSPECTION_FAILED,
                ScriptLoaderFailure(
                    ScriptLoaderFailureKind.CONTENT_CHANGED_AFTER_LOAD,
                    f"path={current.path!r}; reopen it through AXIS File Open",
                ),
            )
            return None
        self.selected_contract = current
        return current

    def __call__(self, path: object):
        contract = self._inspect_with_notice(path)
        if contract is None:
            return ""

        try:
            result = self.stock_open_file_name(contract.path)
        except Exception as error:
            self._present(
                AxisUiFaultKind.SCRIPT_LOAD_SUBMISSION_FAILED,
                ScriptLoaderFailure(
                    ScriptLoaderFailureKind.STOCK_OPEN_FAILED,
                    f"path={contract.path!r} cause={error}",
                ),
            )
            return ""

        axis_loaded_path = self.namespace.get("loaded_file")
        if not same_machine_file(contract.path, axis_loaded_path):
            self._present(
                AxisUiFaultKind.SCRIPT_LOAD_SUBMISSION_FAILED,
                ScriptLoaderFailure(
                    ScriptLoaderFailureKind.STOCK_OPEN_FAILED,
                    "stock AXIS returned without selecting the inspected file: "
                    f"inspected={contract.path!r} axis_selected={axis_loaded_path!r}",
                ),
            )
            return result

        self.selected_contract = contract
        self.load_notice.clear()
        print(
            "DMC2_SCRIPT_SELECTION result=inspected-and-forwarded-to-stock-axis "
            f"path={contract.path!r} contract_source={contract.source.value!r} "
            "consumer_acceptance=pending-run-guard-status-check",
            flush=True,
        )
        return result


def install_axis_script_loader(namespace: Mapping[str, object]) -> AxisScriptLoader:
    """Route stock AXIS Open controls through the Rust contract inspector."""
    live_plotter = namespace["live_plotter"]
    existing = getattr(live_plotter, "_dmc2_axis_script_loader", None)
    if existing is not None:
        return existing

    project_root = Path(str(namespace["rcfile"])).resolve().parents[2]
    executable = project_root / "native" / "bin" / "dmc2ctl"
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise RuntimeError(
            "DMC2_SCRIPT_INSPECTOR_NOT_EXECUTABLE: "
            f"path={executable}; action: build and stage the matched dmc2ctl binary"
        )

    root_window = namespace["root_window"]
    commands = namespace["commands"]
    stock_open_file_name = commands.open_file_name
    loader = AxisScriptLoader(
        namespace=namespace,
        stock_open_file_name=stock_open_file_name,
        inspector=ScriptInspector(executable, project_root),
    )

    tk = root_window.tk
    stock_tcl_command = "dmc2_stock_open_file_name"
    if str(tk.call("info", "commands", stock_tcl_command)):
        raise RuntimeError(f"AXIS Tcl command already exists: {stock_tcl_command}")
    if not str(tk.call("info", "commands", "open_file_name")):
        raise RuntimeError("AXIS's stock open_file_name Tcl command is unavailable")
    callback_command = root_window.register(loader)
    renamed = False
    try:
        tk.call("rename", "open_file_name", stock_tcl_command)
        renamed = True
        tk.call("interp", "alias", "", "open_file_name", "", callback_command)
        commands.open_file_name = loader
    except Exception:
        if renamed:
            try:
                if str(tk.call("info", "commands", "open_file_name")):
                    tk.call("rename", "open_file_name", "")
                tk.call("rename", stock_tcl_command, "open_file_name")
            except Exception:
                pass
        try:
            root_window.deletecommand(callback_command)
        except Exception:
            pass
        commands.open_file_name = stock_open_file_name
        raise

    live_plotter._dmc2_axis_script_loader = loader
    live_plotter._dmc2_stock_open_file_name = stock_open_file_name
    live_plotter._dmc2_stock_open_file_name_tcl = stock_tcl_command
    return loader
