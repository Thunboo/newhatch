#!/usr/bin/env python3
"""Stateful Docker CLI replacement. Never connects to a Docker daemon."""

import json
import os
import sys
import time
from pathlib import Path

root = Path.cwd()
state_path = root / "fake-state.json"
state = json.loads(state_path.read_text())
args = sys.argv[1:]
with (root / "calls.jsonl").open("a") as output:
    output.write(json.dumps(args) + "\n")


def save():
    temporary = state_path.with_suffix(".tmp")
    temporary.write_text(json.dumps(state))
    temporary.replace(state_path)


if args[:3] == ["inspect", "--type", "container"]:
    payload = []
    for service, container in state["containers"].items():
        if container["id"] in args[3:]:
            payload.append({
                "Id": container["id"],
                "State": {"Status": container["state"]},
                "Config": {
                    "Env": [f"{key}={value}" for key, value in container["env"].items()]
                           + ["NEWHATCH_PASSWORD=fixture-secret"],
                    "Labels": {
                        "com.docker.compose.project": state["name"],
                        "com.docker.compose.service": service,
                    },
                },
            })
    print(json.dumps(payload))
    sys.exit(0)

assert args[0] == "compose", args
command_index = next(index for index, arg in enumerate(args) if arg in {"config", "ps", "up", "stop", "logs"})
command, options = args[command_index], args[command_index + 1:]
managed = {"collector", "analyzer", "frontend"}

if command == "config":
    print(json.dumps({"name": state["name"], "services": {
        service: {"environment": env} for service, env in state["desired"].items()
    }}))
elif command == "ps":
    selected = managed.intersection(options)
    assert selected, "ps must explicitly select managed services"
    rows = [{"ID": container["id"], "Service": service, "State": container["state"]}
            for service, container in state["containers"].items() if service in selected]
    if "--format" in options:
        if state.get("ndjson"):
            for row in rows:
                print(json.dumps(row))
        else:
            print(json.dumps(rows))
    else:
        print("NAME IMAGE COMMAND SERVICE CREATED STATUS PORTS")
        for row in rows:
            print(f'{state["name"]}-{row["Service"]} image command {row["Service"]} today {row["State"]}')
elif command in {"up", "stop"}:
    service = options[-1]
    assert service in managed, "mutation of excluded service"
    if command == "up":
        assert "--no-deps" in options
        assert "--build" not in options
        if state.get("fail_up") == service:
            print("fixture: startup failed", file=sys.stderr)
            sys.exit(1)
        if service not in state["images"]:
            state["images"].append(service)
            print(f"Building {service}", flush=True)
        generation = state.get("generation", 0) + 1
        state["generation"] = generation
        state["containers"][service] = {
            "id": f"{service}-{generation}",
            "state": "exited" if state.get("early_exit") == service else "running",
            "env": state["desired"][service],
        }
    else:
        state["containers"][service]["state"] = "exited"
    save()
    print(f"{service}: {command} complete", flush=True)
elif command == "logs":
    assert options[-1] in managed
    assert options[options.index("--tail") + 1] == "200"
    assert "--follow" in options
    (root / "log-pid").write_text(str(os.getpid()))
    for index in range(200):
        print(f"\x1b[2mfixture log {index}\x1b[0m", flush=True)
    while True:
        print("\x1b[32mfixture live log\x1b[0m", flush=True)
        time.sleep(0.05)
else:
    raise AssertionError(args)
