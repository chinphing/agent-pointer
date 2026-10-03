"""Unit tests for the pointer-cli Harbor adapter.

Runs against the Harbor runtime, so use the interpreter that has ``harbor``
installed (no extra test dependency — stdlib ``unittest`` only)::

    /tmp/harbor-venv/bin/python -m unittest discover -s benchmarks/terminal_bench -t benchmarks -v

or, equivalently, from the repository root::

    PYTHONPATH=benchmarks /tmp/harbor-venv/bin/python -m unittest terminal_bench.test_pointer_agent -v
"""

from __future__ import annotations

import asyncio
import json
import sys
import tempfile
import unittest
from dataclasses import dataclass, field
from pathlib import Path

# Make the adapter importable no matter which cwd the test runner uses.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from terminal_bench.pointer_agent import (  # noqa: E402
    AGENT_NAME,
    DEFAULT_ALLOWED_TOOLS,
    DEFAULT_TIMEOUT_SEC,
    INSTALL_ROOT,
    POINTER_API_KEY_ENV,
    REMOTE_BINARY,
    REMOTE_INSTALL_DIR,
    REMOTE_INSTRUCTION,
    REMOTE_TARBALL,
    PointerAgent,
    build_install_command,
    build_run_command,
    resolve_dist_tarball,
    shell_workspace,
    split_model_name,
)
from harbor.agents.installed.base import NonZeroAgentExitCodeError  # noqa: E402
from harbor.models.agent.context import AgentContext  # noqa: E402


# ---------------------------------------------------------------------------
# Test doubles
# ---------------------------------------------------------------------------


@dataclass
class FakeResult:
    return_code: int = 0
    stdout: str = ""
    stderr: str = ""


@dataclass
class FakeEnvironment:
    """Minimal stand-in for ``BaseEnvironment`` (exec + upload_file)."""

    results: list[FakeResult] = field(default_factory=list)
    calls: list[dict] = field(default_factory=list)
    uploads: list[tuple[Path, str]] = field(default_factory=list)
    default_user: str | None = "agent"

    async def exec(
        self,
        command: str,
        cwd: str | None = None,
        env: dict[str, str] | None = None,
        timeout_sec: int | None = None,
        user: str | int | None = None,
    ) -> FakeResult:
        self.calls.append(
            {
                "command": command,
                "cwd": cwd,
                "env": env,
                "timeout_sec": timeout_sec,
                "user": user,
            }
        )
        if self.results:
            return self.results.pop(0)
        return FakeResult()

    async def upload_file(self, source_path, target_path: str) -> None:
        source = Path(source_path)
        self.uploads.append((source, target_path))
        self.uploaded_bytes = getattr(self, "uploaded_bytes", {})
        if source.is_file():
            self.uploaded_bytes[target_path] = source.read_bytes()


def make_agent(tmp_dir: Path, **kwargs) -> PointerAgent:
    return PointerAgent(
        logs_dir=tmp_dir,
        model_name=kwargs.pop("model_name", "openai/gpt-4o-mini"),
        **kwargs,
    )


# ---------------------------------------------------------------------------
# Pure helpers
# ---------------------------------------------------------------------------


class TestSplitModelName(unittest.TestCase):
    def test_provider_prefixed(self):
        self.assertEqual(split_model_name("openai/gpt-4o"), ("openai", "gpt-4o"))

    def test_bare_model(self):
        self.assertEqual(split_model_name("gpt-4o"), (None, "gpt-4o"))

    def test_keeps_nested_model_path(self):
        self.assertEqual(
            split_model_name("openrouter/anthropic/claude-3"),
            ("openrouter", "anthropic/claude-3"),
        )


class TestShellWorkspace(unittest.TestCase):
    def test_pwd_stays_a_live_expansion(self):
        self.assertEqual(shell_workspace("$PWD"), '"$PWD"')

    def test_other_paths_are_quoted(self):
        self.assertEqual(shell_workspace("/app"), "/app")
        self.assertEqual(shell_workspace("/my app"), "'/my app'")


