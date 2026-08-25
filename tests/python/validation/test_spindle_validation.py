from __future__ import annotations

import unittest

from dmc2_validation import validate_h100_spindle_integration


class H100SpindleValidationTests(unittest.TestCase):
    def test_native_rust_component_contract_is_complete(self):
        result = validate_h100_spindle_integration()

        self.assertIn("native Rust HAL component", result)
        self.assertIn("fail-stopped sequencer release", result)


if __name__ == "__main__":
    unittest.main()
