from __future__ import annotations

import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parent
PANEL = ROOT / "status_panel.xml"
POSTGUI = ROOT / "status_postgui.hal"


class StatusPanelLayoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = ET.parse(PANEL).getroot()
        cls.main = cls.root.find("vbox")
        cls.parent = {
            child: parent
            for parent in cls.root.iter()
            for child in parent
        }

    def frame(self, title: str):
        return next(
            frame
            for frame in self.root.iter("labelframe")
            if frame.attrib["text"] == title
        )

    @staticmethod
    def table_rows(table):
        rows = []
        current = None
        for child in table:
            if child.tag == "tablerow":
                if current is not None:
                    rows.append(current)
                current = []
            elif child.tag not in {"tablesticky", "tablespan"}:
                if current is not None:
                    current.append(child)
        if current is not None:
            rows.append(current)
        return rows

    def ancestor_frame_title(self, node):
        while node in self.parent:
            node = self.parent[node]
            if node.tag == "labelframe":
                return node.attrib["text"]
        return None

    def test_control_ready_and_fault_are_top_level(self):
        frames = self.main.findall("labelframe")
        self.assertEqual(frames[0].attrib["text"], "CONTROL STATE")
        pins = {
            node.attrib.get("halpin")
            for node in frames[0].iter()
        }
        self.assertIn('"controller-ready"', pins)
        self.assertIn('"controller-fault"', pins)
        self.assertIn('"linuxcnc-error-active"', pins)
        self.assertIn('"linuxcnc-warning-active"', pins)
        self.assertIn('"linuxcnc-unknown-code"', pins)

    def test_linuxcnc_diagnostics_are_top_level_and_observational(self):
        frame = self.frame("CONTROL STATE")
        pins = {node.attrib.get("halpin") for node in frame.iter()}
        self.assertTrue(
            {
                '"linuxcnc-error-active"',
                '"linuxcnc-warning-active"',
                '"linuxcnc-unknown-code"',
            }
            <= pins
        )
        postgui = POSTGUI.read_text(encoding="utf-8")
        self.assertIn(
            "dmc2-task-monitor.linuxcnc-error-active => pyvcp.linuxcnc-error-active",
            postgui,
        )
        self.assertIn(
            "dmc2-task-monitor.unknown-code-active => pyvcp.linuxcnc-unknown-code",
            postgui,
        )

    def test_all_operator_text_is_left_justified_and_uses_one_font(self):
        for label in self.root.iter("label"):
            self.assertEqual(label.attrib.get("anchor"), '"w"')
            self.assertIn("Helvetica", label.attrib["font"])
        for frame in self.root.iter("labelframe"):
            self.assertIn("Helvetica", frame.attrib["font"])
        for widget_name in ("number", "s32", "u32", "button"):
            for widget in self.root.iter(widget_name):
                self.assertIn("Helvetica", widget.attrib["font"])
                if widget_name != "button":
                    self.assertEqual(widget.attrib.get("anchor"), '"w"')

    def test_short_panels_remain_paired_side_by_side(self):
        rows = self.main.findall("hbox")
        frame_pairs = [
            [frame.attrib["text"] for frame in row.findall("labelframe")]
            for row in rows
        ]
        self.assertIn(
            ["MACHINE COORDINATES (mm)", "MESA GENERATED PULSES"],
            frame_pairs,
        )
        self.assertIn(
            ["LIMIT SWITCHES", "CONTACT SENSORS"],
            frame_pairs,
        )

    def test_live_seen_columns_are_identical(self):
        limits = self.table_rows(self.frame("LIMIT SWITCHES").find("table"))
        contacts = self.table_rows(self.frame("CONTACT SENSORS").find("table"))
        for rows in (limits, contacts):
            self.assertEqual(
                [node.attrib.get("text") for node in rows[0]],
                ['"Input"', '"LIVE"', '"SEEN"'],
            )
            self.assertEqual(
                [node.attrib.get("width") for node in rows[0]],
                ["17", "7", "7"],
            )

    def test_buttons_have_separate_readable_placements(self):
        buttons = {
            button.attrib["halpin"]: button
            for button in self.root.iter("button")
        }
        self.assertEqual(
            set(buttons),
            {
                '"home-all"',
                '"puck-test-start"',
                '"puck-test-stop"',
                '"clear-display-latches"',
            },
        )
        self.assertEqual(
            self.ancestor_frame_title(buttons['"home-all"']),
            "POSITION AND HOMING",
        )
        self.assertEqual(
            self.ancestor_frame_title(buttons['"clear-display-latches"']),
            "DISPLAY HISTORY",
        )
        self.assertEqual(
            self.ancestor_frame_title(buttons['"puck-test-start"']),
            "PUCK CONNECTIVITY TEST - NO MOTION",
        )
        self.assertEqual(
            self.ancestor_frame_title(buttons['"puck-test-stop"']),
            "PUCK CONNECTIVITY TEST - NO MOTION",
        )
        self.assertEqual(
            buttons['"clear-display-latches"'].attrib["text"],
            '"CLEAR SEEN"',
        )

    def test_puck_connectivity_test_is_explicitly_no_motion(self):
        frame = self.frame("PUCK CONNECTIVITY TEST - NO MOTION")
        pins = {
            node.attrib.get("halpin")
            for node in frame.iter()
            if node.attrib.get("halpin") is not None
        }
        self.assertEqual(
            pins,
            {
                '"puck-test-start"',
                '"puck-test-stop"',
                '"puck-test-active"',
                '"puck-test-time-left"',
            },
        )
        text = " ".join(node.attrib.get("text", "") for node in frame.iter())
        self.assertIn("PASS = Puck / IN0 SEEN", text)

    def test_home_button_routes_to_linuxcnc_home_all(self):
        postgui = POSTGUI.read_text(encoding="utf-8")
        self.assertIn(
            "net dmc2-home-all-request pyvcp.home-all => halui.home-all",
            postgui,
        )

    def test_position_and_homing_is_one_compact_inline_row(self):
        frame = self.frame("POSITION AND HOMING")
        row = frame.find("hbox")
        visible = [
            node
            for node in row
            if node.tag not in {"boxexpand", "boxfill", "boxanchor"}
        ]
        self.assertEqual(
            [(node.tag, node.attrib.get("text"), node.attrib.get("halpin")) for node in visible],
            [
                ("button", '"HOME ALL"', '"home-all"'),
                ("label", '"      "', None),
                ("label", '"KNOWN"', None),
                ("rectled", None, '"position-known"'),
                ("label", '"      "', None),
                ("label", '"UNKNOWN"', None),
                ("rectled", None, '"position-unknown"'),
            ],
        )
        self.assertEqual(list(frame.iter("vbox")), [])
        text = " ".join(node.attrib.get("text", "") for node in frame.iter())
        self.assertNotIn("coordinates are not trusted", text)
        self.assertNotIn("Sequence:", text)

    def test_only_physical_selector_positions_are_displayed(self):
        pins = {node.attrib.get("halpin") for node in self.root.iter()}
        required = {
            '"pendant-axis-x"',
            '"pendant-axis-y"',
            '"pendant-axis-z"',
            '"pendant-axis-4"',
            '"pendant-axis-5"',
            '"pendant-axis-off"',
            '"pendant-multiplier-x1"',
            '"pendant-multiplier-x10"',
            '"pendant-multiplier-x100"',
        }
        hidden_internal_states = {
            '"pendant-axis-invalid"',
            '"pendant-multiplier-off"',
            '"pendant-multiplier-invalid"',
        }
        self.assertTrue(required <= pins)
        self.assertTrue(hidden_internal_states.isdisjoint(pins))

    def test_axis_and_scale_positions_share_vertical_columns(self):
        table = self.frame("PENDANT SELECTORS").find("table")
        axis, scale = self.table_rows(table)
        self.assertEqual(axis[0].attrib["text"], '"Axis"')
        self.assertEqual(scale[0].attrib["text"], '"Scale"')
        for index, axis_text, scale_text in (
            (1, '"X"', '"X1"'),
            (4, '"Y"', '"X10"'),
            (7, '"Z"', '"X100"'),
        ):
            self.assertEqual(axis[index].attrib["text"], axis_text)
            self.assertEqual(scale[index].attrib["text"], scale_text)
        for index in (3, 6):
            self.assertEqual(axis[index].attrib["width"], "3")
            self.assertEqual(scale[index].attrib["width"], "3")

    def test_pendant_indicator_groups_have_explicit_spacing(self):
        rows = self.table_rows(self.frame("PENDANT STATUS").find("table"))
        for row in rows[:2]:
            spacers = [
                node
                for node in row
                if node.tag == "label" and node.attrib.get("text") == '"    "'
            ]
            self.assertEqual(len(spacers), 2)
            self.assertTrue(all(node.attrib["width"] == "4" for node in spacers))

    def test_debug_counters_do_not_clutter_operator_panel(self):
        labels = {
            node.attrib.get("text", "").lower()
            for node in self.root.iter("label")
        }
        self.assertFalse(any("encoder errors" in label for label in labels))
        self.assertEqual(list(self.root.iter("u32")), [])


if __name__ == "__main__":
    unittest.main()
