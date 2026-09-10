#!/usr/bin/env python3
"""Validate immutable CI inputs and install the reviewed Jenkins closure."""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parent.parent
CI_LOCK_PATH = Path("tools/ci-inputs.lock.json")
JENKINS_LOCK_PATH = Path("tools/jenkins/controller-plugins.lock.json")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
SAFE_ID = re.compile(r"[a-z0-9][a-z0-9-]{0,127}\Z")
SAFE_VERSION = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,191}\Z")
ACTION = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}\Z")
IMAGE = re.compile(r"[^\s@]+@sha256:[0-9a-f]{64}\Z")
ARCHIVE_IMAGE_DECLARATION = re.compile(
    r'^REPRODUCIBLE_ARCHIVE_IMAGE = "([^"\s]+@sha256:[0-9a-f]{64})"$'
)
MAX_JSON_BYTES = 1024 * 1024
MAX_DOWNLOAD_BYTES = 256 * 1024 * 1024
MAX_TREE_FILES = 200_000
MAX_TREE_BYTES = 4 * 1024 * 1024 * 1024
CI_PATH_PREFIXES = (".github/workflows/", "tools/", "docker/")
CI_PATHS = {"Jenkinsfile", "tools/package_offline_cargo_bundle.sh"}
INSTALL_COMMAND = re.compile(
    r"^\s*(?:sudo\s+)?(?:apt(?:-get)?\s+install|apk\s+add|brew\s+install|"
    r"dnf\s+install|yum\s+install|pip(?:3)?\s+install|npm\s+install\s+-g)\b"
)


class SupplyChainError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SupplyChainError(message)


def load_json(path: Path) -> dict:
    size = path.stat().st_size
    require(0 < size <= MAX_JSON_BYTES, f"{path} is empty or too large")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise SupplyChainError(f"cannot read {path}: {error}") from error
    require(isinstance(value, dict), f"{path} is not a JSON object")
    return value


def exact_keys(value: dict, expected: set[str], context: str) -> None:
    actual = set(value)
    require(actual == expected, f"{context} fields differ: {sorted(actual ^ expected)}")


def safe_relative(value: str, context: str) -> None:
    candidate = PurePosixPath(value)
    require(
        bool(value)
        and not candidate.is_absolute()
        and ".." not in candidate.parts
        and "\\" not in value
        and not any(ord(character) < 32 for character in value),
        f"unsafe {context}: {value!r}",
    )


