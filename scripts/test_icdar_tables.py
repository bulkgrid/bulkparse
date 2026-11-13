import unittest
from pathlib import Path
if not (Path(__file__).resolve().parents[1]/'target/opendataloader-bench/src/evaluator_table.py').is_file():
 raise unittest.SkipTest('Provision the upstream table evaluator to run ICDAR scoring tests')
from benchmark_icdar_tables import canonical,assignment,score_page,prediction_tables

def table(text='A',span=1):return {'cells':[{'row':0,'col':0,'rowspan':1,'colspan':span,'text':text}]}
class TableEvaluation(unittest.TestCase):
 def test_spans_holes_and_escaping(self):
  t=table('<A>',2);t['cells'].append({'row':1,'col':1,'rowspan':1,'colspan':1,'text':'B'})
  s=canonical(t['cells']);self.assertEqual(s.count('<td'),3);self.assertIn('&lt;A&gt;',s);self.assertIn('colspan="2"',s)
  self.assertEqual(score_page([t],[t])['teds'],1)
 def test_missing_extra_and_wrong_structure(self):
  self.assertEqual(score_page([table()],[])['teds'],0)
  self.assertEqual(score_page([table()],[table(),table('different')])['teds'],.5)
  self.assertLess(score_page([table(span=2)],[table()])['teds_s'],1)
  self.assertEqual(score_page([table('A')],[table('B')])['matched'],0)
 def test_global_assignment_not_greedy(self):
  self.assertEqual(set(assignment([[.9,.8],[.85,.1]])),{(0,1),(1,0)})
  self.assertEqual(assignment([[]]),[])
 def test_prediction_provenance_and_rowspan(self):
  v={'document':{'blocks':[{'type':'table','source':{'page':2},'rows':[[{'source':{'cell':1},'row_span':2,'col_span':1,'blocks':[]},{'source':{'cell':2},'blocks':[]}],[{'source':{'cell':2},'blocks':[]}]]}]}}
  t=prediction_tables(v)[0];self.assertEqual(t['cells'][2]['col'],1);self.assertEqual(canonical(t['cells']).count('<td'),3)
if __name__=='__main__':unittest.main()
