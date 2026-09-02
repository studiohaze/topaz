#!/usr/bin/env python3
# B3 — full justification penalty parity (Latin + CJK), Topaz ≡ fixed-point Rust.
import subprocess, sys, os
HERE = os.path.dirname(os.path.abspath(__file__))
TOPAZ_REPO = "/root/topaz-compiler"
REF = os.path.join(HERE, "just_fp")
KERNEL = os.path.join(HERE, "just.tpz")
def advc(c):
    if c == " ": return 4000
    cp = ord(c[0]) if c else 0
    if (0xAC00 <= cp <= 0xD7A3) or (0x4E00 <= cp <= 0x9FFF): return 10000
    if c in ".,;:!?\"'()[]{}。、，．「」『』《》〈〉": return 3000
    return 5000
def line(s): return [(advc(c), c) for c in s]

CASES = []  # (name, policy=(target,minGap,ratioM,maxAdj,maxGap,maxGapRatioR,mode,isLast), clusters)
def add(n,p,cl): CASES.append((n,p,cl))
# ── Latin (mode 0) ──
W = "the cat sat"
add("lat_within", (60000,1,500,50000,8000,0,0,0), line(W))
add("lat_over",   (80000,1,500,50000,8000,0,0,0), line(W))
add("lat_exceeds",(120000,1,500,50000,8000,0,0,0), line(W))
add("lat_short",  (200000,1,500,50000,8000,0,0,0), line(W))
add("lat_last",   (80000,1,500,50000,8000,0,0,1), line(W))
add("lat_trail",  (80000,1,500,50000,8000,0,0,0), line("the cat sat  "))
# ── CJK (mode 1) ── Korean syllables = 10000 mpt; maxGapRatioR = 0.3*1e8 = 30000000
RR = 30000000
add("cjk_within",     (54000,1,500,100000,8000,RR,1,0), line("가나다라마"))   # adj 4000, gaps 4 -> small
add("cjk_ratio_over", (70000,1,500,100000,8000,RR,1,0), line("가나다라마"))   # adj 20000 -> ratio cap
add("cjk_width_over", (98000,1,500,100000,4000,RR,1,0), line("가나다라마"))   # maxGap 4000 -> width cap
add("cjk_exceeds",    (200000,1,300,100000,8000,RR,1,0), line("가나다라마가나다라마"))  # adj>maxAdj
add("cjk_punct_gap",  (62000,1,500,100000,8000,RR,1,0), line("가나。다라"))   # 。punct breaks gap candidacy
add("cjk_kr_wordspace",(60000,1,500,100000,8000,RR,1,0), line("가 나 다"))    # korean-word-space gaps + dedup
add("cjk_few_gaps",   (70000,5,500,100000,8000,RR,1,0), line("가나다라마"))   # gaps 4 < minGap 5 -> CAP
add("cjk_short",      (300000,1,500,100000,8000,RR,1,0), line("가나다라마"))  # short line -> CAP
add("cjk_last",       (70000,1,500,100000,8000,RR,1,1), line("가나다라마"))   # last -> 0
add("cjk_mixed_punct",(64000,1,500,100000,8000,RR,1,0), line("가나다,라마"))  # ascii comma punct
add("cjk_cjkpunct",   (64000,1,500,100000,8000,RR,1,0), line("가나다、라마"))  # ideographic comma
add("cjk_long",       (150000,1,400,100000,8000,RR,1,0), line("문장입니다정말좋은날씨네요오늘"))
add("cjk_ws_mixed",   (88000,1,400,100000,8000,RR,1,0), line("가 나다 라마 바사"))
add("cjk_d7a3_edge",  (54000,1,500,100000,8000,RR,1,0), line("힣힣힣힣힣"))  # U+D7A3 in range
add("cjk_d7a4_out",   (54000,1,500,100000,8000,RR,1,0), line("힤힤힤힤힤"))  # U+D7A4 NOT cjk-just -> gaps 0 -> CAP
# ── R2 antithesis additions ──
CASES.append(("cjk_ratio_sentinel",(15958,1,500,120000,2000,12659149,1,0),[(7500,"가"),(7501,"나")]))  # float-vs-fixed re-bless sentinel
CASES.append(("cjk_rep_adv_le1",(54000,1,500,100000,8000,RR,1,0),[(1,"가"),(10000,"나"),(10000,"다")]))  # adv<=1 -> rep None -> CAP
CASES.append(("cjk_rep_adv_2",  (54000,1,500,100000,8000,RR,1,0),[(2,"가"),(10000,"나"),(10000,"다")]))  # adv 2 -> ok
add("cjk_nbsp_internal",(60000,1,500,100000,8000,RR,1,0), line("가 나 다"))   # NBSP korean-word-space gaps
add("cjk_mingap0_nocjk",(70000,0,500,100000,8000,RR,1,0), line("abc"))               # gap=0,minGap=0 -> guard CAP (no crash)
add("lat_mingap0_nogap",(70000,0,500,100000,8000,0,0,0), line("word"))               # latin no spaces, minGap0 -> guard CAP
add("cjk_both_overage", (110000,1,300,120000,1000,5000000,1,0), line("가나다라마"))    # won>gap AND ror>tol
add("cjk_maxadj_exact", (50000+100000,1,200,100000,8000,RR,1,0), line("가나다라마"))   # adjustment == maxAdj (not > )
add("cjk_maxadj_plus1", (50000+100001,1,200,100000,8000,RR,1,0), line("가나다라마"))   # adjustment == maxAdj+1 -> cap
# Latin won boundary (won==gap no cap / won==gap+1 cap): original 53000, gaps 2, maxGap 8000 -> won = adj-16000
add("lat_won_eq",  (53000+16000+2,1,500,90000,8000,0,0,0), line("the cat sat"))  # won==2==gap -> no cap -> 0
add("lat_won_gt",  (53000+16000+3,1,500,90000,8000,0,0,0), line("the cat sat"))  # won==3>gap 2 -> cap

def run(args, inp, cwd=None): return subprocess.run(args, input=inp, capture_output=True, text=True, cwd=cwd)
def main():
    if run(["rustc","-O",os.path.join(HERE,"just_fp.rs"),"-o",REF],"").returncode: print("REF BUILD FAIL"); sys.exit(1)
    if run(["cargo","build","-q","-p","topaz_cli"],"",cwd=TOPAZ_REPO).returncode: print("TOPAZ BUILD FAIL"); sys.exit(1)
    npass=nfail=0; fails=[]
    for name,p,cl in CASES:
        inp = " ".join(str(x) for x in p) + "\n" + "\n".join(f"{a}\t{t}" for a,t in cl)
        ref = run([REF],inp).stdout.strip()
        tzr = run(["cargo","run","-q","-p","topaz_cli","--","run",KERNEL],inp,cwd=TOPAZ_REPO)
        tz = ("TOPAZ_ERR: "+tzr.stderr.strip()[:200]) if tzr.returncode else tzr.stdout.strip()
        if ref==tz: npass+=1
        else: nfail+=1; fails.append((name,ref,tz))
    print(f"=== B3 JUSTIFICATION PARITY (Latin+CJK): {npass}/{npass+nfail} identical ===")
    for n,r,t in fails: print(f"[FAIL] {n}  ref={r} topaz={t}")
    print("RESULT:", "ALL IDENTICAL ✓" if nfail==0 else f"{nfail} MISMATCH ✗")
    sys.exit(0 if nfail==0 else 1)
if __name__=="__main__": main()