def validate_ci_lock(root: Path) -> dict:
    lock = load_json(root / CI_LOCK_PATH)
    exact_keys(
        lock,
        {
            "schema_version",
            "reviewed_on",
            "rust",
            "tools",
            "tracked_remote_inputs",
            "offline_bundle",
            "unsupported_local_inputs",
        },
        "CI input lock",
    )
    require(lock["schema_version"] == "mainframe-env.ci-input-lock@1", "bad CI lock schema")
    require(re.fullmatch(r"20[0-9]{2}-[0-9]{2}-[0-9]{2}", lock["reviewed_on"] or ""), "bad CI lock review date")
    exact_keys(lock["rust"], {"workspace", "msrv", "fuzz"}, "Rust lock")
    for name, toolchain in lock["rust"].items():
        require(isinstance(toolchain, dict), f"Rust {name} lock is not an object")
        expected_rust_keys = {"version", "rustc_commit", "cargo_commit"}
        if name == "fuzz":
            expected_rust_keys.add("toolchain")
        exact_keys(toolchain, expected_rust_keys, f"Rust {name} lock")
        if name == "fuzz":
            require(
                re.fullmatch(r"nightly-20[0-9]{2}-[0-9]{2}-[0-9]{2}", toolchain["toolchain"] or "")
                is not None,
                "bad Rust fuzz toolchain",
            )
            require(
                re.fullmatch(r"[1-9][0-9]*\.[0-9]+\.[0-9]+-nightly", toolchain["version"] or "")
                is not None,
                "bad Rust fuzz version",
            )
        else:
            require(re.fullmatch(r"[1-9][0-9]*\.[0-9]+\.[0-9]+", toolchain["version"] or ""), f"bad Rust {name} version")
        require(re.fullmatch(r"[0-9a-f]{40}", toolchain["rustc_commit"] or ""), f"bad Rust {name} commit")
        require(re.fullmatch(r"[0-9a-f]{40}", toolchain["cargo_commit"] or ""), f"bad Cargo {name} commit")

    expected_tools = {
        "cargo-deny",
        "cargo-fuzz",
        "cargo-llvm-cov",
        "docker",
        "git",
        "github-cli",
        "java",
        "postgresql",
        "python",
    }
    require(set(lock["tools"]) == expected_tools, "CI tool lock set differs")
    for name, tool in lock["tools"].items():
        require(isinstance(tool, dict), f"tool {name} lock is not an object")
        require(SAFE_VERSION.fullmatch(tool.get("version", "")) is not None, f"bad {name} version")
    exact_keys(lock["tools"]["cargo-deny"], {"version", "install"}, "cargo-deny lock")
    require(
        lock["tools"]["cargo-deny"]["install"]
        == "cargo +1.98.0 install cargo-deny --version 0.20.2 --locked",
        "cargo-deny installation is not exact and locked",
    )
    for name in ("cargo-fuzz", "cargo-llvm-cov"):
        exact_keys(lock["tools"][name], {"version", "install"}, f"{name} lock")
        require(
            lock["tools"][name]["install"]
            == f"cargo +1.98.0 install {name} --version {lock['tools'][name]['version']} --locked",
            f"{name} installation is not exact and locked",
        )

    remote = lock["tracked_remote_inputs"]
    exact_keys(remote, {"github_actions", "container_images", "package_install_commands"}, "remote input lock")
    for name, values in remote.items():
        require(isinstance(values, list) and values == sorted(set(values)), f"{name} lock must be sorted and unique")
    require(all(ACTION.fullmatch(value) is not None for value in remote["github_actions"]), "action lock contains a floating coordinate")
    require(all(IMAGE.fullmatch(value) is not None for value in remote["container_images"]), "container lock contains a mutable coordinate")
    require(not remote["package_install_commands"], "tracked CI package installation is forbidden")

    offline = lock["offline_bundle"]
    exact_keys(offline, {"generator", "inputs"}, "offline input lock")
    safe_relative(offline["generator"], "offline generator")
    require(isinstance(offline["inputs"], list) and offline["inputs"] == sorted(set(offline["inputs"])), "offline inputs must be sorted and unique")
    require(offline["generator"] in offline["inputs"], "offline generator is not bound as an input")
    for relative in offline["inputs"]:
        safe_relative(relative, "offline input")
        require((root / relative).is_file(), f"offline input is missing: {relative}")

    unsupported = lock["unsupported_local_inputs"]
    require(isinstance(unsupported, list) and unsupported == sorted(set(unsupported)), "unsupported local inputs must be sorted and unique")
    for relative in unsupported:
        safe_relative(relative, "unsupported local input")
    return lock


def validate_jenkins_lock(root: Path) -> dict:
    lock = load_json(root / JENKINS_LOCK_PATH)
    exact_keys(lock, {"schema_version", "reviewed_on", "controller", "required_plugins", "plugins"}, "Jenkins lock")
    require(lock["schema_version"] == "mainframe-env.jenkins-input-lock@1", "bad Jenkins lock schema")
    require(re.fullmatch(r"20[0-9]{2}-[0-9]{2}-[0-9]{2}", lock["reviewed_on"] or ""), "bad Jenkins review date")
    controller = lock["controller"]
    exact_keys(controller, {"version", "url", "sha256"}, "Jenkins controller lock")
    require(SAFE_VERSION.fullmatch(controller["version"] or "") is not None, "bad Jenkins version")
    expected_url = f"https://get.jenkins.io/war-stable/{controller['version']}/jenkins.war"
    require(controller["url"] == expected_url, "Jenkins controller URL is not version-pinned")
    require(SHA256.fullmatch(controller["sha256"] or "") is not None, "bad Jenkins digest")
    plugins = lock["plugins"]
    require(isinstance(plugins, list) and 1 <= len(plugins) <= 256, "bad Jenkins plugin count")
    names = []
    for plugin in plugins:
        require(isinstance(plugin, dict), "Jenkins plugin entry is not an object")
        exact_keys(plugin, {"id", "version", "sha256"}, "Jenkins plugin")
        require(SAFE_ID.fullmatch(plugin["id"] or "") is not None, "bad Jenkins plugin id")
        require(SAFE_VERSION.fullmatch(plugin["version"] or "") is not None, f"bad {plugin['id']} version")
        require(SHA256.fullmatch(plugin["sha256"] or "") is not None, f"bad {plugin['id']} digest")
        names.append(plugin["id"])
    require(names == sorted(set(names)), "Jenkins plugins must be sorted and unique")
    required = lock["required_plugins"]
    require(isinstance(required, list) and required == sorted(set(required)), "required plugins must be sorted and unique")
    require(set(required) <= set(names), "a required Jenkins plugin is not locked")
    return lock


