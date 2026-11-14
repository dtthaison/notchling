from huggingface_hub import HfApi
api = HfApi()
seen = 0
for m in api.list_models(pipeline_tag="sentence-similarity", sort="downloads", limit=400, expand=["createdAt", "downloads", "tags", "safetensors"]):
    if not m.created_at or m.created_at.year < 2025:
        continue
    tags = m.tags or []
    lic = [t for t in tags if t.startswith("license:")]
    multi = any(t in tags for t in ["multilingual", "vi"]) or "multilingual" in m.id.lower()
    params = None
    if m.safetensors and getattr(m.safetensors, "total", None):
        params = round(m.safetensors.total / 1e6)
    print(m.id, m.created_at.date(), m.downloads, lic, "multi" if multi else "", params)
    seen += 1
    if seen > 60:
        break
