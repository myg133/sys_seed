#!/usr/bin/env python3
"""Zero-dependency checks for the example bundle.

This is a smoke check, not a JSON Schema validator. It verifies the
constraints that matter for review: required fields, enums, capability
dependencies, permission code shape, and admin/config structure.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
errors = []

CAPS = {
    "manifest", "config", "admin-ui", "events", "audit.management",
    "audit.business", "identity", "authorization", "proxy.http", "observability",
}
PERM_RE = re.compile(r"^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$")
KEY_RE = re.compile(r"^app\.[a-z][a-z0-9-]+\.[a-zA-Z][a-zA-Z0-9]*(\.[a-zA-Z][a-zA-Z0-9]*)*$")


def err(msg):
    errors.append(msg)


def check_manifest(m):
    for f in ("id", "name", "version", "capabilities", "permissions", "menus", "runtime"):
        if f not in m:
            err(f"manifest: missing {f}")
    if not re.match(r"^[a-z][a-z0-9-]*$", m.get("id", "")):
        err("manifest: id must match ^[a-z][a-z0-9-]*$")

    caps = m.get("capabilities", [])
    if not isinstance(caps, list):
        err("manifest: capabilities must be an array")
    else:
        unknown = set(caps) - CAPS
        if unknown:
            err(f"manifest: unknown capabilities {sorted(unknown)}")
        for required in ("manifest", "config"):
            if required not in caps:
                err(f"manifest: capabilities must include {required}")
        if "admin-ui" in caps and "config" not in caps:
            err("manifest: admin-ui requires config")
        if "authorization" in caps and "identity" not in caps:
            err("manifest: authorization requires identity")
        if "proxy.http" in caps and "identity" not in caps:
            err("manifest: proxy.http requires identity")

    for p in m.get("permissions", []):
        if not PERM_RE.match(p.get("code", "")):
            err(f"permission: bad code {p.get('code')!r} (must be module:resource:action)")
        if p.get("type") not in ("menu", "button", "field"):
            err(f"permission {p.get('code')}: bad type")

    rt = m.get("runtime", {})
    if rt.get("kind") not in ("remote", "in-process"):
        err("runtime: kind must be remote or in-process")
    if not re.match(r"^\d+\.\d+$", rt.get("contract", "")):
        err("runtime: contract must look like 1.0")
    if rt.get("reachability") not in ("bidirectional", "outbound-only"):
        err("runtime: reachability invalid")
    if rt.get("kind") == "remote" and not rt.get("baseUrl"):
        err("runtime: baseUrl required for remote")

    for key, d in (m.get("config") or {}).items():
        if not KEY_RE.match(key):
            err(f"config key: bad shape {key!r}")
        for f in ("type", "labelKey", "default", "scopes"):
            if f not in d:
                err(f"config {key}: missing {f}")
        if d.get("type") == "enum" and not d.get("options"):
            err(f"config {key}: enum requires options")
        if d.get("type") == "enum":
            for opt in d.get("options", []):
                if "labelKey" not in opt:
                    err(f"config {key}: enum option missing labelKey")

    admin = m.get("admin")
    if admin:
        if "version" not in admin or "pages" not in admin:
            err("admin: missing version/pages")
        for page in admin.get("pages", []):
            for f in ("id", "placement", "title", "sections"):
                if f not in page:
                    err(f"admin page: missing {f}")
            if page.get("placement") not in ("app", "profile"):
                err("admin page: placement must be app or profile")
            for sec in page.get("sections", []):
                comp = sec.get("component")
                if comp not in ("form", "table", "stats", "actions", "tabs", "section"):
                    err(f"admin section: unknown component {comp!r}")
                if comp == "form":
                    for f in ("title", "source", "fields"):
                        if f not in sec:
                            err("admin form: missing " + f)
                    for field in sec.get("fields", []):
                        for f in ("key", "label", "type"):
                            if f not in field:
                                err(f"admin form field: missing {f}")


def main():
    manifest = json.load(open(ROOT / "examples/manifest.aibridge.json"))
    check_manifest(manifest)

    for name in ("audit-event.json", "problem.json"):
        json.load(open(ROOT / "examples" / name))

    if errors:
        print("FAIL")
        for e in errors:
            print(" -", e)
        return 1
    print("OK: examples conform to the checked constraints")
    return 0


if __name__ == "__main__":
    sys.exit(main())
