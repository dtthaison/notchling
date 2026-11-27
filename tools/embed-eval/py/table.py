import json, glob, os
ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "data") + "/"
rows = []
for f in sorted(glob.glob(ROOT + "res_*.json")):
    try:
        r = json.load(open(f))
    except Exception:
        print("bad", f)
        continue
    rows.append(r)
langs = ["all", "en", "vi", "vi-nodiac", "mixed"]
for s in ("settings", "chat"):
    print(f"\n## {s}  (R@1 / R@5 / MRR@10)")
    print("| model | " + " | ".join(f"dense {l}" for l in langs) + " | hybrid(AND) all | hybrid(OR) all | doc ms | query ms |")
    for r in rows:
        x = r[s]
        cells = []
        for l in langs:
            v = x["dense"].get(l)
            cells.append(f"{v[0]:.2f}/{v[1]:.2f}/{v[2]:.2f}" if v else "-")
        h = x["hybrid"]["all"]; ho = x["hybrid_or"]["all"]
        print(f"| {r['model']} | " + " | ".join(cells) + f" | {h[0]:.2f}/{h[1]:.2f}/{h[2]:.2f} | {ho[0]:.2f}/{ho[1]:.2f}/{ho[2]:.2f} | {x['doc_ms_each']} | {x['query_ms']} |")
    for l in langs:
        pass
print("\nload s / maxrss MB:", [(r["model"], r["load_s"], r["maxrss_mb"]) for r in rows])
for s in ("settings", "chat"):
    print(f"\n## {s} hybrid(OR) by language R@1/R@5")
    for r in rows:
        x = r[s]["hybrid_or"]
        print(r["model"], {l: (x[l][0], x[l][1]) for l in langs if l in x})
