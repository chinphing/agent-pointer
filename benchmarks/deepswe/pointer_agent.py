"""Pier adapter that runs the headless ``pointer-cli`` inside a DeepSWE task.

``pointer-cli`` ships as a statically linked musl binary
(``crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz``). DeepSWE task images
are prebuilt and run the agent with ``network_mode = "no-network"``, so the
adapter:

* declares **no** ``install_spec()`` — Pier keeps the prebuilt image instead of
  rebuilding it with an agent layer;
* uploads the archive with ``environment.upload_file`` and installs it offline
  (no ``curl``/``wget`` inside the task container);
* declares a ``network_allowlist()`` derived from the inference ``base_url`` so
  Pier's filtered-egress proxy lets the model call through;
* drives one ``pointer-cli run`` per trial through ``exec_as_agent`` so the
  proxy variables Pier injects in ``agent_process_env`` reach the process.

Usage::

    PYTHONPATH=benchmarks pier run \\
      -p deep-swe/tasks \\
      --agent-import-path deepswe.pointer_agent:PointerAgent \\
      -m openai/gpt-5.5

Everything the adapter builds (install command, run command, credential and
network resolution, exit-code handling) lives in module-level pure helpers so it
can be unit tested without a container.
"""

from __future__ import annotations

import json
import shlex
import tempfile
from pathlib import Path

from pier.agents.installed.base import (
    BaseInstalledAgent,
    CliFlag,
    with_prompt_template,
)
from pier.agents.network import allowlist_from_urls
from pier.environments.base import BaseEnvironment
from pier.models.agent.context import AgentContext
from pier.models.agent.install import AgentInstallSpec
from pier.models.agent.network import NetworkAllowlist
from pier.models.trial.paths import EnvironmentPaths

#: Agent name reported in trial metadata.
AGENT_NAME = "pointer-cli"

#: Container paths. ``/installed-agent`` is created by ``BaseInstalledAgent.setup``.
INSTALL_ROOT = "/installed-agent"
REMOTE_TARBALL = f"{INSTALL_ROOT}/pointer-cli-linux-x64.tar.gz"
REMOTE_INSTALL_DIR = f"{INSTALL_ROOT}/pointer-cli-linux-x64"
REMOTE_INSTRUCTION = f"{INSTALL_ROOT}/pointer-instruction.txt"
REMOTE_BINARY = "/usr/local/bin/pointer-cli"

#: Host-side default archive location, relative to the repository root.
DEFAULT_DIST_TARBALL = "crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz"

#: ``pointer-cli run`` defaults applied by the adapter. ``$PWD`` stays a live
#: shell expansion so the run targets the directory Pier executes in (the task
#: workdir), never the host cwd.
DEFAULT_WORKSPACE = "$PWD"
DEFAULT_APPROVAL = "auto"

#: Tool ids from ``pointer-core`` that are meaningful for a headless benchmark
#: run. ``task_board_*`` / ``run_subagent`` / GUI tools stay disabled.
DEFAULT_ALLOWED_TOOLS = (
    "terminal,file_read,file_write,file_edit,file_grep,file_glob,file_list"
)

#: Wall-clock budget handed to one ``pointer-cli run``. Kept above the 600s CLI
#: default so long SWE tasks are not truncated by the agent's own deadline;
#: Pier's ``[agent] timeout_sec`` still governs the upper bound.
DEFAULT_TIMEOUT_SEC = 3600

#: Extra seconds added to the exec-level timeout so ``pointer-cli``'s own
#: ``--timeout`` fires first and writes its artifacts before Pier kills the
#: command.
EXEC_TIMEOUT_GRACE_SEC = 120

#: Commit message for the best-effort finalize step. The verifier collects
#: ``git diff --binary <base_commit> HEAD``, so leftover uncommitted work would
#: be lost when the agent dies before committing its own changes.
FINALIZE_COMMIT_MESSAGE = "pointer-cli: finalize work"

#: Environment variable carrying the provider key into the container.
#: ``pointer-cli`` reads the key only from the variable named by
#: ``--api-key-env``, so the adapter always names it explicitly.
POINTER_API_KEY_ENV = "POINTER_API_KEY"

