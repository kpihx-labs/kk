#!/usr/bin/env -S uv run --script
"""Copy public upstream notices at crate VCS revisions; preserve exact bytes."""
import base64
import json
import subprocess
from pathlib import PurePosixPath
from urllib.parse import urlparse

from release import ROOT, digest, licenses


def api(endpoint):
    return json.loads(subprocess.check_output(["gh", "api", endpoint], text=True))


def main():
    destination = ROOT / "third_party"
    destination.mkdir(exist_ok=True)
    path = destination / "notices.json"
    manifest = json.loads(path.read_text()) if path.exists() else {"format": 1, "packages": {}, "sources": {}}
    cache = {}
    for package in licenses()[0]:
        if package["notices"]:
            continue
        name = package["name"] + "@" + package["version"]
        if name in manifest["packages"]:
            continue
        revision = package["revision"]
        url = urlparse(package["repository"] or "")
        components = url.path.strip("/").split("/")
        if url.hostname != "github.com" or len(components) < 2:
            raise ValueError("Review upstream notice source: " + name)
        repository = "/".join(components[:2]).removesuffix(".git")
        if revision is None:
            if package["license"] != "CC0-1.0":
                raise ValueError("Missing immutable upstream revision: " + name)
            repository = "spdx/license-list-data"
            revision = api("repos/" + repository + "/commits/main")["sha"]
            result = api("repos/" + repository + "/contents/text/CC0-1.0.txt?ref=" + revision)
            records = [("CC0-1.0.txt", result["sha"], base64.b64decode(result["content"]))]
        else:
            key = (repository, revision, package["path_in_vcs"])
            if key not in cache:
                tree = api("repos/" + repository + "/git/trees/" + revision)["tree"]
                trees = [("", tree)]
                relative = PurePosixPath(package["path_in_vcs"])
                if relative.is_absolute() or ".." in relative.parts or len(relative.parts) > 10:
                    raise ValueError("Unsafe crate workspace path")
                prefix = ""
                for part in relative.parts:
                    child = next((entry for entry in tree if entry["path"] == part and entry["type"] == "tree"), None)
                    if child is None:
                        raise ValueError("Crate path does not match its pinned repository: " + name)
                    prefix += part + "/"
                    tree = api("repos/" + repository + "/git/trees/" + child["sha"])["tree"]
                    trees.append((prefix, tree))
                entries = []
                for prefix, tree in trees:
                    for entry in tree:
                        wanted = entry["path"].upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))
                        if entry["type"] == "tree" and (wanted or entry["path"] == ".reuse"):
                            nested = api("repos/" + repository + "/git/trees/" + entry["sha"] + "?recursive=1")["tree"]
                            if len(nested) > 128:
                                raise ValueError("Oversized upstream notice tree")
                            entries.extend({**child, "path":prefix + entry["path"] + "/" + child["path"]} for child in nested if child["type"] == "blob")
                        elif wanted and entry["type"] == "blob":
                            entries.append({**entry, "path":prefix + entry["path"]})
                records = []
                for entry in entries:
                    name_path = PurePosixPath(entry["path"])
                    if name_path.is_absolute() or ".." in name_path.parts or entry.get("mode") == "120000" or entry.get("size", 0) > 1024 * 1024:
                        raise ValueError("Unsafe upstream notice entry")
                    blob = api("repos/" + repository + "/git/blobs/" + entry["sha"])
                    records.append((entry["path"], entry["sha"], base64.b64decode(blob["content"])))
                cache[key] = records
            records = cache[key]
        if not records and "Apache-2.0" in package["license"].replace("/", " OR ").split(" OR "):
            readme = api("repos/" + repository + "/contents/README.md?ref=" + revision)
            text = base64.b64decode(readme["content"])
            if b"Apache License" not in text and b"Apache-2.0" not in text:
                raise ValueError("Upstream README does not contain the declared alternative grant: " + name)
            records = [("README.md", readme["sha"], text)]
            if b"AUTHORS" in text:
                authors = api("repos/" + repository + "/contents/AUTHORS?ref=" + revision)
                records.append(("AUTHORS", authors["sha"], base64.b64decode(authors["content"])))
            standard_repo = "spdx/license-list-data"
            standard_revision = api("repos/" + standard_repo + "/commits/main")["sha"]
            standard = api("repos/" + standard_repo + "/contents/text/Apache-2.0.txt?ref=" + standard_revision)
            standard_data = base64.b64decode(standard["content"])
            relative = f"licenses/spdx-license-list-data-{standard_revision[:12]}/Apache-2.0.txt"
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if not target.exists():
                with target.open("xb") as output:
                    output.write(standard_data)
            elif target.is_symlink() or digest(target.read_bytes()) != digest(standard_data):
                raise ValueError("Previously collected standard license changed")
            manifest["sources"][relative] = {"repository":standard_repo,"revision":standard_revision,"blob":standard["sha"],"sha256":digest(standard_data)}
            manifest.setdefault("selections", {})[name] = {"license":"Apache-2.0","standard_text":relative,"basis":"exact upstream README alternative grant"}
        if not records:
            raise ValueError("No upstream license at pinned revision: " + name)
        included = []
        for filename, blob, data in records:
            relative = f"licenses/{repository.replace('/', '-')}-{revision[:12]}/{filename}"
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                if target.is_symlink() or digest(target.read_bytes()) != digest(data):
                    raise ValueError("Previously collected notice differs: " + relative)
            else:
                with target.open("xb") as output:
                    output.write(data)
            manifest["sources"][relative] = {"repository": repository, "revision": revision, "blob": blob, "sha256": digest(data)}
            included.append(relative)
        manifest["packages"][name] = included
        if name in manifest.get("selections", {}):
            manifest["packages"][name].append(manifest["selections"][name]["standard_text"])
        # Keep completed provenance if a later upstream lookup is unavailable.
        path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        print("Collected pinned public notices: " + name, flush=True)


if __name__ == "__main__":
    main()
