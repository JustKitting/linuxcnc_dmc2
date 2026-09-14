"""Asynchronous AXIS presentation of the standard Rust offline mapper."""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import json
from queue import Queue, Empty
import subprocess
from threading import Lock, Thread


class Outcome(Enum):
    RESULT = "result"
    FAILED = "failed"
    CANCELLED = "cancelled"


class FieldKind(Enum):
    TEXT = "text"
    OBJECT = "object"
    SETUP = "setup"
    DESIGN = "design"
    ANALYSIS = "analysis"
    CAPTURE = "capture"
    FILE = "file"
    DIRECTORY = "directory"
    NEW_PATH = "new-path"


@dataclass(frozen=True)
class Field:
    key: str
    label: str
    kind: FieldKind


@dataclass(frozen=True)
class Operation:
    command: str
    label: str
    description: str
    fields: tuple[Field, ...]
    result: str
    input: str

    @classmethod
    def read(cls, row):
        result, input_kind = row["result"], row["input"]
        if result not in ("json", "request") or input_kind not in ("none", "request"):
            raise ValueError("The mapper catalog has an unsupported input/output type. Reload the matched DMC2 binary and catalog.")
        fields = tuple(Field(item["key"], item["label"], FieldKind(item["kind"])) for item in row["fields"])
        return cls(row["command"], row["label"], row["description"], fields, result, input_kind)


@dataclass(frozen=True)
class Result:
    outcome: Outcome
    message: str
    data: object = None
    documents: tuple[tuple[str, str, bool], ...] = ()


def decoded(output, kind):
    if kind == "request":
        return None, (("Request draft", output, True),)
    data = json.loads(output)
    if data.get("schema") == "dmc2.analysis-inspection.v1":
        documents = (
            ("Analysis report", json.dumps(json.loads(data["manifest_text"]), indent=2, ensure_ascii=False), False),
            ("Residual rows", data["residuals_csv"], False),
            ("Retained request", data["request_text"], False),
        )
    else:
        documents = (("Result", json.dumps(data, indent=2, ensure_ascii=False), False),)
    return data, documents


class MapperJob:
    """Own exactly one offline child. Only an operator Cancel requests termination."""
    def __init__(self, project, arguments, result_kind="json", request=None):
        self.project = project
        self.arguments = tuple(arguments)
        self.result_kind = result_kind
        self.request = request
        self.queue = Queue()
        self.lock = Lock()
        self.process = None
        self.cancelled = False
        self.thread = Thread(target=self._run, name="dmc2-object-map", daemon=True)

    def start(self):
        self.thread.start()

    def _run(self):
        try:
            with self.lock:
                if self.cancelled:
                    self.queue.put(Result(Outcome.CANCELLED, "Cancelled before starting."))
                    return
            # Process creation must not hold a lock used by a Tk callback.
            process = subprocess.Popen(
                [str(self.project / "native/bin/dmc2ctl"), "object-map", *self.arguments],
                cwd=self.project, stdin=subprocess.PIPE if self.request is not None else subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            with self.lock:
                self.process = process
                cancelled = self.cancelled
            if cancelled:
                try:
                    process.terminate()
                except ProcessLookupError:
                    pass  # Operator cancelled while this child was starting.
            output, errors = process.communicate(
                self.request.encode("utf-8") if self.request is not None else None
            )
            output = output.decode("utf-8")
            errors = errors.decode("utf-8", errors="replace")
            if process.returncode:
                if self.cancelled:
                    result = Result(Outcome.CANCELLED, "Mapper command cancelled. Some files may have been published. For interrupted object/setup creation, retry the same ID and label to retain its missing record. For an analysis or export, inspect the output and retry with a new ID or path. Existing files are preserved.")
                else:
                    result = Result(Outcome.FAILED, errors.strip() or f"The offline mapper exited with status {process.returncode} without a diagnostic. Retry this operation; if it repeats, preserve its inputs and inspect the DMC2 binary.")
            else:
                data, documents = decoded(output, self.result_kind)
                message = data.get("message", "Result available.") if isinstance(data, dict) else "Request draft available. Fill REQUIRED values, assign contact roles, then Save request as."
                if self.cancelled:
                    message = "The command returned a result before cancellation took effect. Its output is retained below."
                if errors.strip():
                    message += "\n" + errors.strip()
                result = Result(Outcome.RESULT, message, data, documents)
            self.queue.put(result)
        except Exception as error:
            self.queue.put(Result(Outcome.FAILED, f"Could not read the offline mapper result: {error}. Correct the named input or binary issue and retry. Any previously retained records remain available."))
        finally:
            with self.lock:
                self.process = None

    def cancel(self):
        with self.lock:
            self.cancelled = True
            process = self.process
        if process is not None and process.poll() is None:
            try:
                process.terminate()
            except ProcessLookupError:
                pass  # The owned child exited between poll and terminate.

    def take_result(self):
        try:
            return self.queue.get_nowait()
        except Empty:
            return None