def tracked_files(root: Path) -> list[str]:
    try:
        output = subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
        values = [part.decode("utf-8") for part in output.split(b"\0") if part]
    except (OSError, UnicodeError, subprocess.CalledProcessError) as error:
        raise SupplyChainError(f"cannot enumerate tracked files: {error}") from error
    require(len(values) <= MAX_TREE_FILES, "tracked file count is unbounded")
    for value in values:
        safe_relative(value, "tracked file")
    return values


def scan_external_inputs(root: Path, tracked: list[str]) -> dict[str, list[str]]:
    actions: set[str] = set()
    images: set[str] = set()
    installers: set[str] = set()
    for relative in tracked:
        if relative not in CI_PATHS and not relative.startswith(CI_PATH_PREFIXES):
            continue
        path = root / relative
        if not path.is_file() or path.stat().st_size > MAX_JSON_BYTES:
            raise SupplyChainError(f"tracked CI file is missing or too large: {relative}")
        text = path.read_text(encoding="utf-8")
        is_workflow = relative.startswith(".github/workflows/") and relative.endswith((".yml", ".yaml"))
        is_compose = relative == "docker/compose.yaml"
        if is_compose:
            # Local output tags are allowed only beside their reviewed build
            # recipe. They cannot silently become mutable registry inputs.
            recipes = load_json(root / 'docker/inputs.lock.json')['local_images']
            for block in re.split(r'^  [a-z][a-z0-9-]*:\s*$', text, flags=re.MULTILINE):
                image = re.search(r'^    image: (\S+)\s*$', block, re.MULTILINE)
                if image and not IMAGE.fullmatch(image.group(1)):
                    coordinate = image.group(1)
                    require(coordinate in recipes, f'unreviewed local image: {coordinate}')
                    recipe = recipes[coordinate]
                    require(recipe in tracked, f'untracked container recipe: {recipe}')
                    require(re.search(r'^    build:\s*$', block, re.MULTILINE) is not None
                            and f'      dockerfile: {recipe}\n' in block
                            and '      context: ..\n' in block,
                            f'local image lacks its reviewed build recipe: {coordinate}')
        for number, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            archive_image = ARCHIVE_IMAGE_DECLARATION.fullmatch(stripped)
            if archive_image:
                images.add(archive_image.group(1))
            if is_compose:
                image = re.match(r'image:\s*(\S+)', stripped)
                if image and IMAGE.fullmatch(image.group(1)):
                    images.add(image.group(1))
            if is_workflow:
                use = re.match(r"-?\s*uses:\s*['\"]?([^'\"#\s]+)", stripped)
                if use:
                    coordinate = use.group(1)
                    require(ACTION.fullmatch(coordinate) is not None, f"floating action at {relative}:{number}: {coordinate}")
                    actions.add(coordinate)
                image = re.match(r"image:\s*['\"]?([^'\"#\s]+)", stripped)
                if image:
                    coordinate = image.group(1)
                    require(IMAGE.fullmatch(coordinate) is not None, f"mutable container at {relative}:{number}: {coordinate}")
                    images.add(coordinate)
                runner = re.match(r"runs-on:\s*(.+)", stripped)
                if runner:
                    require("self-hosted" in runner.group(1), f"mutable hosted runner at {relative}:{number}")
            if relative.lower().endswith(("dockerfile", ".dockerfile")):
                base = re.match(r"FROM\s+(?:--platform=\S+\s+)?(\S+)", stripped, re.IGNORECASE)
                if base:
                    require(IMAGE.fullmatch(base.group(1)) is not None, f"mutable base image at {relative}:{number}")
                    images.add(base.group(1))
            if re.search(r"\b(?:docker|podman)\s+(?:build|run)\b", stripped):
                raise SupplyChainError(f"container command needs structured digest validation at {relative}:{number}")
            if INSTALL_COMMAND.match(stripped):
                installers.add(f"{relative}:{number}:{stripped}")
    return {
        "github_actions": sorted(actions),
        "container_images": sorted(images),
        "package_install_commands": sorted(installers),
    }


