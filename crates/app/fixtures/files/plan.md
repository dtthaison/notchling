# Colab plan

The film's plan, as the session wrote it.

## Steps

1. Cut the first reel from `film/plan/colab/plan.md`.
2. Send the **rough cut** to the editor, then wait for notes.
3. Export with `ffmpeg -i in.mov out.mp4`.

> Keep every take; delete nothing until the edit is locked.

| Reel | Length | State |
| --- | --- | --- |
| One | 1:20 | Cut |
| Two | 2:05 | Notes |

```sh
ffmpeg -i in.mov -vf scale=1280:-2 out.mp4
```

See [the notes](notes.md) and [the site](https://example.com).
