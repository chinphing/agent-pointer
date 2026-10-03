#!/usr/bin/env python3
"""Minimal OpenAI-compatible SSE mock provider for pointer-cli E2E checks.

Only the subset the agent loop actually reads is implemented: an SSE body of
``data: <StreamChunk>`` lines (native streaming ``tool_calls``), terminated by
``data: [DONE]``. Every request body is appended to ``MOCK_LOG`` (one JSON
object per line) so the caller can assert on the advertised ``tools`` list.

Environment:
  MOCK_MODE   text_only | tool_then_text | tool_forever | sleep_tool |
              exec_sleep_tool | fork_sleep_tool |
              reasoning_length_then_text | subagent_then_text
              (default: tool_then_text)
  MOCK_LOG    file to append one JSON line per received request (optional)
  MOCK_PORT   listen port (default: 8731)
"""

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODE = os.environ.get("MOCK_MODE", "tool_then_text")
LOG = os.environ.get("MOCK_LOG", "")
PORT = int(os.environ.get("MOCK_PORT", "8731"))

_lock = threading.Lock()
_count = 0


def _sse(obj):
    return ("data: " + json.dumps(obj, separators=(",", ":")) + "\n\n").encode("utf-8")


def _tool_call_payload(command):
    return [
        {
            "choices": [
                {
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_mock_1",
                                "type": "function",
                                "function": {
                                    "name": "terminal",
                                    "arguments": json.dumps({"command": command}),
                                },
                            }
                        ]
                    },
                    "finish_reason": None,
                }
            ]
        },
        {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]},
    ]


def _run_subagent_payload(agent_id, goal):
    """Native ``run_subagent`` tool call, used to spawn a scoped sub-agent."""
    return [
        {
            "choices": [
                {
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_mock_subagent",
                                "type": "function",
                                "function": {
                                    "name": "run_subagent",
                                    "arguments": json.dumps(
                                        {"agentId": agent_id, "goal": goal}
                                    ),
                                },
                            }
                        ]
                    },
                    "finish_reason": None,
                }
            ]
        },
        {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]},
    ]


def _text_payload(text):
    return [
        {"choices": [{"delta": {"content": text}, "finish_reason": None}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    ]


def _reasoning_only_length_payload(reasoning):
    """Reasoning deltas with no visible content, ending on ``finish_reason=length``.

    Mirrors a thinking model that spent the whole output budget on
    chain-of-thought: nothing observable was produced, so re-sending the same
    request at the same budget would stop identically.
    """
    return [
        {"choices": [{"delta": {"reasoning_content": reasoning}, "finish_reason": None}]},
        {"choices": [{"delta": {}, "finish_reason": "length"}]},
    ]


def _usage_payload(prompt_tokens, completion_tokens):
    """Final SSE chunk carrying the OpenAI-compatible `usage` block.

    Emitted because the agent records per-run token usage from the last chunk
    that has `usage` (streaming with `stream_options.include_usage`).
    """
    return [
        {
            "choices": [],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens,
            },
        }
    ]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # keep stderr quiet
        pass

    def do_POST(self):
        global _count
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            req = json.loads(raw or b"{}")
        except Exception:
            req = {}
        with _lock:
            _count += 1
            n = _count
        if LOG:
            tools = [
                (t.get("function") or {}).get("name")
                for t in (req.get("tools") or [])
            ]
            with open(LOG, "a", encoding="utf-8") as fh:
                fh.write(
                    json.dumps(
                        {
                            "n": n,
                            "path": self.path,
                            "tools": tools,
                            "max_tokens": req.get("max_tokens"),
                        }
                    )
                    + "\n"
                )

        if not self.path.rstrip("/").endswith("/chat/completions"):
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return

        if MODE == "text_only":
            payloads = _text_payload("MOCK_FINAL_OK")
        elif MODE == "tool_forever":
            payloads = _tool_call_payload("echo mock-loop")
        elif MODE == "sleep_tool" and n == 1:
            payloads = _tool_call_payload("sleep 300")
        elif MODE == "exec_sleep_tool" and n == 1:
            # `exec` makes `sh -lc` replace itself, so the long-lived process is
            # the terminal tool's *direct* child (no forked grandchild).
            payloads = _tool_call_payload("exec sleep 300")
        elif MODE == "fork_sleep_tool" and n == 1:
            # `&` forces the shell to fork, so `sleep` is a *grandchild* of the
            # CLI (child of the `sh -lc` wrapper). The pid is written to
            # $POINTER_REPRO_PIDFILE so a caller can tell whether the descendant
            # outlived the cancelled run.
            payloads = _tool_call_payload(
                'sleep 300 & echo $! > "$POINTER_REPRO_PIDFILE"; wait'
            )
        elif MODE == "tool_then_text" and n == 1:
            payloads = _tool_call_payload("echo mock-tool-ok")
        elif MODE == "reasoning_length_then_text" and n == 1:
            # First turn: burn the budget on reasoning only (finish_reason=length).
            payloads = _reasoning_only_length_payload("thinking " * 64)
        elif MODE == "subagent_then_text" and n == 1:
            # First turn: delegate to a scoped sub-agent, then let both the
            # sub-agent and the lead finish with plain text.
            payloads = _run_subagent_payload(
                "self",
                "What: probe sub-agent trajectory\nDone when: a reply is returned",
            )
        else:  # text_only already handled; follow-up turn after a tool result
            payloads = _text_payload("MOCK_FINAL_OK")

        # Append a usage-only chunk so callers can assert token accounting.
        payloads = payloads + _usage_payload(11 * n, 7 * n)

        body = b"".join(_sse(p) for p in payloads) + b"data: [DONE]\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()


def main():
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
