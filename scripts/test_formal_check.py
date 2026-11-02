"""Regression checks for the trace harness's trust boundary (no Lean build)."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("formal_check", Path(__file__).with_name("formal-check.py"))
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)


class TraceInputs(unittest.TestCase):
    def test_only_output_can_change(self):
        expected = [{"input": {"amount": 1}, "result": None}]
        valid = copy.deepcopy(expected)
        valid[0]["result"] = {"amount": 2}
        harness.check_inputs(valid, expected, "result")
        mutations = [[], expected + expected, [{"input": {"amount": 2}, "result": None}],
                     [{"input": {"amount": 1}}], [{"input": {"amount": 1}, "result": None, "extra": 0}]]
        for mutated in mutations:
            with self.subTest(mutated=mutated), self.assertRaises(ValueError):
                harness.check_inputs(mutated, expected, "result")

    def test_reordering_is_not_allowed(self):
        rows = [{"input": 1, "result": None}, {"input": 2, "result": None}]
        with self.assertRaises(ValueError):
            harness.check_inputs(rows[::-1], rows, "result")

    def test_local_hash_cannot_replace_upstream_cases(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            upstream = root / "upstream.json"
            upstream.write_text('[{"input": 1, "result": null}]')
            corpus = root / "corpus.json"
            corpus.write_text(upstream.read_text())
            provenance = {"repository": "0xLattice/lattice-formal", "revision": harness.REVISION,
                          "cases": 1, "sha256": harness.sha256(corpus),
                          "sources": [{"path": "upstream.json", "sha256": harness.sha256(upstream)}]}
            meta = root / "provenance.json"
            meta.write_text(json.dumps(provenance))
            harness.load_corpus(root, corpus)
            corpus.write_text('[{"input": 2, "result": null}]')
            provenance["sha256"] = harness.sha256(corpus)
            meta.write_text(json.dumps(provenance))
            with self.assertRaisesRegex(ValueError, "reproduce pinned upstream"):
                harness.load_corpus(root, corpus)
            # Changing the upstream file without updating its recorded hash also fails.
            upstream.write_text(corpus.read_text())
            with self.assertRaisesRegex(ValueError, "upstream corpus provenance"):
                harness.load_corpus(root, corpus)


if __name__ == "__main__":
    unittest.main()
