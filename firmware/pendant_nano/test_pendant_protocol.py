import unittest

from pendant_protocol import ProtocolError, parse_packet, signed_int32_delta


class ProtocolTests(unittest.TestCase):
    def test_valid_packet(self):
        packet = parse_packet("P3,42,840,-2,-7,0,-1,X,X10,1,0,1")
        self.assertEqual(packet.sequence, 42)
        self.assertEqual(packet.detent_count, -2)
        self.assertEqual(packet.transition_count, -7)
        self.assertEqual(packet.latest_detent_signal, -1)
        self.assertEqual(packet.axis, "X")
        self.assertEqual(packet.multiplier, "X10")
        self.assertTrue(packet.deadman_held)
        self.assertFalse(packet.estop_pressed)
        self.assertTrue(packet.selector_valid)

    def test_off_axis_can_report_x1_only_while_side_button_is_held(self):
        packet = parse_packet("P3,7,140,0,0,0,0,N,X1,1,0,0")
        self.assertEqual(packet.axis, "N")
        self.assertEqual(packet.multiplier, "X1")
        self.assertTrue(packet.deadman_held)
        self.assertFalse(packet.selector_valid)
        self.assertFalse(packet.cnc_axis_valid)

    def test_axis_four_is_decoded_but_not_a_configured_cnc_axis(self):
        packet = parse_packet("P3,1,20,0,0,0,0,4,X1,1,0,1")
        self.assertTrue(packet.selector_valid)
        self.assertFalse(packet.cnc_axis_valid)

    def test_rejects_bad_boolean(self):
        with self.assertRaises(ProtocolError):
            parse_packet("P3,1,20,0,0,0,0,X,X1,maybe,0,1")

    def test_rejects_more_than_one_command_signal_per_poll(self):
        with self.assertRaises(ProtocolError):
            parse_packet("P3,1,20,1000000,4000000,0,1000000,X,X1,1,0,1")

    def test_rejects_bad_field_count(self):
        with self.assertRaises(ProtocolError):
            parse_packet("P3,1,20")

    def test_rejects_old_protocol(self):
        with self.assertRaises(ProtocolError):
            parse_packet("P2,42,840,-2,-7,0,X,X10,1,0,1")

    def test_signed_wraparound(self):
        self.assertEqual(signed_int32_delta(-2147483648, 2147483647), 1)
        self.assertEqual(signed_int32_delta(2147483647, -2147483648), -1)


if __name__ == "__main__":
    unittest.main()
