#!/usr/bin/env python3
"""Regenerate bundled notices after changing Cargo.lock (maintainers only)."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import urlopen
import hashlib
import json
import os
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
CARGO = os.environ.get("CARGO_BIN", str(Path.home() / ".cargo/bin/cargo"))


def download(url):
    cache = ROOT / "target/license-cache" / (hashlib.sha256(url.encode()).hexdigest() + ".json")
    if cache.exists():
        return json.loads(cache.read_text())
    try:
        with urlopen(url, timeout=30) as response:
            result = response.read().decode("utf-8")
    except HTTPError as error:
        if error.code == 404:
            result = None
        else:
            raise
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(json.dumps(result))
    return result


def upstream(pkg, directory):
    vcs = json.loads((directory / ".cargo_vcs_info.json").read_text())
    sha = vcs["git"]["sha1"]
    repo = pkg["repository"].removesuffix(".git")
    parts = repo.split("/")
    owner, name = parts[3:5]
    if parts[2] == "github.com":
        base = f"https://raw.githubusercontent.com/{owner}/{name}/{sha}"
    elif parts[2] == "codeberg.org":
        base = f"https://codeberg.org/{owner}/{name}/raw/commit/{sha}"
    else:
        raise RuntimeError(f"Review license source for {pkg['name']}")
    return base


def main():
    packages = {}
    for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
        metadata = json.loads(subprocess.check_output(
            [CARGO, "metadata", "--locked", "--format-version", "1", "--filter-platform", target],
            cwd=ROOT,
        ))
        packages.update({p["id"]: p for p in metadata["packages"] if p["source"]})

    records, missing = {}, {}
    for identifier, pkg in packages.items():
        directory = Path(pkg["manifest_path"]).parent
        texts = []
        for path in sorted(directory.rglob("*")):
            if not path.is_file() or path.stat().st_size > 1_000_000:
                continue
            if not re.search(r"(^|[-_])(licen[cs]e|copying|copyright|notice|unlicense)([-_.]|$)", path.name, re.I):
                continue
            if path.suffix in (".rs", ".py", ".toml", ".json"):
                continue
            texts.append((path.relative_to(directory).as_posix(), path.read_text()))
        if texts:
            records[identifier] = texts
        else:
            missing[identifier] = upstream(pkg, directory)

    names = ("LICENSE", "LICENSE.txt", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "LICENSE-MIT.txt", "LICENSE-APACHE.txt", "LICENSES/MIT.txt", "LICENSES/Apache-2.0.txt")
    urls = sorted({f"{base}/{name}" for base in missing.values() for name in names})
    with ThreadPoolExecutor(max_workers=8) as pool:
        fetched = dict(zip(urls, pool.map(download, urls)))
    for identifier, base in missing.items():
        records[identifier] = [(name, fetched[f"{base}/{name}"]) for name in names if fetched[f"{base}/{name}"]]
        pkg = packages[identifier]
        if not records[identifier] and pkg["name"] == "dispatch" and pkg["version"] == "0.2.0" and pkg["license"] == "MIT":
            # This published crate/commit declares MIT but ships no license file.
            # Preserve its attribution and include the unmodified SPDX template.
            standard = download("https://raw.githubusercontent.com/spdx/license-list-data/main/text/MIT.txt")
            records[identifier] = [("SPDX MIT template (no license file in published crate)",
                                    "Published author: Steven Sheldon\n" + standard)]
        if not records[identifier]:
            raise RuntimeError(f"Missing license text: {pkg['name']} {pkg['version']}")

    digest = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    output = ["AI Monitor — third-party notices", f"Cargo.lock SHA-256: {digest}",
              "Inventory of resolved macOS dependencies, including build-time components.",
              "License texts are reproduced from published crates or their recorded upstream commits."]
    texts = {}

    def reference(content):
        normalized = "\n".join(line.rstrip() for line in content.splitlines()).strip()
        if normalized not in texts:
            texts[normalized] = f"L{len(texts) + 1:03}"
        return texts[normalized]

    for identifier, pkg in sorted(packages.items(), key=lambda item: (item[1]["name"], item[1]["version"])):
        output.extend(["\n" + "=" * 72, f"{pkg['name']} {pkg['version']}",
                       f"SPDX: {pkg.get('license') or 'See license text'}", f"Source: {pkg.get('repository') or pkg['source']}",
                       "Authors (published crate metadata): " + (", ".join(pkg.get("authors", [])) or "See source/license text")])
        for name, content in records[identifier]:
            output.append(f"License text [{reference(content)}]: {name}")
    output.extend(["\n" + "=" * 72, "Rust standard library — MIT OR Apache-2.0", "Source: https://github.com/rust-lang/rust"])
    for name in ("LICENSE-MIT", "LICENSE-APACHE"):
        # Standard license texts, not any local toolchain paths or configuration.
        output.append(f"License text [{reference(download(f'https://raw.githubusercontent.com/rust-lang/rust/1.99.1/{name}'))}]: {name}")
    for content, label in texts.items():
        output.extend(["\n" + "=" * 72, f"[{label}]", content])
    (ROOT / "packaging/THIRD_PARTY_NOTICES.txt").write_text("\n".join(output).rstrip() + "\n")
    print(f"Generated license notices for {len(packages)} dependency packages.")


if __name__ == "__main__":
    main()