#: pointer-cli exit codes (``crates/pointer-cli/src/main.rs``).
EXIT_SUCCESS = 0
EXIT_USAGE = 2
EXIT_TIMEOUT = 10
EXIT_AUTH = 11
EXIT_MODEL = 12
EXIT_INTERNAL = 13
EXIT_TURN_LIMIT = 14

#: Exit codes meaning "the agent ran, but ran out of budget". These are agent
#: outcomes rather than infrastructure failures, so the trial keeps going and
#: the verifier decides the reward.
TOLERATED_EXIT_CODES = frozenset({EXIT_TIMEOUT, EXIT_TURN_LIMIT})

#: Default inference host when the user does not override ``base_url``.
DEFAULT_INFERENCE_DOMAIN = "api.openai.com"

#: Host env vars consulted (in order) when resolving the provider API key.
PROVIDER_KEY_ENV_NAMES = ("POINTER_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY")

#: ``usage.json`` field mapping (pointer-cli totals -> Pier AgentContext).
_USAGE_FIELD_MAP = (
    ("prompt_tokens", "n_input_tokens"),
    ("cached_tokens", "n_cache_tokens"),
    ("completion_tokens", "n_output_tokens"),
)


# ---------------------------------------------------------------------------
# Pure helpers (unit-testable without a container)
# ---------------------------------------------------------------------------


def split_model_name(model_name: str) -> tuple[str | None, str]:
    """Split Pier's ``provider/model`` into ``(provider, model id)``.

    ``pointer-cli`` talks to an OpenAI-compatible endpoint and only needs the
    bare model id, so the provider prefix is dropped.
    """
    if "/" in model_name:
        provider, model = model_name.split("/", 1)
        return provider or None, model
    return None, model_name


def find_repo_root(start: Path) -> Path | None:
    """Walk up from *start* until the Pointer repository root is found."""
    for candidate in (start, *start.parents):
        if (candidate / "crates" / "pointer-cli" / "Cargo.toml").is_file():
            return candidate
    return None


def resolve_dist_tarball(
    configured: str | None,
    *,
    module_file: Path | None = None,
) -> Path:
    """Resolve the host path of the ``pointer-cli`` distribution archive.

    ``configured`` wins when set; relative values resolve against the
    repository root. Otherwise the default ``dist/`` build is used.
    """
    anchor = (module_file or Path(__file__)).resolve()
    if configured:
        path = Path(configured).expanduser()
        if path.is_absolute():
            return path
        root = find_repo_root(anchor) or Path.cwd()
        return root / path
    root = find_repo_root(anchor)
    if root is None:
        return Path(DEFAULT_DIST_TARBALL)
    return root / DEFAULT_DIST_TARBALL


def build_install_command(
    *,
    tarball: str = REMOTE_TARBALL,
    install_dir: str = REMOTE_INSTALL_DIR,
    bindir: str = "/usr/local/bin",
    binary: str = REMOTE_BINARY,
) -> str:
    """Build the root-side install command (unpack + offline installer)."""
    return " && ".join(
        (
            f"rm -rf {shlex.quote(install_dir)}",
            f"mkdir -p {shlex.quote(install_dir)}",
            f"tar -xzf {shlex.quote(tarball)} -C {shlex.quote(install_dir)} "
            "--strip-components=1",
            f"sh {shlex.quote(f'{install_dir}/install.sh')} --bindir "
            f"{shlex.quote(bindir)}",
            f"{shlex.quote(binary)} --version",
        )
    )


def shell_workspace(workspace: str) -> str:
    """Render ``--workspace`` as a shell token.

    ``$PWD`` stays a live expansion (see ``DEFAULT_WORKSPACE``); any other
    value is quoted.
    """
    if workspace == DEFAULT_WORKSPACE:
        return '"$PWD"'
    return shlex.quote(workspace)


