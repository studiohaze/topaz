#!/usr/bin/env python3
# B3a — LatinWordSpace justification penalty parity (Topaz ≡ fixed-point Rust).
import subprocess, sys, os
HERE = os.path.dirname(os.path.abspath(__file__))
TOPAZ_REPO = "/root/topaz-compiler"
REF = os.path.join(HERE, "just_latin_fp")
KERNEL = os.path.join(HERE, "just_latin.tpz")
def advc(c): return 4000 if c==" " else 5000
def line(s): return [(advc(c), c) for c in s]   # one cluster per char

CASES = []  # (name, policy=(target,minGap,ratioM,maxAdj,maxGap,isLast), clusters[(adv,text)])
def add(n,p,cl): CASES.append((n,p,cl))
W = "the cat sat"  # 9 letters(45000) + 2 spaces(8000) = original 53000, gapCount 2
add("within_cap",   (60000,1,500,50000,8000,0), line(W))   # adj 7000 -> over_num -9000 -> 0
add("over_cap",     (80000,1,500,50000,8000,0), line(W))   # adj 27000 -> over_num 11000 -> CAP+pen
add("exceeds_max",  (120000,1,500,50000,8000,0), line(W))  # adj 67000 > maxAdj -> CAP+overage
add("short_line",   (200000,1,500,50000,8000,0), line(W))  # 53000/200000<0.5 -> CAP
add("few_gaps",     (80000,3,500,50000,8000,0), line(W))   # gaps 2 < 3 -> CAP
add("tiny_adj",     (53001,1,500,50000,8000,0), line(W))   # adj 1 -> 0
add("zero_adj",     (53000,1,500,50000,8000,0), line(W))   # adj 0 -> 0
add("is_last",      (80000,1,500,50000,8000,1), line(W))   # last -> 0
add("trailing_sp",  (80000,1,500,50000,8000,0), line("the cat sat  "))  # trailing spaces trimmed
add("all_spaces",   (40000,1,500,50000,8000,0), line("     "))          # eff=0 -> CAP
add("nbsp_trail",   (80000,1,500,50000,8000,0), line("the cat sat")+[(4000," ")])  # NBSP trailing trimmed
add("boundary_over",(53000+2*8000+3,1,500,50000,8000,0), line(W))  # over_num = 3 > gapCount 2 -> CAP+pen
add("boundary_eq",  (53000+2*8000+2,1,500,50000,8000,0), line(W))  # over_num = 2 == gapCount 2 -> NOT > -> 0
add("many_words",   (200000,1,300,90000,8000,0), line("a b c d e f g h i j"))

def run(args, inp, cwd=None): return subprocess.run(args, input=inp, capture_output=True, text=True, cwd=cwd)
def main():
    if run(["rustc","-O",os.path.join(HERE,"just_latin_fp.rs"),"-o",REF],"").returncode: print("REF BUILD FAIL"); sys.exit(1)
    if run(["cargo","build","-q","-p","topaz_cli"],"",cwd=TOPAZ_REPO).returncode: print("TOPAZ BUILD FAIL"); sys.exit(1)
    npass=nfail=0; fails=[]
    for name,p,cl in CASES:
        inp = f"{p[0]} {p[1]} {p[2]} {p[3]} {p[4]} {p[5]}\n" + "\n".join(f"{a}\t{t}" for a,t in cl)
        ref = run([REF],inp).stdout.strip()
        tzr = run(["cargo","run","-q","-p","topaz_cli","--","run",KERNEL],inp,cwd=TOPAZ_REPO)
        tz = ("TOPAZ_ERR: "+tzr.stderr.strip()[:200]) if tzr.returncode else tzr.stdout.strip()
        if ref==tz: npass+=1
        else: nfail+=1; fails.append((name,ref,tz))
    print(f"=== B3a LATIN JUSTIFICATION PARITY: {npass}/{npass+nfail} identical ===")
    for n,r,t in fails: print(f"[FAIL] {n}  ref={r} topaz={t}")
    print("RESULT:", "ALL IDENTICAL ✓" if nfail==0 else f"{nfail} MISMATCH ✗")
    sys.exit(0 if nfail==0 else 1)
if __name__=="__main__": main()
