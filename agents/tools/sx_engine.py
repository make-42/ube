#!/usr/bin/env python3
"""Print what you need to port one SearXNG engine to ube, without importing SearXNG.

    git clone --depth 1 https://github.com/searxng/searxng.git /tmp/searxng
    python3 agents/tools/sx_engine.py /tmp/searxng bing_videos
    python3 agents/tools/sx_engine.py /tmp/searxng brave --funcs request,response,extract_json_data

Shows: module docstring (first line), module-level constants, the source of
request()/response()/init()/fetch_vqd()-style functions, and every
settings.yml instance that uses the module (needs `pip install pyyaml`).
Static analysis only (ast): nothing from SearXNG is executed.
"""
import argparse, ast, os, sys

CONSTS = {
    "about", "categories", "paging", "max_page", "page_size", "results_per_page", "safesearch",
    "time_range_support", "language_support", "base_url", "search_url", "search_api", "api_url",
    "search_type", "time_range_map", "time_range_dict", "safesearch_map", "safe_search_map",
    "cookies", "headers", "method", "enable_http3",
}
DEFAULT_FUNCS = {"request", "response", "init", "fetch_traits"}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("searxng_dir")
    ap.add_argument("module", help="engine module name, e.g. bing_videos (file searx/engines/<module>.py)")
    ap.add_argument("--funcs", default="", help="extra function names to print, comma separated")
    ap.add_argument("--all-funcs", action="store_true", help="print every top-level function")
    a = ap.parse_args()

    path = os.path.join(a.searxng_dir, "searx", "engines", a.module + ".py")
    if not os.path.exists(path):
        sys.exit(f"no such engine module: {path}")
    src = open(path, encoding="utf-8").read()
    tree = ast.parse(src)
    want = DEFAULT_FUNCS | {f for f in a.funcs.split(",") if f}

    print(f"######## {a.module}  ({src.count(chr(10))} lines)  {path}")
    doc = ast.get_docstring(tree)
    if doc:
        print("DOC:", doc.strip().splitlines()[0][:200])
    imports = sorted({n.module for n in tree.body if isinstance(n, ast.ImportFrom) and n.module and "searx.engines" in n.module})
    if imports:
        print("SHARES CODE WITH:", ", ".join(imports), " <- read those too")

    print("\n--- module constants")
    for n in tree.body:
        if isinstance(n, ast.Assign) and len(n.targets) == 1 and isinstance(n.targets[0], ast.Name) and n.targets[0].id in CONSTS:
            print(" ", ast.get_source_segment(src, n).replace("\n", " ")[:300])

    print("\n--- functions")
    for n in tree.body:
        if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and (a.all_funcs or n.name in want):
            print(f"\n# def {n.name}  (line {n.lineno})")
            print(ast.get_source_segment(src, n))

    print("\n--- settings.yml instances using this module")
    try:
        import yaml
        st = yaml.safe_load(open(os.path.join(a.searxng_dir, "searx", "settings.yml"), encoding="utf-8"))
        found = [e for e in st["engines"] if (e.get("engine") or e.get("name")) == a.module]
        for e in found:
            flags = [k for k in ("disabled", "inactive") if e.get(k)]
            rest = {k: v for k, v in e.items() if k not in ("name", "engine", "disabled", "inactive")}
            print(f"  - {e['name']}  {'[' + ','.join(flags) + ']' if flags else '[enabled by default]'}  {rest}")
        if not found:
            print("  (none: the module is not configured in settings.yml)")
    except ImportError:
        print("  (install pyyaml to list instances)")


if __name__ == "__main__":
    main()
