"""Unit tests for the pointer-cli Pier adapter.

Runs against Pier, so use the interpreter that has ``datacurve-pier`` installed
(no extra test dependency — stdlib ``unittest`` only)::

    PYTHONPATH=benchmarks /tmp/pier-venv/bin/python -m unittest \\
        deepswe.test_pointer_agent -v

or from the repository root::

    PYTHONPATH=benchmarks /tmp/pier-venv/bin/python -m unittest discover \\
        -s benchmarks/deepswe -t benchmarks -v
"""

from __future__ import annotations

import json
import os
import shlex
import sys
import tempfile
import unittest
from dataclasses import dataclass, field
from pathlib import Path

# Make the adapter importable no matter which cwd the test runner uses.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from deepswe.pointer_agent import (  # noqa: E402
    AGENT_NAME,
    DEFAULT_ALLOWED_TOOLS,
    DEFAULT_TIMEOUT_SEC,
    FINALIZE_COMMIT_MESSAGE,
    POINTER_API_KEY_ENV,
    REMOTE_BINARY,
    REMOTE_INSTALL_DIR,
    REMOTE_INSTRUCTION,
    REMOTE_TARBALL,
    PointerAgent,
    build_finalize_command,
    build_install_command,
    build_run_command,
    build_tolerant_wrapper,
    network_allowlist_for,
    resolve_credentials,
    resolve_dist_tarball,
    shell_workspace,
    split_model_name,
    usage_context_fields,
)
from pier.agents.installed.base import NonZeroAgentExitCodeError  # noqa: E402
from pier.models.agent.context import AgentContext  # noqa: E402


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
    uploaded_bytes: dict[str, bytes] = field(default_factory=dict)
    default_user: str | None = "agent"

    def agent_process_env(self, env):
        """Pier injects proxy vars here; the fake is a pass-through."""
        return env

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
        if source.is_file():
            self.uploaded_bytes[target_path] = source.read_bytes()


def make_agent(tmp_dir: Path, **kwargs) -> PointerAgent:
    return PointerAgent(
        logs_dir=tmp_dir,
        model_name=kwargs.pop("model_name", "openai/gpt-4o-mini"),
        **kwargs,
    )


class BaseAgentTest(unittest.TestCase):
    """Keep ambient base-url env vars from leaking into assertions."""

    def setUp(self) -> None:
        for key in ("POINTER_BASE_URL", "OPENAI_BASE_URL", "OPENAI_API_BASE"):
            self._saved = getattr(self, "_saved", {})
            self._saved[key] = os.environ.pop(key, None)

    def tearDown(self) -> None:
        for key, value in getattr(self, "_saved", {}).items():
            if value is not None:
                os.environ[key] = value


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
        for flag in (
            "--base-url",
            "--allowed-tools",
            "--approval",
            "--timeout",
            "--max-turns",
            "--prompt-profile",
        ):
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
            prompt_profile="minimal",
        )
        self.assertIn("--base-url https://example.test/v1", command)
        self.assertIn("--allowed-tools terminal,file_read", command)
        self.assertIn("--approval auto", command)
        self.assertIn("--timeout 120", command)
        self.assertIn("--max-turns 25", command)
        self.assertIn("--prompt-profile minimal", command)

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


class TestBuildTolerantWrapper(unittest.TestCase):
    def test_budget_exit_codes_are_remapped_to_zero(self):
        wrapped = build_tolerant_wrapper("pointer-cli run --model m")
        self.assertIn("pointer-cli run --model m", wrapped)
        self.assertIn("case $pointer_cli_rc in 0|10|14) exit 0;; esac", wrapped)
        self.assertIn("exit $pointer_cli_rc", wrapped)

    def test_custom_tolerated_codes(self):
        wrapped = build_tolerant_wrapper("cmd", tolerated=frozenset({7}))
        self.assertIn("0|7) exit 0;; esac", wrapped)


class TestBuildFinalizeCommand(unittest.TestCase):
    def test_stages_and_commits_leftover_work(self):
        command = build_finalize_command()
        self.assertIn('cd "$PWD"', command)
        self.assertIn("git rev-parse --git-dir", command)
        self.assertIn("git add -A", command)
        self.assertIn("git diff --cached --quiet", command)
        self.assertIn("commit -q --no-verify", command)
        self.assertIn(f"-m {shlex.quote(FINALIZE_COMMIT_MESSAGE)}", command)
        # Best-effort: every failure path exits 0 so Pier never sees a failure.
        self.assertIn("|| exit 0", command)
        self.assertTrue(command.rstrip().endswith("exit 0"))

    def test_workspace_and_message_are_quoted(self):
        command = build_finalize_command(workspace="/my work", message="fix; rm -rf /")
        self.assertIn("cd '/my work'", command)
        self.assertIn("'fix; rm -rf /'", command)


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


