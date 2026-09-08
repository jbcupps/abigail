"""Isolated Windows setup acceptance probe. Build both daemons and stage the bundle first.

No personal profile or paid API is used. Outbound proxies are disabled test endpoints;
this is an offline-dependency probe, not an OS firewall isolation test.
Use --ui to leave the isolated daemon and Vite server running for browser inspection.
"""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target")))
if not TARGET.is_absolute():
    TARGET = ROOT / TARGET
BUNDLE = ROOT / "hive-app/resources/bootstrap"
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(url, token=None, body=None, expected=200):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(url, headers=headers,
                                 data=json.dumps(body).encode() if body is not None else None)
    try:
        response = HTTP.open(req, timeout=240)
    except urllib.error.HTTPError as error:
        assert error.code == expected, (url, error.code, expected)
        return None
    assert response.status == expected, (url, response.status, expected)
    value = json.load(response)
    return value


def stop(process):
    if process.poll() is None:
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=NO_WINDOW, check=False)
        else:
            process.terminate()
        process.wait(timeout=20)


class Daemon:
    def __init__(self, profile, bundle=BUNDLE):
        self.profile = profile
        self.token = uuid.uuid4().hex + uuid.uuid4().hex
        self.env = os.environ.copy()
        for key in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "ABIGAIL_PERSISTENCE_URL",
                    "ABIGAIL_ENTITY_AUTH_TOKEN", "ABIGAIL_HIVE_URL", "CLAUDECODE"):
            self.env.pop(key, None)
        self.env.update(ABIGAIL_LOCAL_AUTH_TOKEN=self.token,
                        ABIGAIL_BOOTSTRAP_DIR=str(bundle),
                        ABIGAIL_ENTITY_DAEMON_PATH=str(TARGET / "debug/entity-daemon.exe"),
                        LOCALAPPDATA=str(profile / "local"), USERPROFILE=str(profile), HOME=str(profile),
                        HTTP_PROXY="http://127.0.0.1:9", HTTPS_PROXY="http://127.0.0.1:9",
                        NO_PROXY="127.0.0.1,localhost")
        self.log = profile / ("daemon-" + uuid.uuid4().hex + ".log")
        with self.log.open("wb") as log:
            self.process = subprocess.Popen(
                [str(TARGET / "debug/hive-daemon.exe"), "--port", "0",
                 "--data-dir", str(profile / "data")], cwd=profile, env=self.env,
                stdout=log, stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
        try:
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                assert self.process.poll() is None, "Daemon stopped; inspect " + str(self.log)
                match = re.search(r"Hive daemon listening on (http://127\.0\.0\.1:\d+)",
                                  self.log.read_text(encoding="utf-8", errors="replace"))
                if match:
                    self.url = match[1]
                    return
                time.sleep(.2)
            raise AssertionError("Daemon never listened; inspect " + str(self.log))
        except BaseException:
            stop(self.process)
            raise

    def api(self, path, body=None):
        value = request(self.url + path, self.token, body)
        assert value["ok"], value.get("error")
        return value["data"]

    def ready(self):
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline:
            status = self.api("/v1/setup")
            assert status["phase"] != "error", status
            if status["phase"] == "ready":
                return status
            time.sleep(.5)
        raise AssertionError("Local completion readiness timed out")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ui", action="store_true")
    args = parser.parse_args()
    profile = Path(tempfile.mkdtemp(prefix="abigail-setup-acceptance-"))
    print("Isolated profile:", profile, flush=True)
    daemon = None
    vite = None
    try:
        daemon = Daemon(profile, profile / "missing-bundle")
        request(daemon.url + "/v1/setup", expected=401)
        request(daemon.url + "/v1/secrets", expected=401)
        assert daemon.api("/v1/setup")["phase"] == "error"
        assert daemon.api("/v1/setup/cancel", {})["phase"] == "cancelled"
        daemon.api("/v1/setup/retry", {})
        time.sleep(.5)
        assert daemon.api("/v1/setup")["phase"] == "error"
        stop(daemon.process)

        daemon = Daemon(profile)
        assert daemon.ready()["active_provider"] == "local"
        transcript = daemon.api("/v1/setup/chat", {"message": "Hello Abigail. What is an API account?"})
        assert len(transcript["messages"]) == 2
        assert transcript["messages"][-1]["content"].strip()
        print("Setup reply:", transcript["messages"][-1]["content"], flush=True)
        blocked = request(daemon.url + "/v1/setup/chat", daemon.token,
                          {"message": "Here is sk-ant-api03-FAKE-DO-NOT-STORE"})
        assert not blocked["ok"]
        assert daemon.api("/v1/setup/chat") == transcript
        print("PASS: missing package, retry, caller authentication, real offline chat, key rejection", flush=True)

        ids = [daemon.api("/v1/entities", {"name": name})["id"] for name in ("Setup test A", "Setup test B")]
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            opened = list(pool.map(lambda identity: daemon.api(f"/v1/entities/{identity}/open", {}), ids))
        assert opened[0]["auth_token"] != opened[1]["auth_token"]
        assert opened[0]["local_url"] != opened[1]["local_url"]
        for index, runtime in enumerate(opened):
            url, token = runtime["local_url"], runtime["auth_token"]
            request(url + "/v1/memory/stats", expected=401)
            request(url + "/v1/memory/stats", opened[1-index]["auth_token"], expected=401)
            assert request(url + "/v1/memory/insert", token,
                           {"content": f"private marker {index}", "weight": "ephemeral"})["ok"]
            rows = request(url + "/v1/memory/recent", token)["data"]
            assert any(row["content"] == f"private marker {index}" for row in rows)
            assert all(row["content"] != f"private marker {1-index}" for row in rows)
        print("PASS: concurrent Entity processes, separate caller tokens and durable memory scopes", flush=True)
        old_token = daemon.token
        stop(daemon.process)

        daemon = Daemon(profile)
        request(daemon.url + "/v1/setup", old_token, expected=401)
        daemon.ready()
        assert daemon.api("/v1/setup/chat") == transcript
        for index, identity in enumerate(ids):
            runtime = daemon.api(f"/v1/entities/{identity}/open", {})
            rows = request(runtime["local_url"] + "/v1/memory/recent", runtime["auth_token"])["data"]
            assert any(row["content"] == f"private marker {index}" for row in rows)
            assert all(row["content"] != f"private marker {1-index}" for row in rows)
            daemon.api(f"/v1/entities/{identity}/close", {})
        assert list((profile / "data").rglob("manifest")), "Database files must be inside the selected profile"
        print("PASS: restart preserved setup transcript and both Entity memories; old caller token rejected", flush=True)

        if args.ui:
            # This is a disposable acceptance profile. Never expose a personal
            # desktop credential through Vite: its source is publicly readable.
            env = daemon.env | {"VITE_HIVE_DAEMON_URL": daemon.url, "VITE_HIVE_AUTH_TOKEN": daemon.token}
            with (profile / "vite.log").open("wb") as log:
                vite = subprocess.Popen(["node", "node_modules/vite/bin/vite.js", "--host", "127.0.0.1", "--port", "1421", "--strictPort"],
                                        cwd=ROOT / "hive-app/src-ui", env=env, stdout=log,
                                        stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
            print("UI READY: http://127.0.0.1:1421 (disposable profile; interrupt to clean up)", flush=True)
            while vite.poll() is None:
                time.sleep(1)
    finally:
        if vite:
            stop(vite)
        if daemon:
            stop(daemon.process)


if __name__ == "__main__":
    main()
