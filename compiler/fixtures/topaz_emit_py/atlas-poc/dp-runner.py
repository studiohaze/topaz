#!/usr/bin/env python3
# B2 — break-plan parity for the fixed-point DP (Topaz ≡ fixed-point Rust reference).
import subprocess, sys, os
HERE = os.path.dirname(os.path.abspath(__file__))
# atlas-poc/../../.. resolves to the compiler workspace root.
TOPAZ_REPO = os.environ.get("TOPAZ_REPO", os.path.abspath(os.path.join(HERE, "../../..")))
REF = os.path.join(HERE, "dp_fp")
KERNEL = os.path.join(HERE, "dp.tpz")
EXPECTED = {}

def adv(c):
    if c == " ": return 4000
    if c in "-‐‑–": return 3000
    cp = ord(c[0]) if c else 0
    if (0xAC00 <= cp <= 0xD7AF) or (0x4E00 <= cp <= 0x9FFF) or (0x3400 <= cp <= 0x4DBF): return 10000
    if c in ")]}」』》〉,.;:!?、。([{「『《〈，；：": return 3000
    return 5000

def chars(s):  # one cluster per character
    return list(s)

# (name, target_mpt, clusters)
CASES = []
def add(name, target, clusters): CASES.append((name, target, clusters))
def expect(name, output): EXPECTED[name] = output

for t in [25000, 30000, 50000, 12000]:
    add(f"kr_short_{t}", t, chars("안녕하세요세계평화"))
for t in [75000, 100000, 41000]:
    add(f"kr_long_{t}", t, chars("가"*60))
add("kr_spaces", 40000, chars("한국어 조판 입니다 정말"))
add("kr_spaces2", 28000, chars("한국어 조판 입니다 정말 좋은 날씨"))
for t in [25000, 18000, 40000]:
    add(f"latin_{t}", t, chars("the quick brown fox jumps"))
add("hyphen_tight", 20000, chars("well-known-thing"))
add("hyphen_wide", 60000, chars("well-known-thing"))
add("mixed", 35000, chars("한국어 and 영어 mixed 텍스트"))
add("punct_close", 30000, chars("가나다라.마바사아.자차"))
add("punct_paren", 30000, chars("가나(다라)마바사아자차"))
add("numsep", 30000, chars("값은 3.14 또는 1,000 입니다"))
add("nbsp", 30000, ["가","나"," ","다","라"," ","마","바","사","아","자","차","카","타"])
add("single", 50000, chars("가"))
add("two", 50000, chars("가나"))
add("fits_one_line", 500000, chars("짧은 문장"))
add("homogeneous_tie", 73000, chars("가"*40))   # near-tie homogeneous CJK widths
add("homogeneous_tie2", 77000, chars("문"*40))
add("strict_first_wins_tie", 20000, chars("가"*5))  # equal-cost predecessor tie at end index 3
expect("strict_first_wins_tie", "0 1 50535000000\n1 3 50010000000\n3 5 10000000\nTOTAL 100555000000")
add("very_long", 80000, chars("가"*200))         # exercises the 80-predecessor window + many lines
add("very_long_latin", 60000, chars("word "*60))
add("overfull_single", 8000, chars("가나다라마바사"))  # each line one wide cluster, target < cluster pair
add("cjk_punct", 40000, chars("문장입니다。다음문장、그리고"))
add("seam_big_target", 23000000000, chars("가나다라마바사아자차"))
# ── R2 antithesis additions ──
add("rebless_sentinel_7cjk", 5329, chars("가가가가가가가"))  # documented fixed-vs-float divergence (homogeneous CJK tie)
add("target_plus1_accept", 9999, chars("가가"))   # 2-unit width 20000? no: 가=10000 -> width 20000; use small advances below
add("window_90", 50000, chars("가"*90))           # 80-predecessor-window clamp
add("window_130", 73000, chars("문"*130))
add("seam_max_ok", 23000000000, chars("가나다"))
add("seam_over", 23000000001, chars("가나다"))     # both OUT_OF_BOUNDS
add("zero_target", 0, chars("가나"))               # both OUT_OF_BOUNDS
add("neg_target", -5, chars("가나"))               # both OUT_OF_BOUNDS

def run(binary_args, inp, cwd=None):
    r = subprocess.run(binary_args, input=inp, capture_output=True, text=True, cwd=cwd)
    return r

def main():
    if subprocess.run(["rustc","-O",os.path.join(HERE,"dp_fp.rs"),"-o",REF], capture_output=True, text=True).returncode:
        print("REF BUILD FAIL"); sys.exit(1)
    if subprocess.run(["cargo","build","-q","-p","topaz_cli"], cwd=TOPAZ_REPO, capture_output=True, text=True).returncode:
        print("TOPAZ BUILD FAIL"); sys.exit(1)
    npass = nfail = 0; fails = []
    for name, target, clusters in CASES:
        inp = f"{target}\n" + "\n".join(f"{adv(c)}\t{c}" for c in clusters)
        ref = run([REF], inp).stdout.strip("\n")
        tzr = run(["cargo","run","-q","-p","topaz_cli","--","run",KERNEL], inp, cwd=TOPAZ_REPO)
        if tzr.returncode != 0:
            nfail += 1; fails.append((name, "<ref>", "TOPAZ_ERR: "+tzr.stderr.strip()[:300])); continue
        tz = tzr.stdout.strip("\n")
        expected = EXPECTED.get(name)
        if expected is not None and ref != expected:
            nfail += 1
            if len(fails) < 12: fails.append((name, expected, "REF_UNPINNED: "+ref))
        elif expected is not None and tz != expected:
            nfail += 1
            if len(fails) < 12: fails.append((name, expected, "TOPAZ_UNPINNED: "+tz))
        elif ref == tz: npass += 1
        else:
            nfail += 1
            if len(fails) < 12: fails.append((name, ref, tz))
    print(f"=== B2 BREAK-PLAN PARITY: {npass}/{npass+nfail} paragraphs identical ===")
    for name, ref, tz in fails:
        print(f"\n[FAIL] {name}\n  ref:\n    {ref.replace(chr(10),chr(10)+'    ')}\n  topaz:\n    {tz.replace(chr(10),chr(10)+'    ')}")
    print("\nRESULT:", "ALL IDENTICAL ✓" if nfail == 0 else f"{nfail} MISMATCH ✗")
    sys.exit(0 if nfail == 0 else 1)

if __name__ == "__main__":
    main()
