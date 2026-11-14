#!/bin/zsh
cd "$(dirname "$0")"
for m in "$@"; do
  .venv/bin/python -I evalq.py $m > ../data/res_$m.json 2> ../data/err_$m.txt
  echo "$m exit $?"
done
