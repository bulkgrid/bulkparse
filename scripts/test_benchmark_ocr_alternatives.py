import unittest
from benchmark_ocr_alternatives import aggregate, score

class QualityAccounting(unittest.TestCase):
    def test_missing_output_penalized(self):
        result = aggregate([score('', 'one two'), score('three', 'three')])
        self.assertEqual(result['character_errors'], 7)
        self.assertEqual(result['word_errors'], 2)
        self.assertEqual(result['exact'], 1)

    def test_blank_false_positive_counted(self):
        result = aggregate([score('noise', ''), score('abcde', 'abcde')])
        self.assertEqual(result['blank_false_positives'], 1)
        self.assertEqual(result['character_error_rate'], 1)

    def test_no_accent_case_or_order_forgiveness(self):
        self.assertTrue(score('e\u0301 \n next', 'é next')['exact'])
        for actual in ['e next', 'É next', 'next é']:
            self.assertFalse(score(actual, 'é next')['exact'])

if __name__ == '__main__':
    unittest.main()
