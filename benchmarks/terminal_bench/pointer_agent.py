"""Harbor adapter that runs the headless ``pointer-cli`` inside a task container.

``pointer-cli`` ships as a statically linked musl binary
(``crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz``). The adapter uploads
that archive with Harbor's ``upload_file`` — the container never needs network
access to install the agent — unpacks it, and drives one ``pointer-cli run``
invocation per trial.

Usage::

    PYTHONPATH=benchmarks harbor run \
      -a terminal_bench.pointer_agent:PointerAgent \
      -m openai/gpt-4o-mini \
      --ae OPENAI_API_KEY \
      -d terminal-bench@2.1

Everything the adapter builds (install command, run command, credential
resolution) lives in module-level pure helpers so it can be unit tested without
a container.
"""

from __future__ import annotations

import json
import shlex
import tempfile
from pathlib import Path
from typing import Annotated, override

from pydantic import Field

from harbor.agents.capabilities import AgentCapabilities
from harbor.agents.installed.base import (
    BaseInstalledAgent,
    with_prompt_template,
)
from harbor.agents.model_connection import ModelConnectionSpec
from harbor.agents.options import Cli, InstalledAgentOptions
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.trial.paths import EnvironmentPaths

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

#: ``pointer-cli run`` defaults applied by the adapter. ``$PWD`` is kept as a
#: live shell expansion so the run targets the directory Harbor executes in
#: (the task workdir), never the host cwd.
DEFAULT_WORKSPACE = "$PWD"
DEFAULT_APPROVAL = "auto"

#: Tool ids from ``pointer-core`` that are meaningful for a headless benchmark
#: run. ``task_board_*`` / ``run_subagent`` / GUI tools stay disabled.
DEFAULT_ALLOWED_TOOLS = (
    "terminal,file_read,file_write,file_edit,file_grep,file_glob,file_list"
)

#: Wall-clock budget handed to one ``pointer-cli run``. Kept above the 600s CLI
#: default so long SWE tasks are not truncated by the agent's own deadline;
#: Harbor's trial timeout still governs the upper bound.
DEFAULT_TIMEOUT_SEC = 3600

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

#: ``usage.json`` field mapping (pointer-cli totals -> Harbor AgentContext).
_USAGE_FIELD_MAP = (
    ("prompt_tokens", "n_input_tokens"),
    ("cached_tokens", "n_cache_tokens"),
    ("completion_tokens", "n_output_tokens"),
)


# ---------------------------------------------------------------------------
# Pure helpers (unit-testable without a container)
# ---------------------------------------------------------------------------