def validate_docker_inputs(root: Path, ci_lock: dict) -> None:
    lock = load_json(root / 'docker/inputs.lock.json')
    exact_keys(lock, {'schema_version', 'recorded_on', 'local_images', 'sources'}, 'Docker input lock')
    require(lock['schema_version'] == 1, 'bad Docker input schema')
    require(lock['local_images'] == {
        'mainframe-env-runtime:dev': 'docker/runtime.Dockerfile',
        'mainframe-env-toolchain:dev': 'docker/toolchain.Dockerfile',
    }, 'Docker local image recipes drifted')
    require(isinstance(lock['sources'], list), 'Docker sources must be a list')
    sources = {}
    for source in lock['sources']:
        exact_keys(source, {'name', 'version', 'url', 'sha256'}, 'Docker source')
        require(source['name'] not in sources, 'duplicate Docker source')
        require(SAFE_VERSION.fullmatch(source['version']) is not None, 'invalid Docker source version')
        require(source['url'].startswith('https://'), 'Docker source must use HTTPS')
        require(SHA256.fullmatch(source['sha256']) is not None, 'Docker source hash is not SHA-256')
        sources[source['name']] = source
    require(set(sources) == {'git', 'postgresql', 'bison', 'flex'}, 'Docker source closure differs')
    for name in ('git', 'postgresql'):
        require(sources[name]['version'] == ci_lock['tools'][name]['version'],
                f'Docker {name} differs from the CI tool lock')


def check_repository(root: Path = ROOT) -> tuple[dict, dict]:
    ci_lock = validate_ci_lock(root)
    jenkins_lock = validate_jenkins_lock(root)
    tracked = tracked_files(root)
    if 'docker/inputs.lock.json' in tracked:
        validate_docker_inputs(root, ci_lock)
    require(not (set(ci_lock["unsupported_local_inputs"]) & set(tracked)), "an explicitly unsupported local CI input became tracked")
    observed = scan_external_inputs(root, tracked)
    require(observed == ci_lock["tracked_remote_inputs"], "tracked remote CI inputs differ from their lock")

    toolchain = (root / "rust-toolchain.toml").read_text(encoding="utf-8")
    workspace = (root / "Cargo.toml").read_text(encoding="utf-8")
    channel = re.search(r'^channel\s*=\s*"([^"]+)"\s*$', toolchain, re.MULTILINE)
    components = re.search(r'^components\s*=\s*\[([^]]+)\]\s*$', toolchain, re.MULTILINE)
    rust_version = re.search(r'^rust-version\s*=\s*"([^"]+)"\s*$', workspace, re.MULTILINE)
    require(channel is not None and channel.group(1) == ci_lock["rust"]["workspace"]["version"], "workspace Rust pin drifted")
    require(
        components is not None
        and all(name in components.group(1) for name in ('"clippy"', '"llvm-tools-preview"', '"rustfmt"')),
        "workspace Rust component lock is incomplete",
    )
    declared_msrv = rust_version.group(1) if rust_version is not None else ""
    normalized_msrv = declared_msrv + ".0" if declared_msrv.count(".") == 1 else declared_msrv
    require(normalized_msrv == ci_lock["rust"]["msrv"]["version"], "workspace MSRV pin drifted")
    assurance = load_json(root / "tools/assurance-gates.json")
    require(
        assurance.get("fuzz", {}).get("toolchain")
        == ci_lock["rust"]["fuzz"]["toolchain"],
        "fuzz toolchain lock drifted",
    )

    jenkins = (root / "Jenkinsfile").read_text(encoding="utf-8")
    msrv = re.search(r"stage\('MSRV'\)(.*?)(?=\n\s*stage\('|\Z)", jenkins, re.DOTALL)
    require(msrv is not None, "Jenkins MSRV stage is missing")
    normalized_msrv = " ".join(msrv.group(1).split())
    expected_msrv = f"cargo +{ci_lock['rust']['msrv']['version']} check --workspace --all-targets --all-features --locked"
    require(expected_msrv in normalized_msrv and " -p " not in f" {normalized_msrv} ", "Jenkins MSRV scope is incomplete")
    require("--gate supply-chain" in jenkins and "tools/supply_chain.py check" in jenkins, "Jenkins supply-chain gate is missing")

    launcher = (root / "tools/jenkins/run-local.sh").read_text(encoding="utf-8")
    require("controller/jenkins.war" in launcher and "exec \"$MAINFRAME_ENV_JAVA\" -jar" in launcher, "Jenkins launcher does not use the locked controller")
    require("jenkins-lts" not in launcher, "Jenkins launcher still uses a floating package")
    bundle = (root / "tools/package_offline_cargo_bundle.sh").read_text(encoding="utf-8")
    require("record-offline" in bundle and "verify-offline" in bundle, "offline bundle input identity is not recorded and verified")
    return ci_lock, jenkins_lock


def command_output(arguments: list[str], env: dict[str, str] | None = None) -> str:
    try:
        return subprocess.check_output(arguments, text=True, stderr=subprocess.STDOUT, env=env).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "output", "")
        raise SupplyChainError(f"cannot identify {' '.join(arguments)}: {detail or error}") from error


