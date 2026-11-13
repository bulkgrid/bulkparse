import unittest
from report_diagnostic_categories import summarize

class CategoryAccounting(unittest.TestCase):
    def test_documents_are_deduplicated_and_uncertainty_is_not_confirmed_loss(self):
        def warning(category, evidence, code):
            return dict(category=category,evidence=evidence,code=code,message=code)
        loss=warning('missing_content','detected','image')
        risk=warning('missing_content','unverified','generic')
        fmt=warning('formatting_limitation','detected','merged_cells')
        info=warning('informational','detected','source_formula_cache')
        rows=[dict(id=str(i),format='test',ok=True,status='partial',diagnostics=ds,render_diagnostics=rs) for i,(ds,rs) in enumerate([
            ([loss,loss,risk,info],[fmt]),([risk],[fmt]),([],[fmt]),([info],[]),([],[])
        ])]
        result=summarize(rows)
        self.assertEqual(result['exclusive_counts'],dict(detected_missing_content=1,unverified_content_risk_only=1,formatting_without_content_warning=1,informational_only=1,no_diagnostics=1))
        self.assertEqual(next(r for r in result['codes'] if r['code']=='image')['count'],1)
        self.assertEqual(result['overlapping_category_counts']['missing_content'],2)
        self.assertEqual(result['overlapping_category_counts']['formatting_limitation'],3)
        self.assertEqual(result['documents'][2]['diagnostics'][0]['stage'],'markdown')

if __name__=='__main__':unittest.main()
