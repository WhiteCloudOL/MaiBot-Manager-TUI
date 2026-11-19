"""Refresh one EdgeOne directory using Tencent Cloud's TC3 authentication."""

from __future__ import annotations

import csv
import hashlib
import hmac
import io
import json
import re
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import unquote, urlsplit
from urllib.request import OpenerDirector, Request, build_opener

ENDPOINT = "teo.tencentcloudapi.com"
API_VERSION = "2022-09-01"


class SyncError(RuntimeError):
    """An actionable synchronization or configuration failure."""


def validate_cache_url(url: str) -> str:
    """Require a literal directory prefix and forbid a whole-host cache target."""
    parsed = urlsplit(url)
    path = unquote(parsed.path)
    if (
        parsed.scheme not in {"http", "https"}
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or not path.strip("/")
        or not parsed.path.endswith("/")
        or any(part in {".", ".."} for part in path.split("/"))
        or re.search(r"[\s*\\\x00-\x1f]", url + path)
    ):
        raise SyncError("MMTUI_CACHE_URL 必须是以 / 结尾的完整目录 URL，不能是域名根路径或通配符")
    return url


@dataclass(frozen=True)
class Credentials:
    secret_id: str = field(repr=False)
    secret_key: str = field(repr=False)
    token: str = field(default="", repr=False)


def load_credentials(environment: dict[str, str], base: Path) -> Credentials:
    """Use environment credentials or an existing local Tencent Cloud CSV."""
    secret_id = environment.get("TENCENTCLOUD_SECRET_ID", "").strip()
    secret_key = environment.get("TENCENTCLOUD_SECRET_KEY", "").strip()
    token = environment.get("TENCENTCLOUD_TOKEN", "").strip()
    if secret_id or secret_key:
        if not secret_id or not secret_key:
            raise SyncError("TENCENTCLOUD_SECRET_ID / TENCENTCLOUD_SECRET_KEY 必须同时设置")
        return Credentials(secret_id, secret_key, token)
    filename = environment.get("EDGEONE_CREDENTIALS_CSV", "").strip()
    if not filename:
        raise SyncError("请设置腾讯云环境变量，或在本机配置 EDGEONE_CREDENTIALS_CSV")
    path = Path(filename).expanduser()
    if not path.is_absolute():
        path = base / path
    try:
        content = path.read_bytes()
    except OSError as error:
        raise SyncError("无法读取 EDGEONE_CREDENTIALS_CSV 指定的本机凭据文件") from error
    for encoding in ("utf-8-sig", "utf-16", "gb18030"):
        try:
            text = content.decode(encoding)
        except UnicodeError:
            continue
        reader = csv.DictReader(io.StringIO(text))
        keys = {name.strip().lower().replace("_", ""): name for name in reader.fieldnames or []}
        if not {"secretid", "secretkey"} <= keys.keys():
            continue
        rows = [row for row in reader if any(row.values())]
        if len(rows) != 1:
            raise SyncError("密钥 CSV 必须只包含一条 SecretId / SecretKey 记录")
        secret_id = (rows[0].get(keys["secretid"]) or "").strip()
        secret_key = (rows[0].get(keys["secretkey"]) or "").strip()
        if not secret_id or not secret_key:
            raise SyncError("密钥 CSV 中存在空的 SecretId / SecretKey")
        return Credentials(secret_id, secret_key, token)
    raise SyncError("无法识别密钥 CSV；需要 SecretId、SecretKey 表头")


