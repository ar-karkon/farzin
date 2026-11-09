import importlib.util
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location("inspect_nnue", Path(__file__).parents[1] / "tools" / "inspect_nnue.py")
inspector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inspector)


class HeaderTests(unittest.TestCase):
    def test_valid_header_and_size(self):
        header = struct.pack("<II", inspector.MAGIC, 15) + bytes(56)
        self.assertEqual(inspector.validate_header(header, inspector.EXPECTED_BYTES)["version"], 15)

    def test_rejects_short_wrong_version_magic_and_length(self):
        valid = struct.pack("<II", inspector.MAGIC, 15) + bytes(56)
        invalid = [
            (valid[:8], inspector.EXPECTED_BYTES),
            (struct.pack("<II", 0, 15) + bytes(56), inspector.EXPECTED_BYTES),
            (struct.pack("<II", inspector.MAGIC, 14) + bytes(56), inspector.EXPECTED_BYTES),
            (valid, inspector.EXPECTED_BYTES - 1),
        ]
        for header, length in invalid:
            with self.subTest(header=header[:8], length=length), self.assertRaises(ValueError):
                inspector.validate_header(header, length)


if __name__ == "__main__":
    unittest.main()
