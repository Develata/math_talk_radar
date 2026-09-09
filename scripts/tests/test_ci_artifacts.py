import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("ci_artifacts", Path(__file__).resolve().parents[1] / "ci_artifacts.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class TransportTests(unittest.TestCase):
    def test_preflight_requires_artifact_lanes_and_release_still_requires_review(self):
        with tempfile.TemporaryDirectory() as temp:
            previous = Path.cwd()
            try:
                os.chdir(temp)
                Path("target/acceptance").mkdir(parents=True)
                lanes = ["quality", "tests", "assurance", "performance", "build", "artifact"]
                needs = {name: {"result": "success", "outputs": {"receipt_sha": "a" * 64}}
                         for name in ["plan", *lanes]}
                with patch.dict(os.environ, {"ACCEPTANCE_PROFILE": "preflight",
                        "GITHUB_OUTPUT": str(Path(temp) / "outputs"), "NEEDS_JSON": json.dumps(needs)}), \
                        patch("sys.argv", ["ci_artifacts.py", "expected"]):
                    module.main()
                    self.assertEqual(set(json.loads(Path("target/acceptance/receipt-hashes.json").read_text())), set(lanes))
                    for lane in ["build", "artifact"]:
                        changed = json.loads(json.dumps(needs))
                        changed[lane]["result"] = "skipped"
                        os.environ["NEEDS_JSON"] = json.dumps(changed)
                        with self.assertRaises(ValueError):
                            module.main()
                    os.environ.update(ACCEPTANCE_PROFILE="release", NEEDS_JSON=json.dumps(needs))
                    with self.assertRaises(ValueError):
                        module.main()
            finally:
                os.chdir(previous)

    def test_publication_rejects_manual_runs_and_preflight_plans(self):
        with tempfile.TemporaryDirectory() as temp:
            previous = Path.cwd()
            try:
                os.chdir(temp)
                plan_path = Path("target/acceptance/control/plan.json")
                plan_path.parent.mkdir(parents=True)
                for event, ref, profile, allowed in [
                    ("workflow_dispatch", "refs/heads/main", "preflight", False),
                    ("workflow_dispatch", "refs/tags/v0.1.0", "release", False),
                    ("push", "refs/heads/main", "release", False),
                    ("push", "refs/tags/v0.1.0", "preflight", False),
                    ("push", "refs/tags/v0.1.0", "release", True),
                ]:
                    with self.subTest(event=event, ref=ref, profile=profile), patch.dict(os.environ,
                            {"GITHUB_EVENT_NAME": event, "GITHUB_REF": ref}):
                        plan_path.write_text(json.dumps({"profile": profile}))
                        if allowed:
                            module.require_publication()
                        else:
                            with self.assertRaises(ValueError):
                                module.require_publication()
            finally:
                os.chdir(previous)

    def test_required_lane_cannot_be_skipped_or_omit_digest(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {
            "ACCEPTANCE_PROFILE": "full", "GITHUB_OUTPUT": str(Path(temp) / "outputs")
        }), patch("sys.argv", ["ci_artifacts.py", "expected"]):
            previous = Path.cwd()
            try:
                os.chdir(temp)
                Path("target/acceptance").mkdir(parents=True)
                lanes = ["quality", "tests", "assurance", "performance"]
                needs = {name: {"result": "success", "outputs": {"receipt_sha": "a" * 64}}
                         for name in ["plan", *lanes]}
                for invalid in ["skipped", "failure", "cancelled"]:
                    needs["tests"]["result"] = invalid
                    os.environ["NEEDS_JSON"] = json.dumps(needs)
                    with self.assertRaises(ValueError):
                        module.main()
                needs["tests"]["result"] = "success"
                needs["tests"]["outputs"]["receipt_sha"] = ""
                os.environ["NEEDS_JSON"] = json.dumps(needs)
                with self.assertRaises(ValueError):
                    module.main()
                needs["tests"]["outputs"]["receipt_sha"] = "a" * 64
                os.environ["NEEDS_JSON"] = json.dumps(needs)
                module.main()
                self.assertEqual(set(json.loads(Path("target/acceptance/receipt-hashes.json").read_text())), set(lanes))
            finally:
                os.chdir(previous)

    def test_published_readback_records_mismatch_and_download_failure(self):
        for outcome in ["pass", "mismatch", "download-failure"]:
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as temp:
                previous = Path.cwd()
                try:
                    os.chdir(temp)
                    control = Path("target/acceptance/control")
                    artifacts = Path("target/acceptance/receipts/build/artifacts")
                    control.mkdir(parents=True)
                    artifacts.mkdir(parents=True)
                    plan = {"identity": {"commit": "a" * 40}, "run_id": "123", "attempt": "2"}
                    (control / "plan.json").write_text(json.dumps(plan))
                    binary = "math_talk_radar-x86_64-unknown-linux-musl"
                    files = {binary: b"verified binary", binary + ".sha256": b"checksum asset"}
                    (artifacts / "manifest.json").write_text(json.dumps({"files": {
                        name: module.hashlib.sha256(data).hexdigest() for name, data in files.items()
                    }}))
                    def download(argv, **kwargs):
                        self.assertEqual(argv[:3], ["gh", "release", "download"])
                        directory = Path(argv[argv.index("--dir") + 1])
                        for name, data in files.items():
                            (directory / name).write_bytes(b"changed" if outcome == "mismatch" else data)
                        return subprocess.CompletedProcess(argv, 1 if outcome == "download-failure" else 0, "", "")
                    with patch.dict(os.environ, {"GITHUB_REF_NAME": "v1", "GITHUB_REPOSITORY": "owner/repo"}), patch.object(module.subprocess, "run", side_effect=download):
                        if outcome == "pass":
                            module.published_readback()
                        else:
                            with self.assertRaises(ValueError):
                                module.published_readback()
                    report = json.loads(Path("target/acceptance/published-readback.json").read_text())
                    self.assertEqual(report["status"], "pass" if outcome == "pass" else "fail")
                    self.assertEqual(report["identity"], plan["identity"])
                    self.assertEqual(report["attempt"], "2")
                finally:
                    os.chdir(previous)

    def test_wrong_hash_traversal_and_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / "archive.tar.gz"
            for name, link in [("../escape", False), ("link", True), ("file", False)]:
                with tarfile.open(archive, "w:gz") as stream:
                    info = tarfile.TarInfo(name)
                    info.size = 0 if link else 4
                    if link:
                        info.type = tarfile.SYMTYPE
                        info.linkname = "/etc/passwd"
                    stream.addfile(info, None if link else io.BytesIO(b"test"))
                with self.assertRaises(ValueError):
                    module.extract(archive, "0" * 64, root / "out")
                if name != "file":
                    with self.assertRaises(ValueError):
                        module.extract(archive, module.sha256(archive), root / "out")
                else:
                    module.extract(archive, module.sha256(archive), root / "out")
                    self.assertEqual((root / "out/file").read_bytes(), b"test")
                    with self.assertRaises(ValueError):
                        module.extract(archive, module.sha256(archive), root / "out")


if __name__ == "__main__":
    unittest.main()
