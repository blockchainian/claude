"""Checks for per-call highlighting regressions using repository-generated output."""

import unittest
import tempfile
from pathlib import Path

import shell_output_corpus as corpus


class HighlightBaselineTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        records = corpus.read_jsonl(corpus.DEFAULT_FIXTURE)
        inputs = corpus.scope_inputs(records)
        inputs['paths'].extend(['changes.diff', 'changes.patch'])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, cls.scopes = corpus.audit(corpus.DEFAULT_FIXTURE, root / 'actual.jsonl',
                                         root / 'audit.log', inputs, 180)
        cls.records = [next(r for r in records
                            if r['provider'] == provider and r.get('minimum_highlighted'))
                       for provider in ('claude', 'codex')]
        cls.observed = [{'provider': r['provider'], 'command': r['command'],
                         'actual': r['expected']} for r in cls.records]

    def test_diff_option_after_separator_is_a_path(self):
        self.assertEqual(corpus.stage_source(['git', 'diff', '--', 'a.rs', '--stat']),
                         ('diff', []))
        self.assertIsNone(corpus.stage_source(['git', 'diff', '--stat', '--', 'a.rs']))

    def test_merged_missing_file_diagnostic_is_plain(self):
        record = next(r for r in corpus.read_jsonl(corpus.DEFAULT_FIXTURE)
                      if r['provider'] == 'codex'
                      and r['command'].startswith('cat agents/not-a-file.py | head'))
        record = {k: v for k, v in record.items() if k != 'expected'}
        self.assertEqual(corpus.truth(record, self.scopes), [None] + [self.scopes['languages']['py']] * 5)

    def test_diff_and_patch_resolve_to_the_same_rust_scope(self):
        diff = self.scopes['paths']['changes.diff']
        self.assertIsNotNone(diff)
        self.assertEqual(diff, self.scopes['paths']['changes.patch'])

    def test_individual_loss_fails_even_when_total_increases(self):
        counts = corpus.highlight_counts(self.records, self.observed)
        baseline = [counts[0] + 1, 0]
        result = corpus.compare_highlights(self.records, counts, baseline)
        self.assertGreater(sum(counts), sum(baseline))
        self.assertEqual(len(result['regressed_calls']), 1)
        loss = result['regressed_calls'][0]
        self.assertEqual((loss['record'], loss['provider'], loss['lost']), (1, 'claude', 1))
        self.assertEqual(loss['command'], self.records[0]['command'])

    def test_unchanged_and_improved_calls_pass(self):
        counts = corpus.highlight_counts(self.records, self.observed)
        for baseline in (counts, [0, 0]):
            result = corpus.compare_highlights(self.records, counts, baseline)
            self.assertEqual(result['regressed_calls'], [])
            for provider in ('claude', 'codex'):
                totals = result['providers'][provider]
                self.assertEqual(totals['delta'], totals['after'] - totals['before'])

    def test_invalid_baseline_is_rejected(self):
        for baseline in ([], [-1, 0], [True, 0], ['1', 0]):
            with self.subTest(baseline=baseline), self.assertRaises(ValueError):
                corpus.compare_highlights(self.records, [1, 1], baseline)

    def test_only_removed_diagnostics_are_corrections(self):
        record = next(r for r in corpus.read_jsonl(corpus.DEFAULT_FIXTURE)
                      if r['provider'] == 'codex'
                      and r['command'].startswith('cat agents/not-a-file.py | head'))
        total = len(record['stdout'].splitlines())
        previous = [[{'start': 0, 'end': total, 'language': 'py'}]]
        for first, corrected in ((1, True), (2, False)):
            observed = [{'actual': [{'start': first, 'end': total, 'language': 'py'}]}]
            result = corpus.compare_highlights([record], [total - first], [total],
                                               observed=observed, baseline_spans=previous)
            self.assertEqual(bool(result['corrected_diagnostics']), corrected)
            self.assertEqual(bool(result['regressed_calls']), not corrected)
        # Count-only baselines cannot prove which lines were removed.
        result = corpus.compare_highlights([record], [total - 1], [total])
        self.assertEqual(len(result['regressed_calls']), 1)

    def test_replay_wrong_color_removal_is_not_lost_highlighting(self):
        record = self.records[1]
        total = len(record['stdout'].splitlines())
        previous = [[{'start': 0, 'end': total, 'language': 'toml'}]]
        observed = [self.observed[1]]
        counts = corpus.highlight_counts([record], observed)
        result = corpus.compare_highlights([record], counts, [total],
                                           observed=observed, baseline_spans=previous,
                                           scopes=self.scopes)
        self.assertEqual(result['regressed_calls'], [])
        self.assertEqual(len(result['corrected_colors']), 1)

    def test_replay_losing_correct_color_fails_even_with_equal_counts(self):
        record = self.records[1]
        previous = [record['expected']]
        count = sum(s['end'] - s['start'] for s in previous[0])
        observed = [{'actual': [{'start': 1, 'end': count + 1, 'language': 'toml'}]}]
        result = corpus.compare_highlights([record], [count], [count],
                                           observed=observed, baseline_spans=previous,
                                           scopes=self.scopes)
        self.assertEqual(result['regressed_calls'][0]['regressed_lines'], [1])

    def test_reordered_report_is_rejected(self):
        with self.assertRaises(ValueError):
            corpus.highlight_counts(self.records, list(reversed(self.observed)))


if __name__ == '__main__':
    unittest.main()
