"""Regression tests for local release delivery and directory-only cache refresh."""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import os
import tarfile
import tempfile
import unittest
import uuid
from pathlib import Path
from unittest.mock import Mock, patch

import edgeone_cache
import release_sync_remote
import sync_release
from edgeone_cache import (
    Credentials,
    EdgeOneClient,
    SyncError,
    load_credentials,
    validate_cache_url,
)
from sync_release import (
    BINARY_NAMES,
    INSTALLER_NAMES,
    GitHubRelease,
    Settings,
    load_environment,
)


def settings() -> Settings:
    return Settings(
        "example-ssh",
        "/srv/downloads/example",
        "https://example.com/project/",
        "zone-example",
    )


class ConfigurationTests(unittest.TestCase):
    def test_dotenv_is_literal_and_environment_has_precedence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / ".env.deploy"
            path.write_text(
                "LOCAL_PATH='C:\\private\\keys.csv'\nVALUE=file\nCOMMAND='$(echo unsafe)'\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"VALUE": "process"}, clear=True):
                result = load_environment(path)
            self.assertEqual(result["LOCAL_PATH"], r"C:\private\keys.csv")
            self.assertEqual(result["COMMAND"], "$(echo unsafe)")
            self.assertEqual(result["VALUE"], "process")

    def test_invalid_or_duplicate_environment_entries_are_reported_without_values(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / ".env.deploy"
            for content in ["invalid-secret", "KEY='secret", "KEY=one\nKEY=two"]:
                path.write_text(content, encoding="utf-8")
                with self.assertRaises(SyncError) as failure:
                    load_environment(path)
                self.assertNotIn("secret", str(failure.exception))

    def test_directory_url_cannot_expand_into_a_host_purge(self) -> None:
        for url in [
            "https://example.com",
            "https://example.com/",
            "https://example.com//",
            "https://example.com/%2f",
            "https://example.com/path",
            "https://example.com/*/",
            "https://example.com/a/../",
            "https://example.com/%2e%2e/",
            "https://u:p@example.com/a/",
            "https://example.com/a/?all=1",
        ]:
            with self.subTest(url=url), self.assertRaises(SyncError):
                validate_cache_url(url)
        self.assertEqual(
            validate_cache_url("https://example.com/project/"),
            "https://example.com/project/",
        )

    def test_credentials_use_environment_or_existing_csv_without_exposing_values(
        self,
    ) -> None:
        credentials = load_credentials(
            {
                "TENCENTCLOUD_SECRET_ID": "sample-id",
                "TENCENTCLOUD_SECRET_KEY": "sample-key",
            },
            Path("."),
        )
        self.assertNotIn("sample-id", repr(credentials))
        self.assertNotIn("sample-key", repr(credentials))
        with self.assertRaises(SyncError):
            load_credentials({"TENCENTCLOUD_SECRET_ID": "sample-id"}, Path("."))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "keys.csv"
            path.write_text("SecretId,SecretKey\nexample-id,example-key\n", encoding="utf-8-sig")
            credentials = load_credentials({"EDGEONE_CREDENTIALS_CSV": "keys.csv"}, path.parent)
            self.assertEqual(credentials.secret_id, "example-id")
            self.assertNotIn("example-key", repr(credentials))

    def test_settings_do_not_include_private_values_in_repr(self) -> None:
        value = Settings(
            "private-alias",
            "/private/directory",
            "https://private.example/a/",
            "zone-private",
            github_token="private-token",
        )
        self.assertNotIn("private", repr(value))


class DownloadTests(unittest.TestCase):
    def test_package_uses_release_commit_scripts_and_only_the_six_public_files(
        self,
    ) -> None:
        client = GitHubRelease(settings(), Mock())
        commit = "a" * 40
        script = b"installer-from-release\n"
        script_digest = hashlib.sha1(f"blob {len(script)}\0".encode() + script).hexdigest()

        def api(suffix: str) -> dict:
            if suffix == "commits/v1.2.3":
                return {"sha": commit}
            self.assertIn(f"?ref={commit}", suffix)
            return {
                "encoding": "base64",
                "content": sync_release.base64.b64encode(script).decode(),
                "sha": script_digest,
            }

        release = {
            "tag_name": "v1.2.3",
            "assets": [{"name": name} for name in BINARY_NAMES],
        }
        with (
            tempfile.TemporaryDirectory() as temporary,
            patch.object(client, "api", side_effect=api),
            patch.object(
                client,
                "download",
                side_effect=lambda item, destination: destination.write_bytes(b"binary"),
            ),
        ):
            archive, manifest = client.package(release, Path(temporary))
            self.assertEqual(manifest["commit"], commit)
            with tarfile.open(archive, "r:gz") as package:
                self.assertEqual(
                    set(package.getnames()),
                    set(BINARY_NAMES + INSTALLER_NAMES) | {"manifest.json"},
                )
                self.assertEqual(package.extractfile("install.sh").read(), script)

    def test_download_must_match_release_sha256_and_size(self) -> None:
        data = b"binary contents"
        opener = Mock()
        opener.open.side_effect = lambda *args, **kwargs: io.BytesIO(data)
        client = GitHubRelease(settings(), opener)
        asset = {
            "name": BINARY_NAMES[0],
            "url": "https://api.github.com/repos/example/repo/releases/assets/123",
            "size": len(data),
            "digest": "sha256:" + hashlib.sha256(data).hexdigest(),
        }
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "binary"
            client.download(asset, target)
            self.assertEqual(target.read_bytes(), data)
            asset["digest"] = "sha256:" + "0" * 64
            with self.assertRaises(SyncError):
                client.download(asset, target)

    def test_incomplete_or_prerelease_metadata_is_rejected(self) -> None:
        client = GitHubRelease(settings(), Mock())
        for metadata in [
            {"tag_name": "v1", "assets": []},
            {"tag_name": "v1", "prerelease": True},
        ]:
            with (
                patch.object(client, "api", return_value=metadata),
                self.assertRaises(SyncError),
            ):
                client.latest()

    def test_tokens_only_go_to_github_api_and_are_stripped_on_cross_origin_redirect(
        self,
    ) -> None:
        client = GitHubRelease(
            Settings(
                "alias",
                "/srv/downloads/site",
                "https://example.com/a/",
                "zone-example",
                github_token="example-token",
            ),
            Mock(),
        )
        request = client.request("https://api.github.com/repos/example/repo")
        self.assertIsNotNone(request.get_header("Authorization"))
        self.assertIsNone(
            client.request("https://github.com/example/asset").get_header("Authorization")
        )
        redirected = sync_release.SafeRedirects().redirect_request(
            request,
            None,
            302,
            "Found",
            {},
            "https://release-assets.githubusercontent.com/asset",
        )
        self.assertIsNone(redirected.get_header("Authorization"))


class CacheTests(unittest.TestCase):
    def test_only_directory_delete_is_submitted(self) -> None:
        client = EdgeOneClient(Credentials("example-id", "example-key"))
        with patch.object(client, "call", return_value={"JobId": "job-example"}) as call:
            self.assertEqual(
                client.purge_directory("zone-example", "https://example.com/a/"),
                "job-example",
            )
            self.assertEqual(
                call.call_args.args,
                (
                    "CreatePurgeTask",
                    {
                        "ZoneId": "zone-example",
                        "Type": "purge_prefix",
                        "Method": "delete",
                        "Targets": ["https://example.com/a/"],
                    },
                ),
            )
        with patch.object(client, "call") as call, self.assertRaises(SyncError):
            client.purge_directory("zone-example", "https://example.com/")
        call.assert_not_called()

    def test_purge_response_and_final_status_must_both_succeed(self) -> None:
        client = EdgeOneClient(Credentials("example-id", "example-key"))
        for result in [{"FailedList": [{}]}, {}]:
            with (
                patch.object(client, "call", return_value=result),
                self.assertRaises(SyncError),
            ):
                client.purge_directory("zone-example", "https://example.com/a/")
        with (
            patch.object(
                client,
                "call",
                side_effect=[
                    {"Tasks": [{"Status": "processing"}]},
                    {"Tasks": [{"Status": "success"}]},
                ],
            ),
            patch.object(edgeone_cache.time, "sleep"),
        ):
            client.wait_for_purge("zone-example", "job-example", 10)
        with (
            patch.object(client, "call", return_value={"Tasks": [{"Status": "failed"}]}),
            self.assertRaises(SyncError),
        ):
            client.wait_for_purge("zone-example", "job-example", 10)

    def test_api_error_does_not_print_secret_material(self) -> None:
        opener = Mock()
        response = {
            "Response": {
                "Error": {"Code": "AuthFailure", "Message": "example-secret"},
                "RequestId": "request-example",
            }
        }
        opener.open.return_value = io.BytesIO(json.dumps(response).encode())
        client = EdgeOneClient(Credentials("example-id", "example-secret"), opener)
        with self.assertRaises(SyncError) as error:
            client.call("DescribePurgeTasks", {})
        self.assertIn("AuthFailure", str(error.exception))
        self.assertNotIn("example-secret", str(error.exception))


@unittest.skipUnless(os.name == "posix", "remote replacement runs on Linux")
class RemoteTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name).resolve()
        self.target = self.parent / "downloads"
        self.target.mkdir()
        self.archive = self.parent / f".downloads.upload-{uuid.uuid4().hex}.tar.gz"

    def package(self) -> dict:
        entries = {}
        with tarfile.open(self.archive, "w:gz") as package:
            for name in release_sync_remote.FILES:
                data = ("new-" + name).encode()
                entries[name] = {
                    "size": len(data),
                    "sha256": hashlib.sha256(data).hexdigest(),
                }
                member = tarfile.TarInfo(name)
                member.size = len(data)
                package.addfile(member, io.BytesIO(data))
            manifest = {"tag": "v1.2.3", "files": entries}
            data = json.dumps(manifest).encode()
            member = tarfile.TarInfo("manifest.json")
            member.size = len(data)
            package.addfile(member, io.BytesIO(data))
        return manifest

    def test_verified_delivery_preserves_unrelated_files_and_fixes_modes(self) -> None:
        manifest = self.package()
        (self.target / "unrelated.txt").write_text("preserve", encoding="utf-8")
        result = release_sync_remote.apply_archive(self.target, self.archive)
        self.assertEqual(result, manifest)
        self.assertEqual((self.target / "unrelated.txt").read_text(), "preserve")
        self.assertFalse(self.archive.exists())
        self.assertEqual((self.target / "install.sh").stat().st_mode & 0o777, 0o755)

    def test_partial_replacement_rolls_back_every_file(self) -> None:
        self.package()
        for name in release_sync_remote.FILES:
            (self.target / name).write_bytes(b"previous")
        replace = os.replace
        replacements = 0

        def interrupted_replace(source: Path, destination: Path) -> None:
            nonlocal replacements
            replacements += 1
            if replacements == 3:
                raise OSError("simulated replacement failure")
            replace(source, destination)

        with (
            patch.object(release_sync_remote.os, "replace", side_effect=interrupted_replace),
            self.assertRaises(OSError),
        ):
            release_sync_remote.apply_archive(self.target, self.archive)
        for name in release_sync_remote.FILES:
            self.assertEqual((self.target / name).read_bytes(), b"previous")

    def test_symlink_destination_is_rejected_before_any_replacement(self) -> None:
        self.package()
        protected = self.parent / "protected"
        protected.write_text("protected", encoding="utf-8")
        (self.target / "install.sh").symlink_to(protected)
        with self.assertRaises(ValueError):
            release_sync_remote.apply_archive(self.target, self.archive)
        self.assertEqual(protected.read_text(), "protected")
        self.assertFalse((self.target / "install.ps1").exists())

    def test_archive_traversal_is_never_extracted(self) -> None:
        with tarfile.open(self.archive, "w:gz") as package:
            member = tarfile.TarInfo("../escape")
            member.size = 1
            package.addfile(member, io.BytesIO(b"x"))
        with self.assertRaises(ValueError):
            release_sync_remote.apply_archive(self.target, self.archive)
        self.assertFalse((self.parent / "escape").exists())

    def test_concurrent_sync_is_rejected(self) -> None:
        with release_sync_remote.destination_lock(self.target):
            with self.assertRaises(RuntimeError):
                with release_sync_remote.destination_lock(self.target):
                    self.fail("must not acquire a concurrent lock")


class WorkflowTests(unittest.TestCase):
    def test_failed_upload_never_submits_cache_purge(self) -> None:
        with (
            patch.object(sync_release.sys, "argv", ["sync_release.py"]),
            patch.object(sync_release, "load_environment", return_value={}),
            patch.object(sync_release.Settings, "load", return_value=settings()),
            patch.object(sync_release, "make_opener"),
            patch.object(sync_release, "load_credentials"),
            patch.object(sync_release, "GitHubRelease") as github,
            patch.object(sync_release, "RemoteDirectory") as remote,
            patch.object(sync_release, "EdgeOneClient") as edgeone,
            contextlib.redirect_stdout(io.StringIO()),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            github.return_value.latest.return_value = {"tag_name": "v1"}
            github.return_value.package.return_value = (Path("package"), {})
            remote.return_value.upload.side_effect = SyncError("failed upload")
            self.assertEqual(sync_release.main(), 1)
            edgeone.return_value.purge_directory.assert_not_called()

    def test_dry_run_reads_no_credentials_and_runs_no_ssh(self) -> None:
        with (
            patch.object(sync_release.sys, "argv", ["sync_release.py", "--dry-run"]),
            patch.object(sync_release, "load_environment", return_value={}),
            patch.object(sync_release.Settings, "load", return_value=settings()),
            patch.object(sync_release, "make_opener"),
            patch.object(sync_release, "load_credentials") as credentials,
            patch.object(sync_release, "GitHubRelease") as github,
            patch.object(sync_release, "RemoteDirectory") as remote,
            patch.object(sync_release, "EdgeOneClient") as edgeone,
            contextlib.redirect_stdout(io.StringIO()),
        ):
            github.return_value.latest.return_value = {"tag_name": "v1"}
            self.assertEqual(sync_release.main(), 0)
            credentials.assert_not_called()
            remote.assert_not_called()
            edgeone.assert_not_called()


if __name__ == "__main__":
    unittest.main()
