#!/usr/bin/env python3
import subprocess, sys, os
HERE = os.path.dirname(os.path.abspath(__file__))
TOPAZ_REPO = "/root/topaz-compiler"
ORACLE = os.path.join(HERE, "oracle")
KERNEL = os.path.join(HERE, "kernel.tpz")
NBSP, NNBSP = " ", " "
HY2010, HY2011, HY2013 = "‐", "‑", "–"

# Each case = a list of cluster strings (real Unicode).
CASES = {
  "korean_pure":        ["안","녕","하","세","요"],
  "korean_space":       ["안","녕"," ","세","계"],
  "latin_space":        ["t","h","e"," ","c","a","t"],
  "ascii_hyphen":       ["a","-","b"],
  "uni_hyphen_2010":    ["a",HY2010,"b"],
  "uni_hyphen_2013":    ["x",HY2013,"y"],
  "nbsp_nobreak":       ["a",NBSP,"b"],
  "nnbsp_nobreak":      ["a",NNBSP,"b"],
  "closing_paren_kr":   ["가",")","나"],
  "closing_paren_lat":  ["a",")","b"],
  "closing_cjk_bracket":["가","」","나"],
  "opening_before_kr":  ["(","가","나"],
  "opening_at_end":     ["가","("],
  "numsep_dot":         ["3",".","1","4"],
  "numsep_comma":       ["1",",","0","0","0"],
  "numsep_colon":       ["1","2",":","3","0"],
  "nonnum_dot":         ["a",".","b"],
  "cjk_punct_ideo":     ["가","。","나"],
  "cjk_punct_comma":    ["A","、","B"],
  "kr_then_forbidden":  ["가","나",")"],
  "double_space":       ["a"," "," ","b"],
  "multi_latin_cluster":["ab","가"],
  "multi_kr_cluster":   ["가나","다"],
  "single_cluster":     ["가"],
  "hardfallback_sym":   ["@","#","가"],
  "kr_space_kr":        ["가"," ","나"],
  "emoji_nonbmp":       ["\U0001F600","가"],
  "closing_then_space": ["가",")"," ","나"],
  "sentence_mixed":     ["한","국","어"," ","조","판","입","니","다","."],
  "colon_no_digits":    ["가",":","나"],
  "comma_after_digit_only": ["3",",","가"],   # next not digit -> not numsep -> closing_punct
  # ── boundary code points: validate the hex->decimal range conversions ──
  "jamo_first":         ["ᄀ","가"],   # U+1100 first Hangul Jamo (range low)
  "jamo_below":         ["ჿ","가"],   # U+10FF just below 0x1100 -> NOT korean
  "hangul_last":        ["힯","가"],   # U+D7AF last of 0xAC00..0xD7AF
  "hangul_above":       ["ힰ","가"],   # U+D7B0 just above -> NOT korean
  "cjk_last":           ["鿿","가"],   # U+9FFF last CJK Unified
  "cjk_above":          ["ꀀ","가"],   # U+A000 just above 0x9FFF -> NOT korean
  "compat_jamo_start":  ["㄰","가"],   # U+3130 start of 0x3130..0x318F
  "uni_hyphen_2011":    ["a","‑","b"],# non-breaking hyphen
  "fullwidth_comma":    ["A","，","B"],# U+FF0C in korean set but NOT line_start -> korean_cluster
  "fullwidth_semicolon":["A","；","B"],# U+FF1B korean set, not line_start
  "fullwidth_colon":    ["A","：","B"],# U+FF1A korean set, not line_start
  "kr_paren_cluster":   ["가(","나"],       # multi-scalar: not all-korean, not all-line-start-forbidden
  "two_char_forbidden": ["))","가"],        # multi-scalar all-line-start-forbidden -> closing_punct
}
# ── R2 antithesis additions: every range edge, each punctuation, numeric negatives, ws/empty edges ──
for _cp in [0x10FF,0x1100,0x11FF,0x1200, 0x312F,0x3130,0x318F,0x3190, 0x33FF,0x3400,
            0x4DBF,0x4DC0,0x4DFF,0x4E00,0x9FFF,0xA000, 0xF8FF,0xF900,0xFAFF,0xFB00,
            0xAC00,0xD7AF,0xD7B0]:
    CASES[f"cp_{_cp:04X}"] = [chr(_cp), "가"]
for _ch in [")","]","}",",",".",";",":","!","?","、","。","」","』","》","〉"]:
    CASES[f"close_{ord(_ch):04X}"] = ["가", _ch, "나"]
for _ch in ["，","；","："]:
    CASES[f"fw_{ord(_ch):04X}"] = ["A", _ch, "B"]
CASES["numneg_multidigit_prev"] = ["12",",","3"]
CASES["numneg_arabic_prev"]     = ["٣",",","3"]
CASES["numneg_double_dot"]      = ["3","..","4"]
CASES["numneg_arabic_next"]     = ["3",".","٤"]
CASES["kr_nbsp_kr"]   = ["가",NBSP,"나"]
CASES["kr_nnbsp_kr"]  = ["가",NNBSP,"나"]
CASES["empty_single"] = [""]
CASES["leading_empty"]= ["","가"]
CASES["middle_empty"] = ["가","","나"]
CASES["trailing_empty"]= ["가",""]

def build():
  r = subprocess.run(["rustc","-O",os.path.join(HERE,"oracle.rs"),"-o",ORACLE], capture_output=True, text=True)
  if r.returncode != 0:
    print("ORACLE BUILD FAILED:\n", r.stderr); sys.exit(1)
  r = subprocess.run(["cargo","build","-q","-p","topaz_cli"], cwd=TOPAZ_REPO, capture_output=True, text=True)
  if r.returncode != 0:
    print("TOPAZ BUILD FAILED:\n", r.stderr); sys.exit(1)

def run_oracle(inp):
  return subprocess.run([ORACLE], input=inp, capture_output=True, text=True).stdout.strip("\n")

def run_topaz(inp):
  r = subprocess.run(["cargo","run","-q","-p","topaz_cli","--","run",KERNEL], cwd=TOPAZ_REPO,
                     input=inp, capture_output=True, text=True)
  if r.returncode != 0:
    return "TOPAZ_ERR: " + (r.stderr.strip() or r.stdout.strip())
  return r.stdout.strip("\n")

def main():
  build()
  npass = nfail = 0
  fails = []
  for name, clusters in CASES.items():
    inp = "\n".join(clusters)
    o = run_oracle(inp); t = run_topaz(inp)
    if o == t:
      npass += 1
    else:
      nfail += 1
      fails.append((name, clusters, o, t))
  print(f"=== DIFFERENTIAL PARITY: {npass}/{npass+nfail} cases identical ===")
  for name, clusters, o, t in fails:
    print(f"\n[FAIL] {name}  clusters={clusters!r}")
    print(f"  oracle:\n    " + o.replace("\n","\n    "))
    print(f"  topaz :\n    " + t.replace("\n","\n    "))
  print("\nRESULT:", "ALL IDENTICAL ✓" if nfail == 0 else f"{nfail} MISMATCH(es) ✗")
  sys.exit(0 if nfail == 0 else 1)

if __name__ == "__main__":
  main()
