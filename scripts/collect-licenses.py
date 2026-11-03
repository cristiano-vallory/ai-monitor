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
# Fonts embedded by src/theme.rs, keyed by license file: a new *-OFL.txt
# without an entry here stops the script instead of shipping without notice.
FONTS = {
    "Inter-OFL.txt": ("Inter", "4.1", "https://github.com/rsms/inter",
                      ("Inter-Regular.ttf", "Inter-SemiBold.ttf")),
    "JetBrainsMono-OFL.txt": ("JetBrains Mono", "2.304", "https://github.com/JetBrains/JetBrainsMono",
                              ("JetBrainsMonoNL-Regular.ttf",)),
}


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


def activated(targets):
    selected = set()
    for target in targets:
        tree = subprocess.check_output(
            [CARGO, "tree", "--locked", "--target", target,
             "-e", "normal,build", "--prefix", "none", "-f", "{p}"],
            cwd=ROOT, text=True,
        )
        for line in tree.splitlines():
            match = re.match(r"^(\S+) v(\S+)", line.strip())
            if match:
                selected.add((match.group(1), match.group(2)))
    return selected


def notices(platform, targets):
    metadata = json.loads(subprocess.check_output(
        [CARGO, "metadata", "--locked", "--format-version", "1"], cwd=ROOT,
    ))
    selected = activated(targets)
    packages = {
        p["id"]: p
        for p in metadata["packages"]
        if p["source"] and (p["name"], p["version"]) in selected
    }

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
              f"Inventory of resolved {platform} dependencies, including build-time components.",
              "License texts are reproduced from published crates or their recorded upstream commits."]
    texts = {}

    def reference(content):
        if not content:
            raise RuntimeError("Empty or missing license text from upstream")
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
        std_text = (
            download(f"https://raw.githubusercontent.com/rust-lang/rust/1.99.1/{name}")
            or download(f"https://raw.githubusercontent.com/rust-lang/rust/master/{name}")
        )
        output.append(f"License text [{reference(std_text)}]: {name}")
    for content, label in texts.items():
        output.extend(["\n" + "=" * 72, f"[{label}]", content])
    output.extend(bundled_fonts())
    return output, len(packages)


def bundled_fonts():
    directory = ROOT / "assets/fonts"
    output = ["\n" + "=" * 72, "Bundled fonts",
              "Embedded in the application binary from assets/fonts."]
    for path in sorted(directory.glob("*-OFL.txt")):
        name, version, source, files = FONTS[path.name]
        for file in files:
            if not (directory / file).is_file():
                raise RuntimeError(f"Missing bundled font: {file}")
        text = "\n".join(line.rstrip() for line in path.read_text().splitlines()).strip()
        output.extend(["\n" + "-" * 72, f"{name} {version}", "SPDX: OFL-1.1",
                       f"Source: {source}", "Files: " + ", ".join(files), "", text])
    return output


def main():
    macos, count = notices(
        "macOS", ("aarch64-apple-darwin", "x86_64-apple-darwin")
    )
    (ROOT / "packaging/THIRD_PARTY_NOTICES.txt").write_text(
        "\n".join(macos).rstrip() + "\n"
    )
    print(f"Generated macOS license notices for {count} dependency packages.")
    linux, count = notices(
        "Linux", ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
    )
    (ROOT / "packaging/THIRD_PARTY_NOTICES-linux.txt").write_text(
        "\n".join(linux).rstrip() + "\n"
    )
    print(f"Generated Linux license notices for {count} dependency packages.")


if __name__ == "__main__":
    main()
