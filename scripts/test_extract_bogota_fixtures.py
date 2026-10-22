"""Regression checks for complete, uncontaminated Bogota corpus extraction."""

from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from extract_bogota_fixtures import validate_extraction


class ExtractionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.destination = Path(self.temporary.name)
        self.eips = [8141, 8250, 8272, 7906]
        self.selected = set()
        for index, eip in enumerate(self.eips):
            kind = "state_tests" if index % 2 else "blockchain_tests"
            relative = Path(kind, f"eip{eip}_test", "fixture.json")
            self.write(relative)
            self.selected.add(relative)

    def write(self, relative: Path) -> None:
        output = self.destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text("{}\n", encoding="utf-8")

    def test_complete_selection_passes(self) -> None:
        validate_extraction(self.destination, self.selected, self.eips, 4)

    def test_wrong_file_total_fails(self) -> None:
        with self.assertRaisesRegex(SystemExit, "fixture count mismatch"):
            validate_extraction(self.destination, self.selected, self.eips, 5)

    def test_stale_json_from_previous_extraction_fails(self) -> None:
        self.write(Path("state_tests", "stale.json"))
        with self.assertRaisesRegex(SystemExit, "fixture selection mismatch"):
            validate_extraction(self.destination, self.selected, self.eips, 4)

    def test_missing_eip_family_fails_even_with_matching_file_total(self) -> None:
        missing = next(path for path in self.selected if "eip7906_test" in path.parts)
        self.selected.remove(missing)
        (self.destination / missing).unlink()
        with self.assertRaisesRegex(SystemExit, "no JSON fixtures selected for EIP-7906"):
            validate_extraction(self.destination, self.selected, self.eips, 3)

    def test_missing_fixture_kind_fails(self) -> None:
        selected = set()
        for relative in self.selected:
            (self.destination / relative).unlink()
            relative = Path("state_tests", *relative.parts[1:])
            self.write(relative)
            selected.add(relative)
        with self.assertRaisesRegex(SystemExit, "no JSON fixtures selected for blockchain_tests"):
            validate_extraction(self.destination, selected, self.eips, 4)


if __name__ == "__main__":
    unittest.main()
