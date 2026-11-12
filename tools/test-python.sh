#!/bin/sh
# Every Python suite, each with the top folder CI gives it (.github/workflows/guard.yml). The tests
# import their modules by bare name (`import preflight`), so a different `-t` fails them with
# ModuleNotFoundError though nothing is wrong. Run from the repository root.
set -eu
cd "$(dirname "$0")/.."
status=0
for suite in guard agents release protocol; do
    echo "== tools/$suite"
    PYTHONPATH="tools/guard" python3 -m unittest discover -s "tools/$suite/tests" -t "tools/$suite" || status=1
done
exit $status
