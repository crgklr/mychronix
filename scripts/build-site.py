#!/usr/bin/env python3
"""Builds Mychronix's website from packaging/site/index.html into a folder.

    scripts/build-site.py <out folder> --version 0.1.10 --date "October 18, 2026"
        --repo crgklr/mychronix --url https://crgklr.github.io/mychronix --fpr <key>

Fills in the template's @VERSION@, @DATE@, @REPO@, @URL@ and @FPR@, makes the
roadmap from the README's "Still to do" list (so it's kept in one place), and
copies the logo, icon and screenshots beside it. scripts/publish-repos.sh runs
it; run it by hand to preview the site.
"""

import argparse
import html
import re
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def inline(text: str, repo: str) -> str:
    """The README's inline Markdown (code, bold, links) as HTML."""
    out, last = [], 0
    for m in re.finditer(r"`([^`]+)`|\*\*([^*]+)\*\*|\[([^\]]+)\]\(([^)]+)\)", text):
        out.append(html.escape(text[last:m.start()]))
        code, bold, label, href = m.groups()
        if code:
            out.append(f"<code>{html.escape(code)}</code>")
        elif bold:
            out.append(f"<b>{html.escape(bold)}</b>")
        else:
            if href == "#share-your-sessions":
                href = "#share"
            elif href.startswith("#"):
                href = f"https://github.com/{repo}{href}"
            elif not re.match(r"[a-z]+:", href):
                href = f"https://github.com/{repo}/blob/main/{href}"
            out.append(f'<a href="{html.escape(href)}">{html.escape(label)}</a>')
        last = m.end()
    out.append(html.escape(text[last:]))
    return "".join(out)


def roadmap(readme: str, repo: str) -> str:
    """The unchecked items of the README's "Still to do", as list items."""
    section = re.search(r"^## Still to do\n(.*?)(?=^## )", readme, re.S | re.M)
    if not section:
        raise SystemExit("the README has no \"Still to do\" section")
    items: list[str] = []
    open_item = False  # whether indented lines continue the last open item
    for line in section.group(1).splitlines():
        if m := re.match(r"- \[ \] (.*)", line):
            items.append(m.group(1).strip())
            open_item = True
        elif line.startswith("  ") and open_item:
            items[-1] += " " + line.strip()
        else:
            open_item = False
    if not items:
        raise SystemExit("the README's \"Still to do\" has no open items")
    return "\n".join(f"        <li>{inline(i, repo)}</li>" for i in items)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("out", type=Path)
    for name in ("version", "date", "repo", "url", "fpr"):
        p.add_argument(f"--{name}", required=True)
    a = p.parse_args()

    page = (ROOT / "packaging/site/index.html").read_text()
    page = page.replace("@ROADMAP@", roadmap((ROOT / "README.md").read_text(), a.repo))
    for key in ("version", "date", "repo", "url", "fpr"):
        page = page.replace(f"@{key.upper()}@", html.escape(getattr(a, key)))
    if left := sorted(set(re.findall(r"@[A-Z]+@", page))):
        raise SystemExit(f"not filled in: {', '.join(left)}")

    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "index.html").write_text(page)
    assets = ROOT / "crates/mychronix/assets"
    shutil.copy(assets / "mychronix.svg", a.out)
    shutil.copy(assets / "mychronix-icon.svg", a.out)
    shutil.copytree(ROOT / "docs/screenshots", a.out / "screenshots", dirs_exist_ok=True)


if __name__ == "__main__":
    main()
