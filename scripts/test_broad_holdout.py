import unittest
from check_broad_holdout import check,ROWS

class HoldoutChecks(unittest.TestCase):
    def test_plain_text_is_not_a_structured_table(self):
        self.assertFalse(any(check('School First grade Second grade Third grade Fourth grade\n'+str(ROWS),{'kind':'table','rows':ROWS}).values()))

    def test_wrong_value_fails_its_row(self):
        md='|School|First grade|Second grade|Third grade|Fourth grade|\n|---|---|---|---|---|\n'
        md+='\n'.join('|'+ '|'.join(r)+'|' for r in ROWS)
        self.assertTrue(all(check(md,{'kind':'table','rows':ROWS}).values()))
        changed=md.replace('|23|24|','|99|24|')
        self.assertFalse(check(changed,{'kind':'table','rows':ROWS})['data_row_1'])

    def test_section_order_matters(self):
        md='# Application for this and that\n## List\n## Another list\n## Header One\n## Another Header One'
        result=check(md,{'kind':'columns'})
        self.assertTrue(result['all_section_labels']);self.assertFalse(result['section_sequence'])

if __name__=='__main__':unittest.main()
