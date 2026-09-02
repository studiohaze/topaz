#!/usr/bin/env python3
"""Generate render-ko.tpz from render.tpz: rename every USER identifier to Hangul (NFC) and
translate comments to Korean, keeping stdlib methods/builtins/keywords/types. The two renderers
are byte-identical in output (a topaz_cli parity test enforces it). Run from the repo root:
    python3 examples/living-docs/gen-render-ko.py
"""
import re, pathlib
root = pathlib.Path(__file__).resolve().parents[2]
src = (root / "examples/living-docs/render.tpz").read_text()

# protect string literals (incl. http://) then line comments, so renames touch CODE only.
stash = []
def keep(m): stash.append(m.group(0)); return "\x00%d\x00" % (len(stash)-1)
src = re.sub(r'"(?:[^"\\]|\\.)*"', keep, src)
src = re.sub(r'//[^\n]*', keep, src)

FN = {"chars":"글자화","escapeChar":"글자정리","escapeAll":"전체정리","findClose":"닫기위치",
 "hasColonBeforeSlash":"슬래시앞콜론","safeUrl":"안전한주소","linkUrlEnd":"링크닫기",
 "escapeAttr":"속성정리","inline":"인라인변환","dropCr":"캐리지제거","splitLines":"줄로분리",
 "isDigit":"숫자인지","olMarker":"순서표식","eqLine":"줄이같음","trimWs":"양끝공백",
 "hasPipe":"파이프있음","splitCells":"칸으로분리","isSeparatorCell":"구분칸인지",
 "isSeparatorRow":"구분행인지","rowHtml":"행html로","isBlockStart":"블록시작인지",
 "mdToHtml":"마크다운변환","listIndent":"들여쓰기","listKind":"목록종류",
 "listTextStart":"목록글시작","renderList":"목록렌더"}
LOC = {"pc":"접두글자","delim":"구분자","closeBr":"대괄호닫기","closeParen":"소괄호닫기",
 "altCs":"대체글자","urlCs":"주소글자","textCs":"텍스트글자","dashes":"대시수","blockN":"블록번호",
 "isTopaz":"토파즈인지","isEnd":"끝인지","lines":"줄들","html":"결과","code":"코드","first":"처음",
 "cells":"칸들","cur":"현재","raw":"원시","depth":"깊이","close":"닫기","url":"주소","tag":"태그",
 "cell":"칸","from":"시작","out":"출력","cs":"글자들","dl":"구분길이","ok":"일치","line":"줄",
 "result":"결과값","child":"자식","rest":"나머지","kind":"종류","indent":"들여깊이","start":"시작점",
 "next":"다음","src":"소스","ind":"들여수","ts":"글시작",
 "i":"색인","j":"제이","k":"케이","c":"글자","s":"문자열","p":"접두","u":"정리주소","t":"비교글자",
 "a":"앞","b":"뒤","n":"엔"}
src = re.sub(r'(?<!\.)\bstartsWith\b', '로시작함', src)
for key in sorted({**FN, **LOC}, key=len, reverse=True):
    src = re.sub(r'\b'+re.escape(key)+r'\b', {**FN, **LOC}[key], src)

