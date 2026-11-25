import json, os, sys, time, sqlite3, unicodedata, resource
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
M = ROOT + "/models/"

QWEN_Q = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
E5I_Q = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery: "

SPECS = {
    "potion-multilingual-128M": dict(kind="m2v", path="minishlab__potion-multilingual-128M"),
    "static-mrl-multilingual": dict(kind="st", path="sentence-transformers__static-similarity-mrl-multilingual-v1"),
    "e5-small": dict(kind="st", path="intfloat__multilingual-e5-small", q="query: ", d="passage: "),
    "e5-base": dict(kind="st", path="intfloat__multilingual-e5-base", q="query: ", d="passage: "),
    "e5-large-instruct": dict(kind="st", path="intfloat__multilingual-e5-large-instruct", q=E5I_Q, d=""),
    "granite-107m": dict(kind="st", path="ibm-granite__granite-embedding-107m-multilingual"),
    "granite-278m": dict(kind="st", path="ibm-granite__granite-embedding-278m-multilingual"),
    "gte-multilingual-base": dict(kind="st", path="Alibaba-NLP__gte-multilingual-base", trust=True),
    "arctic-m-v2": dict(kind="st", path="Snowflake__snowflake-arctic-embed-m-v2.0", q="query: ", d="", trust=True, cfg={"use_memory_efficient_attention": False, "unpad_inputs": False}),
    "qwen3-0.6b": dict(kind="st", path="Qwen__Qwen3-Embedding-0.6B", q=QWEN_Q, d=""),
    "bge-m3": dict(kind="st", path="BAAI__bge-m3"),
    "granite-97m-r2": dict(kind="st", path="ibm-granite__granite-embedding-97m-multilingual-r2"),
    "f2llm-v2-80m": dict(kind="st", path="codefuse-ai__F2LLM-v2-80M", q="Instruct: Given a question, retrieve passages that can help answer the question.\nQuery: ", d=""),
    "f2llm-v2-160m": dict(kind="st", path="codefuse-ai__F2LLM-v2-160M", q="Instruct: Given a question, retrieve passages that can help answer the question.\nQuery: ", d=""),
    "f2llm-v2-330m": dict(kind="st", path="codefuse-ai__F2LLM-v2-330M", q="Instruct: Given a question, retrieve passages that can help answer the question.\nQuery: ", d=""),
    "harrier-270m": dict(kind="st", path="microsoft__harrier-oss-v1-270m", q=E5I_Q, d=""),
}


def fold(s):
    s = s.replace("đ", "d").replace("Đ", "D")
    return s


def fts_rank(docs, query):
    db = sqlite3.connect(":memory:")
    db.execute("CREATE VIRTUAL TABLE t USING fts5(id UNINDEXED, body, tokenize='unicode61 remove_diacritics 2')")
    for k, v in docs.items():
        db.execute("INSERT INTO t VALUES (?, ?)", (k, fold(v)))
    words = [w for w in "".join(c if c.isalnum() else " " for c in fold(query)).split() if w]
    if not words:
        return [], []
    q_and = " AND ".join('"%s"*' % w for w in words)
    q_or = " OR ".join('"%s"*' % w for w in words)
    a = [r[0] for r in db.execute("SELECT id FROM t WHERE t MATCH ? ORDER BY bm25(t)", (q_and,))]
    o = [r[0] for r in db.execute("SELECT id FROM t WHERE t MATCH ? ORDER BY bm25(t)", (q_or,))]
    return a, o


def rrf(lists, k=60):
    sc = {}
    for lst in lists:
        for r, d in enumerate(lst):
            sc[d] = sc.get(d, 0) + 1.0 / (k + r + 1)
    return [d for d, _ in sorted(sc.items(), key=lambda x: -x[1])]


def metrics(rankings, queries):
    out = {}
    for (q, gold, lang), rk in zip(queries, rankings):
        g = set(gold)
        r1 = 1.0 if rk[:1] and rk[0] in g else 0.0
        r5 = 1.0 if any(d in g for d in rk[:5]) else 0.0
        mrr = 0.0
        for i, d in enumerate(rk[:10]):
            if d in g:
                mrr = 1.0 / (i + 1)
                break
        for key in (lang, "all"):
            out.setdefault(key, []).append((r1, r5, mrr))
    return {k: tuple(round(float(np.mean([x[i] for x in v])), 3) for i in range(3)) + (len(v),) for k, v in out.items()}


def load(name):
    sp = SPECS[name]
    t = time.time()
    if sp["kind"] == "m2v":
        from model2vec import StaticModel
        m = StaticModel.from_pretrained(M + sp["path"])
        enc = lambda xs: np.asarray(m.encode(xs))
    else:
        from sentence_transformers import SentenceTransformer
        m = SentenceTransformer(M + sp["path"], device="cpu", trust_remote_code=sp.get("trust", False), config_kwargs=sp.get("cfg"))
        enc = lambda xs: m.encode(xs, normalize_embeddings=True, batch_size=16)
    return sp, enc, time.time() - t


def run(name, sets):
    sp, enc, lt = load(name)
    res = {"model": name, "load_s": round(lt, 2)}
    for sname, data in sets.items():
        ids = list(data["docs"].keys())
        texts = [sp.get("d", "") + data["docs"][i] for i in ids]
        t = time.time()
        D = enc(texts)
        dt = time.time() - t
        D = D / np.linalg.norm(D, axis=1, keepdims=True)
        qs = data["queries"]
        enc([sp.get("q", "") + "warm up"])
        t = time.time()
        Q = np.vstack([enc([sp.get("q", "") + q[0]]) for q in qs])
        qt = (time.time() - t) / len(qs)
        Q = Q / np.linalg.norm(Q, axis=1, keepdims=True)
        S = Q @ D.T
        dense = [[ids[j] for j in np.argsort(-S[i])] for i in range(len(qs))]
        hyb, hybo = [], []
        for i, q in enumerate(qs):
            a, o = fts_rank(data["docs"], q[0])
            hyb.append(rrf([a, dense[i][:50]])); hybo.append(rrf([o, dense[i][:50]]))
        res[sname] = {"dense": metrics(dense, qs), "hybrid": metrics(hyb, qs), "hybrid_or": metrics(hybo, qs), "doc_ms_each": round(1000 * dt / len(ids), 2), "query_ms": round(1000 * qt, 2)}
    res["maxrss_mb"] = round(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1e6)
    return res


if __name__ == "__main__":
    sets = {s: json.load(open(f"{ROOT}/data/{s}.json")) for s in ("settings", "chat")}
    if sys.argv[1] == "fts":
        out = {"model": "fts5"}
        for sname, data in sets.items():
            ra, ro = [], []
            for q in data["queries"]:
                a, o = fts_rank(data["docs"], q[0])
                ra.append(a)
                ro.append(o)
            out[sname] = {"and": metrics(ra, data["queries"]), "or": metrics(ro, data["queries"])}
        print(json.dumps(out))
    else:
        print(json.dumps(run(sys.argv[1], sets)))
