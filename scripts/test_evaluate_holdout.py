import unittest
import subprocess
import tempfile
import venv
from pathlib import Path
from evaluate_holdout import ordered_pair_recall, quality, table_recall, fingerprint_engine


class HoldoutTests(unittest.TestCase):
    def test_engine_preserves_python_virtual_environment(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)/'venv'
            venv.EnvBuilder(with_pip=False,symlinks=True).create(root)
            engine=fingerprint_engine({'name':'test','argv':[str(root/'bin/python')],
                'version':'test','configuration':{'purpose':'venv regression'}})
            actual=subprocess.check_output([*engine['argv'],'-c','import sys; print(sys.prefix)'],text=True).strip()
            self.assertEqual(Path(actual).resolve(),root.resolve())

    def test_missing_blocks_reduce_order_recall(self):
        gold=['red fox','blue bird','green tree']
        self.assertEqual(ordered_pair_recall('red fox blue bird green tree',gold)['ordered_pair_recall'],1)
        self.assertEqual(ordered_pair_recall('green tree blue bird red fox',gold)['ordered_pair_recall'],0)
        self.assertAlmostEqual(ordered_pair_recall('red fox blue bird',gold)['ordered_pair_recall'],1/3)

    def test_plain_table_text_is_not_structured_table(self):
        expected=[['Item','Count'],['Apples','12']]
        self.assertEqual(table_recall('Item Count Apples 12',expected),0)
        self.assertEqual(table_recall('| Item | Count |\n| --- | --- |\n| Apples | 12 |',expected),1)
        self.assertEqual(table_recall('| Item | Count |\n| Apples | 21 |',expected),.5)

    def test_failure_empty_output_scores_zero_recall(self):
        result=quality('', 'Expected body text', {'ordered_blocks':['Expected','body text']})
        self.assertEqual(result['token_recall_proxy'],0)
        self.assertEqual(result['ordered_pair_recall'],0)

    def test_normalization_is_separate_from_literal_score(self):
        result=quality('Ｆｉｌｅ','File',{})
        self.assertEqual(result['token_recall_proxy'],0)
        self.assertEqual(result['nfkc_token_recall_proxy'],1)


if __name__=='__main__':unittest.main()