# restore comments+strings, fix the one interpolated identifier, then translate comments.
src = re.sub(r'\x00(\d+)\x00', lambda m: stash[int(m.group(1))], src)
src = src.replace("{blockN}", "{블록번호}")
CMT = {
"// Markdown → HTML renderer (A1), written in Topaz.":"// 마크다운 → HTML 렌더러 — 전부 토파즈로 작성 (방송대 출품 한글 식별자판).",
"// Pure: the same input yields the same HTML on the interpreter and the native/wasm build.":"// 순수 함수: 같은 입력 → 인터프리터와 네이티브/wasm 빌드에서 동일한 HTML.",
"// Strings are inspected via `.scalars()`; the array stdlib (slice/join) keeps it clean.":"// 문자열은 `.scalars()`로 글자 배열로 보고, 배열 stdlib(slice/join)로 깔끔하게.",
"// --- §safe links: a DEFAULT-DENY URL allowlist + attribute escaping --------------":"// --- §안전한 링크: 기본-거부 URL 허용목록 + 속성 이스케이프 ---",
"  // browsers strip leading whitespace/controls from a URL, so check the TRIMMED form.":"  // 브라우저는 URL 앞 공백/제어문자를 제거하므로 다듬은 형태로 검사한다.",
"  // (C3a string stdlib: `trimStart` + `startsWith` — no hand-rolled char scan.)":"  // (C3a 문자열 stdlib: `trimStart` + `로시작함` — 수작업 글자 스캔 없이.)",
"  // protocol-relative (`//host`, `/\\host`, `\\host`) is external navigation — reject (the":"  // 프로토콜-상대(`//host`, `/\\host`, `\\host`)는 외부 이동이라 거부 (아래",
"  // `/` branch below is for SAME-ORIGIN root-relative only).":"  // `/` 분기는 동일-출처 루트-상대 전용).",
"  // a relative path with no scheme (no ':' before the first '/') is safe; any other":"  // 스킴 없는 상대 경로(첫 '/' 앞 ':' 없음)는 안전; 그 외 스킴",
"  // scheme (javascript:/data:/vbscript:/…) is rejected and the link renders inert.":"  // (javascript:/data:/vbscript:/…)은 거부되어 링크가 무력화된다.",
"// the `)` that closes a link's `(url)`, BALANCING inner parens so a URL like":"// 링크 `(url)`을 닫는 `)` — 안쪽 괄호를 균형 맞춰 `foo(1)`/`alert(1)` 같은",
"// `foo(1)` or `alert(1)` is captured whole (not cut at the first `)`).":"// 주소를 통째로 잡는다 (첫 `)`에서 잘리지 않게).",
"// --- §inline: code / bold / italic / links, over a scalar slice ------------------":"// --- §인라인: 코드/굵게/기울임/링크 — 글자 슬라이스 위에서 ---",
"      // backslash escape: the next char is literal (HTML-escaped, no markdown meaning)":"      // 백슬래시 이스케이프: 다음 글자는 리터럴 (HTML 이스케이프, 마크다운 의미 없음)",
"      // image ![alt](url) — the SAME default-deny URL safety as links":"      // 이미지 ![대체텍스트](주소) — 링크와 동일한 기본-거부 URL 안전성",
"// drop every CR so CRLF (and stray CR) input matches cleanly (`---\\r\\n` → <hr>)":"// 모든 CR 제거 — CRLF(및 떠도는 CR) 입력도 깨끗이 매칭 (`---\\r\\n` → <hr>)",
"  // C3a `str.split` on the newline, then drop CR per line (split keeps the `\\r`":"  // C3a `str.split`로 줄바꿈 분리 후 줄마다 CR 제거 (split은 CRLF의 `\\r`을",
"  // from a CRLF, unlike the old hand-rolled scanner).":"  // 남기므로 — 옛 수작업 스캐너와 달리).",
"// an ordered-list marker `N. ` → the char length of the marker, or 0":"// 순서 목록 마커 `N. ` → 마커의 글자 길이, 없으면 0",
"// --- §nested lists (A4): strict — spaces only, 2 per level; ul/ol; no loose lists ----":"// --- §중첩 목록 (A4): 엄격 — 공백만, 레벨당 2칸; ul/ol; loose 목록 없음 ---",
"// render a list and its nested children: items at exactly `indent` spaces of the same kind;":"// 목록과 그 중첩 자식을 렌더: 정확히 `들여깊이`칸·같은 종류의 항목;",
"// a deeper item (indent + 2) nests inside the previous <li>. Returns the html + the next line.":"// 더 깊은 항목(들여깊이+2)은 이전 <li> 안에 중첩. html과 다음 줄을 반환.",
"// --- §blocks: headings / lists / blockquote / fenced code / rule / paragraph -----":"// --- §블록: 제목/목록/인용/펜스코드/구분선/문단 ---",
"// split a table row on '|', trim cells, drop the empty cells the OUTER pipes create":"// 표 행을 '|'로 나누고 칸을 다듬고 바깥 파이프가 만든 빈 칸은 버린다",
"      // an escaped char (esp. GFM `\\|`) is kept WHOLE in the cell, not a split":"      // 이스케이프된 글자(특히 GFM `\\|`)는 칸에 통째로 — 분리점이 아님;",
"      // point; `inline` resolves the backslash escape when it renders the cell.":"      // `인라인변환`이 칸을 렌더할 때 백슬래시 이스케이프를 처리한다.",
"// a separator CELL: optional leading `:`, one+ `-`, optional trailing `:` — and nothing":"// 구분 칸: 앞 `:` 선택, `-` 한 개 이상, 뒤 `:` 선택 — 그 외엔 아무것도",
"// else (so `:--`, `--:`, `:-:`, `---` pass; `:--:--`, `:`, `a-` do not).":"// 없음 (`:--`,`--:`,`:-:`,`---`는 통과; `:--:--`,`:`,`a-`는 불가).",
"// a separator row: has a pipe and every cell is a valid separator cell.":"// 구분 행: 파이프가 있고 모든 칸이 유효한 구분 칸.",
"// does this line begin a (non-table) block? a table body stops at any such line":"// 이 줄이 (표가 아닌) 블록의 시작인가? 표 본문은 그런 줄에서 멈춘다",
"// (GFM: a table ends at a blank line or the start of another block structure).":"// (GFM: 표는 빈 줄이나 다른 블록 구조의 시작에서 끝난다).",
"      // a ```topaz fence becomes an ordinal COMPUTE placeholder; the JS shell extracts":"      // ```topaz 펜스는 순번 계산 플레이스홀더가 된다; JS 셸이 블록 소스를",
"      // the block sources (in document order) and injects each executed result. Any other":"      // (문서 순서대로) 추출해 실행 결과를 주입한다. 그 외 펜스는",
"      // fence stays a static, HTML-escaped code block.":"      // 정적인 HTML-이스케이프 코드 블록으로 남는다.",
"      // a GFM table — checked AFTER headings/lists/quotes/hr so a `#`/`-`/`>` line is never":"      // GFM 표 — 제목/목록/인용/hr 다음에 검사하므로 `#`/`-`/`>` 줄이 헤더로",
"      // stolen as a header; the delimiter row must have the SAME cell count as the header.":"      // 도둑맞지 않는다; 구분 행은 헤더와 칸 수가 같아야 한다.",
"// §22 entry: render the host's text payload (the editor textarea / piped stdin).":"// §22 진입점: 호스트의 텍스트 페이로드(에디터 textarea / 파이프 stdin)를 렌더.",
}
for a, bb in CMT.items(): src = src.replace(a, bb)
(root / "examples/living-docs/render-ko.tpz").write_text(src)
print("render-ko.tpz regenerated:", len(src.splitlines()), "lines")