def field(output: str, name: str) -> str:
    prefix = f"{name}: "
    value = next((line.removeprefix(prefix) for line in output.splitlines() if line.startswith(prefix)), None)
    require(value is not None, f"tool output omits {name}")
    return value


def verify_rust(name: str, locked: dict) -> None:
    version = locked["version"]
    toolchain = locked.get("toolchain", version)
    rustc = command_output(["rustup", "run", toolchain, "rustc", "-Vv"])
    cargo = command_output(["rustup", "run", toolchain, "cargo", "-Vv"])
    require(field(rustc, "release") == version, f"Rust {name} release drifted")
    require(field(rustc, "commit-hash") == locked["rustc_commit"], f"Rust {name} compiler commit drifted")
    require(field(cargo, "release") == version, f"Rust {name} Cargo release drifted")
    require(field(cargo, "commit-hash") == locked["cargo_commit"], f"Rust {name} Cargo commit drifted")


def verify_active_rust(locked: dict) -> None:
    rustc = command_output(["rustc", "-Vv"])
    cargo = command_output(["cargo", "-Vv"])
    require(field(rustc, "release") == locked["version"], "active Rust release drifted")
    require(field(rustc, "commit-hash") == locked["rustc_commit"], "active compiler commit drifted")
    require(field(cargo, "release") == locked["version"], "active Cargo release drifted")
    require(field(cargo, "commit-hash") == locked["cargo_commit"], "active Cargo commit drifted")


def java_executable() -> str:
    configured = os.environ.get("MAINFRAME_ENV_JAVA")
    candidate = configured or shutil.which("java")
    require(bool(candidate), "Java is missing; set MAINFRAME_ENV_JAVA")
    return str(candidate)


def verify_runtime(ci_lock: dict, scope: str) -> None:
    tools = ci_lock["tools"]
    if scope in {"ci", "controller", "offline", "all"}:
        expected_python = tuple(map(int, tools["python"]["version"].split(".")))
        require(sys.version_info[:3] == expected_python, f"Python must be exactly {tools['python']['version']}")
        git = command_output(["git", "--version"])
        match = re.search(r"git version ([0-9]+\.[0-9]+\.[0-9]+)", git)
        require(match is not None and match.group(1) == tools["git"]["version"], f"Git must be exactly {tools['git']['version']}")
    if scope in {"offline", "all"}:
        docker = command_output(["docker", "--version"])
        match = re.search(r"Docker version ([0-9]+\.[0-9]+\.[0-9]+),", docker)
        require(
            match is not None and match.group(1) == tools["docker"]["version"],
            f"Docker must be exactly {tools['docker']['version']}",
        )
    if scope in {"ci", "offline", "all"}:
        verify_rust("workspace", ci_lock["rust"]["workspace"])
        verify_active_rust(ci_lock["rust"]["workspace"])
    if scope in {"ci", "all"}:
        verify_rust("MSRV", ci_lock["rust"]["msrv"])
        verify_rust("fuzz", ci_lock["rust"]["fuzz"])
        deny = command_output(["cargo", "deny", "--version"])
        require(deny == f"cargo-deny {tools['cargo-deny']['version']}", f"cargo-deny must be exactly {tools['cargo-deny']['version']}")
        fuzz = command_output(["cargo", "fuzz", "--version"])
        require(fuzz == f"cargo-fuzz {tools['cargo-fuzz']['version']}", f"cargo-fuzz must be exactly {tools['cargo-fuzz']['version']}")
        coverage = command_output(["cargo", "llvm-cov", "--version"])
        require(coverage == f"cargo-llvm-cov {tools['cargo-llvm-cov']['version']}", f"cargo-llvm-cov must be exactly {tools['cargo-llvm-cov']['version']}")
    if scope in {"controller", "all"}:
        java = command_output([java_executable(), "-version"])
        match = re.search(r'version "([0-9.]+)', java)
        require(match is not None and match.group(1) == tools["java"]["version"], f"Java must be exactly {tools['java']['version']}")
    if scope in {"postgres", "all"}:
        postgres = command_output(["postgres", "--version"])
        match = re.search(r"PostgreSQL\)?\s+([0-9]+\.[0-9]+)", postgres)
        require(match is not None and match.group(1) == tools["postgresql"]["version"], f"PostgreSQL must be exactly {tools['postgresql']['version']}")
    if scope in {"release", "all"}:
        github = command_output(["gh", "--version"])
        match = re.search(r"gh version ([0-9]+\.[0-9]+\.[0-9]+)", github)
        require(match is not None and match.group(1) == tools["github-cli"]["version"], f"GitHub CLI must be exactly {tools['github-cli']['version']}")