def split_model_name(model_name: str) -> tuple[str | None, str]:
    """Split Harbor's ``provider/model`` into ``(provider, model id)``.

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
    if extra_flags.strip():
        parts.append(extra_flags.strip())
    return " ".join(parts)


# ---------------------------------------------------------------------------
# Options
# ---------------------------------------------------------------------------


class PointerCliOptions(InstalledAgentOptions):
    """User-facing ``--ak`` kwargs accepted by :class:`PointerAgent`."""

    dist_tarball: str | None = Field(
        default=None,
        description=(
            "Host path to pointer-cli-linux-x64.tar.gz. Relative paths resolve "
            "against the repository root; defaults to the crates/pointer-cli "
            "dist build."
        ),
    )
    workspace: str = Field(
        default=DEFAULT_WORKSPACE,
        description=(
            "Workspace root for pointer-cli's file/terminal tools. The default "
            "'$PWD' expands to the directory Harbor executes in."
        ),
    )
    approval: Annotated[str, Cli("--approval")] = Field(
        default=DEFAULT_APPROVAL,
        description="Tool approval mode; 'auto' is the unattended default.",
    )
    allowed_tools: Annotated[str, Cli("--allowed-tools")] = Field(
        default=DEFAULT_ALLOWED_TOOLS,
        description="Comma-separated tool allow list for the run.",
    )
    timeout_sec: Annotated[int, Cli("--timeout")] = Field(
        default=DEFAULT_TIMEOUT_SEC,
        description="Wall-clock budget in seconds for one pointer-cli run.",
    )
    max_turns: Annotated[int | None, Cli("--max-turns")] = Field(
        default=None,
        description="Optional hard cap on pointer-cli tool-call rounds.",
    )
    base_url: Annotated[str | None, Cli("--base-url")] = Field(
        default=None,
        description=(
            "Override the OpenAI-compatible endpoint. Defaults to the "
            "endpoint Harbor resolves for the selected model."
        ),
    )


# ---------------------------------------------------------------------------
# Agent
# ---------------------------------------------------------------------------


class PointerAgent(BaseInstalledAgent):
    """Install and run ``pointer-cli`` inside a Harbor task container."""

    capabilities = AgentCapabilities(atif=True)
    MODEL_CONNECTION = ModelConnectionSpec(passthrough=True)
    options_model = PointerCliOptions

    @staticmethod
    @override
    def name() -> str:
        return AGENT_NAME

    @override
    def get_version_command(self) -> str | None:
        return f"{REMOTE_BINARY} --version"

    @override
    def parse_version(self, stdout: str) -> str:
        text = stdout.strip()
        for line in text.splitlines():
            line = line.strip()
            if line:
                return line.removeprefix("pointer-cli").strip()
        return text

    # -- options ----------------------------------------------------------

    @property
    def _options(self) -> PointerCliOptions:
        options = self.options
        if not isinstance(options, PointerCliOptions):
            raise ValueError("PointerAgent requires PointerCliOptions")
        return options

    def _dist_tarball(self) -> Path:
        return resolve_dist_tarball(self._options.dist_tarball)

    # -- install ----------------------------------------------------------

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        # ``tar`` is the only container tool the install needs: the archive is
        # uploaded by Harbor, so no curl/wget on the target image.
        await self.ensure_system_dependencies(environment, ("tar",))

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
            timeout_sec=self._options.timeout_sec,
        )

    # -- run --------------------------------------------------------------

    def _resolve_model_id(self) -> str:
        if not self.model_name:
            raise ValueError(
                "pointer-cli requires a model. Pass -m <provider>/<model>."
            )
        _, model = split_model_name(self.model_name)
        if not model:
            raise ValueError(f"Cannot derive a model id from {self.model_name!r}")
        return model

    def _resolve_credentials(self) -> tuple[str, dict[str, str]]:
        """Return ``(env var name, extra env)`` for the provider API key."""
        access = self.model_connection
        if access.api_key:
            return POINTER_API_KEY_ENV, {POINTER_API_KEY_ENV: access.api_key}

        # Fall back to a provider key Harbor already made available.
        for name in self._provider_key_envs():
            value = self._get_env(name)
            if value:
                return name, {}

        raise ValueError(
            "No API key found for model "
            f"{self.model_name!r}. Export one of "
            f"{', '.join(self._provider_key_names())} or pass "
            "--ae <NAME>=<value>."
        )

    def _provider_key_names(self) -> tuple[str, ...]:
        names = [POINTER_API_KEY_ENV, "OPENAI_API_KEY"]
        provider = self._parsed_model_provider
        if provider:
            names.append(f"{provider.upper().replace('-', '_')}_API_KEY")
        names.append("ANTHROPIC_API_KEY")
        return tuple(dict.fromkeys(names))

    def _provider_key_envs(self) -> tuple[str, ...]:
        return (*self._provider_key_names(), *self._resolved_env_vars.keys())

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        options = self._options
        model = self._resolve_model_id()
        api_key_env, env = self._resolve_credentials()

        # Upload the instruction as a file: pointer-cli reads it verbatim, so
        # no shell quoting can corrupt the prompt.
        with tempfile.TemporaryDirectory(prefix="pointer-cli-instruction-") as tmp:
            local_instruction = Path(tmp) / "instruction.txt"
            local_instruction.write_text(instruction, encoding="utf-8")
            await environment.upload_file(local_instruction, REMOTE_INSTRUCTION)

        access = self.model_connection
        command = build_run_command(
            model=model,
            api_key_env=api_key_env,
            out_dir=str(EnvironmentPaths.agent_dir),
            workspace=options.workspace,
            base_url=options.base_url or access.base_url,
            allowed_tools=options.allowed_tools,
            approval=options.approval,
            timeout_sec=options.timeout_sec,
            max_turns=options.max_turns,
        )

        result = await environment.exec(
            command=f"set -o pipefail; {command}",
            env=env,
        )
        return_code = result.return_code
        if return_code == EXIT_SUCCESS:
            return
        if return_code in TOLERATED_EXIT_CODES:
            self.logger.warning(
                "pointer-cli exited %s (budget exhausted); continuing to "
                "verification with the artifacts it wrote",
                return_code,
            )
            return
        raise self._classify_exec_error(command, result)

    # -- post run ---------------------------------------------------------

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        """Backfill token usage from pointer-cli's ``usage.json``."""
        usage_path = self.logs_dir / "usage.json"
        try:
            document = json.loads(usage_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return
        totals = document.get("totals")
        if not isinstance(totals, dict):
            return
        for source, target in _USAGE_FIELD_MAP:
            value = totals.get(source)
            if isinstance(value, int):
                setattr(context, target, value)
        cost = totals.get("cost_usd")
        if isinstance(cost, (int, float)):
            context.cost_usd = float(cost)
