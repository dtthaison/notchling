import sys
from huggingface_hub import HfApi
api=HfApi()
for mid in sys.argv[1:]:
    try:
        info=api.model_info(mid, files_metadata=True)
        lic=[t for t in info.tags if t.startswith('license:')]
        files=[(s.rfilename,round(s.size/1e6)) for s in info.siblings if s.rfilename.endswith(('.safetensors','.bin','.onnx')) and s.size and s.size>1e6]
        print(mid, info.sha[:10], lic, 'gated=',info.gated, info.last_modified.date() if info.last_modified else '', files[:10])
    except Exception as e:
        print(mid,'ERR',type(e).__name__, str(e)[:100])
