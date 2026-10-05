#!/usr/bin/env python3
"""J-coefficient sensitivity: re-rank (workload, policy) cells under
perturbed write/migration weights using runs/*.json (no rerun needed).

J = mean_lat + write_w * pw/accesses + migr_w * mig/accesses.
Variants: write_w x{0.25,1,4} x migr_w x{0.25,1,4} (9 combos incl. frozen).
For each workload+variant: rank policies by median J; report whether
grifin-full's rank vs best-strong-baseline is stable, and whether the
HEAD-TO-HEAD verdicts used by the gate flip under any variant.
"""
import json, sys, glob, os
from statistics import median

def load_runs(d):
    recs = []
    for f in glob.glob(os.path.join(d, "runs", "*.json")):
        r = json.load(open(f))
        recs.append(r)
    return recs

def j_of(r, ww, mw):
    n = r["events"] - r["warmup_skipped"]
    return r["mean_lat_ns"] + ww * r["physical_writes"] / n + mw * r["migrations"] / n

def main(d):
    recs = load_runs(d)
    cells = {}
    for r in recs:
        cells.setdefault((r["workload"], r["policy"]), []).append(r)
    workloads = sorted({w for w, _ in cells})
    variants = [(ww, mw) for ww in (1000.0, 4000.0, 16000.0) for mw in (50.0, 200.0, 800.0)]
    print(f"# sensitivity: {d} ({len(recs)} runs)")
    print("frozen coeffs: write_w=4000 migr_w=200\n")
    print("| workload | variant(ww,mw) | grifin-full rank | best-baseline | full_J | base_J | delta% | verdict |")
    print("|---|---|---|---|---|---|---|---|")
    flips = 0
    for w in workloads:
        base_verdict = None
        for (ww, mw) in variants:
            med = {}
            for (w2, p), rs in cells.items():
                if w2 != w:
                    continue
                med[p] = median(j_of(r, ww, mw) for r in rs)
            order = sorted(med)
            full = med.get("grifin-full", float("nan"))
            bb = min((med[b], b) for b in ("arc", "tinylfu", "lirs"))
            rank = sum(1 for p in order if med[p] < full) + 1
            dlt = (full - bb[0]) / bb[0] * 100 if bb[0] else 0.0
            verdict = "WIN" if dlt <= -15 else ("tie" if abs(dlt) < 5 else "LOSE")
            frozen = (ww, mw) == (4000.0, 200.0)
            if frozen:
                base_verdict = verdict
            flag = ""
            if not frozen and verdict != base_verdict:
                flag = " <-- FLIP vs frozen"
                flips += 1
            print(f"| {w} | ({ww:.0f},{mw:.0f}){'*' if frozen else ''} | {rank}/{len(order)} | {bb[1]} | {full:.0f} | {bb[0]:.0f} | {dlt:+.1}% | {verdict} |{flag}")
    print(f"\nflips vs frozen verdict: {flips}")
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "results/v1-final"))
