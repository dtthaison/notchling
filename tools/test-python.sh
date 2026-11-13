#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
status=0
for suite in guard agents release protocol; do
    echo "== tools/$suite"
    PYTHONPATH="tools/guard" python3 -m unittest discover -s "tools/$suite/tests" -t "tools/$suite" || status=1
done
exit $status
