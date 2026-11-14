import os, sys, time
from concurrent.futures import ThreadPoolExecutor
os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
from huggingface_hub import snapshot_download

ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "models")
MODELS = sys.argv[1:]
ALLOW = ["*.json", "*.txt", "*.model", "*.py", "model.safetensors", "*/model.safetensors", "sentencepiece*", "tokenizer*", "1_Pooling/*", "2_Dense/*", "3_Dense/*", "0_StaticEmbedding/*", "README.md"]

def get(mid):
    t = time.time()
    allow = list(ALLOW)
    if mid == "BAAI/bge-m3":
        allow.append("pytorch_model.bin")
    p = snapshot_download(mid, local_dir=os.path.join(ROOT, mid.replace("/", "__")), allow_patterns=allow, ignore_patterns=["onnx/*", "openvino/*"])
    return f"{mid} {time.time()-t:.0f}s {p}"

with ThreadPoolExecutor(4) as ex:
    for r in ex.map(get, MODELS):
        print(r, flush=True)