def authorization(credentials: Credentials, payload: bytes, timestamp: int) -> str:
    """Sign the exact body sent to the fixed EdgeOne API endpoint."""
    date = datetime.fromtimestamp(timestamp, timezone.utc).strftime("%Y-%m-%d")
    headers = f"content-type:application/json; charset=utf-8\nhost:{ENDPOINT}\n"
    canonical = "\n".join(
        [
            "POST",
            "/",
            "",
            headers,
            "content-type;host",
            hashlib.sha256(payload).hexdigest(),
        ]
    )
    scope = f"{date}/teo/tc3_request"
    signing_text = "\n".join(
        [
            "TC3-HMAC-SHA256",
            str(timestamp),
            scope,
            hashlib.sha256(canonical.encode()).hexdigest(),
        ]
    )
    date_key = hmac.digest(("TC3" + credentials.secret_key).encode(), date.encode(), "sha256")
    service_key = hmac.digest(date_key, b"teo", "sha256")
    signing_key = hmac.digest(service_key, b"tc3_request", "sha256")
    signature = hmac.new(signing_key, signing_text.encode(), "sha256").hexdigest()
    return (
        f"TC3-HMAC-SHA256 Credential={credentials.secret_id}/{scope}, "
        f"SignedHeaders=content-type;host, Signature={signature}"
    )


class EdgeOneClient:
    """Submit directory purges and wait for their actual completion."""

    def __init__(self, credentials: Credentials, opener: OpenerDirector | None = None) -> None:
        self.credentials = credentials
        self.opener = opener or build_opener()

    def call(self, action: str, parameters: dict[str, Any]) -> dict[str, Any]:
        payload = json.dumps(parameters, ensure_ascii=False, separators=(",", ":")).encode()
        timestamp = int(time.time())
        headers = {
            "Content-Type": "application/json; charset=utf-8",
            "Host": ENDPOINT,
            "X-TC-Action": action,
            "X-TC-Version": API_VERSION,
            "X-TC-Timestamp": str(timestamp),
            "Authorization": authorization(self.credentials, payload, timestamp),
        }
        if self.credentials.token:
            headers["X-TC-Token"] = self.credentials.token
        request = Request(f"https://{ENDPOINT}/", data=payload, headers=headers, method="POST")
        try:
            with self.opener.open(request, timeout=30) as response:
                result = json.load(response)
        except HTTPError as error:
            raise SyncError(f"EdgeOne API HTTP {error.code}，请检查权限和网络") from error
        except (URLError, TimeoutError, OSError) as error:
            raise SyncError(
                "EdgeOne 请求未得到明确响应；未自动重试，请先在控制台核实任务"
            ) from error
        except (ValueError, UnicodeError) as error:
            raise SyncError("EdgeOne API 返回了无效 JSON") from error
        body = result.get("Response") if isinstance(result, dict) else None
        if not isinstance(body, dict):
            raise SyncError("EdgeOne API 响应缺少 Response")
        if "Error" in body:
            detail = body["Error"]
            code = (
                detail.get("Code", "UnknownError") if isinstance(detail, dict) else "UnknownError"
            )
            raise SyncError(
                f"EdgeOne API 失败：{code}，RequestId={body.get('RequestId', 'unknown')}"
            )
        return body

    def purge_directory(self, zone_id: str, url: str) -> str:
        """Only delete cached resources under the configured non-root URL prefix."""
        validate_cache_url(url)
        result = self.call(
            "CreatePurgeTask",
            {
                "ZoneId": zone_id,
                "Type": "purge_prefix",
                "Method": "delete",
                "Targets": [url],
            },
        )
        if result.get("FailedList"):
            raise SyncError("EdgeOne 拒绝了目录刷新目标，请在控制台查看失败原因")
        job_id = result.get("JobId")
        if not isinstance(job_id, str) or not job_id:
            raise SyncError("EdgeOne 未返回 JobId，请先在控制台核实任务")
        return job_id

    def wait_for_purge(self, zone_id: str, job_id: str, seconds: int) -> None:
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            result = self.call(
                "DescribePurgeTasks",
                {
                    "ZoneId": zone_id,
                    "Filters": [{"Name": "job-id", "Values": [job_id]}],
                },
            )
            statuses = {task.get("Status") for task in result.get("Tasks", [])}
            if statuses and statuses <= {"success"}:
                return
            if statuses & {"failed", "timeout", "canceled"}:
                raise SyncError(f"目录刷新任务失败：JobId={job_id}")
            time.sleep(min(3, max(0, deadline - time.monotonic())))
        raise SyncError(f"目录刷新任务已提交但尚未确认完成：JobId={job_id}；请到控制台查看进度")
