from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest


SCRIPTS_DIR = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

from check_duplicate_rust_fns import find_duplicates


class DuplicateRustFunctionsTest(unittest.TestCase):
    def test_detects_duplicate_free_functions_across_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "first.rs").write_text("fn shared_helper() {}\n", encoding="utf-8")
            (root / "second.rs").write_text(
                "pub async fn shared_helper() {}\n", encoding="utf-8"
            )

            duplicates = find_duplicates([root])

            self.assertEqual(set(duplicates), {"shared_helper"})
            self.assertEqual(len(duplicates["shared_helper"]), 2)

    def test_ignores_methods_trait_items_and_binary_main(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / "first.rs").write_text(
                """
trait Reader {
    fn read(&self);
}

impl Reader for Value {
    fn read(&self) {}
}

fn main() {}
""",
                encoding="utf-8",
            )
            (root / "second.rs").write_text(
                """
impl Value {
    fn read(&self) {}
}

fn main() {}
""",
                encoding="utf-8",
            )

            self.assertEqual(find_duplicates([root]), {})


if __name__ == "__main__":
    unittest.main()
