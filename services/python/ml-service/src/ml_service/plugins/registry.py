"""Plugin registry: discovers built-ins and external plugins (PLUGIN_DIR), validates, and syncs ml.algorithms.

External plugins are Python files/packages exposing `PLUGINS = [...]`. They are hot-reloadable via
`POST /api/v1/ml/algorithms/reload`. Invalid plugins are recorded (status 'invalid') and never selectable.
"""

from __future__ import annotations

import importlib.util
import json
import re
import sys
import threading
import traceback
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from types import ModuleType
from typing import Any

from jsonschema import Draft202012Validator
from sqlalchemy import text

from ml_service.logging import get_logger
from ml_service.plugins.base import PLUGIN_BASES, AlgorithmPlugin, resolve_params, schema_defaults
from ml_service.plugins.builtin import BUILTIN_PLUGINS
from ml_service.plugins.smoke import smoke_test

log = get_logger(__name__)
NAME_RE = re.compile(r"^[a-z][a-z0-9_]{2,40}$")
EXTERNAL_PREFIX = "ml_plugin_ext_"


@dataclass
class AlgorithmEntry:
    name: str
    kind: str
    version: str
    display_name: str
    description: str
    param_schema: dict[str, Any]
    source: str  # builtin | plugin
    module: str
    status: str  # available | invalid | disabled
    error: str | None = None
    cls: type[AlgorithmPlugin] | None = None
    loaded_at: datetime = field(default_factory=lambda: datetime.now(UTC))

    def public(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "kind": self.kind,
            "version": self.version,
            "display_name": self.display_name,
            "description": self.description,
            "param_schema": self.param_schema,
            "source": self.source,
            "module": self.module,
            "status": self.status,
            "error": self.error,
            "defaults": schema_defaults(self.param_schema),
        }


@dataclass
class InvalidModule:
    module: str
    error: str


def validate_plugin_class(cls: Any, *, run_smoke: bool) -> None:
    """Raise ValueError describing the first contract violation."""
    if not isinstance(cls, type) or not issubclass(cls, AlgorithmPlugin):
        raise ValueError("not an AlgorithmPlugin subclass")
    for attr in ("name", "kind", "version", "display_name", "description", "param_schema"):
        if not hasattr(cls, attr):
            raise ValueError(f"missing class attribute '{attr}'")
    if not NAME_RE.match(cls.name):
        raise ValueError(f"invalid name '{cls.name}' (must match {NAME_RE.pattern})")
    base = PLUGIN_BASES.get(cls.kind)
    if base is None or not issubclass(cls, base):
        raise ValueError(f"kind '{cls.kind}' must subclass {base.__name__ if base else 'a known base'}")
    if getattr(cls, "__abstractmethods__", None):
        raise ValueError(f"abstract methods not implemented: {sorted(cls.__abstractmethods__)}")
    if not isinstance(cls.version, str) or not cls.version:
        raise ValueError("version must be a non-empty string")
    Draft202012Validator.check_schema(cls.param_schema)
    resolve_params(cls.param_schema, {})  # defaults must be valid
    if run_smoke:
        smoke_test(cls)