def sha256_file(path: Path, limit: int = MAX_DOWNLOAD_BYTES) -> str:
    metadata = path.stat()
    require(stat.S_ISREG(metadata.st_mode) and 0 < metadata.st_size <= limit, f"{path} is not a bounded regular file")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def manifest(path: Path) -> dict[str, str]:
    try:
        with zipfile.ZipFile(path) as archive:
            info = archive.getinfo("META-INF/MANIFEST.MF")
            require(info.file_size <= MAX_JSON_BYTES, f"{path} manifest is too large")
            text = archive.read(info).decode("utf-8")
    except (OSError, KeyError, UnicodeError, zipfile.BadZipFile) as error:
        raise SupplyChainError(f"cannot inspect {path}: {error}") from error
    lines: list[str] = []
    for line in text.replace("\r\n", "\n").split("\n"):
        if line.startswith(" ") and lines:
            lines[-1] += line[1:]
        else:
            lines.append(line)
    return dict(line.split(": ", 1) for line in lines if ": " in line)


def plugin_dependencies(fields: dict[str, str]) -> set[str]:
    dependencies = set()
    for entry in fields.get("Plugin-Dependencies", "").split(","):
        if not entry:
            continue
        parts = entry.split(";")
        if "resolution:=optional" not in parts[1:]:
            dependency = parts[0].split(":", 1)[0]
            require(SAFE_ID.fullmatch(dependency) is not None, f"bad plugin dependency: {dependency}")
            dependencies.add(dependency)
    return dependencies


def verify_jenkins_artifacts(jenkins_lock: dict, controller: Path, plugins: Path) -> None:
    require(sha256_file(controller) == jenkins_lock["controller"]["sha256"], "Jenkins controller digest drifted")
    require(manifest(controller).get("Jenkins-Version") == jenkins_lock["controller"]["version"], "Jenkins controller version drifted")
    locked = {plugin["id"]: plugin for plugin in jenkins_lock["plugins"]}
    plugin_paths = list(plugins.glob("*.jpi")) + list(plugins.glob("*.hpi"))
    actual = {path.stem: path for path in plugin_paths}
    require(len(actual) == len(plugin_paths), "duplicate Jenkins plugin archives are active")
    require(set(actual) == set(locked), f"active Jenkins plugin set differs: {sorted(set(actual) ^ set(locked))}")
    disabled = sorted(path.name for path in plugins.glob("*.disabled"))
    require(not disabled, f"locked Jenkins plugins are disabled: {disabled}")
    dependencies: dict[str, set[str]] = {}
    for name, plugin in locked.items():
        path = actual[name]
        require(sha256_file(path) == plugin["sha256"], f"Jenkins plugin digest drifted: {name}")
        fields = manifest(path)
        require(fields.get("Short-Name") == name, f"Jenkins plugin identity drifted: {name}")
        require(fields.get("Plugin-Version") == plugin["version"], f"Jenkins plugin version drifted: {name}")
        dependencies[name] = plugin_dependencies(fields)
    missing = {f"{name}->{dependency}" for name, values in dependencies.items() for dependency in values if dependency not in locked}
    require(not missing, f"Jenkins plugin dependency closure is incomplete: {sorted(missing)}")


def verify_jenkins_home(jenkins_lock: dict, home: Path) -> None:
    require(home.is_absolute() and len(home.parts) >= 3, "Jenkins home must be a specific absolute path")
    verify_jenkins_artifacts(jenkins_lock, home / "controller/jenkins.war", home / "plugins")
    locked = {plugin["id"] for plugin in jenkins_lock["plugins"]}
    pinned = {
        path.name.removesuffix(".jpi.pinned")
        for path in (home / "plugins").glob("*.jpi.pinned")
    }
    require(pinned == locked, f"Jenkins plugin pin markers differ: {sorted(pinned ^ locked)}")


def download(url: str, destination: Path, expected: str) -> None:
    require(url.startswith("https://"), "Jenkins artifact URL is not HTTPS")
    request = urllib.request.Request(url, headers={"User-Agent": "mainframe-env-supply-chain/1"})
    digest = hashlib.sha256()
    total = 0
    try:
        with urllib.request.urlopen(request, timeout=60) as response, destination.open("wb") as output:
            require(response.geturl().startswith("https://"), "Jenkins artifact redirected away from HTTPS")
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                require(total <= MAX_DOWNLOAD_BYTES, "Jenkins artifact exceeds its bound")
                digest.update(chunk)
                output.write(chunk)
    except (OSError, urllib.error.URLError) as error:
        raise SupplyChainError(f"cannot download {url}: {error}") from error
    require(total > 0 and digest.hexdigest() == expected, f"Jenkins artifact digest mismatch: {url}")


