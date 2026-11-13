"""Check the release metadata boundary before generating installable formulae."""
import copy
import unittest
from homebrew import TARGETS, render


class Formula(unittest.TestCase):
    def setUp(self):
        names = [f"lenso-v1.2.3-{target}.tar.gz" for targets in TARGETS.values() for target in targets.values()]
        self.checksums = "".join("a" * 64 + "  " + name + "\n" for name in names).encode()
        self.release = {"tag_name": "v1.2.3", "draft": False, "prerelease": False,
                        "assets": [{"name": name, "state": "uploaded", "digest": "sha256:" + "a" * 64} for name in names]}

    def test_all_four_platforms_and_homebrew_ownership(self):
        formula = render(self.release, self.checksums)
        for asset in self.release["assets"]:
            self.assertIn("/releases/download/v1.2.3/" + asset["name"], formula)
        self.assertIn('assert_equal "managed"', formula)
        self.assertNotIn("lenso-install.json", formula)

    def test_unpublished_or_unstable_release_is_rejected(self):
        for field, value in [("draft", True), ("prerelease", True), ("tag_name", "code-v1.2.3"), ("tag_name", "v1.2.3-rc.1")]:
            release = {**self.release, field: value}
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                render(release, self.checksums)

    def test_missing_conflicting_and_duplicate_pins_are_rejected(self):
        for change in ("missing", "digest", "state"):
            release = copy.deepcopy(self.release)
            if change == "missing":
                release["assets"].pop()
            else:
                release["assets"][0][change] = "invalid"
            with self.subTest(change=change), self.assertRaises(ValueError):
                render(release, self.checksums)
        with self.assertRaises(ValueError):
            render(self.release, self.checksums + self.checksums)


if __name__ == "__main__":
    unittest.main()
