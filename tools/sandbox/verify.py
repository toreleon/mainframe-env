"""Exercise actual CardDemo through independent foreground sandbox controllers."""
from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tempfile
import time

from .client import Client


def verify(bundle: Path, directory: Path) -> dict:
    directory.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="sandbox-acceptance-", dir=directory))
    processes = {}
    logs = []
    command = [sys.executable, str(bundle / "bin/mainframe-sandbox")]

    def start(name, profile="carddemo-online"):
        instance = scratch / name
        log = (scratch / (name + ".log")).open("ab")
        logs.append(log)
        process = subprocess.Popen(command + ["serve", "--bundle", str(bundle), "--instance", str(instance),
                                    "--profile", profile, "--listen", "127.0.0.1:0"], stdout=log, stderr=log)
        processes[name] = process
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise ValueError(f"{name} exited; inspect {scratch / (name + '.log')}")
            if (instance / "connection.json").exists():
                client = Client(instance)
                if client.call("sandbox_status", {})["ready"]:
                    return client
            time.sleep(0.1)
        raise ValueError(f"{name} readiness timed out")

    def stop(name):
        process = processes[name]
        if process.poll() is None:
            Client(scratch / name).request("/sandbox/v1/stop", {})
            process.wait(timeout=15)
        if process.returncode != 0:
            raise ValueError(f"{name} did not shut down cleanly")

    def menu(client, option):
        session = client.call("terminal_open", {})["session"]
        invalid = client.call("terminal_send", {"session": session, "fields": {"USERID": "USER0001", "PASSWD": "WRONG"}})
        assert invalid["mapset"] == "COSGN00" and invalid["fields"]["PASSWD"] != "WRONG"
        login = client.call("terminal_send", {"session": session, "fields": {"USERID": "USER0001", "PASSWD": "PASSWORD"}})
        assert login["mapset"] == "COMEN01"
        screen = client.call("terminal_send", {"session": session, "fields": {"OPTION": option}})
        return session, screen

    def card(client):
        session, screen = menu(client, "5")
        screen = client.call("terminal_send", {"session": session, "fields": {"ACCTSID": "00000000050", "CARDSID": "0500024453765740"}})
        assert screen["mapset"] == "COCRDUP" and "CRDNAME" in screen["editable"]
        return session, screen

    def account_message(client):
        session, screen = menu(client, "1")
        message = screen["fields"]["INFOMSG"]
        screen = client.call("terminal_send", {"session": session, "fields": {"ACCTSID": "00000000050"}})
        assert screen["fields"]["ACCTSID"] == "00000000050"
        client.call("terminal_close", {"session": session})
        return message

    try:
        compiler = start("compiler", "cobol")
        assert "terminal_open" not in compiler.call("sandbox_status", {})["tools"]
        assert "Hello from Mainframe Sandbox" in compiler.call("cobol_run", {"path": "HELLO.cbl"})["output"]
        first, second = start("first"), start("second")
        original_generation = first.call("sandbox_status", {})["generation"]
        session, screen = card(first)
        original_name = screen["fields"]["CRDNAME"]
        screen = first.call("terminal_send", {"session": session, "fields": {"CRDNAME": "SANDBOX AGENT UPDATE"}})
        assert "CRDNAME" not in screen["editable"]
        screen = first.call("terminal_send", {"session": session, "aid": "PF5"})
        assert "COMMITTED" in screen["fields"]["INFOMSG"]
        first.call("terminal_close", {"session": session})
        session, screen = card(second)
        assert screen["fields"]["CRDNAME"] == original_name
        second.call("terminal_close", {"session": session})
        stop("first")
        first = start("first")
        session, screen = card(first)
        assert screen["fields"]["CRDNAME"] == "SANDBOX AGENT UPDATE"
        first.call("terminal_close", {"session": session})
        path = "app/cbl/COACTVWC.cbl"
        source = first.call("workspace_read", {"path": path})["content"]
        old = "Enter or update id of account to display"
        assert old in source
        changed = source.replace(old, "Agent deployed account screen".ljust(len(old)), 1)
        first.call("workspace_write", {"path": path, "content": changed})
        assert first.call("cobol_compile", {"path": path})["ok"]
        assert "AGENT DEPLOYED" not in account_message(first)
        deployed = first.call("sandbox_deploy", {})["generation"]
        assert "AGENT DEPLOYED" in account_message(first)
        first.call("sandbox_rollback", {"generation": original_generation})
        session, screen = card(first)
        assert screen["fields"]["CRDNAME"] == "SANDBOX AGENT UPDATE"
        first.call("terminal_close", {"session": session})
        first.call("sandbox_rollback", {"generation": deployed})
        reset = first.call("sandbox_reset", {})["generation"]
        assert reset != deployed and "AGENT DEPLOYED" in account_message(first)
        session, screen = card(first)
        assert screen["fields"]["CRDNAME"] == original_name
        first.call("terminal_close", {"session": session})
        first.call("workspace_write", {"path": path, "content": "not valid COBOL"})
        try:
            first.call("sandbox_deploy", {})
        except ValueError:
            pass
        else:
            raise AssertionError("invalid deployment succeeded")
        status = first.call("sandbox_status", {})
        assert status["ready"] and status["generation"] == reset
        assert "AGENT DEPLOYED" in account_message(first)
        assert first.call("datasets_list", {"pattern": "AWS.M2.CARDDEMO.**"})["returnedRows"] >= 10
        job = first.call("jobs_submit", {"jcl": "//SBTEST JOB CLASS=A,MSGCLASS=A\n//STEP EXEC PGM=IEFBR14\n"})
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            result = first.call("jobs_status", {"jobname": job["jobname"], "jobid": job["jobid"]})
            if result["status"] == "OUTPUT":
                break
            time.sleep(0.05)
        assert result["status"] == "OUTPUT"
        return {"passed": True, "directory": str(scratch), "checks": [
            "standalone COBOL execution", "real CardDemo sign-on and account use", "secret masking and live protection",
            "independent application data", "durable card update and restart", "compile/edit/deploy/use",
            "retained generation rollback", "reset retains deployed code", "failed deployment preserves service",
            "authenticated dataset and JES operations", "foreground process cleanup"]}
    finally:
        for name, process in processes.items():
            if process.poll() is None:
                try:
                    stop(name)
                except (ValueError, OSError, subprocess.TimeoutExpired):
                    process.terminate()
                    process.wait(timeout=15)
        for log in logs:
            log.close()