def install_jenkins(jenkins_lock: dict, home: Path) -> None:
    require(home.is_absolute() and len(home.parts) >= 3, "Jenkins home must be a specific absolute path")
    home.mkdir(parents=True, exist_ok=True)
    plugin_directory = home / "plugins"
    existing = {path.stem for suffix in ("*.jpi", "*.hpi") for path in plugin_directory.glob(suffix)} if plugin_directory.is_dir() else set()
    locked = {plugin["id"] for plugin in jenkins_lock["plugins"]}
    require(not (existing - locked), f"unreviewed Jenkins plugins are installed: {sorted(existing - locked)}")
    with tempfile.TemporaryDirectory(prefix="jenkins-inputs-", dir=home) as temporary:
        staging = Path(temporary)
        staged_plugins = staging / "plugins"
        staged_plugins.mkdir()
        controller = staging / "jenkins.war"

        def fetch_plugin(plugin: dict) -> None:
            quoted_name = urllib.parse.quote(plugin["id"], safe="-")
            quoted_version = urllib.parse.quote(plugin["version"], safe="._-")
            url = f"https://updates.jenkins.io/download/plugins/{quoted_name}/{quoted_version}/{quoted_name}.hpi"
            download(url, staged_plugins / f"{plugin['id']}.jpi", plugin["sha256"])

        with ThreadPoolExecutor(max_workers=4) as executor:
            controller_download = executor.submit(
                download,
                jenkins_lock["controller"]["url"],
                controller,
                jenkins_lock["controller"]["sha256"],
            )
            plugin_downloads = [executor.submit(fetch_plugin, plugin) for plugin in jenkins_lock["plugins"]]
            controller_download.result()
            for plugin_download in plugin_downloads:
                plugin_download.result()
        verify_jenkins_artifacts(jenkins_lock, controller, staged_plugins)
        controller_directory = home / "controller"
        controller_directory.mkdir(exist_ok=True)
        plugin_directory.mkdir(exist_ok=True)
        for source, destination in [(controller, controller_directory / "jenkins.war")]:
            temporary_destination = destination.with_suffix(destination.suffix + ".new")
            shutil.copyfile(source, temporary_destination)
            os.chmod(temporary_destination, 0o644)
            os.replace(temporary_destination, destination)
        for plugin in jenkins_lock["plugins"]:
            destination = plugin_directory / f"{plugin['id']}.jpi"
            temporary_destination = destination.with_suffix(".jpi.new")
            shutil.copyfile(staged_plugins / destination.name, temporary_destination)
            os.chmod(temporary_destination, 0o644)
            os.replace(temporary_destination, destination)
            (plugin_directory / f"{plugin['id']}.jpi.pinned").touch(mode=0o644)
    verify_jenkins_home(jenkins_lock, home)


def length_prefixed(digest: hashlib._Hash, value: bytes) -> None:
    digest.update(len(value).to_bytes(8, "big"))
    digest.update(value)


def tree_identity(directory: Path) -> dict[str, int | str]:
    require(directory.is_dir() and not directory.is_symlink(), f"tree is missing or unsafe: {directory}")
    files = sorted(path for path in directory.rglob("*") if path.is_file() or path.is_symlink())
    require(len(files) <= MAX_TREE_FILES, "offline vendor tree has too many files")
    digest = hashlib.sha256(b"mainframe-env.offline-tree@1\0")
    total = 0
    for path in files:
        metadata = path.lstat()
        require(stat.S_ISREG(metadata.st_mode), f"offline vendor tree contains a non-regular file: {path}")
        total += metadata.st_size
        require(total <= MAX_TREE_BYTES, "offline vendor tree is too large")
        relative = path.relative_to(directory).as_posix()
        safe_relative(relative, "offline vendor path")
        length_prefixed(digest, relative.encode("utf-8"))
        digest.update(stat.S_IMODE(metadata.st_mode).to_bytes(4, "big"))
        digest.update(metadata.st_size.to_bytes(8, "big"))
        with path.open("rb") as source:
            while chunk := source.read(1024 * 1024):
                digest.update(chunk)
    return {"schema_version": "mainframe-env.offline-tree@1", "sha256": digest.hexdigest(), "files": len(files), "bytes": total}


