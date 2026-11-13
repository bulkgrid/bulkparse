#!/usr/bin/env python3
"""Freeze source-grounded development expectations; never used by the parser."""
import hashlib,json,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'reports/multiformat-tables-layout-2026-11-12'
def digest(b):return hashlib.sha256(b).hexdigest()
def main():
 spec=json.loads((ROOT/'reports/multiformat-legacy-quality-2026-11-12/quality-checks.json').read_text())
 spec['scope']='The previous 54 selected checks are unchanged. Added table-content/layout and explicit-structure checks are development expectations derived from source records, not an independent holdout or whole-document score. Structure checks separately require retained nested-table/merge semantics; expanded flat Markdown can preserve content while failing those checks.'
 odt='docling-main/tests/data/odf/sources/text_document_03.odt';odp='docling-main/tests/data/odf/sources/odf_presentation_01.odp';ppt='docling-main/tests/data/pptx/sources/powerpoint_sample.pptx';pic='docling-main/tests/data/pptx/sources/powerpoint_with_image.pptx';rtf='anydoc-main/tests/fixtures/rtf/handmade-merge.rtf';xlsb='anydoc-main/tests/fixtures/xlsb/handmade-sheet.xlsb'
 def add(path,desc,patterns,parts=(),struct=False,evidence=''):
  source={'sha256':digest((ROOT/path).read_bytes()),'parts':list(parts),'evidence':evidence}
  if parts:
   with zipfile.ZipFile(ROOT/path) as z:source['part_sha256']={p:digest(z.read(p)) for p in parts}
  spec['checks'].append({'id':f'T{len(spec["checks"])-53:02}','path':path,'area':'Explicit table structure' if struct else 'Table content and layout','description':desc,'required_patterns':patterns,'forbidden_patterns':[],'source':source,'cohort':'table_structure' if struct else 'table_content_layout'})
 add(odt,'Both rich-cell lists retain all items',[r'A First',r'A Second',r'A Third',r'B (?:\*\*|<strong>)First',r'B (?:\*|<em>)Second',r'B Third'],['content.xml'],evidence='First table, second row: two text:list children and text:span emphasis.')
 add(odt,'Numbered cell list retains all three entries',[r'Number one',r'Number two',r'Number three'],['content.xml'],evidence='First table, third row, first cell: ordered text:list.')
 add(odt,'Inline rich-cell emphasis survives', [r'This is simple text with (?:\*\*|<strong>)bold(?:\*\*|</strong>)',r'This is simple text with[^\n]*(?:~~|<del>)strikethrough(?:~~|</del>)',r'This is simple text with[^\n]*(?:\*|<em>)italic(?:\*|</em>)'],['content.xml'])
 add(odt,'Cell line breaks survive',[r'First Paragraph(?:\s*<br\s*/?>)+\s*Second Paragraph'],['content.xml'],evidence='Two explicit text:line-break nodes between these phrases; this content check requires a retained break, not exact blank-line count.')
 add(odt,'Nested table contents survive',[r'Cell 1',r'Cell 2',r'Cell 3',r'Rich cell'],['content.xml'],evidence='Second outer table contains two nested tables.')
 add(odt,'List-only cells in a two-column table survive',[rf'R{r}C{c} item {i}' for r in (1,2) for c in (1,2) for i in (1,2)],['content.xml'])
 add(odt,'Mixed list and regular-text cells both survive',[r'List item 1',r'List item 2',r'Regular text in second cell'],['content.xml'])
 add(odp,'List-backed slide table values survive',[r'Class1',r'Class2',r'A merged with B',r'R1',r'R4',r'True',r'False'],['content.xml'])
 add(odp,'Slide table precedes its lower footnote',[r'(?s)Test Table Slide.*Class1.*With footnote'],['content.xml'],evidence='Title y=1.79cm, table y=5.405cm, footnote y=15.094cm.')
 add(ppt,'Slide table precedes its lower footnote',[r'(?s)Test Table Slide.*Class1.*With footnote'],['ppt/slides/slide1.xml'],evidence='Own transform y values: title644526, table1945640, footnote5433848 EMU.')
 add(pic,'Caption precedes the lower linked image description',[r'(?s)Image linked to file.*A cartoon duck holding a paper'],['ppt/slides/slide1.xml'],evidence='Caption top4503683, linked picture top5407188 EMU; XML creation order is reversed.')
 add(rtf,'Orphan continuation text is not discarded',[r'stray continuation',r'Wide B2'],evidence='The fourth row has clvmrg without a preceding matching restart.')
 add(odt,'Nested tables remain tables inside their outer cells',[r'(?s)<td>\s*<table>.*Cell 1.*</table>\s*</td>',r'(?s)<td>\s*<p>Rich cell.*<table>.*Cell 2.*</table>\s*</td>'],['content.xml'],True)
 add(rtf,'Unequal boundaries preserve horizontal and vertical spans',[r'<td rowspan="2">A1</td><td colspan="2">B1</td>',r'<td colspan="3">Wide B2</td>',r'<td colspan="2">A3 wide</td><td colspan="2">B3 wide</td>'],struct=True,evidence='Union of trleft/cellx edges:0,3000,4500,6000,9000. A1 clvmgf/clvmrg chain spans two rows.')
 add(ppt,'Declared table headers and vertical merges survive',[r'<th colspan="3">Class1</th>',r'<td rowspan="2">R3</td>',r'<td rowspan="3">R4</td>'],['ppt/slides/slide1.xml'],True,evidence='tblPr firstRow=1, gridSpan=3, rowSpan=2/3.')
 (OUT/'quality-checks.json').write_text(json.dumps(spec,indent=2,ensure_ascii=False)+'\n')
 print(len(spec['checks']))
if __name__=='__main__':main()
