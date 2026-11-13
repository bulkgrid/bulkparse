#!/usr/bin/env python3
"""Freeze source-grounded legacy expectations, including known remaining gaps."""
import hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'reports/multiformat-legacy-layout-2026-11-12'
def main():
 spec=json.loads((ROOT/'reports/multiformat-tables-layout-2026-11-12/quality-checks.json').read_text())
 spec['scope']='The previous 69 predicates are unchanged. New expectations inspect stored Word styles/table records, PowerPoint text/master records, and RTF controls. Remaining-gap checks are intentionally retained and reported separately. All are development checks, not blind, independent, or whole-document quality scores.'
 def add(path,desc,patterns,evidence,cohort='legacy_fixes',forbidden=()):
  spec['checks'].append({'id':f'L{len(spec["checks"])-68:02}','path':path,'area':{'legacy_fixes':'Legacy formatting and layout','legacy_structure':'Legacy table structure','legacy_remaining':'Remaining legacy gaps'}[cohort],'description':desc,'required_patterns':patterns,'forbidden_patterns':list(forbidden),'source':{'sha256':hashlib.sha256((ROOT/path).read_bytes()).hexdigest(),'evidence':evidence},'cohort':cohort})
 greek='docling-main/tests/data/doc/sources/greek_heading_styles.doc';word='docling-main/tests/data/doc/sources/legacy_sample.doc';doc='anydoc-main/tests/fixtures/doc/text.doc';ppt='docling-main/tests/data/ppt/sources/legacy_sample.ppt';rtf='anydoc-main/tests/fixtures/rtf/text.rtf';cocoa='anydoc-main/tests/fixtures/rtf/handmade-cocoa.rtf'
 add(greek,'Localized Word headings retain stored heading level',[r'(?m)^## Problem mit lokalisierten Dokumenten$',r'(?m)^## Vorgeschlagene Änderung$',r'(?m)^## Verifizierung$'],'STSH built-in heading metadata and PAPX references; the unrelated family-4 table style UPX must not invalidate all formatting.')
 add(word,'Word section heading levels survive',[r'(?m)^# Let’s swim!$',r'(?m)^## Let’s eat$'],'Paragraph style references and inherited heading roles in WordDocument/1Table.')
 add(word,'Word caption emphasis survives',[r'\*Figure 1: This is a cute duckling\*'],'Caption style plus character properties.')
 add(word,'Word quotation keeps paragraph role and emphasis',[r'(?m)^> \*I like to eat leaves\*$'],'Quote paragraph style and italic character formatting.')
 add(word,'Word table header and rich cell emphasis survive',[r'\|  \| Food \| Calories per portion \|',r'\*\*Leaves\*\*',r'\*\*Berries\*\*',r'\*\*Grain\*\*'],'Row-ending PAPX has header and TDefTable boundaries; cell paragraphs contain bold runs.')
 add(doc,'Character-style override clears inherited bold',[r'\*\*Style-bold paragraph with a\*\* NotBold-styled span \*\*inside\.\*\*'],'CHPX sprmCIstd selects NotBold character style; surrounding paragraph is bold.')
 add(doc,'Word row/cell merge semantics survive',[r'<td colspan="2">Wide head</td><td>End</td>',r'<td rowspan="2">Tall</td><td>B2</td><td>C2</td>',r'<tr><td>B3</td><td>C3</td></tr>'],'TDefTable row geometry and TC80 horizontal/vertical merge flags; explicit structure check, expanded GFM may retain alignment without spans.','legacy_structure')
 add('anydoc-main/tests/fixtures/ppt/handmade-multimaster.ppt','Each slide uses its referenced master',[r'(?m)^- \*\*Alpha master body text\*\*$',r'(?m)^\*Beta master body text\*$'],'Two live master IDs define different body defaults; first bold/bulleted, second italic/unbulleted.')
 add('anydoc-main/tests/fixtures/ppt/pres.ppt','PPT nested bullet levels survive',[r'(?m)^- Top level point$',r'(?m)^  - Nested detail$',r'(?m)^- Second point with emphasis$'],'TextPFRun indent levels and bullet flags, with the active master defaults.')
 add(ppt,'PPT local character emphasis survives',[r'\*\*Class1\*\*',r'\*\*Class2\*\*'],'StyleTextPropAtom character-run bold bits for the table labels; table geometry remains unresolved.')
 add(cocoa,'Escaped LF and CRLF preserve separate RTF paragraphs',[r'(?s)\*\*Release notes\*\*\n\nTextEdit ends',r'backslash before the line break\.\n\nA second paragraph',r'CRLF pair\.\n\n'],'Backslash followed by physical LF/CRLF acts as a paragraph terminator; unescaped physical newlines are insignificant.')
 add(rtf,'RTF label-local pard does not replace heading role',[r'(?m)^#\s+(?:\*\*)?Fixture Document(?:\*\*)?$',r'(?m)^##\s+(?:\*\*)?Lists(?:\*\*)?$',r'(?m)^##\s+(?:\*\*)?Table(?:\*\*)?$'],'Outer s1/s2 and outlinelevel controls precede listtext groups containing local pard/plain/tab controls.')
 add(word,'Word automatic numbered list structure',[r'(?m)^1\. Wear sunglasses$',r'(?m)^2\. Don’t forget to drink water$',r'(?m)^3\. Use sun cream$'],'List override and level records specify numbered list; not fixed in this iteration.','legacy_remaining')
 add(rtf,'RTF auxiliary note bodies survive',[r'Footnote after an astral character\.',r'Endnote body text\.'],'RTF footnote/endnote destinations contain these bodies; not fixed in this iteration.','legacy_remaining')
 add(cocoa,'RTF bullet labels become structural list items',[r'(?m)^- First item$',r'(?m)^- Second item$'],'listtable/listoverridetable and ls/ilvl specify bullets; body labels retained but structural lists not yet reconstructed.','legacy_remaining')
 add(ppt,'Legacy PPT merged table remains a table',[r'(?:<table>|\|[^\n]*Class1[^\n]*\|)'],'PowerPoint OfficeArt shapes encode the table; current reader and AnyDoc flatten these cells into text.','legacy_remaining')
 (OUT/'quality-checks.json').write_text(json.dumps(spec,indent=2,ensure_ascii=False)+'\n');print(len(spec['checks']))
if __name__=='__main__':main()