class TestBuildInstallCommand(unittest.TestCase):
    def test_unpacks_uploaded_archive_and_runs_offline_installer(self):
        command = build_install_command()
        self.assertIn(f"tar -xzf {REMOTE_TARBALL}", command)
        self.assertIn(f"-C {REMOTE_INSTALL_DIR}", command)
        self.assertIn("--strip-components=1", command)
        self.assertIn(f"sh {REMOTE_INSTALL_DIR}/install.sh --bindir /usr/local/bin", command)
        self.assertIn(f"{REMOTE_BINARY} --version", command)
        # The install must never reach for the network.
        for tool in ("curl", "wget"):
            self.assertNotIn(tool, command)

    def test_paths_are_quoted(self):
        command = build_install_command(
            tarball="/tmp/my archive.tar.gz",
            install_dir="/opt/pointer cli",
            bindir="/opt/bin dir",
        )
        self.assertIn("'/tmp/my archive.tar.gz'", command)
        self.assertIn("'/opt/pointer cli'", command)
        self.assertIn("'/opt/bin dir'", command)


class TestBuildRunCommand(unittest.TestCase):
    def test_required_flags(self):
        command = build_run_command(
            model="gpt-4o-mini",
            api_key_env=POINTER_API_KEY_ENV,
            out_dir="/logs/agent",
        )
        self.assertTrue(command.startswith(f"{REMOTE_BINARY} run"))
        self.assertIn(f"--instruction-file {REMOTE_INSTRUCTION}", command)
        self.assertIn('--workspace "$PWD"', command)
        self.assertIn("--model gpt-4o-mini", command)
        self.assertIn(f"--api-key-env {POINTER_API_KEY_ENV}", command)
        self.assertIn("--out-dir /logs/agent", command)

    def test_optional_flags_are_omitted_when_unset(self):
        command = build_run_command(
            model="gpt-4o-mini",
            api_key_env=POINTER_API_KEY_ENV,
            out_dir="/logs/agent",
        )
        for flag in ("--base-url", "--allowed-tools", "--approval", "--timeout", "--max-turns"):
            self.assertNotIn(flag, command)

    def test_optional_flags_are_rendered_when_set(self):
        command = build_run_command(
            model="gpt-4o-mini",
            api_key_env=POINTER_API_KEY_ENV,
            out_dir="/logs/agent",
            base_url="https://example.test/v1",
            allowed_tools="terminal,file_read",
            approval="auto",
            timeout_sec=120,
            max_turns=25,
        )
        self.assertIn("--base-url https://example.test/v1", command)
        self.assertIn("--allowed-tools terminal,file_read", command)
        self.assertIn("--approval auto", command)
        self.assertIn("--timeout 120", command)
        self.assertIn("--max-turns 25", command)

    def test_instruction_path_is_quoted(self):
        command = build_run_command(
            model="gpt-4o-mini",
            api_key_env=POINTER_API_KEY_ENV,
            out_dir="/logs/agent",
            instruction_path="/installed-agent/my instruction.txt",
        )
        self.assertIn("--instruction-file '/installed-agent/my instruction.txt'", command)

    def test_extra_flags_are_appended_verbatim(self):
        command = build_run_command(
            model="gpt-4o-mini",
            api_key_env=POINTER_API_KEY_ENV,
            out_dir="/logs/agent",
            extra_flags="--timeout 42",
        )
        self.assertTrue(command.endswith("--timeout 42"))


