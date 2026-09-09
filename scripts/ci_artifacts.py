#!/usr/bin/env python3
"""Transport only: trusted job-output digests before extracting CI evidence.

The Rust xtask remains the authority for selection and acceptance semantics.
Archives preserve executable modes; uploads never ship the Cargo target cache.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import time


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def extract(archive, expected_sha, destination, allowed=None):
    if not re.fullmatch(r"[a-f0-9]{64}", expected_sha or ""):
        raise ValueError("missing or invalid upstream archive digest")
    if sha256(archive) != expected_sha:
        raise ValueError("archive digest differs from upstream job output")
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as stream:
        members = stream.getmembers()
        seen = set()
        for member in members:
            path = PurePosixPath(member.name)
            if (path.is_absolute() or ".." in path.parts or not path.parts
                    or not (member.isfile() or member.isdir())
                    or member.name in seen
                    or (allowed is not None and member.name not in allowed)
                    or (destination / member.name).exists()):
                raise ValueError(f"unsafe or duplicate archive entry: {member.name}")
            seen.add(member.name)
        if allowed is not None and seen != allowed:
            raise ValueError("incomplete control archive")
        # Reject symlinks already present in any extraction ancestor too.
        for member in members:
            path = destination / member.name
            for parent in [path, *path.parents]:
                if parent.is_symlink():
                    raise ValueError("symlink in extraction path")
                if parent == destination:
                    break
        stream.extractall(destination, members=members, filter="data")


def pack(lane, shards):
    if not re.fullmatch(r"[a-z-]+", lane):
        raise ValueError("invalid lane")
    archive = Path(f"target/acceptance/receipts-{lane}.tar.gz")
    paths = []
    for shard in shards.split():
        if not re.fullmatch(r"[a-z-]+", shard):
            raise ValueError("invalid shard")
        path = Path("target/acceptance/receipts") / shard
        if path.is_dir():
            paths.append((path, shard))
    if not paths:
        raise ValueError("no execution receipts to archive")
    with tarfile.open(archive, "x:gz") as stream:
        for path, shard in paths:
            stream.add(path, arcname=shard)
    digest = sha256(archive)
    if "GITHUB_OUTPUT" in os.environ:
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"sha256={digest}\n")
    print(digest)


def published_readback():
    directory = Path("target/acceptance/published-readback")
    directory.mkdir()
    with open("target/acceptance/control/plan.json") as stream:
        plan = json.load(stream)
    with open("target/acceptance/receipts/build/artifacts/manifest.json") as stream:
        manifest = json.load(stream)
    binary = "math_talk_radar-x86_64-unknown-linux-musl"
    names = [binary, binary + ".sha256"]
    command = ["gh", "release", "download", os.environ["GITHUB_REF_NAME"], "--repo", os.environ["GITHUB_REPOSITORY"],
               "--pattern", names[0], "--pattern", names[1], "--dir", str(directory)]
    report = {"schema_version": 1, "kind": "published-readback", "identity": plan["identity"],
              "run_id": plan["run_id"], "attempt": plan["attempt"], "argv": command,
              "started_at": int(time.time()), "status": "fail", "exit_code": None}
    started = time.monotonic()
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=180)
        report.update(exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr)
        if result.returncode != 0:
            raise ValueError("published asset download failed")
        actual = {name: sha256(directory / name) for name in names}
        report["sha256"] = actual
        if any(actual[name] != manifest["files"][name] for name in names):
            raise ValueError("published bytes differ from the verified build")
        report["status"] = "pass"
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
    finally:
        report["duration_ms"] = int((time.monotonic() - started) * 1000)
        Path("target/acceptance/published-readback.json").write_text(json.dumps(report, indent=2))
    if report["status"] != "pass":
        raise ValueError("published read-back failed; see its execution receipt")


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    control = commands.add_parser("control")
    control.add_argument("--sha256", required=True)
    receipt = commands.add_parser("receipts")
    receipt.add_argument("--hashes", default="RECEIPT_HASHES", help="environment variable containing lane-to-digest JSON")
    packing = commands.add_parser("pack")
    packing.add_argument("--lane", required=True)
    packing.add_argument("--shards", required=True)
    commands.add_parser("expected")
    commands.add_parser("published-readback")
    args = parser.parse_args()
    if args.command == "control":
        directory = Path("target/acceptance/control")
        extract("target/acceptance/downloads/control.tar.gz", args.sha256, directory, {"xtask", "plan.json"})
        plan = json.loads((directory / "plan.json").read_text())
        if sha256(directory / "xtask") != plan["runner_sha256"]:
            raise ValueError("runner digest differs from plan")
        for key, actual in [("GITHUB_SHA", plan["identity"]["commit"]), ("GITHUB_RUN_ID", plan["run_id"]), ("GITHUB_RUN_ATTEMPT", plan["attempt"])]:
            if os.environ.get(key) != actual:
                raise ValueError(f"control bundle differs from {key}")
    elif args.command == "receipts":
        hashes = json.loads(os.environ[args.hashes])
        if not isinstance(hashes, dict) or not hashes:
            raise ValueError("expected upstream receipt hashes are missing")
        for lane, digest in hashes.items():
            if not re.fullmatch(r"[a-z-]+", lane):
                raise ValueError("invalid receipt lane")
            extract(f"target/acceptance/downloads/receipts-{lane}.tar.gz", digest, "target/acceptance/receipts")
    elif args.command == "expected":
        needs = json.loads(os.environ["NEEDS_JSON"])
        profile = os.environ["ACCEPTANCE_PROFILE"]
        lanes = ["quality", "tests", "assurance", "performance"]
        if profile == "release":
            lanes += ["build", "artifact", "review"]
        elif profile == "live":
            lanes = ["build", "live"]
        elif profile not in ["full", "shadow"]:
            raise ValueError("CI cannot use an unapproved selective profile")
        Path("target/acceptance/needs.json").write_text(json.dumps(needs, indent=2))
        for lane in ["plan", *lanes]:
            if needs.get(lane, {}).get("result") != "success":
                raise ValueError(f"required lane did not succeed: {lane}")
        hashes = {lane: needs[lane].get("outputs", {}).get("receipt_sha", "") for lane in lanes}
        if any(not re.fullmatch(r"[a-f0-9]{64}", digest) for digest in hashes.values()):
            raise ValueError("required receipt archive digest missing")
        Path("target/acceptance/receipt-hashes.json").write_text(json.dumps(hashes))
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write("receipt_hashes=" + json.dumps(hashes, separators=(",", ":")) + "\n")
    elif args.command == "published-readback":
        published_readback()
    else:
        pack(args.lane, args.shards)


if __name__ == "__main__":
    main()
