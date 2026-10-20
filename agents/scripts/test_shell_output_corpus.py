"""Checks for per-call highlighting regressions using repository-generated output."""

import unittest

import shell_output_corpus as corpus


class HighlightBaselineTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        records = corpus.read_jsonl(corpus.DEFAULT_FIXTURE)
        cls.records = [next(r for r in records
                            if r['provider'] == provider and r.get('minimum_highlighted'))
                       for provider in ('claude', 'codex')]
        cls.observed = [{'provider': r['provider'], 'command': r['command'],
                         'actual': r['expected']} for r in cls.records]

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

    def test_reordered_report_is_rejected(self):
        with self.assertRaises(ValueError):
            corpus.highlight_counts(self.records, list(reversed(self.observed)))


if __name__ == '__main__':
    unittest.main()
