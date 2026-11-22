"""Invocation-local Codex/DeepSeek provider binding; never exports credentials."""
import base64
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tomllib
from urllib.parse import urlsplit

TOKEN_ENV = "VELELLA_REMOTE_MODEL_TOKEN"


def isolated_environment():
    # The source provider is read before switching HOME. No account files or
    # global CLI configuration are copied into this session-owned environment.
    root = Path(__file__).resolve().parent
    meta = root.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise ValueError("unsafe session directory")
    folders = {}
    for name in ("native-home", "codex-home"):
        folder = root / name
        try:
            folder.mkdir(mode=0o700)
        except FileExistsError:
            pass
        meta = folder.lstat()
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
            raise ValueError("unsafe native directory")
        folders[name] = folder
    # app-server preflight and exec must see the same empty configuration.
    config = folders["codex-home"] / "config.toml"
    if config.exists() or config.is_symlink():
        raise ValueError("unexpected native configuration")
    env = {key: value for key, value in os.environ.items() if key in {"PATH", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL", "TERM", "TZ"}}
    env.update({"HOME":str(folders["native-home"]), "CODEX_HOME":str(folders["codex-home"]), "XDG_CONFIG_HOME":str(folders["native-home"] / ".config"), "XDG_DATA_HOME":str(folders["native-home"] / ".local/share"), "XDG_CACHE_HOME":str(folders["native-home"] / ".cache"), "NO_PROXY":"127.0.0.1,localhost,::1"})
    return env


def load_profile():
    if sys.platform != "linux" or not hasattr(os, "O_NOFOLLOW"):
        raise ValueError("unsupported provider host")
    home = Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")
    source = home / "config.toml"
    descriptor = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_uid != os.getuid() or before.st_nlink != 1 or before.st_size > 1024 * 1024 or before.st_mode & 0o022:
            raise ValueError("unsafe configuration")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            raw = stream.read(1024 * 1024 + 1)
        after = os.fstat(descriptor)
        if len(raw) > 1024 * 1024 or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError("configuration changed")
    finally:
        os.close(descriptor)
    config = tomllib.loads(raw.decode("utf-8"))
    model, provider = config.get("model"), config.get("model_provider")
    if not isinstance(model, str) or not model.startswith("deepseek") or len(model) > 200 or not isinstance(provider, str) or not provider or len(provider) > 80 or not all(c.isascii() and (c.isalnum() or c in "_-") for c in provider):
        raise ValueError("unsupported model provider")
    entry = config.get("model_providers", {}).get(provider)
    if not isinstance(entry, dict):
        raise ValueError("provider missing")
    allowed = {"name", "base_url", "wire_api", "requires_openai_auth", "experimental_bearer_token", "env_key"}
    if set(entry) - allowed or entry.get("wire_api") != "responses" or entry.get("requires_openai_auth") is not False:
        raise ValueError("unsupported provider configuration")
    endpoint = entry.get("base_url")
    if not isinstance(endpoint, str) or len(endpoint) > 2048:
        raise ValueError("invalid endpoint")
    url = urlsplit(endpoint)
    if url.scheme not in ("http", "https") or url.hostname not in ("127.0.0.1", "localhost", "::1") or url.username or url.password or url.query or url.fragment:
        raise ValueError("unsupported endpoint")
    inline, env_key = entry.get("experimental_bearer_token"), entry.get("env_key")
    if bool(inline) == bool(env_key):
        raise ValueError("ambiguous credential source")
    if env_key and (not isinstance(env_key, str) or len(env_key) > 128 or not all(c.isascii() and (c.isalnum() or c == "_") for c in env_key)):
        raise ValueError("invalid credential source")
    token = inline if inline else os.environ.get(env_key)
    if not isinstance(token, str) or not token or len(token) > 32768 or any(ord(c) < 33 or ord(c) > 126 for c in token):
        raise ValueError("credential unavailable")
    binding = {"version": 1, "provider": provider, "model": model, "base_url": endpoint, "wire_api": "responses", "requires_openai_auth": False, "credential_source": "inline" if inline else "env", "env_key": env_key or None}
    fingerprint = "sha256:" + hashlib.sha256(json.dumps(binding, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return binding, fingerprint, token


def probe():
    binding, fingerprint, _ = load_profile()
    url = urlsplit(binding["base_url"])
    return {"model": binding["model"], "provider": binding["provider"], "provider_fingerprint": fingerprint, "endpoint_host": url.hostname, "wire_api": binding["wire_api"], "credential_available": True}


def execution(expected, arguments):
    binding, fingerprint, token = load_profile()
    if expected != fingerprint:
        raise ValueError("provider changed")
    if not isinstance(arguments, list) or len(arguments) > 128 or any(not isinstance(a, str) or len(a) > 65536 or "\0" in a for a in arguments):
        raise ValueError("invalid native arguments")
    if "exec" not in arguments or "--json" not in arguments or "--ignore-user-config" not in arguments or arguments.count("-m") != 1:
        raise ValueError("managed structured execution required")
    index = arguments.index("-m")
    if index + 1 >= len(arguments) or arguments[index + 1] != binding["model"]:
        raise ValueError("model mismatch")
    # Accept only the grammar emitted by hybrid.rs. A denylist misses quoted
    # TOML keys, whitespace, feature flags and attached option spellings.
    configs = set()
    position = 0
    allowed_configs = {"mcp_servers.velella_local", "hooks.SessionStart", "features.fast_mode", "features.apps", "features.multi_agent", "skills.config"}
    options = set()
    while position < len(arguments) and arguments[position] != "exec":
        option = arguments[position]
        if position + 1 >= len(arguments) or option not in ("-c", "-m", "-a", "-s"):
            raise ValueError("unsupported native option")
        value = arguments[position + 1]
        if option != "-c":
            if option in options:
                raise ValueError("duplicate native option")
            options.add(option)
        if option == "-c":
            if any(ord(c) < 32 for c in value):
                raise ValueError("multiline native configuration rejected")
            key, separator, _ = value.partition("=")
            if not separator or key not in allowed_configs or key in configs:
                raise ValueError("native configuration rejected")
            tomllib.loads(value)
            if key.startswith("features.") and value != key+"=false":
                raise ValueError("nonstandard execution mode")
            configs.add(key)
        elif option == "-a" and value != "on-request":
            raise ValueError("approval policy changed")
        elif option == "-s" and value != "workspace-write":
            raise ValueError("sandbox policy changed")
        position += 2
    if not {"features.fast_mode", "features.apps", "features.multi_agent", "skills.config", "mcp_servers.velella_local"}.issubset(configs) or options != {"-m", "-a", "-s"}:
        raise ValueError("managed role policy missing")
    tail = arguments[position:]
    if tail[:4] != ["exec", "--json", "--skip-git-repo-check", "--ignore-user-config"]:
        raise ValueError("managed execution flags required")
    tail = tail[4:]
    if len(tail) == 3 and tail[0] == "resume":
        import uuid
        uuid.UUID(tail[1])
        tail = tail[2:]
    if len(tail) != 1 or tail[0].startswith("-"):
        raise ValueError("invalid managed prompt")
    provider = {"name": "Velella bound DeepSeek", "base_url": binding["base_url"], "wire_api": "responses", "requires_openai_auth": False, "env_key": TOKEN_ENV}
    table = "{" + ",".join(key + "=" + ("false" if value is False else json.dumps(value)) for key, value in provider.items()) + "}"
    prefix = ["codex", "-c", "model_provider=" + json.dumps(binding["provider"]), "-c", "model_providers." + binding["provider"] + "=" + table, "-c", "shell_environment_policy.exclude=[" + json.dumps(TOKEN_ENV) + "]"]
    env = isolated_environment()
    env[TOKEN_ENV] = token
    return prefix + arguments, env


def main():
    if len(sys.argv) == 2 and sys.argv[1] == "probe":
        print(json.dumps(probe(), separators=(",", ":")))
    elif len(sys.argv) == 2 and sys.argv[1] == "policy":
        command = ["node", str(Path(__file__).resolve().parent / "skill-preflight.cjs")]
        os.execvpe(command[0], command, isolated_environment())
    elif len(sys.argv) == 4 and sys.argv[1] == "exec":
        arguments = json.loads(base64.b64decode(sys.argv[3], validate=True))
        command, env = execution(sys.argv[2], arguments)
        os.execvpe(command[0], command, env)
    else:
        raise ValueError("invalid provider mode")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("Remote Codex provider could not be safely bound; no fallback or replay was performed.", file=sys.stderr)
        raise SystemExit(1)