class TestResolveDistTarball(unittest.TestCase):
    def test_default_points_at_the_repo_dist_build(self):
        path = resolve_dist_tarball(None)
        self.assertTrue(
            str(path).endswith("crates/pointer-cli/dist/pointer-cli-linux-x64.tar.gz"),
            f"unexpected default path: {path}",
        )

    def test_absolute_override_is_honoured(self):
        path = resolve_dist_tarball("/tmp/custom/pointer-cli.tar.gz")
        self.assertEqual(path, Path("/tmp/custom/pointer-cli.tar.gz"))

    def test_relative_override_resolves_against_repo_root(self):
        path = resolve_dist_tarball("crates/pointer-cli/dist/other.tar.gz")
        self.assertTrue(path.is_absolute())
        self.assertTrue(
            str(path).endswith("crates/pointer-cli/dist/other.tar.gz"),
            f"unexpected path: {path}",
        )


# ---------------------------------------------------------------------------
# Agent wiring
# ---------------------------------------------------------------------------


class TestAgentOptions(unittest.TestCase):
    def test_name_and_capabilities(self):
        self.assertEqual(PointerAgent.name(), AGENT_NAME)
        self.assertTrue(PointerAgent.capabilities.atif)

    def test_model_id_drops_provider_prefix(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            self.assertEqual(agent._resolve_model_id(), "gpt-4o-mini")

    def test_default_cli_flags(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            flags = agent.build_cli_flags()
            self.assertIn("--approval auto", flags)
            self.assertIn(f"--allowed-tools {DEFAULT_ALLOWED_TOOLS}", flags)
            self.assertIn(f"--timeout {DEFAULT_TIMEOUT_SEC}", flags)
            self.assertNotIn("--max-turns", flags)

    def test_kwargs_override_defaults(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(
                Path(tmp),
                timeout_sec=90,
                max_turns=7,
                allowed_tools="terminal",
                approval="manual",
                base_url="https://example.test/v1",
            )
            flags = agent.build_cli_flags()
            self.assertIn("--timeout 90", flags)
            self.assertIn("--max-turns 7", flags)
            self.assertIn("--allowed-tools terminal", flags)
            self.assertIn("--approval manual", flags)
            self.assertIn("--base-url https://example.test/v1", flags)

    def test_unknown_kwarg_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                make_agent(Path(tmp), not_a_real_option=True)

    def test_credentials_come_from_the_provider_env(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp), extra_env={"OPENAI_API_KEY": "sk-test"})
            name, env = agent._resolve_credentials()
            self.assertEqual(name, POINTER_API_KEY_ENV)
            self.assertEqual(env, {POINTER_API_KEY_ENV: "sk-test"})

    def test_missing_credentials_raise(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            with self.assertRaises(ValueError):
                agent._resolve_credentials()


class TestInstall(unittest.IsolatedAsyncioTestCase):
    def _tarball(self) -> Path:
        return resolve_dist_tarball(None)

    async def test_uploads_archive_and_installs_as_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            environment = FakeEnvironment()

            await agent.install(environment)

            self.assertEqual(
                environment.uploads,
                [(self._tarball(), REMOTE_TARBALL)],
            )
            install_calls = [
                call for call in environment.calls if "install.sh" in call["command"]
            ]
            self.assertEqual(len(install_calls), 1)
            call = install_calls[0]
            self.assertEqual(call["user"], "root")
            self.assertIn(f"tar -xzf {REMOTE_TARBALL}", call["command"])
            self.assertIn("--bindir /usr/local/bin", call["command"])

    async def test_missing_archive_fails_before_touching_the_container(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp), dist_tarball="/nonexistent/pointer-cli.tar.gz")
            environment = FakeEnvironment()

            with self.assertRaises(FileNotFoundError):
                await agent.install(environment)

            self.assertEqual(environment.uploads, [])


class TestRun(unittest.IsolatedAsyncioTestCase):
    async def _run(self, environment: FakeEnvironment, instruction: str = "do the task"):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp), extra_env={"OPENAI_API_KEY": "sk-test"})
            await agent.run(instruction, environment, AgentContext())
            return agent

    async def test_uploads_instruction_and_runs_pointer_cli(self):
        environment = FakeEnvironment()
        await self._run(environment, instruction="fix the bug\nsecond line")

        self.assertEqual(len(environment.uploads), 1)
        _, target = environment.uploads[0]
        self.assertEqual(target, REMOTE_INSTRUCTION)
        # Content is captured while the staging temp dir still exists.
        self.assertEqual(
            environment.uploaded_bytes[REMOTE_INSTRUCTION],
            b"fix the bug\nsecond line",
        )

        run_calls = [call for call in environment.calls if " run " in call["command"]]
        self.assertEqual(len(run_calls), 1)
        call = run_calls[0]
        self.assertEqual(
            call["env"],
            {POINTER_API_KEY_ENV: "sk-test"},
        )
        self.assertIn(f"--instruction-file {REMOTE_INSTRUCTION}", call["command"])
        self.assertIn("--out-dir /logs/agent", call["command"])
        self.assertIn('--workspace "$PWD"', call["command"])
        self.assertIn("--model gpt-4o-mini", call["command"])
        self.assertIn("--base-url https://api.openai.com/v1", call["command"])
        self.assertIn(f"--allowed-tools {DEFAULT_ALLOWED_TOOLS}", call["command"])
        self.assertTrue(call["command"].startswith("set -o pipefail; "))

    async def test_run_command_has_no_duplicated_option_flags(self):
        # Regression: ``run`` used to pass ``build_cli_flags()`` *and* the
        # explicit option kwargs, so pointer-cli saw ``--approval`` twice and
        # exited 2 before doing any work.
        environment = FakeEnvironment()
        await self._run(environment)

        run_calls = [call for call in environment.calls if " run " in call["command"]]
        self.assertEqual(len(run_calls), 1)
        command = run_calls[0]["command"]
        for flag in ("--approval", "--allowed-tools", "--timeout", "--base-url", "--max-turns"):
            self.assertLessEqual(
                command.count(flag), 1, f"{flag} duplicated in: {command}"
            )
        # The defaults are always rendered exactly once.
        for flag in ("--approval", "--allowed-tools", "--timeout", "--base-url"):
            self.assertEqual(command.count(flag), 1, f"{flag} missing in: {command}")

    async def test_turn_limit_exit_code_is_tolerated(self):
        environment = FakeEnvironment(results=[FakeResult(return_code=14)])
        await self._run(environment)  # must not raise

    async def test_timeout_exit_code_is_tolerated(self):
        environment = FakeEnvironment(results=[FakeResult(return_code=10)])
        await self._run(environment)  # must not raise

    async def test_auth_failure_raises(self):
        environment = FakeEnvironment(
            results=[FakeResult(return_code=11, stderr="Invalid API key provided")]
        )
        with self.assertRaises(NonZeroAgentExitCodeError):
            await self._run(environment)


class TestPopulateContext(unittest.TestCase):
    def test_usage_json_backfills_token_counts(self):
        with tempfile.TemporaryDirectory() as tmp:
            logs_dir = Path(tmp)
            (logs_dir / "usage.json").write_text(
                json.dumps(
                    {
                        "totals": {
                            "prompt_tokens": 1200,
                            "completion_tokens": 340,
                            "cached_tokens": 100,
                            "cost_usd": None,
                        }
                    }
                ),
                encoding="utf-8",
            )
            agent = make_agent(logs_dir)
            context = AgentContext()

            agent.populate_context_post_run(context)

            self.assertEqual(context.n_input_tokens, 1200)
            self.assertEqual(context.n_output_tokens, 340)
            self.assertEqual(context.n_cache_tokens, 100)
            self.assertIsNone(context.cost_usd)

    def test_missing_usage_json_is_ignored(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            context = AgentContext()
            agent.populate_context_post_run(context)
            self.assertTrue(context.is_empty())


if __name__ == "__main__":
    asyncio.set_event_loop_policy(asyncio.DefaultEventLoopPolicy())
    unittest.main()