def build_run_command(
    *,
    model: str,
    api_key_env: str,
    out_dir: str,
    workspace: str = DEFAULT_WORKSPACE,
    instruction_path: str = REMOTE_INSTRUCTION,
    base_url: str | None = None,
    allowed_tools: str | None = None,
    approval: str | None = None,
    timeout_sec: int | None = None,
    max_turns: int | None = None,
    prompt_profile: str | None = None,
    extra_flags: str = "",
    binary: str = REMOTE_BINARY,
) -> str:
    """Build the agent-side ``pointer-cli run`` command.

    The instruction travels as a file (``--instruction-file``) so prompts with
    quotes, newlines, or CJK survive shell transport unchanged.
    """
    parts = [
        shlex.quote(binary),
        "run",
        f"--instruction-file {shlex.quote(instruction_path)}",
        f"--workspace {shell_workspace(workspace)}",
        f"--model {shlex.quote(model)}",
        f"--api-key-env {shlex.quote(api_key_env)}",
        f"--out-dir {shlex.quote(out_dir)}",
    ]
    if base_url:
        parts.append(f"--base-url {shlex.quote(base_url)}")
    if approval:
        parts.append(f"--approval {shlex.quote(approval)}")
    if allowed_tools:
        parts.append(f"--allowed-tools {shlex.quote(allowed_tools)}")
    if timeout_sec is not None:
        parts.append(f"--timeout {int(timeout_sec)}")
    if max_turns is not None:
        parts.append(f"--max-turns {int(max_turns)}")
    if prompt_profile:
        parts.append(f"--prompt-profile {shlex.quote(str(prompt_profile))}")
    if extra_flags.strip():
        parts.append(extra_flags.strip())
    return " ".join(parts)


def build_tolerant_wrapper(
    command: str,
    *,
    tolerated: frozenset[int] = TOLERATED_EXIT_CODES,
) -> str:
    """Wrap *command* so budget-exhaustion exits do not fail the trial.

    ``pointer-cli`` returns ``10`` (timeout) / ``14`` (tool-round limit) after
    writing its artifacts. Pier's ``_exec`` treats any non-zero exit as a
    failure, so the wrapper re-maps only those codes to ``0`` while every real
    failure (usage/auth/model/internal) propagates unchanged.
    """
    codes = "|".join(str(code) for code in sorted({EXIT_SUCCESS, *tolerated}))
    return (
        f"{command}\n"
        "pointer_cli_rc=$?\n"
        f"case $pointer_cli_rc in {codes}) exit 0;; esac\n"
        "exit $pointer_cli_rc"
    )


def build_finalize_command(
    *,
    workspace: str = DEFAULT_WORKSPACE,
    message: str = FINALIZE_COMMIT_MESSAGE,
) -> str:
    """Build the best-effort "commit whatever the agent left" command.

    The verifier collects ``git diff --binary <base_commit> HEAD``, so work the
    agent never committed is invisible — an agent that dies mid-run (exit 13,
    truncated output) leaves a 0-byte patch and every f2p test fails. This
    stages and commits any leftover changes right before ``run()`` returns.

    Every step is guarded: a clean tree, a missing ``git``, or a non-repository
    workspace exits ``0`` without touching anything, and the final ``exit 0``
    keeps Pier's ``_exec`` from treating the helper as a trial failure.
    """
    return (
        f"cd {shell_workspace(workspace)} 2>/dev/null || exit 0\n"
        "git rev-parse --git-dir >/dev/null 2>&1 || exit 0\n"
        "git add -A 2>/dev/null || exit 0\n"
        "git diff --cached --quiet && exit 0\n"
        "git -c user.name='pointer-cli' -c user.email='pointer-cli@localhost' "
        f"commit -q --no-verify -m {shlex.quote(message)} >/dev/null 2>&1 || true\n"
        "exit 0"
    )


