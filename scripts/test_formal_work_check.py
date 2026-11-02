"""Checks the input boundary of the production-to-Lean adapter."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('work', Path(__file__).with_name('formal-work-check.py'))
work = importlib.util.module_from_spec(spec)
spec.loader.exec_module(work)


class WorkCorpus(unittest.TestCase):
    def test_boundary_cases_and_determinism(self):
        rows = work.corpus()
        self.assertEqual(rows, work.corpus())
        pairs = {(int(r['target']), int(r['hash'])) for r in rows}
        self.assertEqual(len(rows), len(pairs))
        for pair in [(0, 0), (1, 0), (1, 1), (1, 2), (work.MAX, work.MAX),
                     (1 << 255, (1 << 255) + 1)]:
            self.assertIn(pair, pairs)
        self.assertTrue(all(0 <= a <= work.MAX and 0 <= b <= work.MAX for a, b in pairs))

    def test_outputs_only(self):
        original = work.corpus()[:2]
        changed = copy.deepcopy(original)
        changed[0]['hashWork'] = '123'
        work.check_inputs(changed, original)
        for invalid in [changed[:1], list(reversed(changed)), [dict(changed[0], target='999'), changed[1]],
                        [dict(changed[0], extra=1), changed[1]]]:
            with self.assertRaises(ValueError):
                work.check_inputs(invalid, original)


if __name__ == '__main__':
    unittest.main()