class TestResolveCredentials(unittest.TestCase):
    def test_prefers_pointer_api_key(self):
        name, env = resolve_credentials(
            lambda key: {"POINTER_API_KEY": "sk-pointer", "OPENAI_API_KEY": "sk-openai"}.get(key)
        )
        self.assertEqual(name, POINTER_API_KEY_ENV)
        self.assertEqual(env, {POINTER_API_KEY_ENV: "sk-pointer"})

    def test_falls_back_to_provider_key(self):
        name, env = resolve_credentials(
            lambda key: {"OPENAI_API_KEY": "sk-openai"}.get(key)
        )
        self.assertEqual(name, POINTER_API_KEY_ENV)
        self.assertEqual(env, {POINTER_API_KEY_ENV: "sk-openai"})

    def test_missing_key_raises(self):
        with self.assertRaises(ValueError):
            resolve_credentials(lambda key: None)


class TestUsageContextFields(unittest.TestCase):
    def test_maps_totals(self):
        fields = usage_context_fields(
            {
                "totals": {
                    "prompt_tokens": 1200,
                    "completion_tokens": 340,
                    "cached_tokens": 100,
                    "cost_usd": None,
                }
            }
        )
        self.assertEqual(
            fields,
            {
                "n_input_tokens": 1200,
                "n_cache_tokens": 100,
                "n_output_tokens": 340,
            },
        )

    def test_ignores_missing_or_malformed(self):
        self.assertEqual(usage_context_fields(None), {})
        self.assertEqual(usage_context_fields({}), {})
        self.assertEqual(usage_context_fields({"totals": "nope"}), {})
        self.assertEqual(usage_context_fields({"totals": {"prompt_tokens": True}}), {})


class TestNetworkAllowlistFor(unittest.TestCase):
    def test_adds_host_from_url_and_keeps_default(self):
        allowlist = network_allowlist_for(["https://api.deepseek.com/v1"])
        self.assertEqual(
            allowlist.domains,
            ["api.deepseek.com", "api.openai.com"],
        )

    def test_empty_input_keeps_default(self):
        self.assertEqual(network_allowlist_for([]).domains, ["api.openai.com"])


# ---------------------------------------------------------------------------
# Agent wiring
# ---------------------------------------------------------------------------