def resolve_credentials(env_lookup) -> tuple[str, dict[str, str]]:
    """Return ``(api_key_env_name, env)`` for the provider API key.

    *env_lookup* is a ``str -> str | None`` callable (usually
    ``BaseInstalledAgent._get_env``). The key is normalised into
    ``POINTER_API_KEY`` so ``--api-key-env`` never leaks a provider-specific
    variable name into the command.
    """
    for name in PROVIDER_KEY_ENV_NAMES:
        value = env_lookup(name)
        if value:
            return POINTER_API_KEY_ENV, {POINTER_API_KEY_ENV: value}
    raise ValueError(
        "No API key found. Export one of "
        f"{', '.join(PROVIDER_KEY_ENV_NAMES)} or pass --ae POINTER_API_KEY=..."
    )


def usage_context_fields(document: object) -> dict[str, int]:
    """Map a parsed ``usage.json`` document to ``AgentContext`` field values."""
    if not isinstance(document, dict):
        return {}
    totals = document.get("totals")
    if not isinstance(totals, dict):
        return {}
    fields: dict[str, int] = {}
    for source, target in _USAGE_FIELD_MAP:
        value = totals.get(source)
        if isinstance(value, int) and not isinstance(value, bool):
            fields[target] = value
    return fields


def network_allowlist_for(base_urls, *, default_domains=(DEFAULT_INFERENCE_DOMAIN,)):
    """Build a :class:`NetworkAllowlist` from the configured inference URLs."""
    return allowlist_from_urls(base_urls, default_domains=default_domains)


# ---------------------------------------------------------------------------
# Agent
# ---------------------------------------------------------------------------


