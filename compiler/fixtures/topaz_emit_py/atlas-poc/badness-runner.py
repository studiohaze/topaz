#!/usr/bin/env python3
# B1 — differential parity for the fixed-point badness (Topaz ≡ fixed-point Rust reference).
import subprocess, sys, os, itertools
HERE = os.path.dirname(os.path.abspath(__file__))
TOPAZ_REPO = "/root/topaz-compiler"
REF = os.path.join(HERE, "badness_fp")
KERNEL = os.path.join(HERE, "badness.tpz")
KINDS = ["latin_space","korean_cluster","latin_hyphen","hard_fallback","closing_punctuation","forced_end"]

def widths(t):
    return sorted(set([
        0, t*40//100, t*52//100 - 1, t*52//100, (t*52+99)//100, t*80//100,
        t, t+1, t+2, t*150//100, t*5,
    ]))

def cases():
    for t in [120_000, 500_000, 23_000_000_000]:
        for w in widths(t):
            for sp in [0,1,5,20]:
                for kind in KINDS:
                    for is_last in [0,1]:
                        for c in [0,1,2,3]:
                            yield f"{w}|{t}|{c}|{sp}|{kind}|{is_last}"

def build():
    r = subprocess.run(["rustc","-O",os.path.join(HERE,"badness_fp.rs"),"-o",REF], capture_output=True, text=True)
    if r.returncode: print("REF BUILD FAIL\n", r.stderr); sys.exit(1)
    r = subprocess.run(["cargo","build","-q","-p","topaz_cli"], cwd=TOPAZ_REPO, capture_output=True, text=True)
    if r.returncode: print("TOPAZ BUILD FAIL\n", r.stderr); sys.exit(1)

def main():
    build()
    corpus = list(cases())
    inp = "\n".join(corpus)
    ref = subprocess.run([REF], input=inp, capture_output=True, text=True).stdout.strip("\n").split("\n")
    tz_run = subprocess.run(["cargo","run","-q","-p","topaz_cli","--","run",KERNEL], cwd=TOPAZ_REPO,
                            input=inp, capture_output=True, text=True)
    if tz_run.returncode != 0:
        print("TOPAZ RUN ERR:\n", tz_run.stderr[:2000]); sys.exit(1)
    tz = tz_run.stdout.strip("\n").split("\n")
    n = len(corpus); npass = nfail = 0; fails = []
    if len(ref) != n or len(tz) != n:
        print(f"LENGTH MISMATCH: corpus={n} ref={len(ref)} topaz={len(tz)}");
    for i in range(n):
        rv = ref[i] if i < len(ref) else "<none>"
        tv = tz[i] if i < len(tz) else "<none>"
        if rv == tv: npass += 1
        else:
            nfail += 1
            if len(fails) < 25: fails.append((corpus[i], rv, tv))
    print(f"=== B1 BADNESS PARITY: {npass}/{n} identical ===")
    for c, rv, tv in fails:
        print(f"[FAIL] {c}  ref={rv} topaz={tv}")
    print("RESULT:", "ALL IDENTICAL ✓" if nfail == 0 else f"{nfail} MISMATCH ✗")
    sys.exit(0 if nfail == 0 else 1)

if __name__ == "__main__":
    main()