class TestAgentWiring(BaseAgentTest):
    def test_name_and_atif_support(self):
        self.assertEqual(PointerAgent.name(), AGENT_NAME)
        self.assertTrue(PointerAgent.SUPPORTS_ATIF)

    def test_install_spec_is_none_so_prebuilt_images_survive(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            self.assertIsNone(agent.install_spec())

    def test_model_id_drops_provider_prefix(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            self.assertEqual(agent._resolve_model_id(), "gpt-4o-mini")

    def test_version_command_and_parser(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            self.assertEqual(agent.get_version_command(), f"{REMOTE_BINARY} --version")
            self.assertEqual(agent.parse_version("pointer-cli 0.3.1\n"), "0.3.1")

    def test_default_cli_flag_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            self.assertEqual(agent._resolved_flag("approval"), "auto")
            self.assertEqual(agent._resolved_flag("allowed_tools"), DEFAULT_ALLOWED_TOOLS)
            self.assertEqual(agent._resolved_flag("timeout_sec"), DEFAULT_TIMEOUT_SEC)
            self.assertIsNone(agent._resolved_flag("max_turns"))
            self.assertIsNone(agent._resolved_flag("base_url"))
            self.assertIsNone(agent._resolved_flag("prompt_profile"))

    def test_kwargs_override_defaults(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(
                Path(tmp),
                timeout_sec=90,
                max_turns=7,
                allowed_tools="terminal",
                approval="manual",
                base_url="https://example.test/v1",
                prompt_profile="minimal",
            )
            self.assertEqual(agent._resolved_flag("timeout_sec"), 90)
            self.assertEqual(agent._resolved_flag("max_turns"), 7)
            self.assertEqual(agent._resolved_flag("allowed_tools"), "terminal")
            self.assertEqual(agent._resolved_flag("approval"), "manual")
            self.assertEqual(agent._resolved_flag("prompt_profile"), "minimal")
            self.assertEqual(agent._resolve_base_url(), "https://example.test/v1")

    def test_base_url_falls_back_to_env(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp), extra_env={"OPENAI_BASE_URL": "https://env.test/v1"})
            self.assertEqual(agent._resolve_base_url(), "https://env.test/v1")

    def test_network_allowlist_uses_base_url(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp), base_url="https://api.deepseek.com/v1")
            self.assertEqual(
                agent.network_allowlist().domains,
                ["api.deepseek.com", "api.openai.com"],
            )

    def test_exec_timeout_leaves_room_for_pointer_cli_deadline(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp), timeout_sec=120)
            self.assertEqual(agent._exec_timeout_sec(), 240)


class TestInstall(BaseAgentTest, unittest.IsolatedAsyncioTestCase):
    def _tarball(self) -> Path:
        return resolve_dist_tarball(None)

    async def test_uploads_archive_and_installs_as_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            agent = make_agent(Path(tmp))
            environment = FakeEnvironment()

            await agent.install(environment)

            self.assertEqual(environment.uploads, [(self._tarball(), REMOTE_TARBALL)])
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


class TestRun(BaseAgentTest, unittest.IsolatedAsyncioTestCase):
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
        self.assertEqual(
            environment.uploaded_bytes[REMOTE_INSTRUCTION],
            b"fix the bug\nsecond line",
        )

        run_calls = [call for call in environment.calls if " run " in call["command"]]
        self.assertEqual(len(run_calls), 1)
        call = run_calls[0]
        # Runs as the default agent user via _exec, so Pier's proxy env applies.
        self.assertIsNone(call["user"])
        # The provider key is normalised into POINTER_API_KEY for --api-key-env.
        self.assertEqual(call["env"][POINTER_API_KEY_ENV], "sk-test")
        self.assertIn(f"--instruction-file {REMOTE_INSTRUCTION}", call["command"])
        self.assertIn("--out-dir /logs/agent", call["command"])
        self.assertIn('--workspace "$PWD"', call["command"])
        self.assertIn("--model gpt-4o-mini", call["command"])
        self.assertIn(f"--allowed-tools {DEFAULT_ALLOWED_TOOLS}", call["command"])
        self.assertTrue(call["command"].startswith("set -o pipefail; "))
        # Budget-exhaustion exits are tolerated by the wrapper.
        self.assertIn("pointer_cli_rc=$?", call["command"])

    async def test_run_command_has_no_duplicated_option_flags(self):
        environment = FakeEnvironment()
        await self._run(environment)

        run_calls = [call for call in environment.calls if " run " in call["command"]]
        self.assertEqual(len(run_calls), 1)
        command = run_calls[0]["command"]
        for flag in ("--approval", "--allowed-tools", "--timeout", "--base-url", "--max-turns"):
            self.assertLessEqual(command.count(flag), 1, f"{flag} duplicated in: {command}")
        # The defaults are always rendered exactly once.
        for flag in ("--approval", "--allowed-tools", "--timeout"):
            self.assertEqual(command.count(flag), 1, f"{flag} missing in: {command}")
        # No base_url was configured, so the optional flag is absent.
        self.assertEqual(command.count("--base-url"), 0)

    async def test_budget_exit_codes_do_not_raise(self):
        # The wrapper maps 10/14 to 0, so a successful exec means "tolerated".
        environment = FakeEnvironment(results=[FakeResult(return_code=0)])
        await self._run(environment)  # must not raise

    async def test_auth_failure_raises(self):
        environment = FakeEnvironment(
            results=[FakeResult(return_code=11, stderr="Invalid API key provided")]
        )
        with self.assertRaises(NonZeroAgentExitCodeError):
            await self._run(environment)

    async def test_finalize_commit_runs_after_the_agent(self):
        environment = FakeEnvironment()
        await self._run(environment)

        finalize_calls = [
            call for call in environment.calls if "git add -A" in call["command"]
        ]
        self.assertEqual(len(finalize_calls), 1)
        call = finalize_calls[0]
        # Goes through exec_as_agent, so Pier's env injection applies.
        self.assertIsNone(call["user"])
        self.assertIn("commit -q --no-verify", call["command"])
        # It must be the last thing run(), so the tree is committed before
        # [[verifier.collect]] reads it.
        self.assertIs(environment.calls[-1], call)

    async def test_finalize_failure_does_not_fail_the_trial(self):
        # run() succeeds, finalize blows up: the exception must be swallowed.
        environment = FakeEnvironment(
            results=[FakeResult(0), FakeResult(return_code=1, stderr="not a git repo")]
        )
        await self._run(environment)  # must not raise


class TestPopulateContext(BaseAgentTest):
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
    unittest.main()