class PluginRegistry:
    def __init__(self, plugin_dir: Path | None, *, smoke_test_builtins: bool = False) -> None:
        self.plugin_dir = plugin_dir
        self.smoke_test_builtins = smoke_test_builtins
        self._entries: dict[str, AlgorithmEntry] = {}
        self._invalid_modules: list[InvalidModule] = []
        self._lock = threading.RLock()

    # ------------------------------------------------------------------ loading
    def load_all(self) -> None:
        with self._lock:
            entries: dict[str, AlgorithmEntry] = {}
            for cls in BUILTIN_PLUGINS:
                entries[cls.name] = self._entry_for(cls, "builtin", cls.__module__, self.smoke_test_builtins)
            self._invalid_modules = []
            for module_name, classes, error in self._discover_external():
                if error is not None:
                    self._invalid_modules.append(InvalidModule(module_name, error))
                    continue
                for cls in classes:
                    name = getattr(cls, "name", None)
                    if isinstance(name, str) and name in entries and entries[name].source == "builtin":
                        self._invalid_modules.append(
                            InvalidModule(module_name, f"plugin name '{name}' clashes with a built-in")
                        )
                        continue
                    if not isinstance(name, str) or not NAME_RE.match(name):
                        # cannot be stored in ml.algorithms (name CHECK) — report at module level only
                        self._invalid_modules.append(
                            InvalidModule(
                                module_name, f"invalid plugin name {name!r} (must match {NAME_RE.pattern})"
                            )
                        )
                        continue
                    entries[name] = self._entry_for(cls, "plugin", module_name, True)
            self._entries = entries
            log.info(
                "plugins_loaded",
                available=sum(e.status == "available" for e in entries.values()),
                invalid=sum(e.status == "invalid" for e in entries.values()) + len(self._invalid_modules),
            )

    @staticmethod
    def _entry_for(cls: Any, source: str, module: str, run_smoke: bool) -> AlgorithmEntry:
        try:
            validate_plugin_class(cls, run_smoke=run_smoke)
            status, error = "available", None
        except Exception as exc:  # plugin code is untrusted: capture everything
            status, error = "invalid", f"{type(exc).__name__}: {exc}"
            log.warning("plugin_invalid", module=module, plugin=getattr(cls, "name", repr(cls)), error=error)
        schema = getattr(cls, "param_schema", {})
        return AlgorithmEntry(
            name=str(getattr(cls, "name", repr(cls))),
            kind=str(getattr(cls, "kind", "unknown")),
            version=str(getattr(cls, "version", "0")),
            display_name=str(getattr(cls, "display_name", getattr(cls, "name", "?"))),
            description=str(getattr(cls, "description", "")),
            param_schema=schema if isinstance(schema, dict) else {},
            source=source,
            module=module,
            status=status,
            error=error,
            cls=cls if status == "available" else None,
        )

    def _discover_external(self) -> list[tuple[str, list[Any], str | None]]:
        results: list[tuple[str, list[Any], str | None]] = []
        if self.plugin_dir is None or not self.plugin_dir.is_dir():
            return results
        # drop previously imported external modules so edits are picked up
        for mod in [m for m in sys.modules if m.startswith(EXTERNAL_PREFIX)]:
            del sys.modules[mod]
        for path in sorted(self.plugin_dir.iterdir()):
            if path.name.startswith(("_", ".")):
                continue
            if path.is_file() and path.suffix == ".py":
                target = path
            elif path.is_dir() and (path / "__init__.py").is_file():
                target = path / "__init__.py"
            else:
                continue
            module_name = EXTERNAL_PREFIX + re.sub(r"\W", "_", path.stem)
            try:
                module = self._import(module_name, target, is_package=path.is_dir())
                plugins = getattr(module, "PLUGINS", None)
                if not isinstance(plugins, list | tuple) or not plugins:
                    raise ValueError("module must define a non-empty PLUGINS list")
                results.append((path.name, list(plugins), None))
            except Exception as exc:
                results.append(
                    (path.name, [], f"{type(exc).__name__}: {exc}\n{traceback.format_exc(limit=3)}")
                )
        return results

    @staticmethod
    def _import(module_name: str, target: Path, *, is_package: bool) -> ModuleType:
        spec = importlib.util.spec_from_file_location(
            module_name, target, submodule_search_locations=[str(target.parent)] if is_package else None
        )
        if spec is None or spec.loader is None:
            raise ImportError(f"cannot load {target}")
        module = importlib.util.module_from_spec(spec)
        sys.modules[module_name] = module
        spec.loader.exec_module(module)
        return module

    # ------------------------------------------------------------------ queries
    def entries(self) -> list[AlgorithmEntry]:
        with self._lock:
            return sorted(self._entries.values(), key=lambda e: (e.kind, e.source != "builtin", e.name))

    def invalid_modules(self) -> list[InvalidModule]:
        with self._lock:
            return list(self._invalid_modules)

    def get(self, name: str) -> AlgorithmEntry | None:
        with self._lock:
            return self._entries.get(name)

    def plugin_class(self, name: str, kind: str) -> type[AlgorithmPlugin]:
        entry = self.get(name)
        if entry is None:
            raise KeyError(f"unknown algorithm '{name}'")
        if entry.kind != kind:
            raise ValueError(f"algorithm '{name}' is {entry.kind}, expected {kind}")
        if entry.status != "available" or entry.cls is None:
            raise ValueError(f"algorithm '{name}' is {entry.status}")
        return entry.cls

    def mark_disabled(self, names: set[str]) -> None:
        with self._lock:
            for name in names:
                entry = self._entries.get(name)
                if entry and entry.status == "available":
                    entry.status = "disabled"

    # ------------------------------------------------------------------ persistence
    def sync_to_db(self) -> None:
        """Upsert ml.algorithms. Rows an operator disabled stay disabled."""
        from ml_service.db import platform_session

        with platform_session() as conn:
            disabled = {
                r[0] for r in conn.execute(text("SELECT name FROM ml.algorithms WHERE status = 'disabled'"))
            }
            self.mark_disabled(disabled)
            for e in self.entries():
                conn.execute(
                    text(
                        """
                        INSERT INTO ml.algorithms
                            (name, kind, version, display_name, description, param_schema, source, module,
                             status, error, loaded_at)
                        VALUES (:name, :kind, :version, :display_name, :description, CAST(:schema AS jsonb),
                                :source, :module, :status, :error, now())
                        ON CONFLICT (name) DO UPDATE SET
                            kind = EXCLUDED.kind, version = EXCLUDED.version,
                            display_name = EXCLUDED.display_name, description = EXCLUDED.description,
                            param_schema = EXCLUDED.param_schema, source = EXCLUDED.source,
                            module = EXCLUDED.module,
                            status = CASE WHEN ml.algorithms.status = 'disabled' THEN 'disabled'
                                          ELSE EXCLUDED.status END,
                            error = EXCLUDED.error, loaded_at = now()
                        """
                    ),
                    {
                        "name": e.name,
                        "kind": e.kind if e.kind in PLUGIN_BASES else "supervised",
                        "version": e.version,
                        "display_name": e.display_name,
                        "description": e.description,
                        "schema": json.dumps(e.param_schema),
                        "source": e.source,
                        "module": e.module,
                        "status": e.status,
                        "error": e.error,
                    },
                )


_registry: PluginRegistry | None = None


def get_registry() -> PluginRegistry:
    global _registry
    if _registry is None:
        from ml_service.config import get_settings

        settings = get_settings()
        _registry = PluginRegistry(settings.plugin_dir, smoke_test_builtins=settings.smoke_test_builtins)
        _registry.load_all()
    return _registry


def set_registry(registry: PluginRegistry) -> None:
    global _registry
    _registry = registry