class PointerAgent(BaseInstalledAgent):
    """Install and run ``pointer-cli`` inside a Pier task container."""

    SUPPORTS_ATIF: bool = True

    CLI_FLAGS = [
        CliFlag(
            "approval",
            cli="--approval",
            type="str",
            default=DEFAULT_APPROVAL,
        ),
        CliFlag(
            "allowed_tools",
            cli="--allowed-tools",
            type="str",
            default=DEFAULT_ALLOWED_TOOLS,
        ),
        CliFlag(
            "timeout_sec",
            cli="--timeout",
            type="int",
            default=DEFAULT_TIMEOUT_SEC,
        ),
        CliFlag(
            "max_turns",
            cli="--max-turns",
            type="int",
            default=None,
        ),
        CliFlag(
            "prompt_profile",
            cli="--prompt-profile",
            type="str",
            default=None,
        ),
        CliFlag(
            "base_url",
            cli="--base-url",
            type="str",
            default=None,
            env_fallback="POINTER_BASE_URL",
        ),
    ]

    def __init__(
        self,
        *args,
        dist_tarball: str | None = None,
        workspace: str = DEFAULT_WORKSPACE,
        **kwargs,
    ):
        self._dist_tarball_option = dist_tarball
        self._workspace = workspace
        super().__init__(*args, **kwargs)

    @staticmethod
    def name() -> str:
        return AGENT_NAME

    def get_version_command(self) -> str | None:
        return f"{REMOTE_BINARY} --version"

    def parse_version(self, stdout: str) -> str:
        text = stdout.strip()
        for line in text.splitlines():
            line = line.strip()
            if line:
                return line.removeprefix("pointer-cli").strip()
        return text

    # -- install ----------------------------------------------------------

    def install_spec(self) -> AgentInstallSpec | None:
        """Return ``None`` so Pier keeps the task's prebuilt image.

        DeepSWE ships a ready Docker image per task; declaring an install spec
        would force Pier to rebuild that image with an agent layer. Returning
        ``None`` marks the agent as not-preinstalled, so :meth:`setup` calls
        :meth:`install`, which uploads the archive instead.
        """
        return None

    def network_allowlist(self) -> NetworkAllowlist:
        """Allow the inference host(s) through Pier's filtered-egress proxy."""
        urls: list[str] = []
        if base_url := self._resolved_flag("base_url"):
            urls.append(str(base_url))
        for key in ("POINTER_BASE_URL", "OPENAI_BASE_URL", "OPENAI_API_BASE"):
            if value := self._get_env(key):
                urls.append(value)
        return network_allowlist_for(urls)

    async def install(self, environment: BaseEnvironment) -> None:
        tarball = self._dist_tarball()
        if not tarball.is_file():
            raise FileNotFoundError(
                f"pointer-cli distribution archive not found: {tarball}. Build it "
                "with crates/pointer-cli/packaging/build-dist.sh, or pass "
                "--ak dist_tarball=/path/to/pointer-cli-linux-x64.tar.gz."
            )

        await environment.upload_file(tarball, REMOTE_TARBALL)
        await self.exec_as_root(
            environment,
            command=build_install_command(),
            timeout_sec=self._exec_timeout_sec(),
        )

    # -- options ----------------------------------------------------------

    def _dist_tarball(self) -> Path:
        return resolve_dist_tarball(self._dist_tarball_option)

    def _resolved_flag(self, kwarg: str):
        return self._resolved_flags.get(kwarg)

    def _pointer_timeout_sec(self) -> int | None:
        value = self._resolved_flag("timeout_sec")
        return int(value) if value is not None else None

    def _exec_timeout_sec(self) -> int | None:
        pointer_timeout = self._pointer_timeout_sec()
        if pointer_timeout is None:
            return None
        return pointer_timeout + EXEC_TIMEOUT_GRACE_SEC

    def _resolve_base_url(self) -> str | None:
        if base_url := self._resolved_flag("base_url"):
            return str(base_url)
        for key in ("POINTER_BASE_URL", "OPENAI_BASE_URL", "OPENAI_API_BASE"):
            if value := self._get_env(key):
                return value
        return None

    def _resolve_model_id(self) -> str:
        if not self.model_name:
            raise ValueError("pointer-cli requires a model. Pass -m <provider>/<model>.")
        _, model = split_model_name(self.model_name)
        if not model:
            raise ValueError(f"Cannot derive a model id from {self.model_name!r}")
        return model

    # -- run --------------------------------------------------------------

    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        model = self._resolve_model_id()
        api_key_env, credentials = resolve_credentials(self._get_env)
        env = self.build_process_env(credentials)

        # Upload the instruction as a file: pointer-cli reads it verbatim, so
        # no shell quoting can corrupt the prompt.
        with tempfile.TemporaryDirectory(prefix="pointer-cli-instruction-") as tmp:
            local_instruction = Path(tmp) / "instruction.txt"
            local_instruction.write_text(instruction, encoding="utf-8")
            await environment.upload_file(local_instruction, REMOTE_INSTRUCTION)

        command = build_run_command(
            model=model,
            api_key_env=api_key_env,
            out_dir=str(EnvironmentPaths.agent_dir),
            workspace=self._workspace,
            base_url=self._resolve_base_url(),
            allowed_tools=self._resolved_flag("allowed_tools"),
            approval=self._resolved_flag("approval"),
            timeout_sec=self._pointer_timeout_sec(),
            max_turns=self._resolved_flag("max_turns"),
            prompt_profile=self._resolved_flag("prompt_profile"),
        )

        await self.exec_as_agent(
            environment,
            command=build_tolerant_wrapper(command),
            env=env,
            timeout_sec=self._exec_timeout_sec(),
        )

        await self._finalize_workspace(environment)

    async def _finalize_workspace(self, environment: BaseEnvironment) -> None:
        """Best-effort commit of whatever the agent left behind.

        Runs through ``exec_as_agent`` so Pier's process env is injected the
        same way as the agent command. Never raises: finalizing is a safety net
        for a crashed agent, so a failure here must not fail the trial.
        """
        try:
            await self.exec_as_agent(
                environment,
                command=build_finalize_command(workspace=self._workspace),
            )
        except Exception:  # noqa: BLE001 - best-effort, never fail the trial
            self.logger.warning(
                "pointer-cli finalize commit failed; trial continues",
                exc_info=True,
            )

    # -- post run ---------------------------------------------------------

    def populate_context_post_run(self, context: AgentContext) -> None:
        """Backfill token usage from pointer-cli's ``usage.json``."""
        usage_path = self.logs_dir / "usage.json"
        try:
            document = json.loads(usage_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return
        for target, value in usage_context_fields(document).items():
            setattr(context, target, value)
