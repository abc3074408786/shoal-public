# /// script
# requires-python = ">=3.11"
# ///
"""Check local BMAD runtime compatibility using the pinned setup's own rules."""
import argparse
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", type=Path, required=True)
    args = parser.parse_args()
    project = args.project_root.resolve()
    package = Path(__file__).resolve().parent.parent
    skills = package / "assets" / "bmad" / "skills"
    hub = skills / "bmad"
    bmad = project / "_bmad"
    if not project.is_dir() or not bmad.is_dir() or bmad.is_symlink():
        print(json.dumps({"ready": False, "reason": "project_setup_missing_or_not_plain", "project": str(project)}))
        return 2
    sys.path.insert(0, str(hub / "scripts"))
    import setup as upstream
    from config_utils import load_central_config

    try:
        load_central_config(project)
        installation = upstream.discover_installation(hub, roots=(skills,))
        checks = {"shared": upstream.tree_state(bmad / "scripts", upstream.shared_scripts_tree(hub / "scripts"))}
        versions = {}
        for module in installation.modules:
            expected = upstream.declared_scripts_tree(upstream.read_module_scripts(module))
            checks[module.module] = upstream.tree_state(bmad / module.module / "scripts", expected)
            versions[module.module] = module.parsed.version
        ready = bool(versions) and all(state == "current" for state in checks.values())
        print(json.dumps({"ready": ready, "project": str(project), "checks": checks,
                          "bundle_versions": versions, "project_scripts_executed": False,
                          "note": "read-only local comparison; no remote update check"}))
        return 0 if ready else 2
    except Exception as error:
        print(json.dumps({"ready": False, "reason": "invalid_setup", "error": str(error)}))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