def executable_identity(name: str, arguments: list[str], executable: str | None = None) -> dict[str, str]:
    resolved = Path(executable or shutil.which(arguments[0]) or "").resolve()
    require(resolved.is_file(), f"offline input tool is missing: {name}")
    output = command_output(arguments)
    require(bool(output), f"offline input tool has no version: {name}")
    return {"version": output.splitlines()[0], "sha256": sha256_file(resolved)}


def offline_record(root: Path, vendor: Path) -> dict:
    ci_lock, jenkins_lock = check_repository(root)
    inputs = {}
    for relative in ci_lock["offline_bundle"]["inputs"]:
        inputs[relative] = sha256_file(root / relative)
    revision = command_output(["git", "rev-parse", "HEAD"])
    require(re.fullmatch(r"[0-9a-f]{40}", revision) is not None, "offline source revision is invalid")
    tools = {
        "cargo": executable_identity("cargo", ["cargo", "-Vv"]),
        "docker": executable_identity("docker", ["docker", "--version"]),
        "git": executable_identity("git", ["git", "--version"]),
        "python": executable_identity("python", [sys.executable, "--version"], sys.executable),
        "rustc": executable_identity("rustc", ["rustc", "-Vv"]),
    }
    archive_images = ci_lock["tracked_remote_inputs"]["container_images"]
    require(len(archive_images) == 1, "offline archive environment lock differs")
    return {
        "schema_version": "mainframe-env.offline-build-inputs@2",
        "source_revision": revision,
        "locked_files": inputs,
        "vendor": tree_identity(vendor),
        "tools": tools,
        "archive_environment": {
            "image": archive_images[0],
            "platform": "linux/amd64",
            "tar": "GNU tar 1.34",
            "gzip": "gzip 1.12",
        },
        "jenkins_controller_version": jenkins_lock["controller"]["version"],
    }


def atomic_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    with tempfile.NamedTemporaryFile(prefix=path.name + ".", dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        output.write(data)
    try:
        os.chmod(temporary, 0o644)
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    subcommands = value.add_subparsers(dest="command", required=True)
    check = subcommands.add_parser("check")
    check.add_argument("--runtime", choices=["ci", "controller", "offline", "postgres", "release", "all"])
    check.add_argument("--jenkins-home", type=Path)
    install = subcommands.add_parser("install-jenkins")
    install.add_argument("--home", required=True, type=Path)
    verify = subcommands.add_parser("verify-jenkins")
    verify.add_argument("--home", required=True, type=Path)
    record = subcommands.add_parser("record-offline")
    record.add_argument("--vendor", required=True, type=Path)
    record.add_argument("--output", required=True, type=Path)
    verify_record = subcommands.add_parser("verify-offline")
    verify_record.add_argument("--vendor", required=True, type=Path)
    verify_record.add_argument("--record", required=True, type=Path)
    return value


def main(arguments: list[str] | None = None) -> int:
    args = parser().parse_args(arguments)
    try:
        if args.command == "check":
            ci_lock, jenkins_lock = check_repository(ROOT)
            if args.runtime:
                verify_runtime(ci_lock, args.runtime)
            if args.jenkins_home:
                verify_jenkins_home(jenkins_lock, args.jenkins_home.resolve())
            print(f"supply-chain: pass plugins={len(jenkins_lock['plugins'])} runtime={args.runtime or 'not-requested'}")
        elif args.command == "install-jenkins":
            jenkins_lock = validate_jenkins_lock(ROOT)
            install_jenkins(jenkins_lock, args.home.resolve())
            print(f"installed locked Jenkins controller and {len(jenkins_lock['plugins'])} plugins")
        elif args.command == "verify-jenkins":
            jenkins_lock = validate_jenkins_lock(ROOT)
            verify_jenkins_home(jenkins_lock, args.home.resolve())
            print(f"jenkins-inputs: pass plugins={len(jenkins_lock['plugins'])}")
        elif args.command == "record-offline":
            record = offline_record(ROOT, args.vendor.resolve())
            atomic_json(args.output.resolve(), record)
            print(f"offline-inputs: recorded files={record['vendor']['files']} bytes={record['vendor']['bytes']}")
        elif args.command == "verify-offline":
            expected = offline_record(ROOT, args.vendor.resolve())
            actual = load_json(args.record.resolve())
            require(actual == expected, "offline build input record is stale or incomplete")
            print(f"offline-inputs: pass files={expected['vendor']['files']} bytes={expected['vendor']['bytes']}")
    except (OSError, KeyError, TypeError, SupplyChainError) as error:
        print(f"supply-chain: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
