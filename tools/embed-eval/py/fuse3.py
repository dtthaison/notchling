import json, os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from evalq import load, fts_rank, rrf, metrics, ROOT

sets = {s: json.load(open(f"{ROOT}/data/{s}.json")) for s in ("settings", "chat")}
names = sys.argv[1:]
encs = {n: load(n)[:2] for n in names + ["static-mrl-multilingual"]}


def dense(name, data):
    sp, enc = encs[name]
    ids = list(data["docs"].keys())
    D = enc([sp.get("d", "") + data["docs"][i] for i in ids])
    D = D / np.linalg.norm(D, axis=1, keepdims=True)
    Q = np.vstack([enc([sp.get("q", "") + q[0]]) for q in data["queries"]])
    Q = Q / np.linalg.norm(Q, axis=1, keepdims=True)
    S = Q @ D.T
    return [[ids[j] for j in np.argsort(-S[i])][:50] for i in range(len(data["queries"]))]


out = {}
for sname, data in sets.items():
    st = dense("static-mrl-multilingual", data)
    for n in names:
        tr = dense(n, data)
        two, three, three_w = [], [], []
        for i, q in enumerate(data["queries"]):
            a, o = fts_rank(data["docs"], q[0])
            two.append(rrf([a, tr[i]]))
            three.append(rrf([a, tr[i], st[i]]))
        out[f"{sname} {n} fts+dense"] = metrics(two, data["queries"])
        out[f"{sname} {n} fts+dense+static"] = metrics(three, data["queries"])
        out[f"{sname} {n} dense+static"] = metrics([rrf([tr[i], st[i]]) for i in range(len(tr))], data["queries"])
for k, v in out.items():
    print(k, {l: v[l][:2] for l in ("all", "en", "vi", "vi-nodiac", "mixed")})
