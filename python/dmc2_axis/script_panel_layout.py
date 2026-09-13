"""Create the stock AXIS Custom Scripts tab and reusable parameter widgets."""

from __future__ import annotations

from .constants import CUSTOM_SCRIPTS_CONTENT, CUSTOM_SCRIPTS_FRAME, GO_TO_HOME_WIDGET_PATH, PROBE_SECTION_PATH


def create_pane(namespace):
    root = namespace["root_window"]
    tk = root.tk
    tabs = str(namespace["vcp_frame"]) + ".dmc2_tabs"
    tk.call(tabs, "itemconfigure", "pendant", "-text", "Pendant")
    frame = str(tk.call(tabs, "insert", "end", "scripts", "-text", "Custom Scripts"))
    if frame != CUSTOM_SCRIPTS_FRAME:
        raise RuntimeError(f"Unexpected Custom Scripts tab path: {frame}")
    canvas = frame + ".body"
    scrollbar = frame + ".scroll"
    tk.call("canvas", canvas, "-highlightthickness", 0, "-width", 1, "-height", 1)
    tk.call("scrollbar", scrollbar, "-orient", "vertical", "-command", (canvas, "yview"))
    tk.call(canvas, "configure", "-yscrollcommand", (scrollbar, "set"))
    tk.call("pack", scrollbar, "-side", "right", "-fill", "y")
    tk.call("pack", canvas, "-side", "left", "-fill", "both", "-expand", 1)
    tk.call("frame", CUSTOM_SCRIPTS_CONTENT, "-padx", 4, "-pady", 4)
    item = tk.call(canvas, "create", "window", 0, 0, "-anchor", "nw", "-window", CUSTOM_SCRIPTS_CONTENT)
    tk.call("bind", CUSTOM_SCRIPTS_CONTENT, "<Configure>", f"{canvas} configure -scrollregion [{canvas} bbox all]")
    tk.call("bind", canvas, "<Configure>", f"{canvas} itemconfigure {item} -width %w")
    # Create the recorder independently, before script/catalog setup can fail.
    tk.call("labelframe", PROBE_SECTION_PATH, "-text", "Probe recorder", "-padx", 4, "-pady", 4)
    tk.call("pack", PROBE_SECTION_PATH, "-side", "bottom", "-fill", "x", "-pady", 4)
    # Match the existing pendant status page, keeping the scripts scrollable.
    tk.call(tabs, "compute_size")
    return CUSTOM_SCRIPTS_CONTENT


def create_script_widgets(binding, script):
    tk, root = binding.tk, binding.root
    path = CUSTOM_SCRIPTS_CONTENT + "." + script.key
    tk.call("labelframe", path, "-text", script.operation.label, "-padx", 4, "-pady", 4)
    tk.call("label", path + ".description", "-text", script.description, "-justify", "left", "-anchor", "w")
    tk.call("pack", path + ".description", "-fill", "x")
    tk.call("bind", path + ".description", "<Configure>", f"{path}.description configure -wraplength [expr {{max(1, %w - 8)}}]")
    fields = path + ".fields"
    tk.call("frame", fields)
    tk.call("pack", fields, "-fill", "x", "-pady", 4)
    tk.call("grid", "columnconfigure", fields, 0, "-weight", 1)
    inputs = []
    for row, parameter in enumerate(script.parameters):
        field = fields + ".p" + str(row)
        tk.call("label", field + "label", "-text", parameter.label + " (mm)", "-anchor", "w")
        tk.call("entry", field, "-textvariable", str(binding.variables[parameter.pin]), "-width", 8)
        tk.call("bind", field, "<FocusOut>", root.register(lambda key=script.key: binding.save_preferences(key)))
        tk.call("grid", field + "label", "-row", row * 2, "-column", 0, "-columnspan", 3, "-sticky", "w")
        tk.call("grid", field, "-row", row * 2 + 1, "-column", 0, "-sticky", "ew", "-padx", 4)
        inputs.append(field)
        for column, sign, text in ((1, -1, "−"), (2, 1, "+")):
            button = field + ("minus" if sign < 0 else "plus")
            tk.call("button", button, "-text", text, "-width", 2, "-takefocus", 0,
                    "-command", root.register(lambda item=parameter, direction=sign, key=script.key: binding.increment(key, item, direction)))
            tk.call("grid", button, "-row", row * 2 + 1, "-column", column, "-padx", 1)
            inputs.append(button)
    tk.call("button", path + ".run", "-text", "Run " + script.operation.label, "-state", "disabled",
            "-command", root.register(lambda key=script.key: binding.run(key)))
    tk.call("pack", path + ".run", "-fill", "x", "-pady", 4)
    tk.call("label", path + ".status", "-text", "Checking requirements…", "-justify", "left", "-anchor", "w")
    tk.call("pack", path + ".status", "-fill", "x")
    tk.call("bind", path + ".status", "<Configure>", f"{path}.status configure -wraplength [expr {{max(1, %w - 8)}}]")
    return {"frame": path, "run": path + ".run", "status": path + ".status", "inputs": inputs}


def create_home_widgets(binding, operation):
    tk, root = binding.tk, binding.root
    frame = CUSTOM_SCRIPTS_CONTENT + ".home"
    tk.call("labelframe", frame, "-text", "Return to home position", "-padx", 4, "-pady", 4)
    tk.call("pack", frame, "-fill", "x", "-pady", 4, "-before", PROBE_SECTION_PATH)
    tk.call("button", GO_TO_HOME_WIDGET_PATH, "-text", operation.label, "-state", "disabled",
            "-command", root.register(lambda: binding.run("home")))
    tk.call("pack", GO_TO_HOME_WIDGET_PATH, "-fill", "x")
    tk.call("label", frame + ".status", "-text", "Uses the established machine home; Home All remains in Manual Control.", "-justify", "left", "-anchor", "w")
    tk.call("pack", frame + ".status", "-fill", "x")
    tk.call("bind", frame + ".status", "<Configure>", f"{frame}.status configure -wraplength [expr {{max(1, %w - 8)}}]")
    return {"run": GO_TO_HOME_WIDGET_PATH, "status": frame + ".status"}
