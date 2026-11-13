import unittest,tempfile,json,hashlib
from pathlib import Path
from score_realworld_judging import aggregate,DIMS
from submit_realworld_judging import validate
class QualityGateTests(unittest.TestCase):
 def setUp(self):
  self.mapping=[{'custom_id':'p-'+str(i),'pair_id':'p','order':i,'document_id':'doc','format':'docx','A':['bulkparse','other'][i],'B':['other','bulkparse'][i],'A_metadata':{'normalized_chars':10},'B_metadata':{'normalized_chars':10}} for i in [0,1]]
  self.verdicts={r['custom_id']:{'reference_usable':True,'reference_problem':'','A':{d:80 if r['A']=='bulkparse' else 60 for d in DIMS},'B':{d:80 if r['B']=='bulkparse' else 60 for d in DIMS}} for r in self.mapping}
 def test_swapping_does_not_change_engine_scores(self):
  q=aggregate(self.mapping,self.verdicts);self.assertEqual(q['summary']['bulkparse']['score'],80);self.assertEqual(q['summary']['other']['score'],60)
 def test_missing_order_prevents_publication(self):
  del self.verdicts['p-1']
  with self.assertRaises(ValueError):aggregate(self.mapping,self.verdicts)
 def test_empty_output_cannot_receive_credit(self):
  self.mapping[0]['A_metadata']['normalized_chars']=0
  with self.assertRaises(ValueError):aggregate(self.mapping,self.verdicts)
 def test_unusable_reference_is_visible_and_not_scored(self):
  self.verdicts['p-1']['reference_usable']=False;self.verdicts['p-1']['reference_problem']='Unreadable print fragments'
  q=aggregate(self.mapping,self.verdicts);self.assertEqual(q['summary'],{});self.assertEqual(len(q['excluded_pairs']),1)
 def test_invalid_score_prevents_publication(self):
  self.verdicts['p-0']['A']['structure']=101
  with self.assertRaises(ValueError):aggregate(self.mapping,self.verdicts)
class PaidSubmissionGateTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.p=Path(self.tmp.name)
  (self.p/'blind-mapping.json').write_text('[]')
  (self.p/'cost-estimate.json').write_text(json.dumps({'conservative_planning_ceiling_usd':20}))
  def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
  manifest={'mapping_sha256':sha(self.p/'blind-mapping.json'),'cost_estimate_sha256':sha(self.p/'cost-estimate.json'),'batches':[]}
  (self.p/'submission-manifest.json').write_text(json.dumps(manifest));self.approval=sha(self.p/'submission-manifest.json')
 def tearDown(self):self.tmp.cleanup()
 def test_wrong_approval_is_rejected(self):
  with self.assertRaises(ValueError):validate(self.p,'wrong',30)
 def test_insufficient_or_nonfinite_budget_is_rejected(self):
  for value in [0,19.99,float('nan'),float('inf')]:
   with self.assertRaises(ValueError):validate(self.p,self.approval,value)
 def test_changed_cost_is_rejected(self):
  (self.p/'cost-estimate.json').write_text(json.dumps({'conservative_planning_ceiling_usd':10}))
  with self.assertRaises(ValueError):validate(self.p,self.approval,30)
if __name__=='__main__':unittest.main()
