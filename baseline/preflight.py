#!/usr/bin/env python3
"""Read-only repository preflight for Cyclone Phase 0.

Records identity, manifest digests, dependency names, Cargo test targets,
and timings. Does not record file bodies, status paths, or environment
variables. Does not mutate repositories.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import resource
import subprocess
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

SCHEMA = "cyclone.baseline.repository-preflight/v0"
LOCK_SCHEMA = "cyclone.corpus.lock/v0"
PROCEDURE = [
    "git.toplevel",
    "git.head",
    "git.branch",
    "git.origin",
    "git.status_count",
    "manifest.discover",
    "deps.extract",
    "tests.discover",
]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_git(repo: Path, args: list[str], timeout: float = 60.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        check=False,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def tool_version(cmd: list[str]) -> str | None:
    try:
        completed = subprocess.run(cmd, check=False, capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = (completed.stdout or completed.stderr).strip().splitlines()
    return text[0] if text else None


def dep_table(table: object) -> tuple[list[str], list[dict[str, str]]]:
    if not isinstance(table, dict):
        return [], []
    names: list[str] = []
    paths: list[dict[str, str]] = []
    for name, spec in table.items():
        names.append(str(name))
        if isinstance(spec, dict) and "path" in spec:
            paths.append({"name": str(name), "path": str(spec["path"])})
    names.sort()
    paths.sort(key=lambda item: item["name"])
    return names, paths


def expand_members(root: Path, members: object) -> list[str]:
    if not isinstance(members, list):
        return []
    found: list[str] = []
    for raw in members:
        pattern = str(raw)
        if any(token in pattern for token in "*?["):
            matches = sorted(path.relative_to(root).as_posix() for path in root.glob(pattern) if path.is_dir())
            found.extend(matches)
        else:
            found.append(pattern)
    return found


def inherited(value: object, workspace_package: dict[str, object], field: str) -> object:
    if isinstance(value, dict) and value.get("workspace") is True:
        return workspace_package.get(field)
    return value


def crate_record(
    root: Path, rel_manifest: str, workspace_package: dict[str, object]
) -> tuple[dict[str, object], int]:
    path = root / rel_manifest
    raw = path.read_bytes()
    data = tomllib.loads(raw.decode("utf-8"))
    package = data.get("package") if isinstance(data.get("package"), dict) else {}
    workspace = data.get("workspace") if isinstance(data.get("workspace"), dict) else {}
    normal, normal_paths = dep_table(data.get("dependencies"))
    dev, dev_paths = dep_table(data.get("dev-dependencies"))
    build, build_paths = dep_table(data.get("build-dependencies"))
    workspace_deps, workspace_paths = dep_table(workspace.get("dependencies"))
    lib = data.get("lib") if isinstance(data.get("lib"), dict) else {}
    has_package = bool(package)
    autotests = has_package and lib.get("autotests", True) is not False
    targets: list[dict[str, object]] = []
    declared = data.get("test")
    if isinstance(declared, list):
        for entry in declared:
            if not isinstance(entry, dict):
                continue
            rel = entry.get("path")
            resolved = None
            size = None
            if isinstance(rel, str):
                candidate = (path.parent / rel).resolve()
                resolved = str(candidate)
                if candidate.is_file():
                    size = candidate.stat().st_size
            targets.append(
                {
                    "name": entry.get("name"),
                    "path": rel,
                    "resolved": resolved,
                    "bytes": size,
                    "harness": entry.get("harness", True),
                }
            )
    discovered: list[dict[str, object]] = []
    if autotests:
        tests_dir = path.parent / "tests"
        if tests_dir.is_dir():
            for child in sorted(tests_dir.iterdir()):
                if child.is_file() and child.suffix == ".rs":
                    discovered.append({"path": child.relative_to(root).as_posix(), "bytes": child.stat().st_size})
                elif child.is_dir() and (child / "main.rs").is_file():
                    main = child / "main.rs"
                    discovered.append({"path": main.relative_to(root).as_posix(), "bytes": main.stat().st_size})
    local_workspace_package = workspace.get("package") if isinstance(workspace.get("package"), dict) else {}
    inherited_from = {**workspace_package, **local_workspace_package}
    edition = inherited(package.get("edition"), inherited_from, "edition")
    if edition is None and isinstance(local_workspace_package.get("edition"), str):
        edition = local_workspace_package["edition"]
    version = inherited(package.get("version"), inherited_from, "version")
    record: dict[str, object] = {
        "manifest": rel_manifest,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "bytes": len(raw),
        "package": package.get("name"),
        "version": version,
        "edition": edition,
        "publish": package.get("publish"),
        "workspace_members": expand_members(root, workspace.get("members")) if rel_manifest == "Cargo.toml" else [],
        "workspace_resolver": workspace.get("resolver") if rel_manifest == "Cargo.toml" else None,
        "dependencies": {"normal": normal, "dev": dev, "build": build, "workspace": workspace_deps},
        "path_dependencies": normal_paths + dev_paths + build_paths + workspace_paths,
        "autotests": autotests,
        "declared_tests": targets,
        "discovered_tests": discovered,
    }
    return record, len(raw)


def manifests_for(root: Path) -> list[str]:
    rels = ["Cargo.toml"]
    root_manifest = root / "Cargo.toml"
    if root_manifest.is_file():
        data = tomllib.loads(root_manifest.read_text(encoding="utf-8"))
        workspace = data.get("workspace") if isinstance(data.get("workspace"), dict) else {}
        for member in expand_members(root, workspace.get("members")):
            candidate = f"{member}/Cargo.toml"
            if (root / candidate).is_file():
                rels.append(candidate)
    if (root / "package.json").is_file():
        rels.append("package.json")
    return rels


def package_json_record(root: Path) -> tuple[dict[str, object], int]:
    path = root / "package.json"
    raw = path.read_bytes()
    data = json.loads(raw.decode("utf-8"))
    scripts = data.get("scripts") if isinstance(data.get("scripts"), dict) else {}
    deps = data.get("dependencies") if isinstance(data.get("dependencies"), dict) else {}
    dev = data.get("devDependencies") if isinstance(data.get("devDependencies"), dict) else {}
    record = {
        "manifest": "package.json",
        "sha256": hashlib.sha256(raw).hexdigest(),
        "bytes": len(raw),
        "name": data.get("name") if isinstance(data.get("name"), str) else None,
        "script_names": sorted(str(key) for key in scripts),
        "dependency_names": sorted(str(key) for key in deps),
        "dev_dependency_names": sorted(str(key) for key in dev),
    }
    return record, len(raw)


def inspect_repo(root: Path) -> dict[str, object]:
    started = time.perf_counter()
    steps: dict[str, float] = {}
    before = resource.getrusage(resource.RUSAGE_SELF)
    child_before = resource.getrusage(resource.RUSAGE_CHILDREN)

    def mark(name: str, begin: float) -> None:
        steps[name] = round((time.perf_counter() - begin) * 1000, 3)

    tick = time.perf_counter()
    toplevel = run_git(root, ["rev-parse", "--show-toplevel"])
    mark("git.toplevel", tick)
    tick = time.perf_counter()
    head = run_git(root, ["rev-parse", "HEAD"])
    mark("git.head", tick)
    tick = time.perf_counter()
    branch = run_git(root, ["rev-parse", "--abbrev-ref", "HEAD"])
    mark("git.branch", tick)
    tick = time.perf_counter()
    origin = run_git(root, ["remote", "get-url", "origin"])
    mark("git.origin", tick)
    tick = time.perf_counter()
    status = run_git(root, ["status", "--porcelain=v1", "--untracked-files=no"])
    mark("git.status_count", tick)

    identity = {
        "toplevel": toplevel.stdout.strip() if toplevel.returncode == 0 else None,
        "head": head.stdout.strip() if head.returncode == 0 else None,
        "branch": branch.stdout.strip() if branch.returncode == 0 else None,
        "origin": origin.stdout.strip() if origin.returncode == 0 else None,
        "git_ok": toplevel.returncode == 0 and head.returncode == 0,
    }
    dirty = None
    if status.returncode == 0:
        dirty = sum(1 for line in status.stdout.splitlines() if line.strip())

    tick = time.perf_counter()
    rels = manifests_for(root) if (root / "Cargo.toml").is_file() or (root / "package.json").is_file() else []
    mark("manifest.discover", tick)

    crates: list[dict[str, object]] = []
    packages: list[dict[str, object]] = []
    bytes_read = 0
    workspace_package: dict[str, object] = {}
    root_manifest = root / "Cargo.toml"
    if root_manifest.is_file():
        root_data = tomllib.loads(root_manifest.read_text(encoding="utf-8"))
        workspace = root_data.get("workspace") if isinstance(root_data.get("workspace"), dict) else {}
        package_table = workspace.get("package") if isinstance(workspace.get("package"), dict) else {}
        workspace_package = package_table
    tick = time.perf_counter()
    for rel in rels:
        if rel == "package.json":
            record, size = package_json_record(root)
            packages.append(record)
            bytes_read += size
        elif (root / rel).is_file():
            record, size = crate_record(root, rel, workspace_package)
            crates.append(record)
            bytes_read += size
    mark("deps.extract", tick)

    tick = time.perf_counter()
    seen_tests: set[str] = set()
    test_bytes = 0
    test_files = 0
    for crate in crates:
        declared = crate["declared_tests"]
        discovered = crate["discovered_tests"]
        if isinstance(declared, list):
            for entry in declared:
                if not isinstance(entry, dict) or not isinstance(entry.get("bytes"), int):
                    continue
                key = str(entry.get("resolved") or entry.get("path"))
                if key in seen_tests:
                    continue
                seen_tests.add(key)
                test_bytes += int(entry["bytes"])
                test_files += 1
        if isinstance(discovered, list):
            for entry in discovered:
                if not isinstance(entry, dict) or not isinstance(entry.get("bytes"), int):
                    continue
                key = str(root / str(entry.get("path")))
                if key in seen_tests:
                    continue
                seen_tests.add(key)
                test_bytes += int(entry["bytes"])
                test_files += 1
    mark("tests.discover", tick)
    # Test file sizes are stat results. Manifest bytes are the bytes read.
    wall_ms = round((time.perf_counter() - started) * 1000, 3)
    after = resource.getrusage(resource.RUSAGE_SELF)
    child_after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu_ms = round(
        (
            (after.ru_utime - before.ru_utime)
            + (after.ru_stime - before.ru_stime)
            + (child_after.ru_utime - child_before.ru_utime)
            + (child_after.ru_stime - child_before.ru_stime)
        )
        * 1000,
        3,
    )
    return {
        "path": str(root),
        "identity": identity,
        "dirty_tracked_count": dirty,
        "crates": crates,
        "packages": packages,
        "test_files": test_files,
        "test_bytes": test_bytes,
        "manifest_bytes_read": bytes_read,
        "sample": {
            "wall_ms": wall_ms,
            "cpu_ms": cpu_ms,
            "steps_ms": steps,
            "process_max_rss_kb": after.ru_maxrss,
        },
    }


def median(values: list[float]) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return round((ordered[mid - 1] + ordered[mid]) / 2, 3)


def measure(repos: list[Path], runs: int) -> dict[str, object]:
    per_repo: dict[str, dict[str, object]] = {}
    for run in range(1, runs + 1):
        for repo in repos:
            observed = inspect_repo(repo)
            key = str(repo)
            slot = per_repo.get(key)
            if slot is None:
                slot = {
                    "path": observed["path"],
                    "identity": observed["identity"],
                    "dirty_tracked_count": observed["dirty_tracked_count"],
                    "crates": observed["crates"],
                    "packages": observed["packages"],
                    "test_files": observed["test_files"],
                    "test_bytes": observed["test_bytes"],
                    "manifest_bytes_read": observed["manifest_bytes_read"],
                    "samples": [],
                }
                per_repo[key] = slot
            sample = observed["sample"]
            assert isinstance(sample, dict)
            sample["run"] = run
            samples = slot["samples"]
            assert isinstance(samples, list)
            samples.append(sample)
    repositories = list(per_repo.values())
    walls = []
    for repo in repositories:
        samples = repo["samples"]
        assert isinstance(samples, list)
        for sample in samples:
            assert isinstance(sample, dict)
            walls.append(float(sample["wall_ms"]))
    return {
        "schema": SCHEMA,
        "captured_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "host": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "rustc": tool_version(["rustc", "--version"]),
            "git": tool_version(["git", "--version"]),
            "tov": tool_version(["tov", "version"]),
            "deliver": tool_version(["deliver", "--version"]),
        },
        "procedure": {
            "id": "repository-preflight",
            "version": 0,
            "steps": PROCEDURE,
            "runs": runs,
            "mutation": False,
            "network": False,
            "inference": "unmeasured",
            "cargo_metadata": False,
            "notes": [
                "Single-operator mechanical samples. They are not agent observations.",
                "Wall time includes a warm page cache on later runs.",
                "Dirty tracked counts omit paths.",
                "process_max_rss_kb is the interpreter high-water mark, not a per-repository allocation.",
                "Manifest bodies are hashed and parsed. They are not stored.",
            ],
        },
        "repositories": repositories,
        "aggregate": {
            "repositories": len(repositories),
            "samples": len(walls),
            "wall_ms_min": min(walls) if walls else None,
            "wall_ms_max": max(walls) if walls else None,
            "wall_ms_median": median(walls),
            "agent_observations": 0,
            "promotion_eligible": False,
        },
    }


def lock_from(document: dict[str, object]) -> dict[str, object]:
    repos = []
    items = document["repositories"]
    assert isinstance(items, list)
    for repo in items:
        assert isinstance(repo, dict)
        identity = repo["identity"]
        assert isinstance(identity, dict)
        crates = repo["crates"]
        assert isinstance(crates, list)
        manifests = []
        for crate in crates:
            assert isinstance(crate, dict)
            manifests.append({"manifest": crate["manifest"], "sha256": crate["sha256"], "bytes": crate["bytes"]})
        packages = repo["packages"]
        assert isinstance(packages, list)
        for package in packages:
            assert isinstance(package, dict)
            manifests.append({"manifest": package["manifest"], "sha256": package["sha256"], "bytes": package["bytes"]})
        repos.append(
            {
                "path": repo["path"],
                "head": identity.get("head"),
                "branch": identity.get("branch"),
                "manifests": manifests,
            }
        )
    return {"schema": LOCK_SCHEMA, "procedure": "repository-preflight/v0", "repositories": repos}


def check_lock(lock_path: Path) -> int:
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    if lock.get("schema") != LOCK_SCHEMA:
        print(f"lock schema mismatch: {lock.get('schema')}", file=sys.stderr)
        return 1
    failures = 0
    repos = lock.get("repositories")
    if not isinstance(repos, list) or not repos:
        print("lock has no repositories", file=sys.stderr)
        return 1
    for repo in repos:
        if not isinstance(repo, dict):
            print("lock repository entry is not an object", file=sys.stderr)
            failures += 1
            continue
        root = Path(str(repo.get("path")))
        head = run_git(root, ["rev-parse", "HEAD"])
        current = head.stdout.strip() if head.returncode == 0 else None
        if current != repo.get("head"):
            print(f"{root}: HEAD {current} differs from lock {repo.get('head')}", file=sys.stderr)
            failures += 1
        manifests = repo.get("manifests")
        if not isinstance(manifests, list):
            print(f"{root}: manifests missing", file=sys.stderr)
            failures += 1
            continue
        for manifest in manifests:
            if not isinstance(manifest, dict):
                failures += 1
                continue
            rel = str(manifest.get("manifest"))
            path = root / rel
            if not path.is_file():
                print(f"{path}: missing", file=sys.stderr)
                failures += 1
                continue
            digest = sha256_file(path)
            if digest != manifest.get("sha256"):
                print(f"{path}: sha256 changed", file=sys.stderr)
                failures += 1
    if failures:
        print(f"corpus lock failed: {failures}", file=sys.stderr)
        return 1
    print(f"corpus lock ok: {len(repos)} repositories")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Cyclone repository-preflight baseline")
    parser.add_argument("repos", nargs="*", type=Path)
    parser.add_argument("--runs", type=int, default=2)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--lock", type=Path)
    parser.add_argument("--check", type=Path, help="verify a corpus lock against current checkouts")
    args = parser.parse_args(argv)
    if args.check:
        return check_lock(args.check)
    if not args.repos or args.out is None:
        parser.error("measurement requires repositories and --out")
    document = measure(args.repos, args.runs)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    if args.lock:
        args.lock.parent.mkdir(parents=True, exist_ok=True)
        args.lock.write_text(json.dumps(lock_from(document), indent=2) + "\n", encoding="utf-8")
    aggregate = document["aggregate"]
    assert isinstance(aggregate, dict)
    print(
        f"measured {aggregate['repositories']} repositories, "
        f"median wall {aggregate['wall_ms_median']} ms, agent observations 0"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
