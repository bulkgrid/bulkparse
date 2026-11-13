import unittest
from evaluate_independent_pdf import match_counts,norm,evaluate,clean_table
class EvaluationChecks(unittest.TestCase):
 def test_duplicate_predictions_cannot_claim_one_reference_twice(self):
  x=match_counts(['Methods','Methods'],['Methods']);self.assertEqual((x['tp'],x['fp'],x['fn']),(1,1,0))
 def test_empty_reference_is_an_unmatched_heading(self):
  x=match_counts(['Introduction'],['Introduction','']);self.assertEqual((x['tp'],x['fp'],x['fn']),(1,0,1))
 def test_unicode_and_punctuation_normalization(self):
  self.assertEqual(norm('1. Ofﬁce—CAFÉ'),norm('1 office café'))
 def test_missing_anchor_stays_in_order_denominator(self):
  ref={'headings':[],'text':'Alpha second missing','order_pairs':[('alpha','second'),('second','missing')],'unscorable_heading_regions':0}
  d={'document':{'blocks':[{'type':'paragraph','inlines':[{'text':'Alpha second'}]}]}}
  r=evaluate(d,ref);self.assertEqual((r['order_correct_pairs'],r['order_reference_pairs']),(1,2))
 def test_table_identity_is_not_table_structure(self):
  self.assertEqual(clean_table({'id':'b1','source':{'page':1},'rows':[{'row_span':2,'id':'b3'}]}),{'rows':[{'row_span':2}]})
if __name__=='__main__':unittest.main()
